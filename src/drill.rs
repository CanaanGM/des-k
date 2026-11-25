// The drill build's stop points (spec 0034 AC-4) and the stress run's numbers (spec 0037
// AC-1). This file is compiled only with `--features drill`, so a release des-k has none,
// never reads DESK_DRILL and times nothing: no environment variable can make the shipped admin
// binary stop itself.
use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::Duration;
use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

/// Where a drill build stops: one at a time, named by DESK_DRILL.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Stop {
    KillAfterHide,
    KillAfterSend,
    KillMidRestore,
    PauseMidSwitch,
}

impl Stop {
    fn name(self) -> &'static str {
        match self {
            Stop::KillAfterHide => "kill_after_hide",
            Stop::KillAfterSend => "kill_after_send",
            Stop::KillMidRestore => "kill_mid_restore",
            Stop::PauseMidSwitch => "pause_mid_switch",
        }
    }
}

// How long pause_mid_switch holds the switch, the main thread and the ws lock with it.
const PAUSE: Duration = Duration::from_secs(15);

static ARMED: OnceLock<Option<Stop>> = OnceLock::new();

/// DESK_DRILL's value read: unset or empty arms nothing, a name arms its stop point, anything
/// else is an error carrying the value, and arms nothing.
fn parse(value: Option<&str>) -> Result<Option<Stop>, String> {
    let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) else { return Ok(None) };
    [Stop::KillAfterHide, Stop::KillAfterSend, Stop::KillMidRestore, Stop::PauseMidSwitch]
        .into_iter()
        .find(|s| s.name() == v)
        .map(Some)
        .ok_or_else(|| v.to_string())
}

/// Reads DESK_DRILL once, at start, and says what is armed.
pub fn arm() {
    let value = std::env::var("DESK_DRILL").ok();
    let armed = match parse(value.as_deref()) {
        Ok(s) => s,
        Err(v) => {
            warn!("drill build: DESK_DRILL={v:?} is not a stop point, nothing armed");
            None
        }
    };
    match armed {
        Some(s) => warn!("drill build: stop point {} armed", s.name()),
        None => warn!("drill build: no stop point"),
    }
    let _ = ARMED.set(armed);
    STRESS.with_borrow_mut(|s| s.times.reserve_exact(KEEP));
}

/// A stop point reached: if it is the armed one, pause, or end des-k at once. A kill runs no
/// panic hook, console handler or exit path, exactly like Task Manager's End task.
pub fn at(stop: Stop) {
    if ARMED.get().copied().flatten() != Some(stop) {
        return;
    }
    if stop == Stop::PauseMidSwitch {
        warn!("drill: pausing {} s at {}", PAUSE.as_secs(), stop.name());
        std::thread::sleep(PAUSE);
        return warn!("drill: carrying on after {}", stop.name());
    }
    warn!("drill: stopping hard at {}", stop.name());
    crate::log::flush();
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 1);
    }
}

// Switch times kept for the Quit line. Room for all of them is taken at start, so measuring
// never allocates during a run and never shows up in the memory the run watches.
const KEEP: usize = 20_000;

#[derive(Default)]
struct Stress {
    times: Vec<u32>, // whole microseconds, the first KEEP switches
    switches: u64,
    reloads: u64,
}

impl Stress {
    fn switched(&mut self, took: Duration) {
        self.switches += 1;
        if self.times.len() < self.times.capacity().min(KEEP) {
            self.times.push(u32::try_from(took.as_micros()).unwrap_or(u32::MAX));
        }
    }
}

thread_local! {
    // Main thread only: every switch and every reload runs there.
    static STRESS: RefCell<Stress> = RefCell::default();
}

/// One `WM_APP` switch, timed by main from its match arm to `ws::switch` returning.
pub fn switched(took: Duration) {
    STRESS.with_borrow_mut(|s| s.switched(took));
}

/// A `settings::reload` that stored a new version.
pub fn reloaded() {
    STRESS.with_borrow_mut(|s| s.reloads += 1);
}

/// The value at rank `ceil(percent / 100 × count)` of a sorted list (nearest rank).
fn rank(sorted: &[u32], percent: usize) -> Option<u32> {
    let at = (percent * sorted.len()).div_ceil(100).max(1);
    sorted.get(at - 1).copied()
}

fn ms(micros: Option<u32>) -> String {
    micros.map_or_else(|| "-".into(), |m| format!("{:.1}ms", f64::from(m) / 1000.0))
}

fn line(switches: u64, reloads: u64, sorted: &[u32]) -> String {
    format!(
        "drill: stress switches={switches} reloads={reloads} p50={} p95={} max={}",
        ms(rank(sorted, 50)),
        ms(rank(sorted, 95)),
        ms(sorted.last().copied())
    )
}

/// At Quit, before `des-k stopped`: the one line `drill.ps1 stress` reads.
pub fn report() {
    let text = STRESS.with_borrow_mut(|s| {
        s.times.sort_unstable();
        line(s.switches, s.reloads, &s.times)
    });
    info!("{text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    // The release check compares p95 across releases, so the pick must stay nearest rank: a
    // switch to interpolation or a floor index would move every number with no change in des-k.
    #[test]
    fn percentiles_are_nearest_rank() {
        let sorted: Vec<u32> = (1..=20).map(|i| i * 1000).collect();
        assert_eq!(rank(&sorted, 50), Some(10_000)); // interpolation says 10,500, floor index 11,000
        assert_eq!(rank(&sorted, 95), Some(19_000)); // interpolation says 19,050, floor index 20,000
        assert_eq!(rank(&sorted[..10], 95), Some(10_000)); // ceil(9.5) is the 10th
        assert_eq!(rank(&[1234], 50), Some(1234));
        assert_eq!(rank(&[1234], 95), Some(1234));
        assert_eq!(rank(&[], 50), None);
    }

    // drill.ps1 parses this line, so its words, units and the `-` for no switches are a contract.
    #[test]
    fn quit_line_reads_as_the_script_expects() {
        let sorted: Vec<u32> = (1..=20).map(|i| i * 1000 + 49).collect();
        assert_eq!(line(20, 21, &sorted), "drill: stress switches=20 reloads=21 p50=10.0ms p95=19.0ms max=20.0ms");
        assert_eq!(line(1, 0, &[1240]), "drill: stress switches=1 reloads=0 p50=1.2ms p95=1.2ms max=1.2ms");
        assert_eq!(line(0, 3, &[]), "drill: stress switches=0 reloads=3 p50=- p95=- max=-");
    }

    // Past the reserved room the count goes on and the buffer never grows, or the run's memory
    // check would measure des-k's own measuring.
    #[test]
    fn times_stop_at_the_reserved_room() {
        let mut s = Stress::default();
        s.times.reserve_exact(KEEP);
        let room = s.times.capacity();
        for _ in 0..KEEP + 5 {
            s.switched(Duration::from_micros(700));
        }
        assert_eq!((s.switches, s.times.len(), s.times.capacity()), (KEEP as u64 + 5, KEEP, room));
    }

    // A typo must never arm a different stop point, and unset must arm none, or a drill run
    // would stop somewhere it did not mean to and prove the wrong thing.
    #[test]
    fn desk_drill_names_exactly_one_stop_point() {
        assert_eq!(parse(Some("kill_after_hide")), Ok(Some(Stop::KillAfterHide)));
        assert_eq!(parse(Some("kill_after_send")), Ok(Some(Stop::KillAfterSend)));
        assert_eq!(parse(Some("kill_mid_restore")), Ok(Some(Stop::KillMidRestore)));
        assert_eq!(parse(Some(" pause_mid_switch ")), Ok(Some(Stop::PauseMidSwitch)));
        assert_eq!(parse(Some("kill_after_hid")), Err("kill_after_hid".into()));
        assert_eq!(parse(Some("KILL_AFTER_HIDE")), Err("KILL_AFTER_HIDE".into()));
        assert_eq!(parse(None), Ok(None));
        assert_eq!(parse(Some("")), Ok(None));
    }
}
