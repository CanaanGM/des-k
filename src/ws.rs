// Per-monitor workspaces. A workspace only remembers the windows it HID; the active
// workspace is simply "whatever is visible on that monitor" at switch time, so new
// windows and windows dragged between monitors need no tracking.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::{
        Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute},
        Gdi::{
            EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW, MonitorFromPoint, MonitorFromRect,
            MonitorFromWindow,
        },
    },
    System::Com::{CLSCTX_ALL, CoCreateInstance},
    UI::{
        Shell::{IVirtualDesktopManager, VirtualDesktopManager},
        WindowsAndMessaging::*,
    },
};
use windows::core::BOOL;

use crate::cloak::Shell;
use crate::indicator::{self, Card, Line as Says};
use crate::restore::{self, Entry, Fp, Pick};
use crate::settings;

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

struct Monitor {
    active: usize,
    hidden: Vec<Vec<isize>>, // one per workspace: the windows it hid, top of z-order first
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
            last_follow: None,
            last_switch: None,
            desktop: Vec::new(),
            device: None,
            area: RECT::default(),
            name: String::new(),
        }
    }

    /// A present screen's, with nothing hidden yet.
    fn of(p: &Screen) -> Self {
        Monitor { device: Some(p.device.clone()), area: p.area, name: p.name.clone(), ..Self::new() }
    }

    /// Gives this screen `count` workspaces (spec 0012 AC-6), lists only. The windows of a
    /// removed workspace go to the new last one; if this screen showed a removed one, it
    /// shows the new last one and the returned windows (top first) must be shown.
    fn resize(&mut self, count: usize) -> Vec<isize> {
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
        self.hidden[last].drain(..).chain(gone).collect()
    }
}

/// A present screen as des-k knows it (spec 0016): Windows' device name, the monitor's own id,
/// its friendly name, its work area, and whether Windows marks it as the main display.
#[derive(Clone, Debug, PartialEq)]
struct Screen {
    device: String,
    id: String,
    name: String,
    area: RECT,
    full: RECT,
    primary: bool,
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

/// A line for the indicator on screen `id` (`device`), showing `active`: made under the lock,
/// shown after it. `line` is the card's (spec 0017 AC-IND-4), `text` the tray balloon's.
struct Note {
    device: String,
    id: String,
    active: usize,
    line: Says,
    text: String,
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
    // Screens with away windows that are plugged in again: checks present in a row, windows returned.
    back: HashMap<String, (u32, usize)>,
    // Windows kept in view on every workspace of their screen (spec 0014).
    pinned: HashSet<isize>,
    // Each saved window's app by (hwnd, pid), which never changes: a save opens no process twice.
    apps: HashMap<(isize, u32), String>,
}

/// Matching reopened windows to saved entries after a new sign in (spec 0003).
struct Restore {
    saved: Vec<Entry>,   // as loaded, never changed
    pending: Vec<usize>, // indexes into saved not used yet
    resolved: HashSet<(isize, u32)>, // windows restore is finished with
    unclear_logged: HashSet<(isize, u32)>,
    cache: HashMap<(isize, u32), (String, String)>, // app and class; only titles change
    deadline: Instant,
    hidden: usize,
    kept: usize,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Default::default);
// When the PC last woke (spec 0016 AC-18). Its own lock: the indicator hears the wake inside
// any call that pumps sent messages, which can run under the ws lock.
static WOKE: Mutex<Option<Instant>> = Mutex::new(None);

/// The PC woke from sleep: screens get longer to come back before their windows fold.
pub fn woke() {
    *WOKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

fn gone_checks() -> u32 {
    let recent = WOKE.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < WAKE_GRACE);
    if recent { WAKE_GONE_CHECKS } else { GONE_CHECKS }
}

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
    }
    s.present.iter().find(|p| p.device == device).map_or_else(|| device.to_string(), |p| p.id.clone())
}

/// A present screen's monitor, by id.
fn mon_of(s: &State, id: &str) -> Option<HMONITOR> {
    s.present.iter().find(|p| p.id == id).and_then(|p| monitor_named(&p.device))
}

/// The name des-k shows for a screen (spec 0017): Windows' friendly name, with " #1", " #2" left
/// to right when two present screens share it. A screen that is not present keeps its last name.
fn label(s: &State, id: &str) -> String {
    let Some(p) = s.present.iter().find(|p| p.id == id) else {
        return s.monitors.get(id).map_or_else(|| id.to_string(), |m| m.name.clone());
    };
    let name = if p.name.is_empty() { p.device.trim_start_matches(r"\\.\").to_string() } else { p.name.clone() };
    // present is sorted left to right.
    let twins: Vec<&Screen> = s.present.iter().filter(|q| q.name == p.name).collect();
    match twins.iter().position(|q| q.id == p.id) {
        Some(i) if twins.len() > 1 => format!("{name} #{}", i + 1),
        _ => name,
    }
}

/// The indicator's card for screen `id`: named only when two or more screens are plugged in,
/// and carrying the id so the indicator finds that screen's own place (spec 0020 AC-9).
fn card(s: &State, id: &str, active: usize, count: usize) -> Card {
    Card { screen_id: id.to_string(), ..Card::new(active, count, (s.present.len() > 1).then(|| label(s, id))) }
}

/// The indicator on `mon` as it is now: the settings page's Show (spec 0012 AC-19).
pub fn show_indicator(mon: HMONITOR) {
    let count = settings::get().workspaces;
    let c = {
        let mut s = state();
        let id = id_of(&mut s, &device_name(mon));
        let active = mon_mut(&mut s, &id).active;
        card(&s, &id, active, count)
    };
    indicator::show(mon, c);
}

/// You moved an away window yourself (spec 0016 AC-6): it stays where you put it.
fn claim(s: &mut State, raw: isize, why: &str) {
    if let Some(a) = s.away.remove(&raw) {
        let name = s.monitors.get(&a.home).map_or(a.home.clone(), |m| m.name.clone());
        info!("{} stays where it is when {name} returns: {why}", who(HWND(raw as *mut _)));
    }
}

fn words(n: usize) -> String {
    if n == 1 { "1 window".into() } else { format!("{n} windows") }
}

fn state() -> MutexGuard<'static, State> {
    // A panic mid-switch must not stop restore_all from running.
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// %LOCALAPPDATA%\des-k, worked out once: the 1 s settings check asks for it.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or(".".into()));
        let dir = base.join("des-k");
        // The admin des-k writes here, and anything running as you could make this folder a
        // junction to one only an admin may write: drop the link, never follow it.
        if is_link(&dir) {
            warn!("the des-k data folder is a link, not a folder: removed, a real folder takes its place");
            let _ = std::fs::remove_dir(&dir);
        }
        // Before the rename it was pdesk: carry its settings, layout and saved gestures over once.
        let old = base.join("pdesk");
        if !dir.exists() && old.is_dir() && !is_link(&old) && let Err(e) = std::fs::rename(&old, &dir) {
            warn!("could not move the old pdesk data folder to des-k: {e}");
        }
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
    .clone()
}

fn is_link(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
}

/// A brand new file at `path`, for the data folder. Whatever was there goes first, and the
/// create fails rather than open what took its place: so the admin des-k never writes through
/// a link someone put there to a file only an admin may write.
pub fn create_fresh(path: &Path) -> std::io::Result<std::fs::File> {
    let _ = std::fs::remove_file(path);
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

/// `text` as the whole of a brand new file, by create_fresh.
pub fn write_new(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    create_fresh(path)?.write_all(text.as_bytes())
}

/// Appends to `path`, unless it is a link: then it starts a new file there (create_fresh).
pub fn append_to(path: &Path) -> std::io::Result<std::fs::File> {
    if is_link(path) {
        return create_fresh(path);
    }
    std::fs::OpenOptions::new().append(true).create(true).open(path)
}

fn state_file() -> PathBuf {
    data_dir().join("state.txt")
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Persist the layout so a restart, hard kill, or reboot keeps it. Lines (spec 0016 for ids,
/// spots and A lines; a spot is `x y w h state`):
/// `L <logon id> <logon time>`, `M <screen id> <active> <left top right bottom>` TAB name,
/// `W <screen id> <workspace> <hwnd> <pid> <spot>` TAB app TAB class TAB title (a hidden window),
/// `P <screen id> <hwnd> <pid> <spot>` TAB fingerprint (a pinned one, spec 0014),
/// `V <screen id> <spot>` TAB fingerprint (a visible one), and
/// `A <home id> <workspace> <hwnd> <pid> <spot> <pinned>` TAB fingerprint (an away one).
/// While a restore runs, its pending entries are written too (hwnd 0), so another restart
/// still has them.
fn save(s: &mut State) {
    save_lines(s, true);
}

/// Without fingerprints: only L, M and A lines (an exit that showed everything). A lines stay,
/// so windows still go home to a screen that returns after a quit (spec 0016 AC-8).
fn save_lines(s: &mut State, fingerprints: bool) {
    let mut out = String::new();
    if let Some((id, time)) = s.logon {
        out += &format!("L {id:x} {time}\n");
    }
    let line = |f: &Fp| format!("\t{}\t{}\t{}\n", f.app, f.class, f.title);
    // Only the apps of windows still listed carry over, so the cache never outgrows them.
    let (mut old, mut apps) = (std::mem::take(&mut s.apps), HashMap::new());
    let mut fp = |h: HWND, pid: u32| {
        let key = (h.0 as isize, pid);
        let app = old.remove(&key).unwrap_or_else(|| restore::app(h, pid));
        // A Store window's app id may not be readable yet: ask again next save.
        if !app.is_empty() && !app.ends_with("\\applicationframehost.exe") {
            apps.insert(key, app.clone());
        }
        line(&Fp { app, class: restore::clean(&class_name(h)), title: restore::clean(&title(h)) })
    };
    s.pinned.retain(|&raw| unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool());
    // The spot of the last check: a hidden window of a screen just unplugged keeps its old one.
    let spot = |raw: isize| s.spots.get(&raw).map_or_else(|| spot_of(HWND(raw as *mut _)), |x| x.1).text();
    for (id, m) in &s.monitors {
        let a = m.area;
        out += &format!("M {id} {} {} {} {} {}\t{}\n", m.active, a.left, a.top, a.right, a.bottom, restore::clean(&m.name));
        if !fingerprints {
            continue;
        }
        for (ws, list) in m.hidden.iter().enumerate() {
            for &h in list {
                let hwnd = HWND(h as *mut _);
                let pid = pid_of(hwnd);
                out += &format!("W {id} {ws} {h} {pid} {}{}", spot(h), fp(hwnd, pid));
            }
        }
        if let Some(mon) = mon_of(s, id) {
            // An away window's A line says where it belongs.
            for h in app_windows_on(mon).into_iter().filter(|h| !s.away.contains_key(&(h.0 as isize))) {
                let (raw, pid) = (h.0 as isize, pid_of(h));
                if s.pinned.contains(&raw) {
                    out += &format!("P {id} {raw} {pid} {}{}", spot(raw), fp(h, pid));
                } else {
                    out += &format!("V {id} {}{}", spot(raw), fp(h, pid));
                }
            }
        }
    }
    for (&raw, a) in &s.away {
        out += &format!("A {} {} {raw} {} {} {}{}", a.home, a.ws, a.pid, a.spot.text(), a.pinned as u8, fp(HWND(raw as *mut _), a.pid));
    }
    if let Some(r) = s.restore.as_ref().filter(|_| fingerprints) {
        for e in r.pending.iter().map(|&i| &r.saved[i]) {
            // A line from an older des-k had no spot: a zero size reads back as none.
            let sp = e.spot.unwrap_or(Spot { rect: RECT::default(), state: 'n' }).text();
            match (e.away, e.ws, e.pin) {
                (true, Some(ws), pin) => out += &format!("A {} {ws} 0 0 {sp} {}{}", e.dev, pin as u8, line(&e.fp)),
                (_, Some(ws), _) => out += &format!("W {} {ws} 0 0 {sp}{}", e.dev, line(&e.fp)),
                (_, None, true) => out += &format!("P {} 0 0 {sp}{}", e.dev, line(&e.fp)),
                (_, None, false) => out += &format!("V {} {sp}{}", e.dev, line(&e.fp)),
            }
        }
    }
    s.apps = apps;
    write_state(out);
}

/// One state.txt line, read. Lines from an older des-k name Windows' device (DISPLAY2) and
/// carry no spots: `upgrade` turns such a name into the id of the present screen it names, or
/// None (spec 0016 AC-15). Their M, P and V lines then go; a W line keeps the device name, so
/// load shows that window rather than lose it.
#[derive(Debug, PartialEq)]
enum Line {
    L(u64, i64),
    M { id: String, active: usize, area: RECT, name: String },
    W { id: String, ws: usize, raw: isize, pid: u32, spot: Option<Spot>, fp: Option<Fp> },
    P { id: String, raw: isize, pid: u32, spot: Option<Spot>, fp: Option<Fp> },
    V { id: String, spot: Option<Spot>, fp: Option<Fp> },
    A { raw: isize, away: Away, fp: Option<Fp> },
}

fn parse_state(text: &str, upgrade: impl Fn(&str) -> Option<String>) -> Vec<Line> {
    text.lines().filter_map(|line| parse_line(line, &upgrade)).collect()
}

fn parse_line(line: &str, upgrade: &dyn Fn(&str) -> Option<String>) -> Option<Line> {
    let (head, tail) = line.split_once('\t').unwrap_or((line, ""));
    // The fingerprint follows the first tab; a line without one is from an older des-k.
    let fp = match tail.splitn(3, '\t').collect::<Vec<_>>().as_slice() {
        [app, class, title] => Some(Fp { app: app.to_string(), class: class.to_string(), title: title.to_string() }),
        _ => None,
    };
    let n = |v: &str| v.parse::<i64>().ok();
    Some(match head.split(' ').collect::<Vec<_>>().as_slice() {
        ["L", id, time] => Line::L(u64::from_str_radix(id, 16).ok()?, time.parse().ok()?),
        ["M", id, active, l, t, r, b] => {
            let area = RECT { left: n(l)? as i32, top: n(t)? as i32, right: n(r)? as i32, bottom: n(b)? as i32 };
            Line::M { id: id.to_string(), active: active.parse().ok()?, area, name: tail.to_string() }
        }
        ["M", dev, active] => Line::M { id: upgrade(dev)?, active: active.parse().ok()?, area: RECT::default(), name: String::new() },
        ["W", id, ws, raw, pid, spot @ ..] if spot.len() == 5 || spot.is_empty() => {
            let id = if spot.is_empty() { upgrade(id).unwrap_or(id.to_string()) } else { id.to_string() };
            Line::W { id, ws: ws.parse().ok()?, raw: raw.parse().ok()?, pid: pid.parse().ok()?, spot: Spot::parse(spot), fp }
        }
        ["P", id, raw, pid, spot @ ..] if spot.len() == 5 || spot.is_empty() => {
            let id = if spot.is_empty() { upgrade(id)? } else { id.to_string() };
            Line::P { id, raw: raw.parse().ok()?, pid: pid.parse().ok()?, spot: Spot::parse(spot), fp }
        }
        ["V", id, spot @ ..] if spot.len() == 5 || spot.is_empty() => {
            let id = if spot.is_empty() { upgrade(id)? } else { id.to_string() };
            Line::V { id, spot: Spot::parse(spot), fp }
        }
        ["A", home, ws, raw, pid, x, y, w, h, st, pinned] => {
            let away = Away { home: home.to_string(), ws: ws.parse().ok()?, spot: Spot::parse(&[x, y, w, h, st])?, pinned: *pinned == "1", pid: pid.parse().ok()? };
            Line::A { raw: raw.parse().ok()?, away, fp }
        }
        _ => return None,
    })
}

fn write_state(text: String) {
    // A crash or power loss mid write must never leave half a file.
    let tmp = data_dir().join("state.tmp");
    if let Err(e) = write_new(&tmp, &text).and_then(|()| std::fs::rename(&tmp, state_file())) {
        error!("state.txt not saved: {e}");
    }
}

/// `des-k --quit` with no des-k running (after a crash or kill, or from the uninstaller):
/// show every window state.txt says des-k hid, if it is still that window and still
/// cloaked on this Windows desktop (the same check as `load`). Then the file is left as a
/// quit leaves it, plus the W lines of any window it could not show.
pub fn rescue() {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return info!("no saved layout, nothing to show") };
    let vdm = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok();
    let Some(shell) = shell_for_exit() else { return };
    let (out, shown, left) = rescued(&text, |raw, pid| {
        let h = HWND(raw as *mut _);
        let on_current = vdm.as_ref().is_some_and(|v| unsafe { v.IsWindowOnCurrentVirtualDesktop(h) }.is_ok_and(|b| b.as_bool()));
        if !(unsafe { IsWindow(Some(h)) }.as_bool() && pid_of(h) == pid && cloaked(h) == 2 && on_current) {
            return None; // closed, reused handle, or not hidden by des-k
        }
        Some(shell.hide(h, false).inspect_err(|e| error!("could not show saved window {raw:#x}: {e}")).is_ok())
    });
    info!("des-k was not running: {shown} hidden window(s) shown, {left} could not be");
    write_state(out);
}

/// rescue's pass over state.txt, apart so it can be tested. `show` gets each saved hidden
/// window (hwnd, pid): None if it is not des-k's any more, else whether it is shown now.
/// Returns the new file, as a quit leaves it plus the W lines of windows still hidden, and
/// how many were shown and left. A lines stay, as a quit keeps them.
fn rescued(text: &str, mut show: impl FnMut(isize, u32) -> Option<bool>) -> (String, usize, usize) {
    let (mut out, mut shown, mut left) = (String::new(), 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once('\t').map_or((line, String::new()), |(h, t)| (h, format!("\t{t}")));
        match head.split(' ').collect::<Vec<_>>().as_slice() {
            ["L", ..] => out += &format!("{line}\n"),
            ["M", id, _, rest @ ..] => out += &format!("M {id} 0{}{tail}\n", rest.iter().map(|v| format!(" {v}")).collect::<String>()),
            // An away window's line stays whatever happens: its screen may still return (spec 0016 AC-8).
            ["A", _, _, raw, pid, ..] => {
                out += &format!("{line}\n");
                if let (Ok(raw), Ok(pid)) = (raw.parse::<isize>(), pid.parse::<u32>())
                    && raw != 0
                {
                    match show(raw, pid) {
                        Some(true) => shown += 1,
                        Some(false) => left += 1,
                        None => {}
                    }
                }
            }
            ["W", _, _, raw, pid, ..] => {
                let (Ok(raw), Ok(pid)) = (raw.parse::<isize>(), pid.parse::<u32>()) else { continue };
                // hwnd 0: a restore's entry still waiting for its window, nothing to show.
                match (raw != 0).then(|| show(raw, pid)).flatten() {
                    Some(true) => shown += 1,
                    Some(false) => {
                        left += 1;
                        out += &format!("{line}\n");
                    }
                    None => {}
                }
            }
            _ => {} // V and P lines go, as on a quit: nothing for a restore to match
        }
    }
    (out, shown, left)
}

pub(crate) fn device_name(m: HMONITOR) -> String {
    let mut mi = MONITORINFOEXW::default();
    mi.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    unsafe { let _ = GetMonitorInfoW(m, &mut mi as *mut _ as *mut MONITORINFO); }
    let n = mi.szDevice.iter().position(|&c| c == 0).unwrap_or(0);
    String::from_utf16_lossy(&mi.szDevice[..n]).replace(r"\\.\", "")
}

/// Every present screen, sorted left to right then top down (spec 0016 AC-1). A monitor's id
/// is read once per device path: the EDID read is a registry look up per screen.
fn screen_ids() -> Vec<Screen> {
    unsafe extern "system" fn push(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        unsafe { (*(out.0 as *mut Vec<HMONITOR>)).push(m) };
        BOOL(1)
    }
    // By device path, then the last id each device name had: an EDID read that fails for a
    // moment must not make a known screen look new.
    type Ids = HashMap<String, String>;
    static KNOWN: LazyLock<Mutex<(Ids, Ids)>> = LazyLock::new(Default::default);
    let mut all = Vec::new();
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut all as *mut _ as isize)); }
    let targets = targets();
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<Screen> = all
        .into_iter()
        .map(|m| {
            let device = device_name(m);
            let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
            unsafe { let _ = GetMonitorInfoW(m, &mut mi); }
            let t = targets.iter().find(|t| t.device == device);
            let read = match t {
                Some(t) if t.internal => Some("internal".to_string()),
                Some(t) => known.0.get(&t.path).cloned().or_else(|| {
                    let id = clean_id(&restore::edid_id(&t.path)?);
                    known.0.insert(t.path.clone(), id.clone());
                    Some(id)
                }),
                None => None,
            };
            let id = read.or_else(|| known.1.get(&device).cloned()).unwrap_or_else(|| clean_id(&device));
            known.1.insert(device.clone(), id.clone());
            let name = t.map_or(String::new(), |t| if t.internal { "Built-in display".into() } else { t.friendly.clone() });
            Screen { device, id, name, area: mi.rcWork, full: mi.rcMonitor, primary: mi.dwFlags & MONITORINFOF_PRIMARY != 0 }
        })
        .collect();
    out.sort_by_key(|p| (p.area.left, p.area.top));
    number_twins(&mut out);
    for (i, p) in out.iter_mut().enumerate() {
        if p.name.is_empty() {
            p.name = format!("Screen {}", i + 1);
        }
    }
    out
}

/// A screen id is one token in state.txt: anything an EDID could hold besides these becomes `_`.
fn clean_id(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.#".contains(c) { c } else { '_' }).collect()
}

/// Two present monitors with one id (no serial) get `#1`, `#2` in `list`'s order, left to
/// right (spec 0016 AC-1).
fn number_twins(list: &mut [Screen]) {
    let ids: Vec<String> = list.iter().map(|p| p.id.clone()).collect();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for p in list.iter_mut() {
        if ids.iter().filter(|i| **i == p.id).count() > 1 {
            let k = seen.entry(p.id.clone()).or_default();
            *k += 1;
            p.id = format!("{}#{k}", p.id);
        }
    }
}

fn log_screens(list: &[Screen]) {
    for p in list {
        info!("screen {}: {} \"{}\"{}", p.id, p.device, p.name, if p.primary { ", main display" } else { "" });
    }
}

fn monitor_named(name: &str) -> Option<HMONITOR> {
    unsafe extern "system" fn push(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        unsafe { (*(out.0 as *mut Vec<HMONITOR>)).push(m) };
        BOOL(1)
    }
    let mut all = Vec::new();
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut all as *mut _ as isize)); }
    all.into_iter().find(|&m| device_name(m) == name)
}

pub(crate) fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    match n {
        1.. => String::from_utf16_lossy(&buf[..n as usize]),
        _ => "(no title)".into(),
    }
}

fn cloaked(hwnd: HWND) -> u32 {
    let mut v = 0u32;
    unsafe { let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut v as *mut _ as _, 4); }
    v
}

pub(crate) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// A window as a log line names it: its exe file name, never its title, which can be an email
/// subject or a document name (spec 0019). `window 0x…` when its process can't be read.
pub(crate) fn who(hwnd: HWND) -> String {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    crate::restore::exe_path(pid)
        .and_then(|p| Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("window {:#X}", hwnd.0 as usize))
}

fn app_windows_on(mon: HMONITOR) -> Vec<HWND> {
    app_windows().into_iter().filter(|&h| unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) } == mon).collect()
}

/// Visible, uncloaked app windows on every screen (Alt+Tab-like filter from spikes/listwin), top of z-order first.
fn app_windows() -> Vec<HWND> {
    unsafe extern "system" fn push(hwnd: HWND, out: LPARAM) -> BOOL {
        unsafe { (*(out.0 as *mut Vec<HWND>)).push(hwnd) };
        BOOL(1)
    }
    let mut all = Vec::new();
    unsafe { let _ = EnumWindows(Some(push), LPARAM(&mut all as *mut _ as isize)); }
    let shell = unsafe { GetShellWindow() };
    let me = std::process::id();
    all.into_iter()
        .filter(|&h| unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
            IsWindowVisible(h).as_bool()
                && GetWindow(h, GW_OWNER).map_or(true, |o| o.is_invalid())
                && ex & WS_EX_TOOLWINDOW.0 == 0
                && GetWindowTextLengthW(h) > 0
                && cloaked(h) == 0
                // Never hide the desktop or taskbars.
                && h != shell
                && !matches!(class_name(h).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
                // Never des-k's own settings page or welcome screen: a hidden one is one the
                // tray's Settings would focus where you cannot see it.
                && pid_of(h) != me
        })
        .collect()
}

pub(crate) fn under_mouse() -> (HMONITOR, String) {
    let mut pt = POINT::default();
    unsafe { let _ = GetCursorPos(&mut pt); }
    let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    (mon, device_name(mon))
}

/// Switch the workspace of the monitor under the mouse by `dir` (-1 / +1), wrapping like a send.
pub fn switch(dir: isize) {
    let (mon, dev) = under_mouse();
    let count = settings::get().workspaces;
    let mut s = state();
    let name = id_of(&mut s, &dev);
    let active = mon_mut(&mut s, &name).active;
    let target = (active as isize + dir).rem_euclid(count as isize) as usize;
    if target == active {
        indicator::show(mon, card(&s, &name, active, count)); // nowhere to go: still show where we are
        return;
    }
    switch_to(&mut s, mon, &name, target, None, None);
}

/// Show workspace `index` (0 based) on the monitor under the mouse. Past the count, or
/// already shown: only the indicator, so you see where you are (spec 0005).
pub fn jump(index: usize) {
    let (mon, dev) = under_mouse();
    let count = settings::get().workspaces;
    let mut s = state();
    let name = id_of(&mut s, &dev);
    let active = mon_mut(&mut s, &name).active;
    if index >= count {
        info!("jump: no workspace {} ({count} workspaces)", index + 1);
    }
    if index >= count {
        return indicator::show(mon, card(&s, &name, active, count).with(Says::NoWorkspace { n: index + 1 }));
    }
    if index == active {
        return indicator::show(mon, card(&s, &name, active, count));
    }
    switch_to(&mut s, mon, &name, index, None, None);
}

/// Jump a hidden window's screen to its workspace when the window comes to the front,
/// so keyboard focus never sits in a window you cannot see (spec 0002).
pub fn follow(hwnd: HWND) {
    // des-k's own switch may have moved focus on since the event fired.
    if unsafe { GetForegroundWindow() } != hwnd {
        return;
    }
    let mut s = state();
    // An owned dialog is never hidden itself, so it follows its owner.
    let owner = unsafe { GetAncestor(hwnd, GA_ROOTOWNER) };
    let found = [hwnd, owner].into_iter().find_map(|h| {
        s.monitors.iter().find_map(|(dev, m)| Some((h, dev.clone(), m.hidden.iter().position(|l| l.contains(&(h.0 as isize)))?)))
    });
    let Some((h, name, ws)) = found else { return };
    if cloaked(h) != 2 {
        return;
    }
    let app = who(hwnd);
    if [hwnd, owner].iter().any(|w| s.restored.get(&(w.0 as isize)).is_some_and(|t| t.elapsed() < RESTORED_IGNORE)) {
        return info!("follow: restore just hid {app}, ignored");
    }
    // Windows can hand focus back to a window a stay send just hid.
    let just_sent = |&(sent, t): &(isize, Instant)| t.elapsed() < Duration::from_millis(FOLLOW_COOLDOWN_MS) && [hwnd, owner].iter().any(|w| w.0 as isize == sent);
    if s.sent.as_ref().is_some_and(just_sent) {
        return info!("follow: just sent {app}, ignored");
    }
    let Some(mon) = mon_of(&s, &name) else {
        return warn!("follow: {name} not found, ignored {app}");
    };
    let m = &s.monitors[&name];
    let recent = |t: Instant| t.elapsed() < Duration::from_millis(FOLLOW_COOLDOWN_MS);
    // Windows can hand focus to a window mid cloak; going straight back would undo your switch.
    if m.last_switch.is_some_and(|(t, left)| recent(t) && left == ws) {
        return info!("follow: {name} just left workspace {}, ignored {app}", ws + 1);
    }
    if m.last_follow.is_some_and(recent) {
        return info!("follow: {name} cooling down, ignored {app}");
    }
    if switch_to(&mut s, mon, &name, ws, Some(hwnd), None) {
        s.monitors.get_mut(&name).expect("switch_to keeps the monitor").last_follow = Some(Instant::now());
        info!("follow: {name} -> workspace {}/{} for {app}", ws + 1, settings::get().workspaces);
    }
}

/// Show workspace `target` (already valid) on one monitor. `focus` is the window to leave
/// in front; `None` focuses the first shown window, but only if we hid the focused one or the
/// desktop has focus.
/// `keep` is a visible window that comes along instead of being hidden (a follow send).
/// Returns whether it switched.
fn switch_to(s: &mut State, mon: HMONITOR, name: &str, target: usize, focus: Option<HWND>, keep: Option<HWND>) -> bool {
    let count = settings::get().workspaces;
    mon_mut(s, name);
    let m = s.monitors.get_mut(name).expect("made above");
    if target == m.active {
        return false;
    }
    let shell = match Shell::new() {
        Ok(sh) => sh,
        Err(e) => {
            warn!("shell unavailable: {e}");
            return false;
        }
    };

    let fg = unsafe { GetForegroundWindow() };
    // Leaving an empty workspace left focus on the desktop; nobody's focus to keep there.
    let fg_desktop = fg.is_invalid() || fg == unsafe { GetShellWindow() } || matches!(class_name(fg).as_str(), "Progman" | "WorkerW");
    let mut fg_hidden = false;
    // Hide bottom-up: hiding the top window first briefly uncovers the ones behind it.
    let mut hid = Vec::new();
    // A pinned window stays in view on every workspace (spec 0014).
    for h in app_windows_on(mon).into_iter().rev().filter(|&h| Some(h) != keep && !s.pinned.contains(&(h.0 as isize))) {
        match shell.hide(h, true) {
            Ok(()) => {
                hid.push(h.0 as isize);
                fg_hidden |= h == fg;
            }
            Err(e) => {
                // The cloak can work and only the taskbar step fail: undo it, or nothing tracks it.
                if cloaked(h) != 0 {
                    let _ = shell.hide(h, false);
                }
                warn!("cloak {:?} failed: {e}", h.0);
            }
        }
    }
    hid.reverse(); // keep top-first, so showing them is top-down (no flash either)
    m.hidden[m.active].extend(hid);

    let (shown, stuck) = show_all(std::mem::take(&mut m.hidden[target]), |raw| uncloak(&shell, raw));
    // Still hidden, so still tracked: the next switch back here, quit or panic tries again.
    m.hidden[target] = stuck;
    m.last_switch = Some((Instant::now(), m.active));
    m.active = target;
    // Those windows now belong to the workspace just left; show_desktop must not bring them back here.
    m.desktop.clear();

    match focus {
        // Normally it already has focus: it is what came to the front.
        Some(h) if unsafe { GetForegroundWindow() } != h => unsafe {
            let _ = SetForegroundWindow(h);
            if GetForegroundWindow() != h {
                warn!("follow: could not focus {}", who(h));
            }
        },
        Some(_) => {}
        // Don't leave keyboard focus in a window we just hid.
        None if fg_hidden || fg_desktop => {
            let next = shown.first().map_or(unsafe { GetShellWindow() }, |&raw| HWND(raw as *mut _));
            unsafe { let _ = SetForegroundWindow(next); }
        }
        None => {}
    }
    info!("{name}: workspace {}/{count} ({} shown)", target + 1, shown.len());
    indicator::show(mon, card(s, name, target, count));
    s.sent = None;
    save(s);
    true
}

/// Shows the windows of `list`, top first. `show` gets each one: None if it is closed, else
/// whether it is in view now. Returns those shown and those still hidden: a window that may
/// still be cloaked is never dropped, so a later switch, quit or panic tries again.
fn show_all(list: Vec<isize>, mut show: impl FnMut(isize) -> Option<bool>) -> (Vec<isize>, Vec<isize>) {
    let (mut shown, mut stuck) = (Vec::new(), Vec::new());
    for raw in list {
        match show(raw) {
            Some(true) => shown.push(raw),
            Some(false) => stuck.push(raw),
            None => {} // closed while hidden
        }
    }
    (shown, stuck)
}

/// show_all's `show` for one window des-k hid.
fn uncloak(shell: &Shell, raw: isize) -> Option<bool> {
    let h = HWND(raw as *mut _);
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        return None;
    }
    Some(shell.hide(h, false).inspect_err(|e| error!("uncloak {raw:#x} failed: {e}, kept to try again")).is_ok())
}

/// Pin or unpin the focused window (spec 0014). A pinned window stays in view on every
/// workspace of the screen it is on, wherever you drag it; sending it anywhere unpins it.
/// The indicator lights every workspace while it is pinned.
pub fn pin() {
    // A dialog is never hidden itself, so its owner is what a switch would hide.
    let h = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) };
    if !app_windows().contains(&h) {
        return info!("pin: nothing to pin ({}, {})", who(h), class_name(h));
    }
    let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
    let count = settings::get().workspaces;
    let mut s = state();
    let name = id_of(&mut s, &device_name(mon));
    let raw = h.0 as isize;
    let pinned = !s.pinned.remove(&raw);
    if pinned {
        s.pinned.insert(raw);
    }
    claim(&mut s, raw, if pinned { "you pinned it" } else { "you unpinned it" });
    let active = mon_mut(&mut s, &name).active;
    let how = if pinned { "pinned to every workspace".to_string() } else { format!("unpinned, stays on workspace {}", active + 1) };
    info!("pin: {} on {name} {how}", who(h));
    indicator::show(mon, card(&s, &name, active, count).with(if pinned { Says::Pinned } else { Says::Unpinned }));
    save(&mut s);
}

/// Where a send goes: one step over (-1 / +1, wrapping), or workspace N (0 based).
pub enum To {
    Step(isize),
    Index(usize),
}

/// Move the focused window to another workspace of its own screen, never the one under
/// the mouse. `follow` switches the screen along with it; otherwise it goes alone and you
/// stay (specs 0004 and 0005).
pub fn send(to: To, follow: bool) {
    let count = settings::get().workspaces;
    let mut s = state();
    let recent = |t: Instant| t.elapsed() < Duration::from_millis(FOLLOW_COOLDOWN_MS);
    // A quick second stay press moves the window the last one sent, not the one that got focus.
    let repeat = s
        .sent
        .filter(|&(raw, t)| !follow && recent(t) && unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool())
        .and_then(|(raw, _)| s.monitors.iter().find_map(|(dev, m)| Some((raw, dev.clone(), m.hidden.iter().position(|l| l.contains(&raw))?))));
    let is_repeat = repeat.is_some();
    let (h, mon, name, from) = match repeat {
        Some((raw, name, from)) => {
            let Some(mon) = mon_of(&s, &name) else {
                return warn!("send: {name} not found");
            };
            (HWND(raw as *mut _), mon, name, from)
        }
        None => {
            // A dialog is never hidden itself, so its owner goes.
            let h = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) };
            // Only what a switch would hide: never the desktop, the taskbar, or a tool window.
            if !app_windows().contains(&h) {
                return info!("send: nothing to send ({}, {})", who(h), class_name(h));
            }
            let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
            let name = id_of(&mut s, &device_name(mon));
            let from = mon_mut(&mut s, &name).active;
            (h, mon, name, from)
        }
    };
    let target = match to {
        To::Step(dir) => (from as isize + dir).rem_euclid(count as isize) as usize,
        To::Index(i) if i < count => i,
        To::Index(i) => {
            info!("send: no workspace {} ({count} workspaces)", i + 1);
            return indicator::show(mon, card(&s, &name, s.monitors[&name].active, count).with(Says::NoWorkspace { n: i + 1 }));
        }
    };
    let t = who(h);
    let unpinned = s.pinned.remove(&(h.0 as isize));
    if unpinned {
        info!("send: {t} unpinned, it goes to one workspace");
    }
    // Without this, a stay press would hide the window into the shown workspace's own list.
    if !is_repeat && target == from {
        info!("send: {t} already on {name} workspace {}/{count}", from + 1);
        return indicator::show(mon, card(&s, &name, from, count).with(Says::Already));
    }
    let line = |how: &str| format!("send: {t} {name} workspace {}/{count} -> {}/{count}, {how}", from + 1, target + 1);
    claim(&mut s, h.0 as isize, "you sent it");

    if follow {
        // Already in front, so it stays on top of the workspace it lands on.
        if switch_to(&mut s, mon, &name, target, Some(h), Some(h)) {
            info!("{}", line("follow"));
        }
        return;
    }

    let shell = || Shell::new().inspect_err(|e| warn!("shell unavailable: {e}")).ok();
    let raw = h.0 as isize;
    let m = s.monitors.get_mut(&name).expect("found or created above");
    let active = m.active;
    if !is_repeat {
        let Some(shell) = shell() else { return };
        if let Err(e) = shell.hide(h, true) {
            // The cloak can work and only the taskbar step fail: undo it, so nothing changes.
            if cloaked(h) != 0 {
                let _ = shell.hide(h, false);
            }
            return warn!("send: could not hide {t}: {e}");
        }
        // In a list straight away, so the panic hook can always show it again. Front = top.
        m.hidden[target].insert(0, raw);
        // It is cloaked now, so the filter skips it.
        let next = app_windows_on(mon).first().copied().unwrap_or(unsafe { GetShellWindow() });
        unsafe { let _ = SetForegroundWindow(next); }
    } else if target == active {
        let Some(shell) = shell() else { return };
        if let Err(e) = shell.hide(h, false) {
            return error!("send: could not show {t}: {e}");
        }
        m.hidden[from].retain(|&w| w != raw);
        unsafe { let _ = SetForegroundWindow(h); }
    } else {
        m.hidden[from].retain(|&w| w != raw);
        m.hidden[target].insert(0, raw);
    }
    m.desktop.clear();
    s.sent = Some((raw, Instant::now()));
    let said = if target == active { Says::SentBack } else { Says::Sent { to: target, unpinned } };
    indicator::show(mon, card(&s, &name, active, count).with(said));
    info!("{}", line("stay"));
    save(&mut s);
}

/// A listed window and where it lives, for the finder (spec 0007).
pub struct Place {
    pub hwnd: HWND,
    pub device: String,
    pub workspace: usize, // 0 based
    pub on_screen: bool,
    pub screen: String, // its screen's name, as `label` gives it (spec 0017)
    pub pinned: bool,
    pub unplugged: bool, // hidden on a screen that left and has not folded yet
}

/// Every listed window on screen (front to back), then every live window des-k hid, by
/// screen, workspace and saved order.
pub fn windows() -> Vec<Place> {
    windows_in(&mut state())
}

fn windows_in(s: &mut State) -> Vec<Place> {
    let mut out: Vec<Place> = app_windows()
        .into_iter()
        .map(|h| {
            let device = device_name(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) });
            let id = id_of(s, &device);
            let workspace = mon_mut(s, &id).active;
            Place { hwnd: h, device, workspace, on_screen: true, screen: label(s, &id), pinned: s.pinned.contains(&(h.0 as isize)), unplugged: false }
        })
        .collect();
    // By device name, as before screens had ids; a screen only just unplugged keeps its old one.
    let mut screens: Vec<(String, &String, &Monitor)> = s.monitors.iter().map(|(id, m)| (m.device.clone().unwrap_or_else(|| id.clone()), id, m)).collect();
    screens.sort_by(|a, b| a.0.cmp(&b.0));
    for (dev, id, m) in screens {
        let (screen, unplugged) = (label(s, id), !s.present.iter().any(|p| &p.id == id));
        for (ws, list) in m.hidden.iter().enumerate() {
            for &raw in list {
                let h = HWND(raw as *mut _);
                if unsafe { IsWindow(Some(h)) }.as_bool() {
                    out.push(Place { hwnd: h, device: dev.clone(), workspace: ws, on_screen: false, screen: screen.clone(), pinned: false, unplugged });
                }
            }
        }
    }
    out
}

/// How many screens are plugged in, and the main display's name: the finder's screen column
/// and its unplugged note (spec 0017 AC-FND-3, AC-FND-7).
pub fn present() -> (usize, String) {
    let s = state();
    let main = s.present.iter().find(|p| p.primary).map_or_else(String::new, |p| label(&s, &p.id));
    (s.present.len(), main)
}

/// One present screen as the tray menu shows it (spec 0017 AC-TRY-3).
#[derive(Clone, Debug, PartialEq)]
pub struct TrayScreen {
    pub name: String, // as the indicator names it, " #1" and " #2" for twins
    pub rect: RECT,   // the whole screen, physical pixels
    pub active: usize, // the workspace it shows, 0 based
    pub main: bool,
}

/// Every present screen, left to right, copied out so the menu paints with no lock held.
pub fn tray_screens() -> Vec<TrayScreen> {
    let full = screens(); // before the lock: it asks Windows
    let s = state();
    s.present
        .iter()
        .map(|p| TrayScreen {
            name: label(&s, &p.id),
            rect: full.iter().find(|(d, _)| *d == p.device).map_or(p.area, |(_, r)| *r),
            active: s.monitors.get(&p.id).map_or(0, |m| m.active),
            main: p.primary,
        })
        .collect()
}

/// Every present screen's device and full rectangle.
pub fn screens() -> Vec<(String, RECT)> {
    unsafe extern "system" fn push(m: HMONITOR, _: HDC, r: *mut RECT, out: LPARAM) -> BOOL {
        unsafe { (*(out.0 as *mut Vec<(String, RECT)>)).push((device_name(m), *r)) };
        BOOL(1)
    }
    let mut all = Vec::new();
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut all as *mut _ as isize)); }
    all
}

/// The workspace `device` shows (0 based), for the overview (spec 0010).
pub fn active(device: &str) -> usize {
    let mut s = state();
    let id = id_of(&mut s, device);
    s.monitors.get(&id).map_or(0, |m| m.active)
}

/// Present screens' devices, numbered as the finder and the settings page number them: left
/// to right, then top to bottom.
pub fn screen_order() -> Vec<String> {
    in_order(screens())
}

fn in_order(mut all: Vec<(String, RECT)>) -> Vec<String> {
    all.sort_by_key(|(_, r)| (r.left, r.top));
    all.into_iter().map(|(dev, _)| dev).collect()
}

/// What lowering the count to `count` would move: one line per screen with something to
/// move, as "Screen 2: 3 windows from 7, 8 move to 6"; a screen not plugged in now goes under
/// its device name (spec 0012 AC-6). The windows on screen count for the workspace shown.
fn moves(s: &State, count: usize) -> Vec<String> {
    let order = screen_order();
    let mut out = Vec::new();
    for m in moved(s, count).into_iter().filter(|m| m.windows > 0) {
        let name = m.device.and_then(|d| order.iter().position(|o| *o == d)).map_or(m.name, |i| format!("Screen {}", i + 1));
        let from: Vec<String> = m.from.iter().map(|ws| (ws + 1).to_string()).collect();
        let (win, go) = if m.windows == 1 { ("window", "moves") } else { ("windows", "move") };
        out.push(format!("{name}: {} {win} from {} {go} to {count}", m.windows, from.join(", ")));
    }
    out
}

/// What one screen would move if the count went down (spec 0017 AC-SET-3): every screen, those
/// with nothing to move too, plugged in ones first in screen order. An unplugged screen counts
/// its away windows, which are hidden on another screen, so that screen leaves them out.
#[derive(Clone, PartialEq, Debug)]
pub struct Moved {
    pub name: String, // as `label` gives it
    pub device: Option<String>,
    pub unplugged: bool,
    pub windows: usize,
    pub from: Vec<usize>, // the removed workspaces with windows, 0 based
}

fn moved(s: &State, count: usize) -> Vec<Moved> {
    let order = screen_order();
    let at = |m: &Monitor| m.device.as_ref().and_then(|d| order.iter().position(|o| o == d));
    let mut ids: Vec<&String> = s.monitors.keys().collect();
    ids.sort_by_key(|id| (at(&s.monitors[*id]).unwrap_or(usize::MAX), id.to_string()));
    let live = |raw: isize| unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool();
    let mut out = Vec::new();
    for id in ids {
        let m = &s.monitors[id];
        let unplugged = !s.present.iter().any(|p| &p.id == id);
        let mut per = vec![0; m.hidden.len().max(count)];
        for (ws, list) in m.hidden.iter().enumerate() {
            per[ws] = if ws == m.active && !unplugged {
                mon_of(s, id).map_or(0, |mon| app_windows_on(mon).iter().filter(|h| !s.away.contains_key(&(h.0 as isize))).count())
            } else {
                list.iter().filter(|&&raw| live(raw) && !s.away.contains_key(&raw)).count()
            };
        }
        for (raw, a) in &s.away {
            if &a.home == id && live(*raw) {
                if a.ws >= per.len() {
                    per.resize(a.ws + 1, 0);
                }
                per[a.ws] += 1;
            }
        }
        let from: Vec<usize> = (count..per.len()).filter(|&ws| per[ws] > 0).collect();
        let windows = from.iter().map(|&ws| per[ws]).sum();
        out.push(Moved { name: label(s, id), device: m.device.clone(), unplugged, windows, from });
    }
    out
}

/// Gives every screen `count` workspaces (spec 0012 AC-6). Raising adds empty ones. Lowering
/// moves the windows of every removed workspace onto the new last one; a screen showing a
/// removed one switches to the new last one with all of them in view, elsewhere they stay
/// hidden. Returns `removal`'s lines, as they were before the move. Main thread, from
/// `settings::apply` only, never under `state()`.
pub fn resize(count: usize) -> Vec<String> {
    let mut s = state();
    let lines = moves(&s, count);
    // An away window of a removed workspace goes home to the new last one (spec 0016 AC-16).
    for a in s.away.values_mut() {
        a.ws = a.ws.min(count - 1);
    }
    let shell = Shell::new();
    for (dev, m) in s.monitors.iter_mut() {
        let show = m.resize(count);
        if show.is_empty() {
            continue;
        }
        let Ok(shell) = &shell else {
            // Kept with the shown workspace: the next switch away and back shows them.
            error!("shell unavailable, {dev}: {} windows stay hidden", show.len());
            m.hidden[count - 1] = show;
            continue;
        };
        // Kept with the shown workspace, like above.
        let (_, stuck) = show_all(show, |raw| uncloak(shell, raw));
        m.hidden[count - 1] = stuck;
    }
    save(&mut s);
    lines
}

/// One present screen, as the settings page heads its picture (spec 0012 AC-18).
#[derive(Clone, PartialEq)]
pub struct ScreenInfo {
    pub device: String,
    pub number: usize, // 1 based, screen_order's
    pub name: String,
    pub rect: RECT,
    pub scale: u32, // percent
}

/// Every present screen in screen_order, with its own name: the monitor's, "Built-in
/// display" for a laptop panel, or the device name when Windows gives none.
pub fn screen_info() -> Vec<ScreenInfo> {
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    let names = display_names();
    let mut all = screens();
    all.sort_by_key(|(_, r)| (r.left, r.top));
    all.into_iter()
        .enumerate()
        .map(|(i, (device, rect))| {
            let (mut dx, mut dy) = (96, 96);
            if let Some(mon) = monitor_named(&device) {
                unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy); }
            }
            let name = names.get(&device).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| device.clone());
            ScreenInfo { device, number: i + 1, name, rect, scale: dx * 100 / 96 }
        })
        .collect()
}

/// One active display config path: its GDI device name (DISPLAY1), whether it is a laptop
/// panel, the monitor's friendly name and its device path.
struct Target {
    device: String,
    internal: bool,
    friendly: String,
    path: String,
}

fn targets() -> Vec<Target> {
    use windows::Win32::Devices::Display::*;
    let mut out = Vec::new();
    let (mut np, mut nm) = (0u32, 0u32);
    unsafe {
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return out;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).is_err() {
            return out;
        }
        let text = |w: &[u16]| String::from_utf16_lossy(&w[..w.iter().position(|&c| c == 0).unwrap_or(w.len())]);
        for p in &paths[..np as usize] {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME { header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: p.sourceInfo.adapterId,
                id: p.sourceInfo.id,
            }, ..Default::default() };
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let device = text(&src.viewGdiDeviceName).replace(r"\\.\", "");
            let tech = p.targetInfo.outputTechnology;
            let internal = [DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED].contains(&tech);
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME { header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: p.targetInfo.adapterId,
                id: p.targetInfo.id,
            }, ..Default::default() };
            let (friendly, path) = if DisplayConfigGetDeviceInfo(&mut tgt.header) == 0 {
                (text(&tgt.monitorFriendlyDeviceName), text(&tgt.monitorDevicePath))
            } else {
                (String::new(), String::new())
            };
            // A cloned screen shows one source on two targets: the first one names it.
            if !out.iter().any(|t: &Target| t.device == device) {
                out.push(Target { device, internal, friendly, path });
            }
        }
    }
    out
}

/// Each active display's name by GDI device name (DISPLAY1), from the display config.
fn display_names() -> HashMap<String, String> {
    targets().into_iter().map(|t| (t.device, if t.internal { "Built-in display".to_string() } else { t.friendly })).collect()
}

/// What the settings page draws of the layout, copied out so it paints with no lock held.
#[derive(Clone, PartialEq, Default)]
pub struct View {
    pub active: Vec<(String, usize)>,       // device and the workspace it shows
    pub windows: Vec<(String, usize, RECT)>, // device, workspace, where it sits
    pub names: Vec<(String, String)>,       // device and its name, as `label` gives it (spec 0017)
    pub removals: Vec<Vec<Moved>>,          // what each screen moves, one list per count asked
    pub waiting: Vec<Waiting>,              // screens not plugged in (spec 0016 AC-9)
    pub present: Vec<Present>,              // screens plugged in, left to right (spec 0020)
}

/// A screen plugged in, for the settings page's Where it shows: its id keys its own place.
#[derive(Clone, PartialEq, Debug)]
pub struct Present {
    pub id: String,
    pub device: String,
    pub name: String, // as `label` gives it, so twins read #1, #2 from the left
    pub full: RECT,
    pub work: RECT,
}

/// A remembered screen not plugged in, with its away windows per workspace (spec 0017 AC-SET-3).
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Waiting {
    pub id: String,
    pub name: String,
    pub windows: Vec<usize>, // per workspace, 0 based
}

/// The layout now, or None while a switch holds it: the page can be asked to refresh from
/// inside one (an activation change mid switch), and the lock is not reentrant.
pub fn view(counts: &[usize]) -> Option<View> {
    let mut s = match STATE.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    let windows = windows_in(&mut s).into_iter().map(|p| (p.device, p.workspace, real_rect(p.hwnd))).collect();
    let active = s.monitors.values().filter_map(|m| Some((m.device.clone()?, m.active))).collect();
    let names = s.present.iter().map(|p| (p.device.clone(), label(&s, &p.id))).collect();
    let removals = counts.iter().map(|&n| moved(&s, n)).collect();
    let present = s.present.iter().map(|p| Present { id: p.id.clone(), device: p.device.clone(), name: label(&s, &p.id), full: p.full, work: p.area }).collect();
    Some(View { active, windows, names, removals, waiting: waiting(&s), present })
}

/// Every remembered screen not plugged in, by name, with how many windows wait for it on each workspace.
fn waiting(s: &State) -> Vec<Waiting> {
    let mut out: Vec<Waiting> = s
        .monitors
        .iter()
        .filter(|(id, m)| m.device.is_none() && !s.present.iter().any(|p| &p.id == *id))
        .map(|(id, m)| {
            let mut windows = vec![0; m.hidden.len()];
            for a in s.away.values().filter(|a| &a.home == id) {
                if a.ws >= windows.len() {
                    windows.resize(a.ws + 1, 0);
                }
                windows[a.ws] += 1;
            }
            Waiting { id: id.clone(), name: m.name.clone(), windows }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    out
}

/// Forget a remembered screen (the settings page's Forget, spec 0016 AC-9): its away windows
/// stay where they are, as ordinary windows of the screens and workspaces they are on now.
pub fn forget(id: &str) {
    let mut s = state();
    if s.present.iter().any(|p| p.id == id) || s.monitors.get(id).is_none_or(|m| m.device.is_some()) {
        return warn!("forget: {id} is not a remembered screen, nothing forgotten");
    }
    let before = s.away.len();
    s.away.retain(|_, a| a.home != id);
    let name = s.monitors.remove(id).map_or(id.to_string(), |m| m.name);
    info!("{name} ({id}) forgotten: {} stay where they are", words(before - s.away.len()));
    save(&mut s);
}

/// Show workspace `ws` on `device`, exactly like a switch there: the indicator shows, and
/// focus moves on only if the focused window got hidden (spec 0010).
pub fn show_workspace(device: &str, ws: usize) {
    if ws >= settings::get().workspaces {
        return;
    }
    let Some(mon) = monitor_named(device) else { return warn!("overview: {device} not found") };
    let mut s = state();
    let id = id_of(&mut s, device);
    switch_to(&mut s, mon, &id, ws, None, None);
}

/// Where a window really sits, in screen coordinates; a minimized one where it restores to.
pub fn real_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    if unsafe { IsIconic(hwnd) }.as_bool() {
        return normal_rect(hwnd);
    }
    unsafe { let _ = GetWindowRect(hwnd, &mut r); }
    r
}

/// Where a window restores to, in screen coordinates.
fn normal_rect(hwnd: HWND) -> RECT {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { let _ = GetWindowPlacement(hwnd, &mut wp); }
    shift(wp.rcNormalPosition, 1)
}

/// rcNormalPosition is in workspace coordinates: `sign` 1 makes it screen coordinates, -1 the reverse.
fn shift(r: RECT, sign: i32) -> RECT {
    let primary = work_area(unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) });
    RECT { left: r.left + sign * primary.left, top: r.top + sign * primary.top, right: r.right + sign * primary.left, bottom: r.bottom + sign * primary.top }
}

/// A window's spot now (spec 0016). A cloaked window reads the same: cloaking does not move it.
fn spot_of(h: HWND) -> Spot {
    let (min, max) = unsafe { (IsIconic(h).as_bool(), IsZoomed(h).as_bool()) };
    let state = if min { 'm' } else if max { 'x' } else { 'n' };
    let rect = if state == 'n' { real_rect(h) } else { normal_rect(h) };
    Spot { rect, state }
}

/// Why the finder could not act on a window.
#[derive(Debug, PartialEq)]
pub enum Miss {
    Gone,
    CantMove,
    Unplugged, // hidden on a screen that just left: check_screens folds it soon (spec 0017 AC-FND-7)
}

fn work_area(mon: HMONITOR) -> RECT {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { let _ = GetMonitorInfoW(mon, &mut mi); }
    mi.rcWork
}

/// A window whose app stopped answering (paused in a debugger, say): ShowWindow and
/// SetWindowPos would wait on it with no time limit, with des-k's lock held.
fn hung(h: HWND) -> bool {
    let hung = unsafe { IsHungAppWindow(h) }.as_bool();
    if hung {
        warn!("{} is not responding, left as it is", who(h));
    }
    hung
}

fn focus(h: HWND) {
    if hung(h) {
        return;
    }
    unsafe {
        let _ = SetForegroundWindow(h);
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
    }
}

/// Win+D for the screen under the mouse only (spec 0011, a touchpad gesture). It minimizes the
/// shown workspace's windows and keeps them; the next time it brings back the ones still
/// minimized, so a window you minimized yourself is never restored by it.
pub fn show_desktop() {
    let (mon, name) = under_mouse();
    let mut s = state();
    let id = id_of(&mut s, &name);
    let m = mon_mut(&mut s, &id);
    if !m.desktop.is_empty() {
        let back: Vec<HWND> = std::mem::take(&mut m.desktop)
            .into_iter()
            .map(|raw| HWND(raw as *mut _))
            .filter(|&h| unsafe { IsWindow(Some(h)).as_bool() && IsIconic(h).as_bool() })
            .collect();
        // Bottom first, so the old top one ends on top, with focus.
        for &h in back.iter().rev() {
            focus(h);
        }
        return info!("show desktop: {name}, {} window(s) back", back.len());
    }
    let list: Vec<HWND> = app_windows_on(mon).into_iter().filter(|&h| !unsafe { IsIconic(h) }.as_bool() && !hung(h)).collect();
    let fg = unsafe { GetForegroundWindow() };
    for &h in &list {
        unsafe { let _ = ShowWindow(h, SW_SHOWMINNOACTIVE); }
    }
    // Don't leave keyboard focus in a window we just minimized.
    if list.contains(&fg) {
        unsafe { let _ = SetForegroundWindow(GetShellWindow()); }
    }
    m.desktop = list.iter().map(|h| h.0 as isize).collect();
    info!("show desktop: {name}, {} window(s) minimized", list.len());
}

/// What app_next and app_previous rotate: the shown workspace's windows on the screen under
/// the mouse, not minimized, top first.
fn stack() -> (String, Vec<HWND>) {
    let (mon, name) = under_mouse();
    (name, app_windows_on(mon).into_iter().filter(|&h| !unsafe { IsIconic(h) }.as_bool()).collect())
}

/// The bottom window comes to the top, with focus; repeated, it visits every window (spec 0011).
pub fn app_next() {
    let (name, stack) = stack();
    let Some(&h) = stack.last() else {
        return info!("app_next: no window on {name}");
    };
    focus(h);
    info!("app_next: {} on {name}", who(h));
}

/// The top window goes to the bottom and the new top one gets focus (spec 0011).
pub fn app_previous() {
    let (name, stack) = stack();
    let Some(&top) = stack.first() else {
        return info!("app_previous: no window on {name}");
    };
    let next = stack.get(1).copied().unwrap_or(top);
    if next != top && !hung(top) {
        unsafe { let _ = SetWindowPos(top, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); }
    }
    focus(next);
    info!("app_previous: {} on {name}", who(next));
}

fn hidden_on(s: &State, raw: isize) -> Option<(String, usize)> {
    s.monitors.iter().find_map(|(dev, m)| Some((dev.clone(), m.hidden.iter().position(|l| l.contains(&raw))?)))
}

/// Land on a window wherever it is: a hidden one switches its screen to its workspace,
/// exactly like a switch; one on screen just gets focus (spec 0007).
pub fn go_to(hwnd: HWND) -> Result<(), Miss> {
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(Miss::Gone);
    }
    let mut s = state();
    let t = who(hwnd);
    let (name, ws) = match hidden_on(&s, hwnd.0 as isize) {
        Some((name, ws)) => {
            // Its screen left: check_screens folds it onto the main display if the screen stays gone.
            let Some(mon) = mon_of(&s, &name) else { return Err(Miss::Unplugged) };
            switch_to(&mut s, mon, &name, ws, Some(hwnd), None);
            (name, ws)
        }
        None => {
            let name = id_of(&mut s, &device_name(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }));
            let ws = s.monitors.get(&name).map_or(0, |m| m.active);
            (name, ws)
        }
    };
    focus(hwnd);
    info!("finder: -> {t} ({name} workspace {})", ws + 1);
    Ok(())
}

/// Bring a window onto `device`'s shown workspace, focused, without switching any screen.
/// From another screen it keeps its relative place and size there. On failure nothing
/// about the window or the state has changed (spec 0007).
pub fn bring(hwnd: HWND, device: &str) -> Result<(), Miss> {
    show_on(hwnd, device, true, "finder")
}

/// `bring`'s work. Without `activate` the window only comes to the front, restored, so the
/// overview that dropped it keeps focus and stays open (spec 0010).
fn show_on(hwnd: HWND, device: &str, activate: bool, who: &str) -> Result<(), Miss> {
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(Miss::Gone);
    }
    let mut s = state();
    let raw = hwnd.0 as isize;
    let t = self::who(hwnd);
    let hidden = hidden_on(&s, raw);
    if let Some((id, _)) = &hidden
        && !s.present.iter().any(|p| &p.id == id)
    {
        return Err(Miss::Unplugged);
    }
    let from = match &hidden {
        // A screen only just unplugged keeps its old device name: no monitor has it now.
        Some((id, _)) => s.monitors[id].device.clone().unwrap_or_else(|| id.clone()),
        None => device_name(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }),
    };
    let land = |h: HWND| if activate { focus(h) } else { raise(h) };
    if hidden.is_none() && from == device {
        land(hwnd);
        return Ok(());
    }
    let shell = Shell::new().map_err(|e| {
        warn!("{who}: shell unavailable: {e}");
        Miss::CantMove
    })?;
    if from != device {
        let (Some(old), Some(new)) = (monitor_named(&from), monitor_named(device)) else {
            warn!("{who}: {from} or {device} not found");
            return Err(Miss::CantMove);
        };
        // Still cloaked if hidden, so it never flashes on its old screen.
        if let Err(e) = move_to(hwnd, old, new) {
            warn!("{who}: could not move {t}: {e}");
            return Err(Miss::CantMove);
        }
    }
    if let Some((dev, ws)) = &hidden {
        if let Err(e) = shell.hide(hwnd, false) {
            error!("{who}: could not show {t}: {e}");
            return Err(Miss::CantMove);
        }
        s.monitors.get_mut(dev).expect("found above").hidden[*ws].retain(|&w| w != raw);
    }
    claim(&mut s, raw, &format!("you brought it to {device}"));
    land(hwnd);
    info!("{who}: brought {t} to {device}");
    save(&mut s);
    Ok(())
}

/// To the front of the normal windows, restored if minimized, without taking focus.
fn raise(h: HWND) {
    if hung(h) {
        return;
    }
    unsafe {
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
        }
        let _ = SetWindowPos(h, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Put a window on workspace `ws` of `device` without switching any screen (an overview
/// drop, spec 0010 AC-7): on the shown workspace it comes to the front, otherwise it is
/// hidden there, moved across screens while cloaked so it never flashes. On failure nothing
/// about the window or the state has changed.
pub fn place(hwnd: HWND, device: &str, ws: usize) -> Result<(), Miss> {
    let count = settings::get().workspaces;
    if ws >= count {
        return Err(Miss::CantMove);
    }
    // Its own lock: show_on takes the state again.
    if ws == active(device) {
        return show_on(hwnd, device, false, "overview");
    }
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err(Miss::Gone);
    }
    let mut s = state();
    let raw = hwnd.0 as isize;
    let t = who(hwnd);
    let hidden = hidden_on(&s, raw);
    if hidden.as_ref().is_some_and(|(dev, w)| dev == device && *w == ws) {
        return Ok(());
    }
    let from = match &hidden {
        Some((id, _)) => s.monitors[id].device.clone().unwrap_or_else(|| id.clone()),
        None => device_name(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }),
    };
    let shell = Shell::new().map_err(|e| {
        warn!("overview: shell unavailable: {e}");
        Miss::CantMove
    })?;
    if hidden.is_none()
        && let Err(e) = shell.hide(hwnd, true)
    {
        // The cloak can work and only the taskbar step fail: undo it, so nothing changes.
        if cloaked(hwnd) != 0 {
            let _ = shell.hide(hwnd, false);
        }
        warn!("overview: could not hide {t}: {e}");
        return Err(Miss::CantMove);
    }
    if from != device {
        let moved = match (monitor_named(&from), monitor_named(device)) {
            (Some(old), Some(new)) => move_to(hwnd, old, new).map_err(|e| format!("could not move {t}: {e}")),
            _ => Err(format!("{from} or {device} not found")),
        };
        if let Err(e) = moved {
            if hidden.is_none() {
                let _ = shell.hide(hwnd, false);
            }
            warn!("overview: {e}");
            return Err(Miss::CantMove);
        }
    }
    if let Some((dev, w)) = &hidden {
        s.monitors.get_mut(dev).expect("found above").hidden[*w].retain(|&x| x != raw);
    }
    if s.pinned.remove(&raw) {
        info!("overview: {t} unpinned, it goes to one workspace");
    }
    claim(&mut s, raw, "you dropped it on a workspace");
    // Front = top, as a send puts it.
    let id = id_of(&mut s, device);
    mon_mut(&mut s, &id).hidden[ws].insert(0, raw);
    info!("overview: {t} -> {device} workspace {}/{count}, hidden", ws + 1);
    save(&mut s);
    Ok(())
}

/// Move a window from one screen's work area to the same relative place on another's.
/// Maximized stays maximized; minimized comes back restored.
fn move_to(hwnd: HWND, old: HMONITOR, new: HMONITOR) -> windows::core::Result<()> {
    let rect = map_rect(work_area(old), work_area(new), normal_rect(hwnd));
    put(hwnd, Spot { rect, state: if unsafe { IsZoomed(hwnd) }.as_bool() { 'x' } else { 'n' } })
}

/// Place a window at `to`: its rect and normal, maximized or minimized (spec 0016). Never
/// takes focus: a maximize that does gives it straight back.
fn put(hwnd: HWND, to: Spot) -> windows::core::Result<()> {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { GetWindowPlacement(hwnd, &mut wp)? };
    let fg = unsafe { GetForegroundWindow() };
    wp.rcNormalPosition = shift(to.rect, -1);
    wp.flags = WINDOWPLACEMENT_FLAGS(0);
    wp.showCmd = match to.state {
        'x' => SW_SHOWMAXIMIZED.0 as u32,
        'm' => SW_SHOWMINNOACTIVE.0 as u32,
        _ => SW_SHOWNOACTIVATE.0 as u32,
    };
    unsafe { SetWindowPlacement(hwnd, &wp)? };
    if to.state == 'x' {
        let new = unsafe { MonitorFromRect(&to.rect, MONITOR_DEFAULTTONEAREST) };
        if unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) } != new {
            // Some windows stay maximized where they were: restore onto the new screen, then maximize there.
            wp.showCmd = SW_SHOWNOACTIVATE.0 as u32;
            unsafe {
                SetWindowPlacement(hwnd, &wp)?;
                let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
            }
        }
        // There is no maximize without activation.
        if unsafe { GetForegroundWindow() } != fg && !fg.is_invalid() {
            unsafe { let _ = SetForegroundWindow(fg); }
        }
    }
    if to.state == 'n' {
        // An app that rescales itself on the new screen's DPI can drift off the mapped size.
        let mut now = RECT::default();
        unsafe { let _ = GetWindowRect(hwnd, &mut now); }
        if now != to.rect {
            let r = to.rect;
            unsafe { let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE); }
        }
    }
    Ok(())
}

/// `r` at the same relative place and size in `to` as it had in `from`, kept inside `to`.
fn map_rect(from: RECT, to: RECT, r: RECT) -> RECT {
    let (fw, fh) = ((from.right - from.left).max(1) as i64, (from.bottom - from.top).max(1) as i64);
    let (tw, th) = (to.right - to.left, to.bottom - to.top);
    let x = |v: i32| to.left + ((v - from.left) as i64 * tw as i64 / fw) as i32;
    let y = |v: i32| to.top + ((v - from.top) as i64 * th as i64 / fh) as i32;
    // A window partly off its old screen would land partly off the new one: pull it in.
    let (w, h) = ((x(r.right) - x(r.left)).min(tw), (y(r.bottom) - y(r.top)).min(th));
    let (left, top) = (x(r.left).clamp(to.left, to.right - w), y(r.top).clamp(to.top, to.bottom - h));
    RECT { left, top, right: left + w, bottom: top + h }
}

/// A fresh Shell for an exit path, waiting out an explorer restart: the exit gets one chance.
fn shell_for_exit() -> Option<Shell> {
    for _ in 0..20 {
        match Shell::new() {
            Ok(sh) => return Some(sh),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    error!("shell unavailable for 5 s: hidden windows stay hidden, state.txt keeps them");
    None
}

/// Uncloak everything we hid. Used on --quit, Ctrl+C and the panic hook.
pub fn restore_all() {
    restore_in(&mut state());
}

fn restore_in(s: &mut State) {
    let shell = shell_for_exit();
    let mut left = 0;
    for m in s.monitors.values_mut() {
        for list in &mut m.hidden {
            // No shell: every window still exists and stays hidden, so every one is kept.
            let (_, stuck) = show_all(std::mem::take(list), |raw| match &shell {
                Some(sh) => uncloak(sh, raw),
                None => unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool().then_some(false),
            });
            left += stuck.len();
            *list = stuck;
        }
        // A remembered screen keeps the workspace it showed, for its windows' return (spec 0016 AC-5).
        if m.device.is_some() {
            m.active = 0;
        }
    }
    if left > 0 {
        error!("{left} window(s) could not be shown: state.txt keeps them, the next start shows or adopts them");
    }
    save_lines(s, left > 0);
}

/// The panic hook's restore_all. The hook runs before unwinding drops a lock guard, so a
/// panic mid switch leaves the state locked on this very thread, and locking again would
/// hang: then show every window cloak.rs recorded instead.
pub fn restore_after_panic() {
    match STATE.try_lock() {
        Ok(mut s) => restore_in(&mut s),
        Err(std::sync::TryLockError::Poisoned(e)) => restore_in(&mut e.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => {
            warn!("panic while the layout was locked: showing every window des-k cloaked");
            let Some(shell) = shell_for_exit() else { return };
            for raw in crate::cloak::ours() {
                let _ = shell.hide(HWND(raw as *mut _), false);
            }
        }
    }
}

/// Adopt the layout saved by a previous run (restart, update, hard kill). A saved window
/// is ours only if it is still the same window (same pid) and still shell-cloaked on the
/// CURRENT Windows desktop (windows on another Windows desktop are cloaked too). If its
/// monitor is gone, its workspace no longer exists (the count was lowered), or it would
/// land in the active workspace, show it rather than lose it.
///
/// After a new sign in every handle is new, so instead the saved windows become the
/// entries a restore matches reopened windows against (spec 0003). Returns whether a
/// restore runs; main then drives it with `restore_tick`.
pub fn load() -> bool {
    let mut guard = state();
    let s = &mut *guard;
    s.logon = restore::logon();
    s.present = screen_ids();
    s.seen = s.present.iter().map(|p| p.id.clone()).collect();
    log_screens(&s.present);
    let Ok(text) = std::fs::read_to_string(state_file()) else { return false };
    let present = s.seen.clone();
    let st = settings::get();
    let count = st.workspaces;
    let by_device: HashMap<String, String> = s.present.iter().map(|p| (p.device.clone(), p.id.clone())).collect();
    let lines = parse_state(&text, |dev| by_device.get(dev).cloned());
    // An away window's W or P line is the same window as its A line: a restore must not match it twice.
    let away_raws: HashSet<isize> = lines.iter().filter_map(|l| if let Line::A { raw, .. } = l { (*raw != 0).then_some(*raw) } else { None }).collect();
    let (mut saved_logon, mut adopt, mut hidden, mut visible, mut pins) = (None, Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut pinned, mut aways, mut adopt_away) = (Vec::new(), Vec::new(), Vec::new());
    for line in lines {
        match line {
            Line::L(id, time) => saved_logon = Some((id, time)),
            Line::M { id, active, area, name } => {
                let here = present.contains(&id);
                let m = mon_mut(s, &id);
                // An active workspace that no longer exists starts at the first one.
                m.active = if active < count { active } else { 0 };
                // Away windows' spots are relative to the area it had: kept until they are home.
                if area != RECT::default() {
                    m.area = area;
                }
                if !here {
                    m.name = name; // remembered (spec 0016 AC-8)
                }
            }
            Line::W { id, ws, raw, pid, spot, fp } => {
                if let Some(fp) = fp.filter(|_| !away_raws.contains(&raw)) {
                    hidden.push((Entry { dev: id.clone(), ws: Some(ws), pin: false, away: false, spot, fp }, raw == 0));
                }
                if raw != 0 {
                    if let Some(spot) = spot {
                        s.spots.insert(raw, (id.clone(), spot));
                    }
                    adopt.push((id, ws, raw, pid));
                }
            }
            Line::V { id, spot, fp } => visible.extend(fp.map(|fp| Entry { dev: id, ws: None, pin: false, away: false, spot, fp })),
            Line::P { id, raw, pid, spot, fp } => {
                if let Some(fp) = fp.filter(|_| !away_raws.contains(&raw)) {
                    pinned.push((Entry { dev: id, ws: None, pin: true, away: false, spot, fp }, raw == 0));
                }
                if raw != 0 {
                    pins.push((raw, pid));
                }
            }
            Line::A { raw, away, fp: Some(fp) } => {
                aways.push((Entry { dev: away.home.clone(), ws: Some(away.ws), pin: away.pinned, away: true, spot: Some(away.spot), fp: fp.clone() }, raw == 0));
                if raw != 0 {
                    adopt_away.push((raw, away, fp));
                }
            }
            Line::A { .. } => {}
        }
    }

    let new_sign_in = saved_logon.is_some() && s.logon.is_some() && saved_logon != s.logon;
    let cut_short = hidden.iter().chain(&aways).any(|(_, cut)| *cut);
    let since = s.logon.map_or(0, |(_, t)| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64) - t);
    let late = since > (st.restore_minutes as i64 + 2) * 60;
    if st.restore_enabled && new_sign_in && late {
        info!("restore: signed in {} min ago, skipped", since / 60);
    }
    let restoring = st.restore_enabled && if new_sign_in { !late } else { cut_short };
    // A restart in the same sign in: the same window, same pid, is still pinned.
    if !new_sign_in {
        let same = |&(raw, pid): &(isize, u32)| unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool() && pid_of(HWND(raw as *mut _)) == pid;
        s.pinned.extend(pins.iter().filter(|p| same(p)).map(|&(raw, _)| raw));
        // Still that window, still open: it still waits for its screen (spec 0016 AC-8).
        for (raw, a, fp) in adopt_away {
            let h = HWND(raw as *mut _);
            let it = same(&(raw, a.pid)) && restore::app(h, a.pid) == fp.app && restore::clean(&class_name(h)) == fp.class;
            if it && s.monitors.contains_key(&a.home) {
                if a.pinned {
                    s.pinned.insert(raw);
                }
                s.away.insert(raw, a);
            }
        }
    }
    if restoring {
        // A new sign in: every saved handle is dead. A resume: only the cut entries are left.
        if new_sign_in {
            adopt.clear();
        }
        let cut = |(e, cut): (Entry, bool)| (new_sign_in || cut).then_some(e);
        let saved: Vec<Entry> =
            hidden.into_iter().filter_map(cut).chain(visible).chain(pinned.into_iter().filter_map(cut)).chain(aways.into_iter().filter_map(cut)).collect();
        info!("restore: {} {} saved window(s), matching for {} min", if new_sign_in { "new sign in," } else { "resuming," }, saved.len(), st.restore_minutes);
        s.restore = Some(Restore {
            pending: (0..saved.len()).collect(),
            saved,
            resolved: HashSet::new(),
            unclear_logged: HashSet::new(),
            cache: HashMap::new(),
            deadline: Instant::now() + Duration::from_secs(st.restore_minutes as u64 * 60),
            hidden: 0,
            kept: 0,
        });
    }

    if !adopt.is_empty() {
        let (Ok(shell), Ok(vdm)) = (Shell::new(), unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }) else {
            warn!("shell unavailable, saved layout not loaded");
            return restoring;
        };
        let (mut kept, mut shown) = (0, 0);
        let mut fold_now = HashSet::new();
        for (dev, ws, raw, pid) in adopt {
            let h = HWND(raw as *mut _);
            let on_current = unsafe { vdm.IsWindowOnCurrentVirtualDesktop(h) }.is_ok_and(|b| b.as_bool());
            if !(unsafe { IsWindow(Some(h)) }.as_bool() && pid_of(h) == pid && cloaked(h) == 2 && on_current) {
                continue; // closed, reused handle, or not hidden by us any more
            }
            match s.monitors.get_mut(&dev) {
                Some(m) if present.contains(&dev) && ws < count && m.active != ws => {
                    m.hidden[ws].push(raw);
                    crate::cloak::adopt(h);
                    kept += 1;
                }
                // A remembered screen not plugged in: folded below, as at an unplug (spec 0016).
                Some(m) if !present.contains(&dev) && ws < count => {
                    m.hidden[ws].push(raw);
                    crate::cloak::adopt(h);
                    kept += 1;
                    fold_now.insert(dev);
                }
                _ => match shell.hide(h, false) {
                    Ok(()) => shown += 1,
                    Err(e) => {
                        // Still hidden: tracked, so a switch, quit or screen check tries again.
                        error!("could not show saved window {raw:#x}: {e}, kept to try again");
                        let m = mon_mut(s, &dev);
                        let ws = ws.min(m.hidden.len() - 1);
                        m.hidden[ws].push(raw);
                        crate::cloak::adopt(h);
                    }
                },
            }
        }
        info!("loaded saved layout: {kept} hidden window(s) adopted, {shown} shown again");
        // At start: the log says it, no note.
        for id in fold_now {
            fold(s, &id, &mut Vec::new());
        }
    }
    if !s.away.is_empty() {
        info!("loaded saved layout: {} wait for a screen not plugged in", words(s.away.len()));
    }
    if !s.pinned.is_empty() {
        info!("loaded saved layout: {} pinned window(s)", s.pinned.len());
    }
    save(s);
    restoring
}

/// One restore step, every half second while a restore runs (spec 0003): hide each
/// reopened window it recognizes back into its saved workspace. Returns whether the
/// restore is still running.
pub fn restore_tick() -> bool {
    let mut guard = state();
    let s = &mut *guard;
    let Some(r) = s.restore.as_mut() else { return false };
    if Instant::now() >= r.deadline || r.pending.is_empty() {
        finish_restore(s);
        return false;
    }
    let Ok(shell) = Shell::new() else { return true }; // explorer not up yet: next tick
    let count = settings::get().workspaces;

    // Bottom of z order first, like a switch, so nothing behind flashes.
    let mut left = Vec::new();
    for h in app_windows().into_iter().rev() {
        let key = (h.0 as isize, pid_of(h));
        if r.resolved.contains(&key) {
            continue;
        }
        let (app, class) = match r.cache.get(&key) {
            Some(ac) => ac.clone(),
            None => {
                let ac = (restore::app(h, key.1), restore::clean(&class_name(h)));
                // A Store window's app id may not be readable yet: ask again next tick.
                if !ac.0.ends_with("\\applicationframehost.exe") {
                    r.cache.insert(key, ac.clone());
                }
                ac
            }
        };
        left.push((h, key, Fp { app, class, title: restore::clean(&title(h)) }));
    }

    let mut changed = false;
    for pass in [1, 2] {
        let mut rest = Vec::new();
        for (h, key, fp) in left {
            let i = match restore::pick(&r.saved, &r.pending, &fp, pass) {
                Pick::Take(i) => i,
                Pick::Unclear if pass == 2 => {
                    if r.unclear_logged.insert(key) {
                        info!("restore: unclear match for {}, left visible", fp.app);
                    }
                    continue;
                }
                _ => {
                    rest.push((h, key, fp));
                    continue;
                }
            };
            let use_up = |r: &mut Restore, why: &str| {
                r.pending.retain(|&p| p != i);
                r.resolved.insert(key);
                r.kept += 1;
                info!("restore: {} stays visible, {why}", fp.app);
            };
            let e = r.saved[i].clone();
            let raw = h.0 as isize;
            let here = s.present.iter().find(|p| p.id == e.dev).cloned();
            // It was away, or its screen is remembered but not plugged in: it waits for that
            // screen, and goes back when it comes (spec 0016 AC-10).
            if let Some(spot) = e.spot
                && (e.away || here.is_none())
                && let Some(home) = s.monitors.get(&e.dev)
            {
                let (home_active, name, from) = (home.active, home.name.clone(), home.area);
                let ws = e.ws.unwrap_or(home_active).min(count - 1);
                let main = s.present.iter().find(|p| p.primary).cloned();
                if here.is_none()
                    && !(e.pin || e.ws.is_none() || ws == home_active)
                    && let Some(main) = main
                {
                    let m = s.monitors.entry(main.id.clone()).or_insert_with(|| Monitor::of(&main));
                    if m.active != ws {
                        // A brand new window fails to cloak for a moment: nothing used up, next tick retries.
                        if shell.hide(h, true).is_err() {
                            if cloaked(h) != 0 {
                                let _ = shell.hide(h, false);
                            }
                            continue;
                        }
                        m.hidden[ws].insert(0, raw);
                        s.restored.retain(|_, t| t.elapsed() < RESTORED_IGNORE);
                        s.restored.insert(raw, Instant::now());
                    }
                    if !hung(h)
                        && let Err(err) = put(h, Spot { rect: remap(from, main.area, spot.rect), state: spot.state })
                    {
                        warn!("restore: could not place {}: {err}", fp.app);
                    }
                }
                if e.pin {
                    s.pinned.insert(raw);
                }
                s.away.insert(raw, Away { home: e.dev.clone(), ws, spot, pinned: e.pin, pid: key.1 });
                r.pending.retain(|&p| p != i);
                r.resolved.insert(key);
                r.kept += 1;
                changed = true;
                info!("restore: {} waits for {name}, workspace {}/{count}", fp.app, ws + 1);
                continue;
            }
            if e.pin {
                s.pinned.insert(raw);
                use_up(r, "pinned to every workspace, as before the restart");
                continue;
            }
            let (dev, Some(ws)) = (e.dev, e.ws) else {
                use_up(r, "as before the restart");
                continue;
            };
            let Some(p) = here else {
                use_up(r, &format!("{dev} is not connected"));
                continue;
            };
            let m = s.monitors.entry(dev.clone()).or_insert_with(|| Monitor::of(&p));
            match () {
                _ if ws >= count => use_up(r, &format!("workspace {} no longer exists", ws + 1)),
                _ if m.active == ws => use_up(r, &format!("{dev} already shows workspace {}", ws + 1)),
                // Apps often open on the main screen and move a moment later.
                _ if device_name(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) }) != p.device => {}
                _ => {
                    let was_fg = unsafe { GetForegroundWindow() } == h;
                    // A brand new window fails to cloak for a moment: nothing used up, next tick retries.
                    if shell.hide(h, true).is_err() {
                        // The cloak can work and only the taskbar step fail: undo it, or nothing tracks it.
                        if cloaked(h) != 0 {
                            let _ = shell.hide(h, false);
                        }
                        continue;
                    }
                    m.hidden[ws].insert(0, raw);
                    s.restored.retain(|_, t| t.elapsed() < RESTORED_IGNORE);
                    s.restored.insert(raw, Instant::now());
                    r.pending.retain(|&p| p != i);
                    r.resolved.insert(key);
                    r.hidden += 1;
                    changed = true;
                    info!("restore: {} -> {dev} workspace {}/{count}", fp.app, ws + 1);
                    if was_fg {
                        let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
                        let next = app_windows_on(mon).first().copied().unwrap_or(unsafe { GetShellWindow() });
                        unsafe {
                            let _ = SetForegroundWindow(next);
                            if GetForegroundWindow() != next {
                                warn!("restore: could not move focus");
                            }
                        }
                    }
                }
            }
        }
        left = rest;
    }
    if r.pending.is_empty() {
        finish_restore(s);
        return false;
    }
    if changed {
        save(s);
    }
    true
}

/// Unmatched entries are dropped: a window that reopens later is never hidden (AC-6).
fn finish_restore(s: &mut State) {
    let Some(r) = s.restore.take() else { return };
    info!("restore: done, {} hidden, {} kept visible, {} saved window(s) not matched", r.hidden, r.kept, r.pending.len());
    save(s);
}

// Tries left to hide every tracked window again after explorer restarted (resync).
static RESYNC: AtomicU32 = AtomicU32::new(0);
const RESYNC_TRIES: u32 = 10;

/// Explorer restarted (the tray heard TaskbarCreated): main's next 1 s check runs resync.
/// Only a flag: the tray's message can arrive inside a call made under the state lock.
pub fn resync_soon() {
    RESYNC.store(RESYNC_TRIES, Ordering::Relaxed);
}

/// After explorer restarts, its new taskbar can list des-k's hidden windows again, and their
/// cloak may be gone with the old explorer. Hide every tracked window again: both steps do
/// nothing to a window still hidden. One des-k could not hide and that is in view is dropped
/// from its list (nothing hidden to lose); a failure on one still cloaked keeps it, and the
/// whole pass runs again next second, RESYNC_TRIES times at most.
pub fn resync() {
    let tries = RESYNC.load(Ordering::Relaxed);
    if tries == 0 {
        return;
    }
    RESYNC.store(tries - 1, Ordering::Relaxed);
    let Ok(shell) = Shell::new() else { return }; // explorer not up yet
    let mut s = state();
    let (mut hidden, mut failed, mut in_view) = (0, 0, 0);
    for m in s.monitors.values_mut() {
        for list in &mut m.hidden {
            list.retain(|&raw| {
                let h = HWND(raw as *mut _);
                if !unsafe { IsWindow(Some(h)) }.as_bool() {
                    return false;
                }
                match shell.hide(h, true) {
                    Ok(()) => hidden += 1,
                    Err(e) if cloaked(h) == 0 => {
                        warn!("explorer restarted: could not hide {raw:#x} again ({e}), it stays in view");
                        in_view += 1;
                        return false;
                    }
                    Err(e) => {
                        warn!("explorer restarted: could not hide {raw:#x} again ({e}), kept to try again");
                        failed += 1;
                    }
                }
                true
            });
        }
    }
    info!("explorer restarted: {hidden} hidden window(s) hidden again, {in_view} left in view, {failed} to try again");
    if failed == 0 {
        RESYNC.store(0, Ordering::Relaxed);
    }
    if in_view > 0 {
        save(&mut s);
    }
}

/// check_screens' bookkeeping, apart so it can be tested: `missing` are the screens with
/// workspaces that are not plugged in now. Returns those gone `limit` ticks in a row or more,
/// with their count.
fn count_gone(gone: &mut HashMap<String, u32>, missing: &[String], tick: bool, limit: u32) -> Vec<(String, u32)> {
    for id in gone.keys().filter(|d| !missing.contains(*d)) {
        info!("{id} is back: its workspaces were kept");
    }
    gone.retain(|d, _| missing.contains(d));
    let mut due = Vec::new();
    for id in missing {
        let n = gone.entry(id.clone()).or_insert_with(|| {
            info!("{id} disconnected: its windows fold onto the main display if it stays gone {limit} s");
            0
        });
        if tick {
            *n += 1;
        }
        if *n >= limit {
            due.push((id.clone(), *n));
        }
    }
    due
}

/// Screens come and go (spec 0016). Called on every display change (`tick` false: only notes
/// what is gone or back) and by main's 1 s watch timer (`tick` true: counts), which also
/// catches a display change lost in a menu's modal loop. Each check runs in one order: claims,
/// closed windows, fold and return, then spots. Notes show after the lock is let go.
pub fn check_screens(tick: bool) {
    let present = screen_ids();
    let notes = check_in(&mut state(), present, tick);
    if notes.is_empty() {
        return;
    }
    let lines: Vec<String> = notes.iter().map(|n| n.text.clone()).collect();
    // One indicator: a note per screen would replace the one before, so the last carries every
    // line. A line about another screen names where its windows went instead of "here".
    let last = notes.last().expect("not empty");
    if let Some(mon) = monitor_named(&last.device) {
        let mut c = card(&state(), &last.id, last.active, settings::get().workspaces);
        for n in &notes {
            let mut line = n.line.clone();
            match &mut line {
                Says::ScreenGone { to, .. } if n.device == last.device => *to = None,
                Says::ScreenBack { here, .. } => *here = n.device == last.device,
                _ => {}
            }
            c = c.with(line);
        }
        indicator::show(mon, c);
    }
    crate::tray::screens_changed(&lines);
}

fn check_in(s: &mut State, present: Vec<Screen>, tick: bool) -> Vec<Note> {
    let mut notes = Vec::new();
    let now: HashSet<String> = present.iter().map(|p| p.id.clone()).collect();
    if now != s.seen {
        log_screens(&present);
    }
    s.present = present;
    for p in &s.present {
        if let Some(m) = s.monitors.get_mut(&p.id) {
            m.device = Some(p.device.clone());
            m.name = p.name.clone();
        }
    }
    let mut dirty = claims(s, &now);
    let before = s.away.len();
    s.away.retain(|&raw, a| {
        let open = unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool() && pid_of(HWND(raw as *mut _)) == a.pid;
        if !open {
            info!("a window waiting for {} closed", a.home);
        }
        open
    });
    dirty |= s.away.len() != before;

    let missing: Vec<String> = s.monitors.iter().filter(|(id, m)| m.device.is_some() && !now.contains(*id)).map(|(id, _)| id.clone()).collect();
    let known: HashSet<String> = s.gone.keys().cloned().collect();
    let due = count_gone(&mut s.gone, &missing, tick, gone_checks());
    for id in missing.iter().filter(|id| !known.contains(*id)) {
        dirty |= mark_away(s, id) > 0;
    }
    for (id, _) in due {
        fold(s, &id, &mut notes);
        dirty = true;
    }

    let homes: HashSet<String> = s.away.values().map(|a| a.home.clone()).collect();
    s.back.retain(|id, _| now.contains(id) && homes.contains(id));
    for id in homes.into_iter().filter(|id| now.contains(id)) {
        let e = s.back.entry(id.clone()).or_insert((0, 0));
        if tick {
            e.0 += 1;
        }
        if let Some(last) = try_return(e.0) {
            dirty |= give_back(s, &id, last, &mut notes);
        }
    }

    // Forgotten once no window waits for it, nor a restore entry still looking for its window.
    let pending: HashSet<String> = s.restore.as_ref().map_or_else(HashSet::new, |r| r.pending.iter().map(|&i| r.saved[i].dev.clone()).collect());
    let forget: Vec<String> = s
        .monitors
        .iter()
        .filter(|(id, m)| m.device.is_none() && !now.contains(*id) && !pending.contains(*id) && !s.away.values().any(|a| &a.home == *id))
        .map(|(id, _)| id.clone())
        .collect();
    for id in forget {
        let name = s.monitors.remove(&id).map_or(String::new(), |m| m.name);
        info!("{name} ({id}) forgotten: no window waits for it any more");
        dirty = true;
    }

    refresh_spots(s, &now);
    for p in &s.present {
        if let Some(m) = s.monitors.get_mut(&p.id)
            && !s.away.values().any(|a| a.home == p.id)
        {
            m.area = p.area;
        }
    }
    s.seen = now;
    if dirty {
        save(s);
    }
    notes
}

/// Whether a screen back for `n` checks tries to take its windows back now, and whether this
/// is the last try (spec 0016 AC-5, AC-12).
fn try_return(n: u32) -> Option<bool> {
    (BACK_CHECKS..BACK_CHECKS + RETURN_TRIES).contains(&n).then_some(n == BACK_CHECKS + RETURN_TRIES - 1)
}

/// Whether an away window's change of screen is your move (spec 0016 AC-6). Windows moves
/// windows off a screen that left and back onto a screen that returns; only a move between two
/// screens that were there at the last check and are there now, not onto its home, is yours.
fn is_claim(prev: &str, cur: &str, home: &str, seen: &HashSet<String>, now: &HashSet<String>) -> bool {
    cur != prev && cur != home && seen.contains(prev) && now.contains(prev) && now.contains(cur)
}

/// The present screen a window is on now: its list's for a hidden one, else the screen its
/// rect is on; None while it is on no screen (Windows has not moved it off a gone one yet).
fn screen_now(s: &State, raw: isize) -> Option<String> {
    if let Some((id, _)) = hidden_on(s, raw) {
        return Some(id);
    }
    let h = HWND(raw as *mut _);
    let m = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONULL) };
    if m.is_invalid() {
        return None;
    }
    let dev = device_name(m);
    s.present.iter().find(|p| p.device == dev).map(|p| p.id.clone())
}

/// Away windows you moved to another screen since the last check: they stay (AC-6).
fn claims(s: &mut State, now: &HashSet<String>) -> bool {
    let list: Vec<(isize, String)> = s.away.iter().map(|(&raw, a)| (raw, a.home.clone())).collect();
    let mut any = false;
    for (raw, home) in list {
        let (Some((prev, _)), Some(cur)) = (s.spots.get(&raw).cloned(), screen_now(s, raw)) else { continue };
        if is_claim(&prev, &cur, &home, &s.seen, now) {
            let name = s.present.iter().find(|p| p.id == cur).map_or(cur.clone(), |p| p.name.clone());
            claim(s, raw, &format!("you moved it to {name}"));
            any = true;
        }
    }
    any
}

/// A screen's first missing check (AC-3): the windows that were in view on it are away from
/// now on, with the spot they had there. Returns how many.
fn mark_away(s: &mut State, id: &str) -> usize {
    let Some(m) = s.monitors.get(id) else { return 0 };
    let hidden: HashSet<isize> = m.hidden.iter().flatten().copied().collect();
    let active = m.active;
    let list: Vec<(isize, Spot)> = s.spots.iter().filter(|(raw, (sid, _))| sid == id && !hidden.contains(raw)).map(|(&raw, &(_, sp))| (raw, sp)).collect();
    let mut n = 0;
    for (raw, spot) in list {
        let h = HWND(raw as *mut _);
        if !unsafe { IsWindow(Some(h)) }.as_bool() || s.away.contains_key(&raw) {
            continue; // closed, or already away from a screen it left before (invariant 3)
        }
        let pinned = s.pinned.contains(&raw);
        s.away.insert(raw, Away { home: id.to_string(), ws: active, spot, pinned, pid: pid_of(h) });
        n += 1;
    }
    if n > 0 {
        info!("{id} disconnected: {} that were in view go back to it when it returns", words(n));
    }
    n
}

/// A screen gone long enough (spec 0016 AC-2): each window it hid goes hidden into the same
/// workspace of the main display, at the same relative spot, or into view there when that
/// workspace is shown. Each keeps an away record of its home; the screen is remembered.
fn fold(s: &mut State, id: &str, notes: &mut Vec<Note>) {
    let Some(main) = s.present.iter().find(|p| p.primary).cloned() else {
        return warn!("{id} unplugged, and no main display to take its windows: trying again next check");
    };
    let Ok(shell) = Shell::new() else { return warn!("shell unavailable: {id}'s windows fold at the next check") };
    let count = settings::get().workspaces;
    let Some(gone) = s.monitors.get_mut(id) else { return };
    let lists = std::mem::replace(&mut gone.hidden, vec![Vec::new(); count]);
    gone.device = None;
    gone.desktop.clear();
    let (from, name) = (gone.area, gone.name.clone());
    s.gone.remove(id);
    let main_active = mon_mut(s, &main.id).active;
    let (mut show, mut n) = (Vec::new(), 0);
    for (ws, list) in lists.into_iter().enumerate() {
        for raw in list {
            let h = HWND(raw as *mut _);
            if !unsafe { IsWindow(Some(h)) }.as_bool() {
                continue;
            }
            let spot = match s.spots.get(&raw) {
                Some((sid, sp)) if sid == id => *sp,
                _ => spot_of(h),
            };
            // A window already away keeps its first home (invariant 3).
            s.away.entry(raw).or_insert(Away { home: id.to_string(), ws, spot, pinned: false, pid: pid_of(h) });
            // Still cloaked, so it never flashes.
            if !hung(h)
                && let Err(e) = put(h, Spot { rect: remap(from, main.area, spot.rect), state: spot.state })
            {
                warn!("fold: could not move {}: {e}", who(h));
            }
            s.spots.insert(raw, (main.id.clone(), spot_of(h)));
            let ws = ws.min(count - 1);
            if ws == main_active {
                show.push(raw);
            } else {
                mon_mut(s, &main.id).hidden[ws].push(raw);
            }
            n += 1;
        }
    }
    // Kept with the shown workspace when one fails to show: a switch away and back shows it.
    let (_, stuck) = show_all(show, |raw| uncloak(&shell, raw));
    mon_mut(s, &main.id).hidden[main_active].extend(stuck);
    let waiting = s.away.values().filter(|a| a.home == id).count();
    info!("{name} ({id}) unplugged: {} it hid moved to {} ({}), {} wait for it", words(n), main.name, main.id, words(waiting));
    if waiting > 0 {
        notes.push(Note {
            device: main.device.clone(),
            id: main.id.clone(),
            active: main_active,
            line: Says::ScreenGone { name: name.clone(), moved: waiting, to: Some(label(s, &main.id)) },
            text: format!("{name} unplugged: {} moved here", words(waiting)),
        });
    }
}

/// A screen back BACK_CHECKS checks (spec 0016 AC-5): each of its away windows goes back to its
/// workspace and spot there, in view on the workspace it shows, hidden on the others. Never
/// takes focus (AC-14). A window that does not move keeps its record for the next try (AC-12).
/// Returns whether anything changed.
fn give_back(s: &mut State, id: &str, last: bool, notes: &mut Vec<Note>) -> bool {
    let Some(p) = s.present.iter().find(|p| p.id == id).cloned() else { return false };
    let Ok(shell) = Shell::new() else {
        warn!("shell unavailable: {}'s windows go back at the next check", p.name);
        return false;
    };
    let count = settings::get().workspaces;
    let (saved, active) = {
        let m = mon_mut(s, id);
        (m.area, m.active)
    };
    let fg = unsafe { GetForegroundWindow() };
    let mine: Vec<(isize, Away)> = s.away.iter().filter(|(_, a)| a.home == id).map(|(&raw, a)| (raw, a.clone())).collect();
    let (mut moved, mut changed, mut fg_hidden) = (0, false, false);
    for (raw, a) in mine {
        let h = HWND(raw as *mut _);
        if hung(h) {
            continue; // counts as a try
        }
        let to = Spot { rect: if saved == p.area { a.spot.rect } else { remap(saved, p.area, a.spot.rect) }, state: a.spot.state };
        let ws = a.ws.min(count - 1);
        let in_view = a.pinned || ws == active;
        let done = match (in_view, hidden_on(s, raw)) {
            // Already home and in place (a short drop Windows undid): nothing to move.
            (true, None) if screen_now(s, raw).as_deref() == Some(id) && spot_of(h) == to => Some(false),
            (true, None) => put(h, to).inspect_err(|e| warn!("return: could not move {}: {e}", who(h))).ok().map(|()| true),
            (true, Some((sid, w))) => {
                let ok = put(h, to).inspect_err(|e| warn!("return: could not move {}: {e}", who(h))).is_ok() && uncloak(&shell, raw) == Some(true);
                if ok {
                    s.monitors.get_mut(&sid).expect("listed").hidden[w].retain(|&x| x != raw);
                }
                ok.then_some(true)
            }
            (false, Some((sid, w))) => {
                // Still cloaked: moved, then listed with its home workspace.
                let ok = put(h, to).inspect_err(|e| warn!("return: could not move {}: {e}", who(h))).is_ok();
                if ok {
                    s.monitors.get_mut(&sid).expect("listed").hidden[w].retain(|&x| x != raw);
                    mon_mut(s, id).hidden[ws].push(raw);
                }
                ok.then_some(true)
            }
            (false, None) => {
                // Hidden first, then moved, so it never flashes on its way home.
                if let Err(e) = shell.hide(h, true) {
                    if cloaked(h) != 0 {
                        let _ = shell.hide(h, false);
                    }
                    warn!("return: could not hide {}: {e}", who(h));
                    None
                } else if let Err(e) = put(h, to) {
                    warn!("return: could not move {}: {e}", who(h));
                    if shell.hide(h, false).is_err() {
                        // Still cloaked: it must stay listed, so its home workspace takes it.
                        mon_mut(s, id).hidden[ws].push(raw);
                        Some(true)
                    } else {
                        None
                    }
                } else {
                    fg_hidden |= h == fg;
                    mon_mut(s, id).hidden[ws].push(raw);
                    Some(true)
                }
            }
        };
        if let Some(did) = done {
            s.away.remove(&raw);
            s.spots.insert(raw, (id.to_string(), spot_of(h)));
            if a.pinned {
                s.pinned.insert(raw);
            }
            moved += did as usize;
            changed = true;
        }
    }
    if fg_hidden {
        // Never leave keyboard focus in a window just hidden.
        let next = app_windows().first().copied().unwrap_or(unsafe { GetShellWindow() });
        unsafe { let _ = SetForegroundWindow(next); }
    }
    let e = s.back.get_mut(id).expect("counted by check_in");
    e.1 += moved;
    let total = e.1;
    let left = s.away.values().filter(|a| a.home == id).count();
    if left == 0 || last {
        e.0 = BACK_CHECKS + RETURN_TRIES; // no more tries until it returns again
        let stuck = if left > 0 { format!(", {left} could not be moved") } else { String::new() };
        info!("{} ({id}) is back: {} returned{stuck}", p.name, words(total));
        if total > 0 || left > 0 {
            notes.push(Note {
                device: p.device.clone(),
                id: p.id.clone(),
                active,
                line: Says::ScreenBack { name: label(s, &p.id), moved: total, left, here: true },
                text: format!("{} is back: {} returned{stuck}", p.name, words(total)),
            });
        }
    }
    changed
}

/// map_rect, from an area nobody knew (an old line) into `to` as it is.
fn remap(from: RECT, to: RECT, r: RECT) -> RECT {
    if from.right > from.left && from.bottom > from.top { map_rect(from, to, r) } else { map_rect(to, to, r) }
}

/// Reads each tracked window's spot (spec 0016): the windows in view on a present screen and
/// every listed hidden one. A window whose last screen is missing keeps the spot it had there,
/// unless it is away already: its record holds that spot, and claims need where it is now.
fn refresh_spots(s: &mut State, now: &HashSet<String>) {
    let mut found: Vec<(isize, String)> = Vec::new();
    for h in app_windows() {
        let m = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONULL) };
        if m.is_invalid() {
            continue;
        }
        let dev = device_name(m);
        if let Some(p) = s.present.iter().find(|p| p.device == dev) {
            found.push((h.0 as isize, p.id.clone()));
        }
    }
    for (id, m) in &s.monitors {
        found.extend(m.hidden.iter().flatten().map(|&raw| (raw, id.clone())));
    }
    let mut fresh = HashMap::new();
    for (raw, id) in found {
        let old = s.spots.remove(&raw);
        let spot = next_spot(old, id, now, s.away.contains_key(&raw), || spot_of(HWND(raw as *mut _)));
        fresh.insert(raw, spot);
    }
    // Off every screen for a moment (Windows still moving it): keep what it had.
    for (raw, old) in std::mem::take(&mut s.spots) {
        if s.away.contains_key(&raw) && unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool() {
            fresh.entry(raw).or_insert(old);
        }
    }
    s.spots = fresh;
}

/// One window's spot after a check: frozen while its last screen is missing (invariant 2).
fn next_spot(old: Option<(String, Spot)>, id: String, now: &HashSet<String>, away: bool, read: impl FnOnce() -> Spot) -> (String, Spot) {
    match old {
        Some(old) if !now.contains(&old.0) && !away => old,
        _ => (id, read()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    // Shift+Enter promises the same relative place and size: a left half stays a left half,
    // even on a screen with another size and position.
    #[test]
    fn map_rect_keeps_the_relative_place_and_size() {
        let laptop = rect(0, 0, 1920, 1140);
        let external = rect(1920, -200, 1920 + 2560, -200 + 1400);
        let left_half = rect(0, 0, 960, 1140);
        assert_eq!(map_rect(laptop, external, left_half), rect(1920, -200, 1920 + 1280, 1200));
    }

    fn with_hidden(active: usize, hidden: &[&[isize]]) -> Monitor {
        let mut m = Monitor::with(hidden.len());
        m.active = active;
        m.hidden = hidden.iter().map(|l| l.to_vec()).collect();
        m
    }

    // Lowering the count must never lose a window: the windows of every removed workspace end
    // up hidden on the new last one, and the workspace you look at does not change (AC-6).
    #[test]
    fn lowering_moves_removed_windows_to_the_new_last_workspace() {
        let mut m = with_hidden(0, &[&[], &[1], &[2, 3], &[4]]);
        assert_eq!(m.resize(2), Vec::<isize>::new());
        assert_eq!(m.active, 0);
        assert_eq!(m.hidden, vec![vec![], vec![1, 2, 3, 4]]);
    }

    // A screen showing a removed workspace switches to the new last one, and the windows that
    // land there are shown, not left hidden behind a workspace you are already on (AC-6).
    #[test]
    fn lowering_below_the_shown_workspace_shows_what_lands_there() {
        let mut m = with_hidden(3, &[&[9], &[1], &[2], &[]]);
        m.last_switch = Some((Instant::now(), 2));
        assert_eq!(m.resize(2), vec![1, 2]);
        assert_eq!(m.active, 1);
        assert_eq!(m.hidden, vec![vec![9], vec![]]);
        assert!(m.last_switch.is_none(), "a back jump to a removed workspace would index past the end");
    }

    // Raising adds empty workspaces and leaves windows where an earlier lowering put them (AC-6).
    #[test]
    fn raising_adds_empty_workspaces_only() {
        let mut m = with_hidden(1, &[&[5], &[]]);
        assert_eq!(m.resize(4), Vec::<isize>::new());
        assert_eq!(m.active, 1);
        assert_eq!(m.hidden, vec![vec![5], vec![], vec![], vec![]]);
    }

    // A screen that drops out for a moment (wake from sleep, a DisplayPort relink) keeps its
    // workspaces: only GONE_CHECKS one second checks in a row count, and display change
    // events alone never do, however many arrive.
    #[test]
    fn a_screen_must_stay_gone_before_its_windows_are_shown() {
        let mut gone = HashMap::new();
        let dp = ["DISPLAY2".to_string()];
        for _ in 0..50 {
            assert!(count_gone(&mut gone, &dp, false, GONE_CHECKS).is_empty());
        }
        for _ in 1..GONE_CHECKS {
            assert!(count_gone(&mut gone, &dp, true, GONE_CHECKS).is_empty());
        }
        assert_eq!(count_gone(&mut gone, &dp, true, GONE_CHECKS), vec![("DISPLAY2".to_string(), GONE_CHECKS)]);
    }

    // Two short drop outs never add up to one long one: a screen that comes back starts at zero.
    #[test]
    fn a_screen_that_comes_back_starts_counting_again() {
        let mut gone = HashMap::new();
        let dp = ["DISPLAY2".to_string()];
        for _ in 1..GONE_CHECKS {
            count_gone(&mut gone, &dp, true, GONE_CHECKS);
        }
        assert!(count_gone(&mut gone, &[], true, GONE_CHECKS).is_empty());
        assert!(gone.is_empty());
        for _ in 1..GONE_CHECKS {
            assert!(count_gone(&mut gone, &dp, true, GONE_CHECKS).is_empty());
        }
    }

    // --quit after a crash must never lose a window: one it could not show keeps its W line,
    // so the next start or --quit still finds it. Shown windows and V lines go and every
    // screen shows workspace 1, as after a quit.
    #[test]
    fn rescue_keeps_the_line_of_a_window_it_could_not_show() {
        let text = "L 1a2b 1700000000\nM DISPLAY1 2\n\
            W DISPLAY1 1 100 7\tapp\tclass\tshown\n\
            W DISPLAY1 2 200 8\tapp\tclass\tstuck\n\
            W DISPLAY1 1 300 9\tapp\tclass\tclosed\n\
            W DISPLAY1 1 0 0\tapp\tclass\tpending\n\
            V DISPLAY1\tapp\tclass\tvisible\n";
        let mut asked = Vec::new();
        let (out, shown, left) = rescued(text, |raw, pid| {
            asked.push((raw, pid));
            match raw {
                100 => Some(true),
                200 => Some(false),
                _ => None,
            }
        });
        assert_eq!((shown, left), (1, 1));
        assert_eq!(out, "L 1a2b 1700000000\nM DISPLAY1 0\nW DISPLAY1 2 200 8\tapp\tclass\tstuck\n");
        assert_eq!(asked, vec![(100, 7), (200, 8), (300, 9)], "a restore's hwnd 0 entry is no window to show");
    }

    // "S · W" numbers in the finder and the settings page must match what you see: screens
    // counted from the left, then from the top.
    #[test]
    fn screens_are_numbered_left_to_right_then_top_down() {
        let r = |left, top| rect(left, top, left + 100, top + 100);
        let got = in_order(vec![("D1".into(), r(0, 0)), ("D2".into(), r(-1920, 0)), ("D3".into(), r(0, -1080)), ("D4".into(), r(2560, 200))]);
        assert_eq!(got, ["D2", "D3", "D1", "D4"]);
    }

    // A window that fails to show is never dropped: it may still be cloaked, and only a list
    // lets a later switch, quit or panic show it. A closed one goes. Shown windows keep their
    // top first order, so focus lands on the old top one.
    #[test]
    fn a_window_that_fails_to_show_stays_tracked() {
        let (shown, stuck) = show_all(vec![1, 2, 3, 4], |raw| match raw {
            2 => Some(false),
            3 => None,
            _ => Some(true),
        });
        assert_eq!(shown, vec![1, 4]);
        assert_eq!(stuck, vec![2]);
    }

    // A window hanging off its old screen must still land fully on the new one, never lost off its edge.
    #[test]
    fn map_rect_stays_inside_the_new_work_area() {
        let a = rect(0, 0, 1000, 1000);
        let b = rect(1000, 0, 2000, 1000);
        assert_eq!(map_rect(a, b, rect(800, 900, 1200, 1300)), rect(1600, 600, 2000, 1000));
        assert_eq!(map_rect(a, b, rect(-50, -50, 1100, 1100)), rect(1000, 0, 2000, 1000));
    }

    // A slow screen after a wake must not fold and then come back a moment later (spec 0016 AC-18).
    #[test]
    fn after_a_wake_a_screen_must_stay_gone_longer() {
        let mut gone = HashMap::new();
        let dp = ["DEL-4123-1".to_string()];
        for _ in 0..GONE_CHECKS {
            assert!(count_gone(&mut gone, &dp, true, WAKE_GONE_CHECKS).is_empty());
        }
        for _ in GONE_CHECKS + 1..WAKE_GONE_CHECKS {
            assert!(count_gone(&mut gone, &dp, true, WAKE_GONE_CHECKS).is_empty());
        }
        assert_eq!(count_gone(&mut gone, &dp, true, WAKE_GONE_CHECKS).len(), 1);
    }

    fn spot(x: i32, state: char) -> Spot {
        Spot { rect: rect(x, 0, x + 100, 100), state }
    }

    // Windows moves windows off a screen before des-k hears it left, and no check runs while
    // the PC sleeps: only the spot from before can say where a window goes back (invariant 2).
    #[test]
    fn a_window_whose_screen_is_missing_keeps_its_spot() {
        let now: HashSet<String> = ["internal".to_string()].into();
        let old = Some(("DEL-1".to_string(), spot(3000, 'x')));
        assert_eq!(next_spot(old.clone(), "internal".into(), &now, false, || spot(0, 'n')), ("DEL-1".to_string(), spot(3000, 'x')));
        // Once away, its record holds that spot, and a claim needs where it is now.
        assert_eq!(next_spot(old, "internal".into(), &now, true, || spot(0, 'n')), ("internal".to_string(), spot(0, 'n')));
    }

    // Unplugging must never look like your move, nor Windows putting a window back on its own
    // screen; a drag between two screens that stayed is yours (spec 0016 AC-6).
    #[test]
    fn only_a_move_between_screens_that_stayed_is_a_claim() {
        let all: HashSet<String> = ["L".to_string(), "A".to_string(), "B".to_string()].into();
        let no_a: HashSet<String> = ["L".to_string(), "B".to_string()].into();
        assert!(is_claim("L", "B", "A", &no_a, &no_a), "dragged from the laptop to B while A is away");
        assert!(!is_claim("A", "L", "A", &all, &no_a), "Windows moved it off A as A left");
        assert!(!is_claim("L", "A", "A", &all, &all), "Windows put it back on its home");
        assert!(!is_claim("L", "L", "A", &no_a, &no_a), "moved on the screen it is on");
        assert!(!is_claim("B", "L", "A", &all, &no_a.iter().filter(|i| *i != "B").cloned().collect()), "B left too");
    }

    // A return is tried BACK_CHECKS checks after the screen is back, then once a check, and
    // stops after RETURN_TRIES tries so a frozen window is not moved forever (AC-5, AC-12).
    #[test]
    fn a_return_waits_then_tries_ten_times() {
        assert_eq!(try_return(1), None, "a screen that flickers on wake is not back yet");
        assert_eq!(try_return(BACK_CHECKS), Some(false));
        assert_eq!(try_return(BACK_CHECKS + RETURN_TRIES - 1), Some(true));
        assert_eq!(try_return(BACK_CHECKS + RETURN_TRIES), None);
        assert_eq!((0..100).filter_map(try_return).count(), RETURN_TRIES as usize);
    }

    // A screen whose work area changed while it was away (another resolution or taskbar) gets
    // its windows at the same relative spot; an area nobody knew still lands on the screen.
    #[test]
    fn a_return_maps_the_spot_when_the_area_changed() {
        let old = rect(0, 0, 1000, 1000);
        let new = rect(0, 0, 2000, 1000);
        assert_eq!(remap(old, new, rect(500, 0, 1000, 500)), rect(1000, 0, 2000, 500));
        assert_eq!(remap(RECT::default(), new, rect(-500, 0, 100, 100)), rect(0, 0, 600, 100));
    }

    // Two monitors of one model with no serial must stay apart, numbered left to right (AC-1).
    #[test]
    fn twins_are_numbered_left_to_right() {
        let at = |id: &str, left| Screen { device: String::new(), id: id.into(), name: String::new(), area: rect(left, 0, left + 10, 10), full: rect(left, 0, left + 10, 10), primary: false };
        let mut list = vec![at("internal", -100), at("DEL-4123", 0), at("DEL-4123", 100)];
        number_twins(&mut list);
        assert_eq!(list.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["internal", "DEL-4123#1", "DEL-4123#2"]);
    }

    // An EDID serial or instance id with spaces or tabs would break the state.txt line.
    #[test]
    fn a_screen_id_is_one_safe_token() {
        assert_eq!(clean_id(r"DISPLAY\DEL4123\5&2a&0 UID\t"), "DISPLAY_DEL4123_5_2a_0_UID_t");
    }

    // Upgrading while docked keeps the layout: each device name becomes the id of the screen it
    // names. Lines of a screen not plugged in go once, except a hidden window's, which keeps
    // its device so load shows it rather than lose it (AC-15).
    #[test]
    fn an_old_state_file_upgrades_to_screen_ids() {
        let text = "M DISPLAY1 2\nM DISPLAY9 1\nW DISPLAY1 1 100 7\ta\tc\tt\nW DISPLAY9 0 200 8\ta\tc\tu\nV DISPLAY9\ta\tc\tv\nP DISPLAY1 300 9\ta\tc\tp\n";
        let up = |dev: &str| (dev == "DISPLAY1").then(|| "DEL-1".to_string());
        let fp = |t: &str| Some(Fp { app: "a".into(), class: "c".into(), title: t.into() });
        assert_eq!(parse_state(text, up), vec![
            Line::M { id: "DEL-1".into(), active: 2, area: RECT::default(), name: String::new() },
            Line::W { id: "DEL-1".into(), ws: 1, raw: 100, pid: 7, spot: None, fp: fp("t") },
            Line::W { id: "DISPLAY9".into(), ws: 0, raw: 200, pid: 8, spot: None, fp: fp("u") },
            Line::P { id: "DEL-1".into(), raw: 300, pid: 9, spot: None, fp: fp("p") },
        ]);
    }

    // What save writes, load reads back: the away record above all, since it is what brings a
    // window home after a crash or a power cut (AC-8).
    #[test]
    fn new_lines_read_back() {
        let away = Away { home: "DEL-4123-ABC".into(), ws: 2, spot: spot(-1920, 'x'), pinned: true, pid: 44 };
        let text = format!(
            "M DEL-4123-ABC 1 0 0 2560 1400\tDELL U2720Q\nA {} 2 77 44 {} 1\ta\tc\tt\nV internal {}\ta\tc\tv\n",
            away.home,
            away.spot.text(),
            Spot { rect: RECT::default(), state: 'n' }.text()
        );
        let lines = parse_state(&text, |_| None);
        assert_eq!(lines[0], Line::M { id: "DEL-4123-ABC".into(), active: 1, area: rect(0, 0, 2560, 1400), name: "DELL U2720Q".into() });
        assert_eq!(lines[1], Line::A { raw: 77, away, fp: Some(Fp { app: "a".into(), class: "c".into(), title: "t".into() }) });
        assert!(matches!(&lines[2], Line::V { spot: None, .. }), "a zero size is a spot nobody knew");
    }

    // --quit after a crash shows what it can, but an away window's line stays: its screen may
    // still come back, after the next start or a reboot (AC-8).
    #[test]
    fn rescue_keeps_away_lines() {
        let text = "M DEL-1 2 0 0 100 100\tDell\nA DEL-1 1 100 7 0 0 10 10 n 0\ta\tc\tt\n";
        let (out, shown, left) = rescued(text, |_, _| Some(true));
        assert_eq!((shown, left), (1, 0));
        assert_eq!(out, "M DEL-1 0 0 0 100 100\tDell\nA DEL-1 1 100 7 0 0 10 10 n 0\ta\tc\tt\n");
    }
}
