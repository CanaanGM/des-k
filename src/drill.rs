// The drill build's stop points (spec 0034 AC-4). This file is compiled only with
// `--features drill`, so a release des-k has none and never reads DESK_DRILL: no environment
// variable can make the shipped admin binary stop itself.
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

#[cfg(test)]
mod tests {
    use super::*;

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
