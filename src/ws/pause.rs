// Pause (spec 0039): showing every hidden window while each keeps its workspace in `held`, hiding
// them again on resume, the 1 s retry of windows a pause could not show, and the guard every path
// that cloaks asks first, and the note a start that comes up paused shows at your first input.
use super::*;
use std::cell::Cell;

/// Whether des-k may hide a window now: never while paused (spec 0039). Every function in ws that
/// cloaks asks this first, and a test lists them.
pub(super) fn hiding_allowed(s: &State) -> bool {
    !s.paused
}

/// Whether des-k is paused. Read without the ws lock: the tray asks from inside its messages,
/// which can arrive inside a call made under that lock.
pub fn paused() -> bool {
    keys::paused()
}

pub(super) fn set_paused(s: &mut State, on: bool) {
    s.paused = on;
    keys::set_paused(on);
    if !on {
        REMIND.set(None);
    }
}

thread_local! {
    /// The tick count of a start that came up paused, while its note waits for your first input
    /// (AC-5). Main thread only.
    static REMIND: Cell<Option<u32>> = const { Cell::new(None) };
}

/// A start that came up paused: its note waits for your first input (AC-5). At logon Windows
/// takes the start balloon and drops it, so this is the reminder that reaches you.
pub fn remind_at_input() {
    REMIND.set(Some(win::get_tick_count()));
}

/// Whether the start reminder shows now: input came after the start (tick counts wrap after 49
/// days, so compared by difference), des-k is still paused, and the screen is not busy.
pub(super) fn reminder_due(start: u32, input: u32, paused: bool, busy: bool) -> bool {
    (input.wrapping_sub(start) as i32) > 0 && paused && !busy
}

/// Main's 1 s timer: shows the start reminder once it is due, on the screen under the mouse.
pub fn remind() {
    let Some(start) = REMIND.get() else { return };
    if !paused() {
        return REMIND.set(None);
    }
    let Some(input) = win::last_input() else { return };
    let (mon, dev) = under_mouse();
    if !reminder_due(start, input, true, indicator::busy(mon)) {
        return;
    }
    REMIND.set(None);
    let card = {
        let mut s = state();
        let id = id_of(&mut s, &dev);
        let active = s.monitors.get(&id).map_or(0, |m| m.active);
        card(&s, &id, active, settings::get().count_for(&id))
    };
    indicator::show(mon, card.with(Says::Paused));
    info!("starting paused: reminder shown");
}

/// The screen and workspace a held window belongs to.
pub(super) fn held_on(s: &State, raw: isize) -> Option<(String, usize)> {
    s.monitors.iter().find_map(|(id, m)| Some((id.clone(), m.held.iter().position(|l| l.iter().any(|&(w, _)| w == raw))?)))
}

/// An H line as a start reads it: screen, workspace, hwnd, pid, spot, fingerprint.
pub(super) type HeldLine = (String, usize, isize, u32, Option<Spot>, Fp);

/// The pause a start finds in state.txt (AC-10): whether it has a Z line, and then each H line
/// with a fingerprint, wherever the Z sits. Without a Z the held workspaces are forgotten, as an
/// older des-k forgets them (AC-11): none, or a running des-k would hide a window you see on its
/// old workspace.
pub(super) fn held_in(lines: &[Line]) -> (bool, Vec<HeldLine>) {
    if !lines.contains(&Line::Z) {
        return (false, Vec::new());
    }
    let held = lines
        .iter()
        .filter_map(|l| match l {
            Line::H { id, ws, raw, pid, spot, fp: Some(fp) } => Some((id.clone(), *ws, *raw, *pid, *spot, fp.clone())),
            _ => None,
        })
        .collect();
    (true, held)
}

/// The tray item and the pause key: pause, or resume while paused.
pub fn toggle() {
    if paused() { resume() } else { pause() }
}

/// Shows a window a pause holds: None if it is closed, else whether it is in view now. Logs
/// nothing: the caller says it once.
pub(super) fn show_quietly(shell: &Shell, raw: isize) -> Option<bool> {
    let h = HWND(raw as *mut _);
    if !win::is_window(Some(h)).as_bool() {
        return None;
    }
    Some(shell.hide(h, false).is_ok())
}

/// Pause (AC-2): every window des-k hid moves from `hidden` to `held`, the file is saved with its
/// Z and H lines, and only then is each one shown, top of z order first. A window that will not
/// show goes back to `hidden` and is tried again each second (`tick`).
pub fn pause() {
    crate::finder::close(true);
    crate::overview::close(true);
    {
        let mut guard = state();
        let s = &mut *guard;
        if s.paused {
            return;
        }
        set_paused(s, true);
        // Marks and a rule's watch belong to hiding; neither comes back after the pause (AC-4).
        s.wants.clear();
        s.settle.clear();
        // On a layout des-k does not know it hid nothing itself and can show nothing (spec 0030).
        if !crate::cloak::unknown() {
            for m in s.monitors.values_mut() {
                let n = m.hidden.len().max(m.held.len());
                m.held.resize(n, Vec::new());
                for (ws, list) in m.hidden.iter_mut().enumerate() {
                    m.held[ws].extend(list.drain(..).map(|raw| (raw, pid_of(HWND(raw as *mut _)))));
                }
            }
        }
        // Written before any window shows (spec 0034's order): a crash from here on leaves every
        // held window in the file, and showing is never refused, so a failed save only logs.
        save(s);
        let order = top_level();
        let at = |raw: isize| order.iter().position(|h| h.0 as isize == raw).unwrap_or(usize::MAX);
        let mut list: Vec<(String, usize, isize)> =
            s.monitors.iter().flat_map(|(id, m)| m.held.iter().enumerate().flat_map(move |(ws, l)| l.iter().map(move |&(raw, _)| (id.clone(), ws, raw)))).collect();
        list.sort_by_key(|&(_, _, raw)| at(raw));
        let shell = Shell::new().inspect_err(|e| warn!("pause: shell unavailable ({e}), hidden windows are tried again each second")).ok();
        let (mut shown, mut changed) = (0, false);
        for (id, ws, raw) in list {
            match shell.as_ref().and_then(|sh| show_quietly(sh, raw)) {
                Some(true) => shown += 1,
                // Closed while hidden: nothing to remember.
                None if shell.is_some() => {
                    s.monitors.get_mut(&id).expect("listed").held[ws].retain(|&(w, _)| w != raw);
                    changed = true;
                }
                // Still hidden, so tracked as hidden: every exit path shows it, and `tick` tries again.
                _ => {
                    let m = s.monitors.get_mut(&id).expect("listed");
                    m.held[ws].retain(|&(w, _)| w != raw);
                    m.hidden[ws].push(raw);
                    if shell.is_some() {
                        warn!("pause: could not show {}, tried again each second", who(HWND(raw as *mut _)));
                    }
                    changed = true;
                }
            }
        }
        if changed {
            save(s);
        }
        info!("paused: {shown} window(s) shown, kept on their workspaces");
    }
    crate::touchpad::drop_pending();
    crate::tray::paused_changed();
}

/// Main's 1 s timer: while paused, each window a pause could not show is tried again, with no
/// log line per try (AC-2); one that shows is held on its workspace. Nothing on a layout des-k
/// does not know, where nothing can be shown.
pub fn tick() {
    if !paused() || crate::cloak::unknown() {
        return;
    }
    let mut guard = state();
    let s = &mut *guard;
    if !s.paused || s.monitors.values().all(|m| m.hidden.iter().all(Vec::is_empty)) {
        return;
    }
    let Ok(shell) = Shell::new() else { return };
    let mut changed = false;
    for m in s.monitors.values_mut() {
        for ws in 0..m.hidden.len() {
            let before = m.hidden[ws].len();
            let (shown, stuck) = show_all(std::mem::take(&mut m.hidden[ws]), |raw| show_quietly(&shell, raw));
            changed |= stuck.len() != before;
            m.hidden[ws] = stuck;
            m.held[ws].extend(shown.into_iter().map(|raw| (raw, pid_of(HWND(raw as *mut _)))));
        }
    }
    if changed {
        save(s);
    }
}

/// A start that comes up paused (AC-10): a held window still cloaked on this Windows desktop (des-k
/// stopped between the pause's save and its shows) is shown first. One that will not show is
/// tracked as hidden, so every exit path shows it and `tick` tries again.
pub(super) fn show_held_cloaked(s: &mut State) {
    let list: Vec<(String, usize, isize)> = s
        .monitors
        .iter()
        .flat_map(|(id, m)| m.held.iter().enumerate().flat_map(move |(ws, l)| l.iter().map(move |&(raw, _)| (id.clone(), ws, raw))))
        .filter(|&(_, _, raw)| cloaked(HWND(raw as *mut _)) == 2)
        .collect();
    if list.is_empty() {
        return;
    }
    let vdm = win::co_create_instance::<IVirtualDesktopManager>(&VirtualDesktopManager, CLSCTX_ALL).ok();
    let shell = Shell::new().ok();
    for (id, ws, raw) in list {
        let h = HWND(raw as *mut _);
        // On another Windows desktop: Windows' cloak, not des-k's.
        if !vdm.as_ref().is_some_and(|v| win::is_window_on_current_virtual_desktop(v, h).is_ok_and(|b| b.as_bool())) {
            continue;
        }
        if shell.as_ref().and_then(|sh| show_quietly(sh, raw)) == Some(true) {
            continue;
        }
        let m = s.monitors.get_mut(&id).expect("listed");
        m.held[ws].retain(|&(w, _)| w != raw);
        m.hidden[ws].push(raw);
        crate::cloak::adopt(h);
        warn!("start: could not show {}, which a pause held, tried again each second", who(h));
    }
}

/// A held window as resume sees it: the screen and workspace it is held on, whether it is still
/// open with the same pid, whether it is pinned, and the present screen it is on now.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Held {
    pub(super) id: String,
    pub(super) ws: usize,
    pub(super) raw: isize,
    pub(super) open: bool,
    pub(super) pinned: bool,
    pub(super) now: Option<String>,
}

/// Each screen's workspace after resume (AC-6, AC-7): the one it showed at the pause (`active`),
/// except that with `follow` on, the screen `top` (the top app window) sits on goes to that
/// window's workspace when the window is held on another workspace of that same screen.
pub(super) fn resume_targets(active: &HashMap<String, usize>, held: &[Held], top: Option<isize>, follow: bool) -> HashMap<String, usize> {
    let mut out = active.clone();
    if let Some(h) = top.filter(|_| follow).and_then(|top| held.iter().find(|h| h.raw == top && h.open))
        && h.now.as_deref() == Some(h.id.as_str())
    {
        out.insert(h.id.clone(), h.ws);
    }
    out
}

/// The window resume follows (AC-7): the top app window, unless the focused window belongs to an
/// app des-k leaves alone. That one is the window you use, and it is never held, so nothing moves
/// (spec 0041 AC-14). Focus, not z order, so an always on top call bar you aren't using doesn't
/// block the follow.
pub(super) fn resume_top(focus_left_alone: bool, top: Option<isize>) -> Option<isize> {
    top.filter(|_| !focus_left_alone)
}

/// The windows resume hides (AC-6): held, still open with the same pid, not pinned, on the
/// screen they were held on, on a workspace other than that screen's target. The rest stay in view.
pub(super) fn resume_hides<'a>(held: &'a [Held], targets: &HashMap<String, usize>) -> Vec<&'a Held> {
    held.iter().filter(|h| h.open && !h.pinned && h.now.as_deref() == Some(h.id.as_str()) && targets.get(&h.id).is_some_and(|&t| t != h.ws)).collect()
}

/// One window in resume's hide list: the screen and workspace it hides on, and whether a pause
/// held it. AC-7's follow hides were in view at the pause, so the log counts them apart (AC-15).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Hide {
    pub(super) id: String,
    pub(super) ws: usize,
    pub(super) raw: isize,
    pub(super) held: bool,
}

/// AC-7's follow hides: on screen `id`, which resume moves to the window `keep` uses, what a
/// switch there would hide (`to_hide` over `windows`), minus every held window of any screen, which
/// AC-6 decides. Each goes onto `was`, the workspace the screen showed at the pause, as a follow
/// leaves them there.
pub(super) fn follow_hides(id: &str, was: usize, unknown: bool, windows: impl FnOnce() -> Vec<HWND>, keep: HWND, pinned: &HashSet<isize>, held: &HashSet<isize>) -> Vec<Hide> {
    to_hide(unknown, windows, Some(keep), pinned)
        .into_iter()
        .map(|h| h.0 as isize)
        .filter(|raw| !held.contains(raw))
        .map(|raw| Hide { id: id.to_string(), ws: was, raw, held: false })
        .collect()
}

/// Resume (AC-6 to AC-8): works out each screen's workspace and the windows to hide, sets that
/// state in memory and saves it before any cloak. A failed save while windows are to hide puts
/// everything back and stays paused; with nothing to hide it only logs.
pub fn resume() {
    {
        let mut guard = state();
        let s = &mut *guard;
        if !s.paused {
            return;
        }
        let windows = app_windows();
        let held: Vec<Held> = s
            .monitors
            .iter()
            .flat_map(|(id, m)| m.held.iter().enumerate().flat_map(move |(ws, l)| l.iter().map(move |&(raw, pid)| (id.clone(), ws, raw, pid))))
            .map(|(id, ws, raw, pid)| {
                let h = HWND(raw as *mut _);
                let open = win::is_window(Some(h)).as_bool() && pid_of(h) == pid;
                Held { id, ws, raw, open, pinned: s.pinned.contains(&raw), now: screen_now(s, raw) }
            })
            .collect();
        let active: HashMap<String, usize> = s.monitors.iter().map(|(id, m)| (id.clone(), m.active)).collect();
        let focus_left_alone = app_window(win::get_ancestor(win::get_foreground_window(), GA_ROOTOWNER)) == Err("left alone");
        let top = resume_top(focus_left_alone, windows.first().map(|&h| win::get_ancestor(h, GA_ROOTOWNER).0 as isize));
        let targets = resume_targets(&active, &held, top, settings::get().follow_activation);
        // The screen AC-7 moved acts as a follow there would: what was in view goes back to the
        // workspace it showed at the pause.
        let followed = top.and_then(|t| held.iter().find(|h| h.raw == t && h.open)).filter(|h| targets.get(&h.id) != active.get(&h.id));
        let all_held: HashSet<isize> = held.iter().map(|h| h.raw).collect();
        let follows = followed.and_then(|f| Some((f, mon_of(s, &f.id)?))).map_or_else(Vec::new, |(f, mon)| {
            follow_hides(&f.id, active[&f.id], crate::cloak::unknown(), || app_windows_on(mon), HWND(f.raw as *mut _), &s.pinned, &all_held)
        });
        // Top first, as a hidden list keeps them; a window app_windows does not list (on another
        // Windows desktop, hidden by its app) is not hidden, as a switch would not hide it.
        let at = |raw: isize| windows.iter().position(|h| h.0 as isize == raw);
        let mut hides: Vec<Hide> = resume_hides(&held, &targets)
            .into_iter()
            .map(|h| Hide { id: h.id.clone(), ws: h.ws, raw: h.raw, held: true })
            .chain(follows)
            .filter(|h| at(h.raw).is_some())
            .collect();
        hides.sort_by_key(|h| at(h.raw));
        let shell = match Shell::new() {
            Ok(sh) => Some(sh),
            Err(e) if !hides.is_empty() => return warn!("resume: shell unavailable ({e}), still paused"),
            Err(_) => None,
        };

        // The final state, in memory first (AC-8).
        let before: Vec<(String, usize, HeldLists)> = s.monitors.iter().map(|(id, m)| (id.clone(), m.active, m.held.clone())).collect();
        s.paused = false;
        for (id, m) in s.monitors.iter_mut() {
            if let Some(&t) = targets.get(id)
                && t != m.active
            {
                m.active = t;
                shown_changed();
            }
            m.held.iter_mut().for_each(Vec::clear);
        }
        for h in &hides {
            mon_mut(s, &h.id).hidden[h.ws].push(h.raw);
        }
        // One save, always, listing every window to hide before any cloak (spec 0034 AC-1).
        if let Err(e) = try_save(s) {
            error!("state.txt not saved, nothing hidden: {e}");
            if !hides.is_empty() {
                for h in &hides {
                    unlist(s, h.raw);
                }
                for (id, active, held) in before {
                    let m = mon_mut(s, &id);
                    if m.active != active {
                        shown_changed();
                    }
                    (m.active, m.held) = (active, held);
                }
                s.paused = true;
                CANT_SAVE.fetch_max(1, Ordering::Relaxed);
                let (mon, dev) = under_mouse();
                let id = id_of(s, &dev);
                let active = mon_mut(s, &id).active;
                indicator::show(mon, card(s, &id, active, settings::get().count_for(&id)).with(Says::NotSaved));
                return;
            }
        }
        // As a switch does: what show desktop minimized now belongs to the workspace just left.
        if let Some(f) = followed {
            mon_mut(s, &f.id).desktop.clear();
        }
        let fg = win::get_foreground_window();
        let fg_desktop = is_desktop(fg, win::get_shell_window(), &class_name(fg));
        let list: Vec<HWND> = hides.iter().map(|h| HWND(h.raw as *mut _)).collect();
        let fg_hidden = shell.as_ref().is_some_and(|sh| cloak_listed(s, sh, &list, fg));
        // Never leave keyboard focus in a window just hidden, as after a switch.
        let first = app_windows_on(win::monitor_from_window(fg, MONITOR_DEFAULTTONEAREST)).first().copied();
        match focus_after(None, win::get_foreground_window(), fg_hidden, fg_desktop, first) {
            FocusAfter::Leave | FocusAfter::Bring(_) => {}
            FocusAfter::Next(h) => {
                let _ = win::set_foreground_window(h);
            }
            FocusAfter::Desktop => {
                let _ = win::set_foreground_window(win::get_shell_window());
            }
        }
        set_paused(s, false);
        // N counts every hide; M only ever looks at held windows, so follow hides never lower it.
        let hidden_now = |h: &&Hide| hidden_on(s, h.raw).is_some();
        let hidden = hides.iter().filter(hidden_now).count();
        let stay = held.iter().filter(|h| h.open).count() - hides.iter().filter(|h| h.held).filter(hidden_now).count();
        info!("resumed: {hidden} window(s) hidden again, {stay} stay in view");
        save(s);
    }
    crate::tray::paused_changed();
}

#[cfg(test)]
mod tests;
