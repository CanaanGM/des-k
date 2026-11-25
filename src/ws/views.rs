// Window queries (app_windows, who, cloaked, titles) and what the finder, the overview, the tray
// and the settings page read about screens and workspaces.
use super::*;

// Bumped whenever a screen's shown workspace or the set of present screens changes: main
// hands the backdrop a new wish only then (spec 0026), so no switch pays for an unchanged one.
pub(crate) static SHOWN: AtomicU64 = AtomicU64::new(0);

pub(super) fn shown_changed() {
    SHOWN.fetch_add(1, Ordering::Relaxed);
}

/// Each present screen's full rect, id and shown workspace, left to right as Windows lists them.
pub(crate) fn shown() -> Vec<(RECT, String, usize)> {
    let s = state();
    s.present.iter().map(|p| (p.full, p.id.clone(), s.monitors.get(&p.id).map_or(0, |m| m.active))).collect()
}
/// Screen `id`'s workspace names, one per workspace in use, empty for none (spec 0023 AC-1).
pub(super) fn names_of(id: &str, count: usize) -> Vec<String> {
    let set = settings::get();
    (0..count).map(|i| set.ws_name(id, i).unwrap_or_default().to_string()).collect()
}

/// `device`'s workspace names, for the overview, which knows screens by device name (spec 0023
/// AC-7). An unknown device has no names.
/// The screen id of a present screen's device name, for the overview's pictures (spec 0026).
pub(crate) fn id(device: &str) -> String {
    id_of(&mut state(), device)
}

pub fn names(device: &str) -> Vec<String> {
    let count = settings::get().workspaces;
    let mut s = state();
    let id = id_of(&mut s, device);
    if s.present.iter().any(|p| p.id == id) { names_of(&id, count) } else { vec![String::new(); count] }
}

pub(super) fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0;
    win::get_window_thread_process_id(hwnd, Some(&mut pid));
    pid
}

pub(crate) fn title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = win::get_window_text(hwnd, &mut buf);
    match n {
        1.. => String::from_utf16_lossy(&buf[..n as usize]),
        _ => "(no title)".into(),
    }
}

pub(super) fn cloaked(hwnd: HWND) -> u32 {
    let mut v = 0u32;
    let _ = win::dwm_get_window_attribute(hwnd, DWMWA_CLOAKED, &mut v);
    v
}

pub(crate) fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = win::get_class_name(hwnd, &mut buf);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// A window as a log line names it: its exe file name, never its title, which can be an email
/// subject or a document name (spec 0019). `window 0x…` when its process can't be read.
pub(crate) fn who(hwnd: HWND) -> String {
    let mut pid = 0;
    win::get_window_thread_process_id(hwnd, Some(&mut pid));
    crate::restore::exe_path(pid)
        .and_then(|p| Path::new(&p).file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("window {:#X}", hwnd.0 as usize))
}

pub(super) fn app_windows_on(mon: HMONITOR) -> Vec<HWND> {
    app_windows().into_iter().filter(|&h| win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST) == mon).collect()
}

/// Visible, uncloaked app windows on every screen (Alt+Tab-like filter from spikes/listwin), top of z-order first.
pub(crate) fn app_windows() -> Vec<HWND> {
    top_level().into_iter().filter(|&h| app_window(h).is_ok()).collect()
}

/// Every top level window, shown or not, top of z order first.
pub(super) fn top_level() -> Vec<HWND> {
    win::enum_windows()
}

/// Whether `h` is the kind of window app_windows lists, else what it lacks (an app rule logs
/// that, spec 0027).
pub(crate) fn app_window(h: HWND) -> Result<(), &'static str> {
    let ex = win::get_window_long_ptr(h, GWL_EXSTYLE) as u32;
    let why = match () {
        _ if !win::is_window_visible(h).as_bool() => "not visible",
        _ if win::get_window(h, GW_OWNER).is_ok_and(|o| !o.is_invalid()) => "owned by another window",
        _ if ex & WS_EX_TOOLWINDOW.0 != 0 => "a tool window",
        _ if win::get_window_text_length(h) == 0 => "no title",
        _ if cloaked(h) != 0 => "cloaked",
        // Never hide the desktop or taskbars.
        _ if h == win::get_shell_window() || matches!(class_name(h).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") => "the shell",
        // Never des-k's own settings page or welcome screen: a hidden one is one the
        // tray's Settings would focus where you cannot see it.
        _ if pid_of(h) == std::process::id() => "des-k's own",
        _ => return Ok(()),
    };
    Err(why)
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
    pub ws_name: String, // its workspace's name on its screen; empty for none, and when pinned (spec 0023)
}

/// Every listed window on screen (front to back), then every live window des-k hid, by
/// screen, workspace and saved order.
pub fn windows() -> Vec<Place> {
    windows_in(&mut state())
}

pub(super) fn windows_in(s: &mut State) -> Vec<Place> {
    let mut out: Vec<Place> = app_windows()
        .into_iter()
        .map(|h| {
            let device = device_name(win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST));
            let id = id_of(s, &device);
            let workspace = mon_mut(s, &id).active;
            let pinned = s.pinned.contains(&(h.0 as isize));
            let ws_name = if pinned { String::new() } else { ws_name(&id, workspace) };
            Place { hwnd: h, device, workspace, on_screen: true, screen: label(s, &id), pinned, unplugged: false, ws_name }
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
                if win::is_window(Some(h)).as_bool() {
                    out.push(Place { hwnd: h, device: dev.clone(), workspace: ws, on_screen: false, screen: screen.clone(), pinned: false, unplugged, ws_name: ws_name(id, ws) });
                }
            }
        }
    }
    out
}

pub(super) fn ws_name(id: &str, ws: usize) -> String {
    settings::get().ws_name(id, ws).unwrap_or_default().to_string()
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
    pub ws_name: String, // the shown workspace's name; empty for none (spec 0023 AC-10)
}

/// Every present screen, left to right, copied out so the menu paints with no lock held.
pub fn tray_screens() -> Vec<TrayScreen> {
    let full = screens(); // before the lock: it asks Windows
    let s = state();
    s.present
        .iter()
        .map(|p| {
            let active = s.monitors.get(&p.id).map_or(0, |m| m.active);
            TrayScreen {
                name: label(&s, &p.id),
                rect: full.iter().find(|(d, _)| *d == p.device).map_or(p.area, |(_, r)| *r),
                active,
                main: p.primary,
                ws_name: ws_name(&p.id, active),
            }
        })
        .collect()
}

/// Every present screen's device and full rectangle.
pub fn screens() -> Vec<(String, RECT)> {
    win::enum_display_monitors().into_iter().map(|(m, r)| (device_name(m), r)).collect()
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

pub(super) fn in_order(mut all: Vec<(String, RECT)>) -> Vec<String> {
    all.sort_by_key(|(_, r)| (r.left, r.top));
    all.into_iter().map(|(dev, _)| dev).collect()
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
    use windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI;
    let names = display_names();
    // By id, read without the ws lock: the settings page asks from inside a switch too.
    let ids: HashMap<String, String> = screen_ids().into_iter().map(|p| (p.device, p.id)).collect();
    let set = settings::get();
    let mut all = screens();
    all.sort_by_key(|(_, r)| (r.left, r.top));
    all.into_iter()
        .enumerate()
        .map(|(i, (device, rect))| {
            let (mut dx, mut dy) = (96, 96);
            if let Some(mon) = monitor_named(&device) {
                let _ = win::get_dpi_for_monitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            }
            let rename = ids.get(&device).and_then(|id| set.screen_rename(id));
            let name = rename.or(names.get(&device).map(String::as_str).filter(|n| !n.is_empty())).unwrap_or(&device).to_string();
            ScreenInfo { device, number: i + 1, name, rect, scale: dx * 100 / 96 }
        })
        .collect()
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
    /// Every listed window, front first: its hwnd, its screen's device, and its workspace (a
    /// hidden one's own, not the one its screen shows). The Apps section reads their apps with no
    /// lock held (spec 0027 AC-13).
    pub open: Vec<(isize, String, usize)>,
}

/// A screen plugged in, for the settings page's Where it shows: its id keys its own place.
#[derive(Clone, PartialEq, Debug)]
pub struct Present {
    pub id: String,
    pub device: String,
    pub name: String, // as `label` gives it, so twins read #1, #2 from the left
    pub windows_name: String, // Windows' own, no rename: the settings page's Screen name hint (spec 0023)
    pub full: RECT,
    pub work: RECT,
}

/// A remembered screen not plugged in, with its away windows per workspace (spec 0017 AC-SET-3).
#[derive(Clone, PartialEq, Default, Debug)]
pub struct Waiting {
    pub id: String,
    pub name: String,
    pub windows_name: String, // Windows' own, as remembered, no rename (spec 0023)
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
    let places = windows_in(&mut s);
    let windows = places.iter().map(|p| (p.device.clone(), p.workspace, real_rect(p.hwnd))).collect();
    let open = places.iter().map(|p| (p.hwnd.0 as isize, p.device.clone(), p.workspace)).collect();
    let active = s.monitors.values().filter_map(|m| Some((m.device.clone()?, m.active))).collect();
    let names = s.present.iter().map(|p| (p.device.clone(), label(&s, &p.id))).collect();
    let removals = counts.iter().map(|&n| moved(&s, n, label)).collect();
    let present = s.present.iter().map(|p| Present { id: p.id.clone(), device: p.device.clone(), name: label(&s, &p.id), windows_name: windows_name(p), full: p.full, work: p.area }).collect();
    Some(View { active, windows, names, removals, waiting: waiting(&s), present, open })
}

/// Every remembered screen not plugged in, by name, with how many windows wait for it on each workspace.
pub(super) fn waiting(s: &State) -> Vec<Waiting> {
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
            Waiting { id: id.clone(), name: label(s, id), windows_name: m.name.clone(), windows }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    out
}

/// A window whose app stopped answering (paused in a debugger, say): ShowWindow and
/// SetWindowPos would wait on it with no time limit, with des-k's lock held.
pub(super) fn hung(h: HWND) -> bool {
    let hung = win::is_hung_app_window(h).as_bool();
    if hung {
        warn!("{} is not responding, left as it is", who(h));
    }
    hung
}

/// How many windows every hidden list holds, for the tray's warning (spec 0030 AC-10); None
/// while the layout is locked, and the warning then leaves the count out.
pub fn hidden_count() -> Option<usize> {
    let s = STATE.try_lock().ok()?;
    Some(s.monitors.values().flat_map(|m| &m.hidden).map(Vec::len).sum())
}

#[cfg(test)]
mod tests;
