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
            EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MONITORINFOEXW, MonitorFromPoint,
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
use crate::indicator;
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

struct Monitor {
    active: usize,
    hidden: Vec<Vec<isize>>, // one per workspace: the windows it hid, top of z-order first
    last_follow: Option<Instant>,
    last_switch: Option<(Instant, usize)>, // when, and the workspace it left
    desktop: Vec<isize>, // what show_desktop minimized, top first; any switch or send here clears it
}

impl Monitor {
    fn new() -> Self {
        Self::with(settings::get().workspaces)
    }

    fn with(count: usize) -> Self {
        Monitor { active: 0, hidden: vec![Vec::new(); count], last_follow: None, last_switch: None, desktop: Vec::new() }
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

#[derive(Default)]
struct State {
    monitors: HashMap<String, Monitor>, // keyed by device name, e.g. DISPLAY2
    logon: Option<(u64, i64)>,          // this sign in, saved as the L line
    restore: Option<Restore>,           // Some while a restore runs
    // Outlives the restore: the last window it hides needs the guard too.
    restored: HashMap<isize, Instant>, // windows restore hid, and when
    // The window the last stay send moved, and when; any switch clears it (spec 0004).
    sent: Option<(isize, Instant)>,
    // Screens with workspaces that are not plugged in now, and how many checks in a row.
    gone: HashMap<String, u32>,
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
            eprintln!("{} is a link, not a folder: removed, a real folder takes its place", dir.display());
            let _ = std::fs::remove_dir(&dir);
        }
        // Before the rename it was pdesk: carry its settings, layout and saved gestures over once.
        let old = base.join("pdesk");
        if !dir.exists() && old.is_dir() && !is_link(&old) && let Err(e) = std::fs::rename(&old, &dir) {
            eprintln!("could not move {} to {}: {e}", old.display(), dir.display());
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

/// Persist the layout so a restart, hard kill, or reboot keeps it. Lines:
/// `L <logon id> <logon time>`, `M <device> <active>`,
/// `W <device> <workspace> <hwnd> <pid>` TAB app TAB class TAB title (a hidden window),
/// `P <device> <hwnd> <pid>` TAB app TAB class TAB title (a pinned one, spec 0014), and
/// `V <device>` TAB app TAB class TAB title (a visible one). While a restore runs, its
/// pending entries are written too (hwnd 0), so another restart still has them.
fn save(s: &mut State) {
    save_lines(s, true);
}

/// Without fingerprints: only L and M lines, and no process opening (an exit that showed everything).
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
    for (dev, m) in &s.monitors {
        out += &format!("M {dev} {}\n", m.active);
        if !fingerprints {
            continue;
        }
        for (ws, list) in m.hidden.iter().enumerate() {
            for &h in list {
                let hwnd = HWND(h as *mut _);
                let pid = pid_of(hwnd);
                out += &format!("W {dev} {ws} {h} {pid}{}", fp(hwnd, pid));
            }
        }
        if let Some(mon) = monitor_named(dev) {
            for h in app_windows_on(mon) {
                let pid = pid_of(h);
                if s.pinned.contains(&(h.0 as isize)) {
                    out += &format!("P {dev} {} {pid}{}", h.0 as isize, fp(h, pid));
                } else {
                    out += &format!("V {dev}{}", fp(h, pid));
                }
            }
        }
    }
    if let Some(r) = s.restore.as_ref().filter(|_| fingerprints) {
        for e in r.pending.iter().map(|&i| &r.saved[i]) {
            match (e.ws, e.pin) {
                (Some(ws), _) => out += &format!("W {} {ws} 0 0{}", e.dev, line(&e.fp)),
                (None, true) => out += &format!("P {} 0 0{}", e.dev, line(&e.fp)),
                (None, false) => out += &format!("V {}{}", e.dev, line(&e.fp)),
            }
        }
    }
    s.apps = apps;
    write_state(out);
}

fn write_state(text: String) {
    // A crash or power loss mid write must never leave half a file.
    let tmp = data_dir().join("state.tmp");
    if let Err(e) = write_new(&tmp, &text).and_then(|()| std::fs::rename(&tmp, state_file())) {
        eprintln!("state.txt not saved: {e}");
    }
}

/// `des-k --quit` with no des-k running (after a crash or kill, or from the uninstaller):
/// show every window state.txt says des-k hid, if it is still that window and still
/// cloaked on this Windows desktop (the same check as `load`). Then the file is left as a
/// quit leaves it, plus the W lines of any window it could not show.
pub fn rescue() {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return println!("no saved layout, nothing to show") };
    let vdm = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok();
    let Some(shell) = shell_for_exit() else { return };
    let (out, shown, left) = rescued(&text, |raw, pid| {
        let h = HWND(raw as *mut _);
        let on_current = vdm.as_ref().is_some_and(|v| unsafe { v.IsWindowOnCurrentVirtualDesktop(h) }.is_ok_and(|b| b.as_bool()));
        if !(unsafe { IsWindow(Some(h)) }.as_bool() && pid_of(h) == pid && cloaked(h) == 2 && on_current) {
            return None; // closed, reused handle, or not hidden by des-k
        }
        Some(shell.hide(h, false).inspect_err(|e| eprintln!("could not show saved window {raw:#x}: {e}")).is_ok())
    });
    println!("des-k was not running: {shown} hidden window(s) shown, {left} could not be");
    write_state(out);
}

/// rescue's pass over state.txt, apart so it can be tested. `show` gets each saved hidden
/// window (hwnd, pid): None if it is not des-k's any more, else whether it is shown now.
/// Returns the new file, as a quit leaves it plus the W lines of windows still hidden, and
/// how many were shown and left.
fn rescued(text: &str, mut show: impl FnMut(isize, u32) -> Option<bool>) -> (String, usize, usize) {
    let (mut out, mut shown, mut left) = (String::new(), 0, 0);
    for line in text.lines() {
        match line.split('\t').next().unwrap_or("").split(' ').collect::<Vec<_>>().as_slice() {
            ["L", ..] => out += &format!("{line}\n"),
            ["M", dev, _] => out += &format!("M {dev} 0\n"),
            ["W", _, _, raw, pid] => {
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

fn present_monitors() -> HashSet<String> {
    unsafe extern "system" fn push(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        unsafe { (*(out.0 as *mut HashSet<String>)).insert(device_name(m)) };
        BOOL(1)
    }
    let mut set = HashSet::new();
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut set as *mut _ as isize)); }
    set
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

/// Switch the workspace of the monitor under the mouse by `dir` (-1 / +1). Clamps, no wrap.
pub fn switch(dir: isize) {
    let (mon, name) = under_mouse();
    let count = settings::get().workspaces;
    let mut s = state();
    let m = s.monitors.entry(name.clone()).or_insert_with(Monitor::new);
    let target = (m.active as isize + dir).clamp(0, count as isize - 1) as usize;
    if target == m.active {
        indicator::show(mon, m.active, count, None); // at the edge: still show where we are
        return;
    }
    switch_to(&mut s, mon, &name, target, None, None);
}

/// Show workspace `index` (0 based) on the monitor under the mouse. Past the count, or
/// already shown: only the indicator, so you see where you are (spec 0005).
pub fn jump(index: usize) {
    let (mon, name) = under_mouse();
    let count = settings::get().workspaces;
    let mut s = state();
    let active = s.monitors.entry(name.clone()).or_insert_with(Monitor::new).active;
    if index >= count {
        println!("jump: no workspace {} ({count} workspaces)", index + 1);
    }
    if index >= count || index == active {
        return indicator::show(mon, active, count, None);
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
    let title = title(hwnd);
    if [hwnd, owner].iter().any(|w| s.restored.get(&(w.0 as isize)).is_some_and(|t| t.elapsed() < RESTORED_IGNORE)) {
        return println!("follow: restore just hid \"{title}\", ignored");
    }
    // Windows can hand focus back to a window a stay send just hid.
    let just_sent = |&(sent, t): &(isize, Instant)| t.elapsed() < Duration::from_millis(FOLLOW_COOLDOWN_MS) && [hwnd, owner].iter().any(|w| w.0 as isize == sent);
    if s.sent.as_ref().is_some_and(just_sent) {
        return println!("follow: just sent \"{title}\", ignored");
    }
    let Some(mon) = monitor_named(&name) else {
        return eprintln!("follow: {name} not found, ignored \"{title}\"");
    };
    let m = &s.monitors[&name];
    let recent = |t: Instant| t.elapsed() < Duration::from_millis(FOLLOW_COOLDOWN_MS);
    // Windows can hand focus to a window mid cloak; going straight back would undo your switch.
    if m.last_switch.is_some_and(|(t, left)| recent(t) && left == ws) {
        return println!("follow: {name} just left workspace {}, ignored \"{title}\"", ws + 1);
    }
    if m.last_follow.is_some_and(recent) {
        return println!("follow: {name} cooling down, ignored \"{title}\"");
    }
    if switch_to(&mut s, mon, &name, ws, Some(hwnd), None) {
        s.monitors.get_mut(&name).expect("switch_to keeps the monitor").last_follow = Some(Instant::now());
        println!("follow: {name} -> workspace {}/{} for \"{title}\"", ws + 1, settings::get().workspaces);
    }
}

/// Show workspace `target` (already valid) on one monitor. `focus` is the window to leave
/// in front; `None` focuses the first shown window, but only if we hid the focused one or the
/// desktop has focus.
/// `keep` is a visible window that comes along instead of being hidden (a follow send).
/// Returns whether it switched.
fn switch_to(s: &mut State, mon: HMONITOR, name: &str, target: usize, focus: Option<HWND>, keep: Option<HWND>) -> bool {
    let count = settings::get().workspaces;
    let m = s.monitors.entry(name.to_string()).or_insert_with(Monitor::new);
    if target == m.active {
        return false;
    }
    let shell = match Shell::new() {
        Ok(sh) => sh,
        Err(e) => {
            eprintln!("shell unavailable: {e}");
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
                eprintln!("cloak {:?} failed: {e}", h.0);
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
                eprintln!("follow: could not focus \"{}\"", title(h));
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
    println!("{name}: workspace {}/{count} ({} shown)", target + 1, shown.len());
    indicator::show(mon, target, count, None);
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
    Some(shell.hide(h, false).inspect_err(|e| eprintln!("uncloak {raw:#x} failed: {e}, kept to try again")).is_ok())
}

/// Pin or unpin the focused window (spec 0014). A pinned window stays in view on every
/// workspace of the screen it is on, wherever you drag it; sending it anywhere unpins it.
/// The indicator lights every workspace while it is pinned.
pub fn pin() {
    // A dialog is never hidden itself, so its owner is what a switch would hide.
    let h = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) };
    if !app_windows().contains(&h) {
        return println!("pin: nothing to pin (\"{}\", {})", title(h), class_name(h));
    }
    let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
    let name = device_name(mon);
    let count = settings::get().workspaces;
    let mut s = state();
    let raw = h.0 as isize;
    let pinned = !s.pinned.remove(&raw);
    if pinned {
        s.pinned.insert(raw);
    }
    let active = s.monitors.entry(name.clone()).or_insert_with(Monitor::new).active;
    let how = if pinned { "pinned to every workspace".to_string() } else { format!("unpinned, stays on workspace {}", active + 1) };
    println!("pin: \"{}\" on {name} {how}", title(h));
    indicator::show_pin(mon, active, count, pinned);
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
            let Some(mon) = monitor_named(&name) else {
                return eprintln!("send: {name} not found");
            };
            (HWND(raw as *mut _), mon, name, from)
        }
        None => {
            // A dialog is never hidden itself, so its owner goes.
            let h = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOTOWNER) };
            // Only what a switch would hide: never the desktop, the taskbar, or a tool window.
            if !app_windows().contains(&h) {
                return println!("send: nothing to send (\"{}\", {})", title(h), class_name(h));
            }
            let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
            let name = device_name(mon);
            let from = s.monitors.entry(name.clone()).or_insert_with(Monitor::new).active;
            (h, mon, name, from)
        }
    };
    let target = match to {
        To::Step(dir) => (from as isize + dir).rem_euclid(count as isize) as usize,
        To::Index(i) if i < count => i,
        To::Index(i) => {
            println!("send: no workspace {} ({count} workspaces)", i + 1);
            return indicator::show(mon, s.monitors[&name].active, count, None);
        }
    };
    let t = title(h);
    if s.pinned.remove(&(h.0 as isize)) {
        println!("send: \"{t}\" unpinned, it goes to one workspace");
    }
    // Without this, a stay press would hide the window into the shown workspace's own list.
    if !is_repeat && target == from {
        println!("send: \"{t}\" already on {name} workspace {}/{count}", from + 1);
        return indicator::show(mon, from, count, None);
    }
    let line = |how: &str| format!("send: \"{t}\" {name} workspace {}/{count} -> {}/{count}, {how}", from + 1, target + 1);

    if follow {
        // Already in front, so it stays on top of the workspace it lands on.
        if switch_to(&mut s, mon, &name, target, Some(h), Some(h)) {
            println!("{}", line("follow"));
        }
        return;
    }

    let shell = || Shell::new().inspect_err(|e| eprintln!("shell unavailable: {e}")).ok();
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
            return eprintln!("send: could not hide \"{t}\": {e}");
        }
        // In a list straight away, so the panic hook can always show it again. Front = top.
        m.hidden[target].insert(0, raw);
        // It is cloaked now, so the filter skips it.
        let next = app_windows_on(mon).first().copied().unwrap_or(unsafe { GetShellWindow() });
        unsafe { let _ = SetForegroundWindow(next); }
    } else if target == active {
        let Some(shell) = shell() else { return };
        if let Err(e) = shell.hide(h, false) {
            return eprintln!("send: could not show \"{t}\": {e}");
        }
        m.hidden[from].retain(|&w| w != raw);
        unsafe { let _ = SetForegroundWindow(h); }
    } else {
        m.hidden[from].retain(|&w| w != raw);
        m.hidden[target].insert(0, raw);
    }
    m.desktop.clear();
    s.sent = Some((raw, Instant::now()));
    indicator::show(mon, active, count, (target != active).then_some(target));
    println!("{}", line("stay"));
    save(&mut s);
}

/// A listed window and where it lives, for the finder (spec 0007).
pub struct Place {
    pub hwnd: HWND,
    pub device: String,
    pub workspace: usize, // 0 based
    pub on_screen: bool,
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
            let workspace = s.monitors.entry(device.clone()).or_insert_with(Monitor::new).active;
            Place { hwnd: h, device, workspace, on_screen: true }
        })
        .collect();
    let mut devices: Vec<&String> = s.monitors.keys().collect();
    devices.sort();
    for dev in devices {
        for (ws, list) in s.monitors[dev].hidden.iter().enumerate() {
            for &raw in list {
                let h = HWND(raw as *mut _);
                if unsafe { IsWindow(Some(h)) }.as_bool() {
                    out.push(Place { hwnd: h, device: dev.clone(), workspace: ws, on_screen: false });
                }
            }
        }
    }
    out
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
    state().monitors.get(device).map_or(0, |m| m.active)
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
    let mut devs: Vec<&String> = s.monitors.keys().collect();
    devs.sort_by_key(|d| (order.iter().position(|o| o == *d).unwrap_or(usize::MAX), d.to_string()));
    let mut out = Vec::new();
    for dev in devs {
        let m = &s.monitors[dev];
        let (mut from, mut n) = (Vec::new(), 0);
        for ws in count..m.hidden.len() {
            let k = if ws == m.active {
                monitor_named(dev).map_or(0, |mon| app_windows_on(mon).len())
            } else {
                m.hidden[ws].iter().filter(|&&h| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool()).count()
            };
            if k > 0 {
                from.push((ws + 1).to_string());
                n += k;
            }
        }
        if n == 0 {
            continue;
        }
        let name = order.iter().position(|o| o == dev).map_or(dev.clone(), |i| format!("Screen {}", i + 1));
        let (win, go) = if n == 1 { ("window", "moves") } else { ("windows", "move") };
        out.push(format!("{name}: {n} {win} from {} {go} to {count}", from.join(", ")));
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
    let shell = Shell::new();
    for (dev, m) in s.monitors.iter_mut() {
        let show = m.resize(count);
        if show.is_empty() {
            continue;
        }
        let Ok(shell) = &shell else {
            // Kept with the shown workspace: the next switch away and back shows them.
            eprintln!("shell unavailable, {dev}: {} windows stay hidden", show.len());
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

/// Each active display's name by GDI device name (DISPLAY1), from the display config.
fn display_names() -> HashMap<String, String> {
    use windows::Win32::Devices::Display::*;
    let mut out = HashMap::new();
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
            let name = if [DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED].contains(&tech) {
                "Built-in display".to_string()
            } else {
                let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME { header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                    adapterId: p.targetInfo.adapterId,
                    id: p.targetInfo.id,
                }, ..Default::default() };
                if DisplayConfigGetDeviceInfo(&mut tgt.header) == 0 { text(&tgt.monitorFriendlyDeviceName) } else { String::new() }
            };
            out.entry(device).or_insert(name);
        }
    }
    out
}

/// What the settings page draws of the layout, copied out so it paints with no lock held.
#[derive(Clone, PartialEq, Default)]
pub struct View {
    pub active: Vec<(String, usize)>,       // device and the workspace it shows
    pub windows: Vec<(String, usize, RECT)>, // device, workspace, where it sits
    pub removals: Vec<Vec<String>>,          // removal's lines, one list per count asked
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
    let active = s.monitors.iter().map(|(d, m)| (d.clone(), m.active)).collect();
    let removals = counts.iter().map(|&n| moves(&s, n)).collect();
    Some(View { active, windows, removals })
}

/// Show workspace `ws` on `device`, exactly like a switch there: the indicator shows, and
/// focus moves on only if the focused window got hidden (spec 0010).
pub fn show_workspace(device: &str, ws: usize) {
    if ws >= settings::get().workspaces {
        return;
    }
    let Some(mon) = monitor_named(device) else { return eprintln!("overview: {device} not found") };
    let mut s = state();
    switch_to(&mut s, mon, device, ws, None, None);
}

/// Where a window really sits, in screen coordinates; a minimized one where it restores to.
pub fn real_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    unsafe {
        if !IsIconic(hwnd).as_bool() {
            let _ = GetWindowRect(hwnd, &mut r);
            return r;
        }
        let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        let _ = GetWindowPlacement(hwnd, &mut wp);
        // rcNormalPosition is in workspace coordinates, as in move_to.
        let primary = work_area(MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY));
        let n = wp.rcNormalPosition;
        RECT { left: n.left + primary.left, top: n.top + primary.top, right: n.right + primary.left, bottom: n.bottom + primary.top }
    }
}

/// Why the finder could not act on a window.
#[derive(Debug, PartialEq)]
pub enum Miss {
    Gone,
    CantMove,
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
        eprintln!("\"{}\" is not responding, left as it is", title(h));
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
    let m = s.monitors.entry(name.clone()).or_insert_with(Monitor::new);
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
        return println!("show desktop: {name}, {} window(s) back", back.len());
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
    println!("show desktop: {name}, {} window(s) minimized", list.len());
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
        return println!("app_next: no window on {name}");
    };
    focus(h);
    println!("app_next: \"{}\" on {name}", title(h));
}

/// The top window goes to the bottom and the new top one gets focus (spec 0011).
pub fn app_previous() {
    let (name, stack) = stack();
    let Some(&top) = stack.first() else {
        return println!("app_previous: no window on {name}");
    };
    let next = stack.get(1).copied().unwrap_or(top);
    if next != top && !hung(top) {
        unsafe { let _ = SetWindowPos(top, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE); }
    }
    focus(next);
    println!("app_previous: \"{}\" on {name}", title(next));
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
    let t = title(hwnd);
    let (name, ws) = match hidden_on(&s, hwnd.0 as isize) {
        Some((name, ws)) => {
            // Its screen left: check_screens shows it if the screen stays gone.
            let Some(mon) = monitor_named(&name) else { return Err(Miss::Gone) };
            switch_to(&mut s, mon, &name, ws, Some(hwnd), None);
            (name, ws)
        }
        None => {
            let name = device_name(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) });
            let ws = s.monitors.get(&name).map_or(0, |m| m.active);
            (name, ws)
        }
    };
    focus(hwnd);
    println!("finder: -> \"{t}\" ({name} workspace {})", ws + 1);
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
    let t = title(hwnd);
    let hidden = hidden_on(&s, raw);
    let from = match &hidden {
        Some((dev, _)) => dev.clone(),
        None => device_name(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }),
    };
    let land = |h: HWND| if activate { focus(h) } else { raise(h) };
    if hidden.is_none() && from == device {
        land(hwnd);
        return Ok(());
    }
    let shell = Shell::new().map_err(|e| {
        eprintln!("{who}: shell unavailable: {e}");
        Miss::CantMove
    })?;
    if from != device {
        let (Some(old), Some(new)) = (monitor_named(&from), monitor_named(device)) else {
            eprintln!("{who}: {from} or {device} not found");
            return Err(Miss::CantMove);
        };
        // Still cloaked if hidden, so it never flashes on its old screen.
        if let Err(e) = move_to(hwnd, old, new) {
            eprintln!("{who}: could not move \"{t}\": {e}");
            return Err(Miss::CantMove);
        }
    }
    if let Some((dev, ws)) = &hidden {
        if let Err(e) = shell.hide(hwnd, false) {
            eprintln!("{who}: could not show \"{t}\": {e}");
            return Err(Miss::CantMove);
        }
        s.monitors.get_mut(dev).expect("found above").hidden[*ws].retain(|&w| w != raw);
    }
    land(hwnd);
    println!("{who}: brought \"{t}\" to {device}");
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
    let t = title(hwnd);
    let hidden = hidden_on(&s, raw);
    if hidden.as_ref().is_some_and(|(dev, w)| dev == device && *w == ws) {
        return Ok(());
    }
    let from = match &hidden {
        Some((dev, _)) => dev.clone(),
        None => device_name(unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) }),
    };
    let shell = Shell::new().map_err(|e| {
        eprintln!("overview: shell unavailable: {e}");
        Miss::CantMove
    })?;
    if hidden.is_none()
        && let Err(e) = shell.hide(hwnd, true)
    {
        // The cloak can work and only the taskbar step fail: undo it, so nothing changes.
        if cloaked(hwnd) != 0 {
            let _ = shell.hide(hwnd, false);
        }
        eprintln!("overview: could not hide \"{t}\": {e}");
        return Err(Miss::CantMove);
    }
    if from != device {
        let moved = match (monitor_named(&from), monitor_named(device)) {
            (Some(old), Some(new)) => move_to(hwnd, old, new).map_err(|e| format!("could not move \"{t}\": {e}")),
            _ => Err(format!("{from} or {device} not found")),
        };
        if let Err(e) = moved {
            if hidden.is_none() {
                let _ = shell.hide(hwnd, false);
            }
            eprintln!("overview: {e}");
            return Err(Miss::CantMove);
        }
    }
    if let Some((dev, w)) = &hidden {
        s.monitors.get_mut(dev).expect("found above").hidden[*w].retain(|&x| x != raw);
    }
    if s.pinned.remove(&raw) {
        println!("overview: \"{t}\" unpinned, it goes to one workspace");
    }
    // Front = top, as a send puts it.
    s.monitors.entry(device.to_string()).or_insert_with(Monitor::new).hidden[ws].insert(0, raw);
    println!("overview: \"{t}\" -> {device} workspace {}/{count}, hidden", ws + 1);
    save(&mut s);
    Ok(())
}

/// Move a window from one screen's work area to the same relative place on another's.
/// Maximized stays maximized; minimized comes back restored.
fn move_to(hwnd: HWND, old: HMONITOR, new: HMONITOR) -> windows::core::Result<()> {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    unsafe { GetWindowPlacement(hwnd, &mut wp)? };
    // rcNormalPosition is in workspace coordinates: offset by the primary screen's work area.
    let primary = work_area(unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) });
    let shift = |r: RECT, sign: i32| RECT {
        left: r.left + sign * primary.left,
        top: r.top + sign * primary.top,
        right: r.right + sign * primary.left,
        bottom: r.bottom + sign * primary.top,
    };
    let to = map_rect(work_area(old), work_area(new), shift(wp.rcNormalPosition, 1));
    let maximized = unsafe { IsZoomed(hwnd) }.as_bool();
    wp.rcNormalPosition = shift(to, -1);
    wp.flags = WINDOWPLACEMENT_FLAGS(0);
    wp.showCmd = if maximized { SW_SHOWMAXIMIZED.0 as u32 } else { SW_SHOWNOACTIVATE.0 as u32 };
    unsafe { SetWindowPlacement(hwnd, &wp)? };
    if maximized && unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) } != new {
        // Some windows stay maximized where they were: restore onto the new screen, then maximize there.
        wp.showCmd = SW_SHOWNOACTIVATE.0 as u32;
        unsafe {
            SetWindowPlacement(hwnd, &wp)?;
            let _ = ShowWindow(hwnd, SW_SHOWMAXIMIZED);
        }
    }
    if !maximized {
        // An app that rescales itself on the new screen's DPI can drift off the mapped size.
        let mut now = RECT::default();
        unsafe { let _ = GetWindowRect(hwnd, &mut now); }
        if now != to {
            unsafe { let _ = SetWindowPos(hwnd, None, to.left, to.top, to.right - to.left, to.bottom - to.top, SWP_NOZORDER | SWP_NOACTIVATE); }
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
    eprintln!("shell unavailable for 5 s: hidden windows stay hidden, state.txt keeps them");
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
        m.active = 0;
    }
    if left > 0 {
        eprintln!("{left} window(s) could not be shown: state.txt keeps them, the next start shows or adopts them");
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
            eprintln!("panic while the layout was locked: showing every window des-k cloaked");
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
    let mut s = state();
    s.logon = restore::logon();
    let Ok(text) = std::fs::read_to_string(state_file()) else { return false };
    let present = present_monitors();
    let st = settings::get();
    let count = st.workspaces;
    let (mut saved_logon, mut adopt, mut hidden, mut visible, mut pins) = (None, Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut pinned = Vec::new();
    for line in text.lines() {
        // The fingerprint follows the first tab; a W line without one is from an older des-k.
        let (head, fp) = match line.split_once('\t') {
            Some((head, tail)) => match tail.splitn(3, '\t').collect::<Vec<_>>().as_slice() {
                [app, class, title] => (head, Some(Fp { app: app.to_string(), class: class.to_string(), title: title.to_string() })),
                _ => (head, None),
            },
            None => (line, None),
        };
        match head.split(' ').collect::<Vec<_>>().as_slice() {
            ["L", id, time] => saved_logon = u64::from_str_radix(id, 16).ok().zip(time.parse::<i64>().ok()),
            ["M", dev, active] if present.contains(*dev) => {
                // An active workspace that no longer exists starts at the first one.
                let a = active.parse::<usize>().ok().filter(|&a| a < count).unwrap_or(0);
                s.monitors.entry(dev.to_string()).or_insert_with(Monitor::new).active = a;
            }
            ["W", dev, ws, raw, pid] => {
                let (Ok(ws), Ok(raw), Ok(pid)) = (ws.parse::<usize>(), raw.parse::<isize>(), pid.parse::<u32>()) else {
                    continue;
                };
                if let Some(fp) = fp {
                    hidden.push((Entry { dev: dev.to_string(), ws: Some(ws), pin: false, fp }, raw == 0));
                }
                if raw != 0 {
                    adopt.push((dev.to_string(), ws, raw, pid));
                }
            }
            ["V", dev] => visible.extend(fp.map(|fp| Entry { dev: dev.to_string(), ws: None, pin: false, fp })),
            ["P", dev, raw, pid] => {
                let (Ok(raw), Ok(pid)) = (raw.parse::<isize>(), pid.parse::<u32>()) else { continue };
                if let Some(fp) = fp {
                    pinned.push((Entry { dev: dev.to_string(), ws: None, pin: true, fp }, raw == 0));
                }
                if raw != 0 {
                    pins.push((raw, pid));
                }
            }
            _ => {}
        }
    }

    let new_sign_in = saved_logon.is_some() && s.logon.is_some() && saved_logon != s.logon;
    let cut_short = hidden.iter().any(|(_, cut)| *cut);
    let since = s.logon.map_or(0, |(_, t)| SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64) - t);
    let late = since > (st.restore_minutes as i64 + 2) * 60;
    if st.restore_enabled && new_sign_in && late {
        println!("restore: signed in {} min ago, skipped", since / 60);
    }
    let restoring = st.restore_enabled && if new_sign_in { !late } else { cut_short };
    // A restart in the same sign in: the same window, same pid, is still pinned.
    if !new_sign_in {
        let same = |&(raw, pid): &(isize, u32)| unsafe { IsWindow(Some(HWND(raw as *mut _))) }.as_bool() && pid_of(HWND(raw as *mut _)) == pid;
        s.pinned.extend(pins.iter().filter(|p| same(p)).map(|&(raw, _)| raw));
    }
    if restoring {
        // A new sign in: every saved handle is dead. A resume: only the cut entries are left.
        if new_sign_in {
            adopt.clear();
        }
        let cut = |(e, cut): (Entry, bool)| (new_sign_in || cut).then_some(e);
        let saved: Vec<Entry> = hidden.into_iter().filter_map(cut).chain(visible).chain(pinned.into_iter().filter_map(cut)).collect();
        println!("restore: {} {} saved window(s), matching for {} min", if new_sign_in { "new sign in," } else { "resuming," }, saved.len(), st.restore_minutes);
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
            eprintln!("shell unavailable, saved layout not loaded");
            return restoring;
        };
        let (mut kept, mut shown) = (0, 0);
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
                _ => match shell.hide(h, false) {
                    Ok(()) => shown += 1,
                    Err(e) => {
                        // Still hidden: tracked, so a switch, quit or screen check tries again.
                        eprintln!("could not show saved window {raw:#x}: {e}, kept to try again");
                        let m = s.monitors.entry(dev).or_insert_with(Monitor::new);
                        let ws = ws.min(m.hidden.len() - 1);
                        m.hidden[ws].push(raw);
                        crate::cloak::adopt(h);
                    }
                },
            }
        }
        println!("loaded saved layout: {kept} hidden window(s) adopted, {shown} shown again");
    }
    if !s.pinned.is_empty() {
        println!("loaded saved layout: {} pinned window(s)", s.pinned.len());
    }
    save(&mut s);
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
                        println!("restore: unclear match for \"{}\" ({}), left visible", fp.title, fp.app);
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
                println!("restore: \"{}\" stays visible, {why}", fp.title);
            };
            let (dev, Some(ws)) = (r.saved[i].dev.clone(), r.saved[i].ws) else {
                if r.saved[i].pin {
                    s.pinned.insert(h.0 as isize);
                    use_up(r, "pinned to every workspace, as before the restart");
                } else {
                    use_up(r, "as before the restart");
                }
                continue;
            };
            match s.monitors.get_mut(&dev) {
                None => use_up(r, &format!("{dev} is not connected")),
                Some(_) if ws >= count => use_up(r, &format!("workspace {} no longer exists", ws + 1)),
                Some(m) if m.active == ws => use_up(r, &format!("{dev} already shows workspace {}", ws + 1)),
                // Apps often open on the main screen and move a moment later.
                Some(_) if device_name(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) }) != dev => {}
                Some(m) => {
                    let was_fg = unsafe { GetForegroundWindow() } == h;
                    // A brand new window fails to cloak for a moment: nothing used up, next tick retries.
                    if shell.hide(h, true).is_err() {
                        // The cloak can work and only the taskbar step fail: undo it, or nothing tracks it.
                        if cloaked(h) != 0 {
                            let _ = shell.hide(h, false);
                        }
                        continue;
                    }
                    m.hidden[ws].insert(0, h.0 as isize);
                    s.restored.retain(|_, t| t.elapsed() < RESTORED_IGNORE);
                    s.restored.insert(h.0 as isize, Instant::now());
                    r.pending.retain(|&p| p != i);
                    r.resolved.insert(key);
                    r.hidden += 1;
                    changed = true;
                    println!("restore: \"{}\" -> {dev} workspace {}/{count}", fp.title, ws + 1);
                    if was_fg {
                        let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
                        let next = app_windows_on(mon).first().copied().unwrap_or(unsafe { GetShellWindow() });
                        unsafe {
                            let _ = SetForegroundWindow(next);
                            if GetForegroundWindow() != next {
                                eprintln!("restore: could not move focus");
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
    println!("restore: done, {} hidden, {} kept visible, {} saved window(s) not matched", r.hidden, r.kept, r.pending.len());
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
                        eprintln!("explorer restarted: could not hide {raw:#x} again ({e}), it stays in view");
                        in_view += 1;
                        return false;
                    }
                    Err(e) => {
                        eprintln!("explorer restarted: could not hide {raw:#x} again ({e}), kept to try again");
                        failed += 1;
                    }
                }
                true
            });
        }
    }
    println!("explorer restarted: {hidden} hidden window(s) hidden again, {in_view} left in view, {failed} to try again");
    if failed == 0 {
        RESYNC.store(0, Ordering::Relaxed);
    }
    if in_view > 0 {
        save(&mut s);
    }
}

/// check_screens' bookkeeping, apart so it can be tested: `missing` are the screens with
/// workspaces that are not plugged in now. Returns those gone GONE_CHECKS ticks in a row
/// or more, with their count.
fn count_gone(gone: &mut HashMap<String, u32>, missing: &[String], tick: bool) -> Vec<(String, u32)> {
    for dev in gone.keys().filter(|d| !missing.contains(*d)) {
        println!("{dev} is back: its workspaces were kept");
    }
    gone.retain(|d, _| missing.contains(d));
    let mut due = Vec::new();
    for dev in missing {
        let n = gone.entry(dev.clone()).or_insert_with(|| {
            println!("{dev} disconnected: its hidden windows are shown if it stays gone {GONE_CHECKS} s");
            0
        });
        if tick {
            *n += 1;
        }
        if *n >= GONE_CHECKS {
            due.push((dev.clone(), *n));
        }
    }
    due
}

/// A monitor that disappears (undocking) takes its hidden workspaces with it, so show those
/// windows, but only once it stayed gone GONE_CHECKS checks in a row. Called on every display
/// change (`tick` false: only notes what is gone or back) and by main's 1 s watch timer
/// (`tick` true: counts), which also catches a display change lost in a menu's modal loop.
pub fn check_screens(tick: bool) {
    let present = present_monitors();
    let mut guard = state();
    let s = &mut *guard;
    let missing: Vec<String> = s.monitors.keys().filter(|d| !present.contains(*d)).cloned().collect();
    let due = count_gone(&mut s.gone, &missing, tick);
    if due.is_empty() {
        return;
    }
    let Ok(shell) = Shell::new() else { return eprintln!("shell unavailable on display change, trying again") };
    for (dev, n) in due {
        let Some(m) = s.monitors.get_mut(&dev) else { continue };
        let mut left = 0;
        for list in &mut m.hidden {
            let (_, stuck) = show_all(std::mem::take(list), |raw| uncloak(&shell, raw));
            left += stuck.len();
            *list = stuck;
        }
        if left == 0 {
            s.monitors.remove(&dev);
            s.gone.remove(&dev);
            println!("{dev} disconnected: its hidden windows are shown again");
        } else if n == GONE_CHECKS {
            // Still tracked: every later check tries again.
            eprintln!("{dev} disconnected: {left} window(s) could not be shown, trying again each second");
        }
    }
    save(s);
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
            assert!(count_gone(&mut gone, &dp, false).is_empty());
        }
        for _ in 1..GONE_CHECKS {
            assert!(count_gone(&mut gone, &dp, true).is_empty());
        }
        assert_eq!(count_gone(&mut gone, &dp, true), vec![("DISPLAY2".to_string(), GONE_CHECKS)]);
    }

    // Two short drop outs never add up to one long one: a screen that comes back starts at zero.
    #[test]
    fn a_screen_that_comes_back_starts_counting_again() {
        let mut gone = HashMap::new();
        let dp = ["DISPLAY2".to_string()];
        for _ in 1..GONE_CHECKS {
            count_gone(&mut gone, &dp, true);
        }
        assert!(count_gone(&mut gone, &[], true).is_empty());
        assert!(gone.is_empty());
        for _ in 1..GONE_CHECKS {
            assert!(count_gone(&mut gone, &dp, true).is_empty());
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
}
