// Focus and attention: focusing a window, show desktop, the app stack for app_next and
// app_previous, and the marks of hidden windows that want you.
use super::*;

pub(super) fn focus(h: HWND) {
    if hung(h) {
        return;
    }
    unsafe { let _ = SetForegroundWindow(h); }
    if win::is_iconic(h).as_bool() {
        let _ = win::show_window(h, SW_RESTORE);
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
            .filter(|&h| win::is_window(Some(h)).as_bool() && win::is_iconic(h).as_bool())
            .collect();
        // Bottom first, so the old top one ends on top, with focus.
        for &h in back.iter().rev() {
            focus(h);
        }
        return info!("show desktop: {name}, {} window(s) back", back.len());
    }
    let list: Vec<HWND> = app_windows_on(mon).into_iter().filter(|&h| !win::is_iconic(h).as_bool() && !hung(h)).collect();
    let fg = unsafe { GetForegroundWindow() };
    for &h in &list {
        let _ = win::show_window(h, SW_SHOWMINNOACTIVE);
    }
    // Don't leave keyboard focus in a window we just minimized.
    if list.contains(&fg) {
        unsafe { let _ = SetForegroundWindow(win::get_shell_window()); }
    }
    m.desktop = list.iter().map(|h| h.0 as isize).collect();
    info!("show desktop: {name}, {} window(s) minimized", list.len());
}

/// What app_next and app_previous rotate: the shown workspace's windows on the screen under
/// the mouse, not minimized, top first.
pub(super) fn stack() -> (String, Vec<HWND>) {
    let (mon, name) = under_mouse();
    (name, app_windows_on(mon).into_iter().filter(|&h| !win::is_iconic(h).as_bool()).collect())
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
        let _ = win::set_window_pos(top, Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    focus(next);
    info!("app_previous: {} on {name}", who(next));
}

/// Where a window that wants you is, for its popup (spec 0022).
pub struct Popup {
    pub mon: HMONITOR,
    pub card: Card,
    pub hwnd: HWND,
    pub ws: usize,
}

/// `h` flashed: mark it if des-k hides it (or its owner, as a follow does), the first time only.
/// A popup for its screen unless that screen is unplugged (spec 0022 AC-1, AC-5 to AC-7).
pub fn attention(h: HWND) -> Option<Popup> {
    if !settings::get().attention_enabled {
        return None;
    }
    let mut s = state();
    let root = win::get_ancestor(h, GA_ROOTOWNER);
    let (raw, (id, ws)) = [h, root].into_iter().filter(|w| !w.is_invalid()).find_map(|w| Some((w.0 as isize, hidden_on(&s, w.0 as isize)?)))?;
    if !s.wants.insert(raw) {
        return None;
    }
    let hwnd = HWND(raw as *mut _);
    info!("attention: {} wants you on {id}, workspace {}", who(hwnd), ws + 1);
    let mon = mon_of(&s, &id)?;
    let active = s.monitors.get(&id).map_or(0, |m| m.active);
    Some(Popup { mon, card: card(&s, &id, active, settings::get().workspaces), hwnd, ws })
}

/// Marked windows on present screens, by device, workspace and window: the overview's dots
/// (spec 0022 AC-3).
pub fn wanted() -> Vec<(String, usize, HWND)> {
    if !settings::get().attention_enabled {
        return vec![];
    }
    let s = state();
    s.wants
        .iter()
        .filter_map(|&h| {
            let (id, ws) = hidden_on(&s, h)?;
            let p = s.present.iter().find(|p| p.id == id)?;
            Some((p.device.clone(), ws, HWND(h as *mut _)))
        })
        .collect()
}

/// `[attention] enabled` went off: every mark goes, so turning it on again brings none back
/// (spec 0022 AC-8).
pub fn forget_wants() {
    state().wants.clear();
}

pub(super) fn hidden_on(s: &State, raw: isize) -> Option<(String, usize)> {
    s.monitors.iter().find_map(|(dev, m)| Some((dev.clone(), m.hidden.iter().position(|l| l.contains(&raw))?)))
}
