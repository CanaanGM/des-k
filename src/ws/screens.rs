// Screens: their ids, names and twins, the 1 s screen check, settling, folding a gone screen's
// windows onto the main display and giving them back, claims and spots, and resizing the count.
use super::*;

/// A present screen as des-k knows it (spec 0016): Windows' device name, the monitor's own id,
/// its friendly name, its work area, and whether Windows marks it as the main display.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Screen {
    pub(super) device: String,
    pub(super) id: String,
    pub(super) name: String,
    pub(super) area: RECT,
    pub(super) full: RECT,
    pub(super) primary: bool,
}

/// A line for the indicator on screen `id` (`device`), showing `active`: made under the lock,
/// shown after it. `line` is the card's (spec 0017 AC-IND-4), `text` the tray balloon's.
pub(super) struct Note {
    pub(super) device: String,
    pub(super) id: String,
    pub(super) active: usize,
    pub(super) line: Says,
    pub(super) text: String,
}

// When the PC last woke (spec 0016 AC-18). Its own lock: the indicator hears the wake inside
// any call that pumps sent messages, which can run under the ws lock.
pub(super) static WOKE: Mutex<Option<Instant>> = Mutex::new(None);

/// The PC woke from sleep: screens get longer to come back before their windows fold.
pub fn woke() {
    *WOKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

pub(super) fn gone_checks() -> u32 {
    let recent = WOKE.lock().unwrap_or_else(|e| e.into_inner()).is_some_and(|t| t.elapsed() < WAKE_GRACE);
    if recent { WAKE_GONE_CHECKS } else { GONE_CHECKS }
}

/// You moved an away window yourself (spec 0016 AC-6): it stays where you put it.
pub(super) fn claim(s: &mut State, raw: isize, why: &str) {
    s.settle.remove(&raw);
    if let Some(a) = s.away.remove(&raw) {
        let name = s.monitors.get(&a.home).map_or(a.home.clone(), |m| m.name.clone());
        info!("{} stays where it is when {name} returns: {why}", who(HWND(raw as *mut _)));
    }
}

pub(super) fn words(n: usize) -> String {
    if n == 1 { "1 window".into() } else { format!("{n} windows") }
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
pub(super) fn screen_ids() -> Vec<Screen> {
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
                    let id = clean_id(&crate::restore::edid_id(&t.path)?);
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
pub(super) fn clean_id(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || "-_.#".contains(c) { c } else { '_' }).collect()
}

/// Two present monitors with one id (no serial) get `#1`, `#2` in `list`'s order, left to
/// right (spec 0016 AC-1).
pub(super) fn number_twins(list: &mut [Screen]) {
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

pub(super) fn log_screens(list: &[Screen]) {
    for p in list {
        info!("screen {}: {} \"{}\"{}", p.id, p.device, p.name, if p.primary { ", main display" } else { "" });
    }
}

pub(super) fn monitor_named(name: &str) -> Option<HMONITOR> {
    unsafe extern "system" fn push(m: HMONITOR, _: HDC, _: *mut RECT, out: LPARAM) -> BOOL {
        unsafe { (*(out.0 as *mut Vec<HMONITOR>)).push(m) };
        BOOL(1)
    }
    let mut all = Vec::new();
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut all as *mut _ as isize)); }
    all.into_iter().find(|&m| device_name(m) == name)
}

/// What lowering the count to `count` would move: one line per screen with something to
/// move, as "Screen 2: 3 windows from 7, 8 move to 6"; a screen not plugged in now goes under
/// its device name (spec 0012 AC-6). The windows on screen count for the workspace shown.
pub(super) fn moves(s: &State, count: usize) -> Vec<String> {
    let order = screen_order();
    let mut out = Vec::new();
    // Logged (settings::store::apply), so a screen not plugged in goes by Windows' name, never a rename.
    for m in moved(s, count, |s, id| s.monitors.get(id).map_or_else(|| id.to_string(), |m| m.name.clone())).into_iter().filter(|m| m.windows > 0) {
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
    pub name: String, // as `label` gives it for the page; Windows' name for the log
    pub device: Option<String>,
    pub unplugged: bool,
    pub windows: usize,
    pub from: Vec<usize>, // the removed workspaces with windows, 0 based
}

pub(super) fn moved(s: &State, count: usize, name: fn(&State, &str) -> String) -> Vec<Moved> {
    let order = screen_order();
    let at = |m: &Monitor| m.device.as_ref().and_then(|d| order.iter().position(|o| o == d));
    let mut ids: Vec<&String> = s.monitors.keys().collect();
    ids.sort_by_key(|id| (at(&s.monitors[*id]).unwrap_or(usize::MAX), id.to_string()));
    let live = |raw: isize| win::is_window(Some(HWND(raw as *mut _))).as_bool();
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
        out.push(Moved { name: name(s, id), device: m.device.clone(), unplugged, windows, from });
    }
    out
}

/// Gives every screen `count` workspaces (spec 0012 AC-6). Raising adds empty ones. Lowering
/// moves the windows of every removed workspace onto the new last one; a screen showing a
/// removed one switches to the new last one with all of them in view, elsewhere they stay
/// hidden. Returns `removal`'s lines, as they were before the move. Main thread, from
/// `settings::store::apply` only, never under `state()`.
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

/// One active display config path: its GDI device name (DISPLAY1), whether it is a laptop
/// panel, the monitor's friendly name and its device path.
pub(super) struct Target {
    pub(super) device: String,
    pub(super) internal: bool,
    pub(super) friendly: String,
    pub(super) path: String,
}

pub(super) fn targets() -> Vec<Target> {
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
pub(super) fn display_names() -> HashMap<String, String> {
    targets().into_iter().map(|t| (t.device, if t.internal { "Built-in display".to_string() } else { t.friendly })).collect()
}

/// A plugged in screen's name as Windows gives it, before any rename or twin number.
pub(super) fn windows_name(p: &Screen) -> String {
    if p.name.is_empty() { p.device.trim_start_matches(r"\\.\").to_string() } else { p.name.clone() }
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

/// Where a window really sits, in screen coordinates; a minimized one where it restores to.
pub fn real_rect(hwnd: HWND) -> RECT {
    let mut r = RECT::default();
    if win::is_iconic(hwnd).as_bool() {
        return normal_rect(hwnd);
    }
    let _ = win::get_window_rect(hwnd, &mut r);
    r
}

/// Where a window restores to, in screen coordinates.
pub(super) fn normal_rect(hwnd: HWND) -> RECT {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    let _ = win::get_window_placement(hwnd, &mut wp);
    shift(wp.rcNormalPosition, 1)
}

/// rcNormalPosition is in workspace coordinates: `sign` 1 makes it screen coordinates, -1 the reverse.
pub(super) fn shift(r: RECT, sign: i32) -> RECT {
    let primary = work_area(unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) });
    RECT { left: r.left + sign * primary.left, top: r.top + sign * primary.top, right: r.right + sign * primary.left, bottom: r.bottom + sign * primary.top }
}

/// A window's spot now (spec 0016). A cloaked window reads the same: cloaking does not move it.
pub(super) fn spot_of(h: HWND) -> Spot {
    let (min, max) = (win::is_iconic(h).as_bool(), win::is_zoomed(h).as_bool());
    let state = if min { 'm' } else if max { 'x' } else { 'n' };
    let rect = if state == 'n' { real_rect(h) } else { normal_rect(h) };
    Spot { rect, state }
}

/// check_screens' bookkeeping, apart so it can be tested: `missing` are the screens with
/// workspaces that are not plugged in now. Returns those gone `limit` ticks in a row or more,
/// with their count.
pub(super) fn count_gone(gone: &mut HashMap<String, u32>, missing: &[String], tick: bool, limit: u32) -> Vec<(String, u32)> {
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

pub(super) fn check_in(s: &mut State, present: Vec<Screen>, tick: bool) -> Vec<Note> {
    let mut notes = Vec::new();
    let now: HashSet<String> = present.iter().map(|p| p.id.clone()).collect();
    if now != s.seen {
        log_screens(&present);
    }
    let layout = layout_of(&present);
    s.calm = count_calm(s.calm, layout == s.layout, tick);
    s.layout = layout;
    // While Windows is still moving windows between screens, a spot read now is half a move.
    let settled = s.calm >= SETTLE_CHECKS;
    if s.present.iter().map(|p| (&p.id, p.full)).ne(present.iter().map(|p| (&p.id, p.full))) {
        shown_changed();
    }
    s.present = present;
    for p in &s.present {
        if let Some(m) = s.monitors.get_mut(&p.id) {
            m.device = Some(p.device.clone());
            m.name = p.name.clone();
        }
    }
    // Claims compare against the last check's spots, so both checks must be settled.
    let mut dirty = settled && s.spots_fresh && claims(s, &now);
    let before = s.away.len();
    s.away.retain(|&raw, a| {
        let open = win::is_window(Some(HWND(raw as *mut _))).as_bool() && pid_of(HWND(raw as *mut _)) == a.pid;
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
        if tick && settled {
            e.0 += 1;
        }
        // Not while another screen is still coming: Windows would move them again after.
        if settled && let Some(last) = try_return(e.0) {
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

    // Spots and areas are read together, so a screen that leaves later hands back a matching pair.
    if settled {
        refresh_spots(s, &now);
        for p in &s.present {
            if let Some(m) = s.monitors.get_mut(&p.id)
                && !s.away.values().any(|a| a.home == p.id)
            {
                m.area = p.area;
            }
        }
    }
    s.spots_fresh = settled;
    s.seen = now;
    if dirty {
        save(s);
    }
    notes
}

/// Whether a screen back for `n` checks tries to take its windows back now, and whether this
/// is the last try (spec 0016 AC-5, AC-12).
pub(super) fn try_return(n: u32) -> Option<bool> {
    (BACK_CHECKS..BACK_CHECKS + RETURN_TRIES).contains(&n).then_some(n == BACK_CHECKS + RETURN_TRIES - 1)
}

/// Which screens are in and where their work areas sit: the layout that must hold to settle.
pub(super) fn layout_of(present: &[Screen]) -> Vec<(String, RECT)> {
    present.iter().map(|p| (p.id.clone(), p.area)).collect()
}

/// Checks in a row the layout held: any change starts again, and only 1 s checks count, so a
/// burst of display changes never settles it early.
pub(super) fn count_calm(calm: u32, same: bool, tick: bool) -> u32 {
    if !same { 0 } else if tick { calm.saturating_add(1) } else { calm }
}

/// Whether an away window's change of screen is your move (spec 0016 AC-6). Windows moves
/// windows off a screen that left and back onto a screen that returns; only a move between two
/// screens that were there at the last check and are there now, not onto its home, is yours.
pub(super) fn is_claim(prev: &str, cur: &str, home: &str, seen: &HashSet<String>, now: &HashSet<String>) -> bool {
    cur != prev && cur != home && seen.contains(prev) && now.contains(prev) && now.contains(cur)
}

/// The present screen a window is on now: its list's for a hidden one, else the screen its
/// rect is on; None while it is on no screen (Windows has not moved it off a gone one yet).
pub(super) fn screen_now(s: &State, raw: isize) -> Option<String> {
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
pub(super) fn claims(s: &mut State, now: &HashSet<String>) -> bool {
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
pub(super) fn mark_away(s: &mut State, id: &str) -> usize {
    let Some(m) = s.monitors.get(id) else { return 0 };
    let hidden: HashSet<isize> = m.hidden.iter().flatten().copied().collect();
    let active = m.active;
    let list: Vec<(isize, Spot)> = s.spots.iter().filter(|(raw, (sid, _))| sid == id && !hidden.contains(raw)).map(|(&raw, &(_, sp))| (raw, sp)).collect();
    let mut n = 0;
    for (raw, spot) in list {
        let h = HWND(raw as *mut _);
        if !win::is_window(Some(h)).as_bool() || s.away.contains_key(&raw) {
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
pub(super) fn fold(s: &mut State, id: &str, notes: &mut Vec<Note>) {
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
            if !win::is_window(Some(h)).as_bool() {
                continue;
            }
            let spot = fold_spot(s.spots.get(&raw), id, || spot_of(h));
            // A window already away keeps its first home (invariant 3).
            s.away.entry(raw).or_insert(Away { home: id.to_string(), ws, spot, pinned: false, pid: pid_of(h) });
            // Still cloaked, so it never flashes.
            if !hung(h)
                && let Err(e) = put(h, Spot { rect: remap(from, main.area, spot.rect), state: spot.state })
            {
                warn!("fold: could not move {}: {e}", who(h));
            }
            s.spots.insert(raw, (main.id.clone(), spot_of(h)));
            match fold_to(ws, count, main_active) {
                FoldTo::Show(_) => show.push(raw),
                FoldTo::Hide(ws) => mon_mut(s, &main.id).hidden[ws].push(raw),
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
            line: Says::ScreenGone { name: label(s, id), moved: waiting, to: Some(label(s, &main.id)) },
            text: format!("{} unplugged: {} moved here", label(s, id), words(waiting)),
        });
    }
}

/// Where a folded window goes on the main display, from workspace `ws` of the screen that left.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum FoldTo {
    /// Into view: its workspace is the one the main display shows.
    Show(usize),
    /// Hidden into that workspace's list.
    Hide(usize),
}

/// The same workspace on the main display, capped at its last one, in view when it is the one
/// shown (`main_active`).
pub(super) fn fold_to(ws: usize, count: usize, main_active: usize) -> FoldTo {
    let ws = ws.min(count - 1);
    if ws == main_active { FoldTo::Show(ws) } else { FoldTo::Hide(ws) }
}

/// A folded window's spot on screen `id`, which just left: the one stored at the last check when
/// it was stored for that screen, since Windows may have moved the window off it already.
/// Otherwise `read` it now.
pub(super) fn fold_spot(stored: Option<&(String, Spot)>, id: &str, read: impl FnOnce() -> Spot) -> Spot {
    match stored {
        Some((sid, sp)) if sid == id => *sp,
        _ => read(),
    }
}

/// A screen back BACK_CHECKS checks (spec 0016 AC-5): each of its away windows goes back to its
/// workspace and spot there, in view on the workspace it shows, hidden on the others. Never
/// takes focus (AC-14). A window that does not move keeps its record for the next try (AC-12).
/// Returns whether anything changed.
pub(super) fn give_back(s: &mut State, id: &str, last: bool, notes: &mut Vec<Note>) -> bool {
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
    let unknown = crate::cloak::unknown();
    // Each window's way home, worked out before any moves: a hung one counts as a try.
    // (hwnd, record, home workspace, goes home in view, the list it is in now)
    let mine: Vec<_> = s
        .away
        .iter()
        .filter(|&(&raw, a)| a.home == id && !hung(HWND(raw as *mut _)))
        .map(|(&raw, a)| {
            let (ws, in_view) = goes_in_view(a, count, active, unknown);
            (raw, a.clone(), ws, in_view, hidden_on(s, raw))
        })
        .collect();
    // The ones in view now that go home hidden: listed and saved before any is cloaked (spec 0034
    // AC-1). Refused: nothing moves, each keeps its away record, and the next check tries again.
    let list: Vec<(String, usize, isize)> = mine.iter().filter(|m| !m.3 && m.4.is_none()).map(|m| (id.to_string(), m.2, m.0)).collect();
    if list_then_save(s, &list).is_err() {
        return false;
    }
    // A listed window whose hide then failed comes off its list: the closing save must say so.
    let (mut moved, mut changed, mut fg_hidden) = (0, !list.is_empty(), false);
    for (raw, a, ws, in_view, hidden) in mine {
        let h = HWND(raw as *mut _);
        let to = home_spot(&a, saved, p.area);
        let way = way_home(in_view, hidden.is_some(), || screen_now(s, raw).as_deref() == Some(id) && spot_of(h) == to);
        let done = match (way, hidden) {
            (Way::InPlace, _) => Some(false),
            (Way::Move, _) => put(h, to).inspect_err(|e| warn!("return: could not move {}: {e}", who(h))).ok().map(|()| true),
            (Way::MoveShow, Some((sid, w))) => {
                let ok = put(h, to).inspect_err(|e| warn!("return: could not move {}: {e}", who(h))).is_ok() && uncloak(&shell, raw) == Some(true);
                if ok {
                    s.monitors.get_mut(&sid).expect("listed").hidden[w].retain(|&x| x != raw);
                }
                ok.then_some(true)
            }
            (Way::MoveRelist, Some((sid, w))) => {
                // Still cloaked: moved, then listed with its home workspace.
                let ok = put(h, to).inspect_err(|e| warn!("return: could not move {}: {e}", who(h))).is_ok();
                if ok {
                    s.monitors.get_mut(&sid).expect("listed").hidden[w].retain(|&x| x != raw);
                    mon_mut(s, id).hidden[ws].push(raw);
                }
                ok.then_some(true)
            }
            // way_home gives these two only for a window hidden now; never reached.
            (Way::MoveShow | Way::MoveRelist, None) => None,
            (Way::HideMove, _) => {
                // Hidden first, then moved, so it never flashes on its way home. Already listed
                // on its home workspace above; off again if it ends up in view where it was.
                if let Err(e) = shell.hide(h, true) {
                    let undo = || {
                        let _ = shell.hide(h, false);
                    };
                    // One that may still be cloaked stays listed (AGENTS.md); its record stays too.
                    if failed_hide(|| cloaked(h), undo) {
                        unlist(s, raw);
                    }
                    warn!("return: could not hide {}: {e}", who(h));
                    None
                } else if let Err(e) = put(h, to) {
                    warn!("return: could not move {}: {e}", who(h));
                    if shell.hide(h, false).is_err() {
                        // Still cloaked: it stays listed, so its home workspace takes it.
                        Some(true)
                    } else {
                        unlist(s, raw);
                        None
                    }
                } else {
                    fg_hidden |= h == fg;
                    Some(true)
                }
            }
        };
        if let Some(did) = done {
            let landed = spot_of(h);
            if did {
                // Where it should be and where it is: an app that resizes itself on arrival shows here.
                info!("return: {} to {}, landed at {}", who(h), to.text(), landed.text());
            }
            s.away.remove(&raw);
            s.spots.insert(raw, (id.to_string(), landed));
            if a.pinned {
                s.pinned.insert(raw);
            }
            moved += did as usize;
            changed = true;
        }
    }
    if fg_hidden {
        // Never leave keyboard focus in a window just hidden.
        let next = app_windows().first().copied().unwrap_or(win::get_shell_window());
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
                text: format!("{} is back: {} returned{stuck}", label(s, &p.id), words(total)),
            });
        }
    }
    changed
}

/// An away window's home workspace (capped at the last one) and whether it goes home in view:
/// when it was pinned, when that workspace is the one its screen shows (`active`), or on a layout
/// des-k does not know, where nothing can be hidden (spec 0030 AC-6).
pub(super) fn goes_in_view(a: &Away, count: usize, active: usize, unknown: bool) -> (usize, bool) {
    let ws = a.ws.min(count - 1);
    (ws, a.pinned || ws == active || unknown)
}

/// Where an away window goes on its screen: its saved spot as is while the work area is the one
/// saved, else remapped into the area it has now.
pub(super) fn home_spot(a: &Away, saved: RECT, now: RECT) -> Spot {
    Spot { rect: if saved == now { a.spot.rect } else { remap(saved, now, a.spot.rect) }, state: a.spot.state }
}

/// How an away window gets home.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Way {
    /// Already home and in place (a short drop Windows undid): nothing to move.
    InPlace,
    /// In view and goes home in view: moved.
    Move,
    /// Hidden and goes home in view: moved, then shown.
    MoveShow,
    /// Hidden and goes home hidden: moved, then listed with its home workspace.
    MoveRelist,
    /// In view and goes home hidden: hidden first, then moved, so it never flashes.
    HideMove,
}

/// `in_view` says it goes home in view, `hidden` that it is hidden now. `in_place` reads where it
/// is, and is called only for a window in view that stays in view.
pub(super) fn way_home(in_view: bool, hidden: bool, in_place: impl FnOnce() -> bool) -> Way {
    match (in_view, hidden) {
        (true, false) if in_place() => Way::InPlace,
        (true, false) => Way::Move,
        (true, true) => Way::MoveShow,
        (false, true) => Way::MoveRelist,
        (false, false) => Way::HideMove,
    }
}

/// map_rect, from an area nobody knew (an old line) into `to` as it is.
pub(super) fn remap(from: RECT, to: RECT, r: RECT) -> RECT {
    if from.right > from.left && from.bottom > from.top { map_rect(from, to, r) } else { map_rect(to, to, r) }
}

/// Reads each tracked window's spot (spec 0016): the windows in view on a present screen and
/// every listed hidden one. A window whose last screen is missing keeps the spot it had there,
/// unless it is away already: its record holds that spot, and claims need where it is now.
pub(super) fn refresh_spots(s: &mut State, now: &HashSet<String>) {
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
        if s.away.contains_key(&raw) && win::is_window(Some(HWND(raw as *mut _))).as_bool() {
            fresh.entry(raw).or_insert(old);
        }
    }
    s.spots = fresh;
}

/// One window's spot after a check: frozen while its last screen is missing (invariant 2).
pub(super) fn next_spot(old: Option<(String, Spot)>, id: String, now: &HashSet<String>, away: bool, read: impl FnOnce() -> Spot) -> (String, Spot) {
    match old {
        Some(old) if !now.contains(&old.0) && !away => old,
        _ => (id, read()),
    }
}

#[cfg(test)]
mod tests;
