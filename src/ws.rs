// Per-monitor workspaces. A workspace only remembers the windows it HID; the active
// workspace is simply "whatever is visible on that monitor" at switch time, so new
// windows and windows dragged between monitors need no tracking.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::{
        Dwm::DWMWA_CLOAKED,
        Gdi::{HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW},
    },
    System::Com::CLSCTX_ALL,
    UI::{
        Shell::{IVirtualDesktopManager, VirtualDesktopManager},
        WindowsAndMessaging::*,
    },
};
use windows::core::BOOL;

use crate::cloak::Shell;
use crate::indicator::{self, Card, Line as Says};
use crate::restore::{Entry, Fp, Pick};
use crate::{keys, settings};

pub(crate) mod focus;
pub(crate) mod moves;
pub(crate) mod pause;
pub(crate) mod restore;
pub(crate) mod rules;
pub(crate) mod screens;
pub(crate) mod store;
pub(crate) mod switch;
pub(crate) mod views;
mod win;

// Each piece sees the others through these, the way their code saw one file before.
use focus::*;
use moves::*;
use pause::*;
use restore::*;
use rules::*;
use screens::*;
use store::*;
use switch::*;
use views::*;

// A rate limit against apps that keep grabbing focus; the stale check and the bounce
// guard in follow() carry correctness (spec 0002).
const FOLLOW_COOLDOWN_MS: u64 = 1000;
// A reopened window often takes focus before restore hides it; that focus is not you (spec 0003).
const RESTORED_IGNORE: Duration = Duration::from_secs(2);
// Screens drop out for a moment on wake from sleep and on a DisplayPort relink; showing all
// their hidden windows then would undo the layout. Counted in 1 s checks, not time, since no
// check runs while the PC sleeps.
const GONE_CHECKS: u32 = 10;
// After a wake external screens can take a while to light up: then a screen must be gone
// longer before its windows fold (spec 0016 AC-18).
const WAKE_GONE_CHECKS: u32 = 30;
const WAKE_GRACE: Duration = Duration::from_secs(30);
// A screen that comes back must stay 2 checks before its windows return: screens flicker on
// wake and on a DisplayPort relink (spec 0016 AC-5).
const BACK_CHECKS: u32 = 2;
// A window that will not move back is tried once a check this many times, then left (AC-12).
const RETURN_TRIES: u32 = 10;
// A dock drops and brings back its screens one by one, and Windows moves windows after each:
// spots are read and windows return only once the layout held this many checks (spec 0016).
const SETTLE_CHECKS: u32 = 3;
// How long a window a rule moved is watched for its app moving it back, and how many times it
// is put back (spec 0027 AC-6).
const SETTLE_FOR: Duration = Duration::from_secs(2);
const SETTLE_PUTS: u8 = 3;
// Windows' shell can show a window a rule just hid (spec 0027 AC-17): cloak it again this long
// after the rule hid it, this many times at most.
const HOLD_FOR: Duration = Duration::from_secs(10);
const RECLOAKS: u8 = 3;
// A brand new window has no shell view for a moment (0x8002802B): retry its cloak this often,
// this many times (about 1 s).
const NEW_RETRY: Duration = Duration::from_millis(15);
const NEW_TRIES: u32 = 66;
/// One list per workspace of windows a pause shows, each with its pid (spec 0039).
type HeldLists = Vec<Vec<(isize, u32)>>;

struct Monitor {
    active: usize,
    hidden: Vec<Vec<isize>>, // one per workspace: the windows it hid, top of z-order first
    // One per workspace, like `hidden`: the windows a pause showed and their pid, which resume
    // hides again (spec 0039). Never cloaked by des-k while held.
    held: HeldLists,
    last_follow: Option<Instant>,
    last_switch: Option<(Instant, usize)>, // when, and the workspace it left
    desktop: Vec<isize>, // what show_desktop minimized, top first; any switch or send here clears it
    // Windows' name for it (DISPLAY2) while plugged in or only just gone; None once its windows
    // folded onto the main display: a remembered screen (spec 0016).
    device: Option<String>,
    area: RECT,   // its last work area: away windows' spots are relative to it
    name: String, // its friendly name
}

impl Monitor {
    fn new() -> Self {
        Self::with(settings::get().workspaces)
    }

    fn with(count: usize) -> Self {
        Monitor {
            active: 0,
            hidden: vec![Vec::new(); count],
            held: vec![Vec::new(); count],
            last_follow: None,
            last_switch: None,
            desktop: Vec::new(),
            device: None,
            area: RECT::default(),
            name: String::new(),
        }
    }

    /// Where back goes: the workspace the last switch left, if it still exists and is not the
    /// one shown (spec 0024 AC-4). Back and jump both ask this, so they never disagree.
    fn back_target(&self, count: usize) -> Option<usize> {
        self.last_switch.map(|(_, ws)| ws).filter(|&ws| ws < count && ws != self.active)
    }

    /// A present screen's, with nothing hidden yet.
    fn of(p: &Screen) -> Self {
        Monitor { device: Some(p.device.clone()), area: p.area, name: p.name.clone(), ..Self::new() }
    }

    /// Gives this screen `count` workspaces (spec 0012 AC-6), lists only. The windows of a
    /// removed workspace go to the new last one; if this screen showed a removed one, it
    /// shows the new last one and the returned windows (top first) must be shown.
    fn resize(&mut self, count: usize) -> Vec<isize> {
        // Held windows of a removed workspace are kept by the new last one, as hidden ones are
        // (spec 0039 AC-12).
        if self.held.len() > count {
            let gone: Vec<(isize, u32)> = self.held.drain(count..).flatten().collect();
            self.held[count - 1].extend(gone);
        }
        self.held.resize(count, Vec::new());
        if self.hidden.len() <= count {
            self.hidden.resize(count, Vec::new());
            return vec![];
        }
        let last = count - 1;
        let gone: Vec<isize> = self.hidden.drain(count..).flatten().collect();
        self.last_switch = self.last_switch.filter(|&(_, ws)| ws < count);
        if self.active < count {
            self.hidden[last].extend(gone);
            return vec![];
        }
        // The windows on screen stay; the ones hidden on the new last one come into view too.
        self.active = last;
        shown_changed();
        self.hidden[last].drain(..).chain(gone).collect()
    }
}

/// Where a window sits and how (spec 0016): `state` is `n` normal, `x` maximized or `m`
/// minimized. A normal window's rect is its window rect, so a snapped half keeps its snapped
/// size; a maximized or minimized one's is where it restores to. Screen coordinates.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Spot {
    pub rect: RECT,
    pub state: char,
}

impl Spot {
    fn text(&self) -> String {
        let r = self.rect;
        format!("{} {} {} {} {}", r.left, r.top, r.right - r.left, r.bottom - r.top, self.state)
    }

    /// Five fields: x y w h state. A zero size is a spot nobody knew (a line from an older des-k).
    fn parse(t: &[&str]) -> Option<Spot> {
        let [x, y, w, h, st] = t else { return None };
        let n = |v: &str| v.parse::<i32>().ok();
        let (x, y, w, h) = (n(x)?, n(y)?, n(w)?, n(h)?);
        let state = st.chars().next().filter(|c| st.len() == 1 && "nxm".contains(*c))?;
        (w > 0 || h > 0).then_some(Spot { rect: RECT { left: x, top: y, right: x + w, bottom: y + h }, state })
    }
}

/// A window open away from its home screen while that screen is unplugged (spec 0016): where
/// it goes back to when the screen returns.
#[derive(Clone, Debug, PartialEq)]
struct Away {
    home: String, // screen id
    ws: usize,
    spot: Spot,
    pinned: bool,
    pid: u32,
}

#[derive(Default)]
struct State {
    monitors: HashMap<String, Monitor>, // keyed by screen id (spec 0016), present and remembered
    logon: Option<(u64, i64)>,          // this sign in, saved as the L line
    restore: Option<Restore>,           // Some while a restore runs
    // Outlives the restore: the last window it hides needs the guard too.
    restored: HashMap<isize, Instant>, // windows restore hid, and when
    // The window the last stay send moved, and when; any switch clears it (spec 0004).
    sent: Option<(isize, Instant)>,
    // Screens with workspaces that are not plugged in now, and how many checks in a row.
    gone: HashMap<String, u32>,
    // Present screens as last read, and the ids present at the last check (spec 0016).
    present: Vec<Screen>,
    seen: HashSet<String>,
    // Windows open away from their unplugged home screen, by hwnd.
    away: HashMap<isize, Away>,
    // Each tracked window's screen id and spot, refreshed each check; frozen while that screen is missing.
    spots: HashMap<isize, (String, Spot)>,
    // Screens with away windows that are plugged in again: settled checks present, windows returned.
    back: HashMap<String, (u32, usize)>,
    // Each present screen's id and work area at the last check, and how many checks in a row
    // that held; whether the last check read spots (claims compare against them).
    layout: Vec<(String, RECT)>,
    calm: u32,
    spots_fresh: bool,
    // Windows kept in view on every workspace of their screen (spec 0014).
    pinned: HashSet<isize>,
    // Each saved window's app by (hwnd, pid), which never changes: a save opens no process twice.
    apps: HashMap<(isize, u32), String>,
    // Hidden windows that flashed since they were hidden (spec 0022); memory only.
    wants: HashSet<isize>,
    // Every window that has shown once while app rules exist, by (hwnd, pid): a rule acts only
    // the first time a window shows (spec 0027 AC-3). Memory only.
    opened: HashSet<(isize, u32)>,
    // Windows a rule just moved to another screen, by hwnd (spec 0027 AC-6).
    settle: HashMap<isize, Settle>,
    // Windows a rule hid, by hwnd, only while they stay in a hidden list: follow ignores them and
    // a shell uncloak is undone (spec 0027 AC-5, AC-17). Memory only.
    ruled: HashMap<isize, Ruled>,
    // The screen id des-k last acted on (spec 0038 AC-7): a key pressed with nothing focused acts
    // there. Memory only; read only through `target`, which checks the screen is present.
    last_acted: Option<String>,
    // Paused (spec 0039): des-k hides nothing and acts on no key, swipe or rule; `held` keeps
    // each shown window's workspace. Written only by `pause`, `resume` and `load`, always with
    // `keys::set_paused`, so the hook and the tray read it without this lock.
    paused: bool,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);
/// `id`'s Monitor, made for a present screen if it has none yet.
fn mon_mut<'a>(s: &'a mut State, id: &str) -> &'a mut Monitor {
    let p = s.present.iter().find(|p| p.id == id);
    s.monitors.entry(id.to_string()).or_insert_with(|| p.map_or_else(Monitor::new, Monitor::of))
}

/// The screen id of a present screen's device name. A device it does not know yet (just
/// plugged in) reads the screens again; failing that, the device name serves.
fn id_of(s: &mut State, device: &str) -> String {
    if !s.present.iter().any(|p| p.device == device) {
        s.present = screen_ids();
        shown_changed();
    }
    s.present.iter().find(|p| p.device == device).map_or_else(|| device.to_string(), |p| p.id.clone())
}

/// A present screen's monitor, by id.
fn mon_of(s: &State, id: &str) -> Option<HMONITOR> {
    s.present.iter().find(|p| p.id == id).and_then(|p| monitor_named(&p.device))
}

/// The name des-k shows for a screen (spec 0017): your rename as typed (spec 0023 AC-11), else
/// Windows' friendly name, with " #1", " #2" left to right when two present screens not renamed
/// share it. A screen that is not present keeps its last name. Never for a log line: the log
/// names a screen by Windows' name and id, so no rename reaches it (AC-15).
fn label(s: &State, id: &str) -> String {
    label_by(&settings::get(), s, id)
}

fn label_by(set: &settings::Settings, s: &State, id: &str) -> String {
    if let Some(rename) = set.screen_rename(id) {
        return rename.to_string();
    }
    let Some(p) = s.present.iter().find(|p| p.id == id) else {
        return s.monitors.get(id).map_or_else(|| id.to_string(), |m| m.name.clone());
    };
    let name = windows_name(p);
    // present is sorted left to right.
    let twins: Vec<&Screen> = s.present.iter().filter(|q| q.name == p.name && set.screen_rename(&q.id).is_none()).collect();
    match twins.iter().position(|q| q.id == p.id) {
        Some(i) if twins.len() > 1 => format!("{name} #{}", i + 1),
        _ => name,
    }
}

fn state() -> MutexGuard<'static, State> {
    // A panic mid-switch must not stop restore_all from running.
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests;
