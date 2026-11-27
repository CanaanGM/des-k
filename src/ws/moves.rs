// Moving windows: pin, send, go to and bring for the finder and the overview, place, and put.
use super::*;

/// Pin or unpin the focused window (spec 0014). A pinned window stays in view on every
/// workspace of the screen it is on, wherever you drag it; sending it anywhere unpins it.
/// The indicator lights every workspace while it is pinned.
pub fn pin() {
    // A dialog is never hidden itself, so its owner is what a switch would hide.
    let h = win::get_ancestor(win::get_foreground_window(), GA_ROOTOWNER);
    match app_window(h) {
        Ok(()) => {}
        Err("left alone") => return alone::say_left_alone(&mut state(), h, "pin"),
        Err(_) => return info!("pin: nothing to pin ({}, {})", who(h), class_name(h)),
    }
    let mon = win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST);
    let mut s = state();
    let name = id_of(&mut s, &device_name(mon));
    let count = settings::get().count_for(&name);
    s.last_acted = Some(name.clone());
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
    // A send that cannot hide would only move a window between lists (spec 0030 AC-6).
    if crate::cloak::unknown() {
        return info!("send: hiding is off on this Windows, nothing sent");
    }
    let mut s = state();
    // Nothing hides while paused (spec 0039); the keys are off then too.
    if !hiding_allowed(&s) {
        return;
    }
    let recent = |t: Instant| t.elapsed() < Duration::from_millis(FOLLOW_COOLDOWN_MS);
    // A quick second stay press moves the window the last one sent, not the one that got focus.
    let repeat = s
        .sent
        .filter(|&(raw, t)| !follow && recent(t) && win::is_window(Some(HWND(raw as *mut _))).as_bool())
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
            let h = win::get_ancestor(win::get_foreground_window(), GA_ROOTOWNER);
            // Only what a switch would hide: never the desktop, the taskbar, or a tool window.
            match app_window(h) {
                Ok(()) => {}
                Err("left alone") => return alone::say_left_alone(&mut s, h, "send"),
                Err(_) => return info!("send: nothing to send ({}, {})", who(h), class_name(h)),
            }
            let mon = win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST);
            let name = id_of(&mut s, &device_name(mon));
            let from = mon_mut(&mut s, &name).active;
            (h, mon, name, from)
        }
    };
    s.last_acted = Some(name.clone());
    // The focused window's screen's own count (spec 0040 AC-4).
    let count = settings::get().count_for(&name);
    let target = match to {
        To::Step(dir) => step_target(from, dir, count),
        To::Index(i) if i < count => i,
        To::Index(i) => {
            info!("send: no workspace {} ({count} workspaces)", i + 1);
            return indicator::show(mon, card(&s, &name, s.monitors[&name].active, count).with(Says::NoWorkspace { n: i + 1 }));
        }
    };
    let t = who(h);
    let raw = h.0 as isize;
    // Without this, a stay press would hide the window into the shown workspace's own list.
    if !is_repeat && target == from {
        if s.pinned.remove(&raw) {
            info!("send: {t} unpinned, it goes to one workspace");
        }
        info!("send: {t} already on {name} workspace {}/{count}", from + 1);
        return indicator::show(mon, card(&s, &name, from, count).with(Says::Already));
    }
    let line = |how: &str| format!("send: {t} {name} workspace {}/{count} -> {}/{count}, {how}", from + 1, target + 1);
    // Only once its move stands, so a send refused for a failed save changes nothing (spec 0034 AC-2).
    let sent = |s: &mut State| {
        let unpinned = s.pinned.remove(&raw);
        if unpinned {
            info!("send: {t} unpinned, it goes to one workspace");
        }
        claim(s, raw, "you sent it");
        unpinned
    };

    if follow {
        let marked = s.pinned.contains(&raw) || s.away.contains_key(&raw);
        // Already in front, so it stays on top of the workspace it lands on.
        if switch_to(&mut s, mon, &name, target, Some(h), Some(h), None) == Switch::Switched {
            sent(&mut s);
            info!("{}", line("follow"));
            if marked {
                save(&mut s);
            }
        }
        return;
    }

    let shell = || Shell::new().inspect_err(|e| warn!("shell unavailable: {e}")).ok();
    let active = s.monitors[&name].active;
    let unpinned;
    if !is_repeat {
        let Some(shell) = shell() else { return };
        // Listed and saved before the cloak (spec 0034 AC-1). Front = top.
        if list_then_save(&mut s, &[(name.clone(), target, raw)]).is_err() {
            return indicator::show(mon, card(&s, &name, active, count).with(Says::NotSaved));
        }
        if let Err(e) = shell.hide(h, true) {
            // The cloak can work and only the taskbar step fail: undo it, so nothing changes.
            if cloaked(h) != 0 {
                let _ = shell.hide(h, false);
            }
            // A window that may still be cloaked is never dropped (AGENTS.md).
            if cloaked(h) == 0 {
                unlist(&mut s, raw);
            }
            save(&mut s);
            return warn!("send: could not hide {t}: {e}");
        }
        #[cfg(feature = "drill")]
        crate::drill::at(crate::drill::Stop::KillAfterSend);
        unpinned = sent(&mut s);
        // It is cloaked now, so the filter skips it.
        let next = app_windows_on(mon).first().copied().unwrap_or(win::get_shell_window());
        let _ = win::set_foreground_window(next);
    } else if target == active {
        let Some(shell) = shell() else { return };
        if let Err(e) = shell.hide(h, false) {
            return error!("send: could not show {t}: {e}");
        }
        unpinned = sent(&mut s);
        s.monitors.get_mut(&name).expect("found above").hidden[from].retain(|&w| w != raw);
        let _ = win::set_foreground_window(h);
    } else {
        unpinned = sent(&mut s);
        let m = s.monitors.get_mut(&name).expect("found above");
        m.hidden[from].retain(|&w| w != raw);
        m.hidden[target].insert(0, raw);
    }
    s.monitors.get_mut(&name).expect("found above").desktop.clear();
    s.sent = Some((raw, Instant::now()));
    let said = if target == active { Says::SentBack } else { Says::Sent { to: target, unpinned } };
    indicator::show(mon, card(&s, &name, active, count).with(said));
    info!("{}", line("stay"));
    save(&mut s);
}

/// Why the finder could not act on a window.
#[derive(Debug, PartialEq)]
pub enum Miss {
    Gone,
    CantMove,
    Unplugged, // hidden on a screen that just left: check_screens folds it soon (spec 0017 AC-FND-7)
}

pub(super) fn work_area(mon: HMONITOR) -> RECT {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = win::get_monitor_info(mon, &mut mi);
    mi.rcWork
}

/// Land on a window wherever it is: a hidden one switches its screen to its workspace,
/// exactly like a switch; one on screen just gets focus (spec 0007).
pub fn go_to(hwnd: HWND) -> Result<(), Miss> {
    if !win::is_window(Some(hwnd)).as_bool() {
        return Err(Miss::Gone);
    }
    let mut s = state();
    let t = who(hwnd);
    let (name, ws) = match hidden_on(&s, hwnd.0 as isize) {
        Some((name, ws)) => {
            // Its screen left: check_screens folds it onto the main display if the screen stays gone.
            let Some(mon) = mon_of(&s, &name) else { return Err(Miss::Unplugged) };
            // Refused: it stays hidden, and focus must not go to it.
            if switch_to(&mut s, mon, &name, ws, Some(hwnd), None, None) == Switch::Refused {
                return Err(Miss::CantMove);
            }
            (name, ws)
        }
        None => {
            let name = id_of(&mut s, &device_name(win::monitor_from_window(hwnd, MONITOR_DEFAULTTONEAREST)));
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
pub(super) fn show_on(hwnd: HWND, device: &str, activate: bool, who: &str) -> Result<(), Miss> {
    if !win::is_window(Some(hwnd)).as_bool() {
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
        None => device_name(win::monitor_from_window(hwnd, MONITOR_DEFAULTTONEAREST)),
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
pub(super) fn raise(h: HWND) {
    if hung(h) {
        return;
    }
    if win::is_iconic(h).as_bool() {
        let _ = win::show_window(h, SW_SHOWNOACTIVATE);
    }
    let _ = win::set_window_pos(h, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
}

/// Put a window on workspace `ws` of `device` without switching any screen (an overview
/// drop, spec 0010 AC-7): on the shown workspace it comes to the front, otherwise it is
/// hidden there, moved across screens while cloaked so it never flashes. On failure nothing
/// about the window or the state has changed.
pub fn place(hwnd: HWND, device: &str, ws: usize) -> Result<(), Miss> {
    // Only a workspace that screen has (spec 0040 AC-4).
    let count = views::count(device);
    if ws >= count {
        return Err(Miss::CantMove);
    }
    // Its own lock: show_on takes the state again.
    if ws == active(device) {
        return show_on(hwnd, device, false, "overview");
    }
    if !win::is_window(Some(hwnd)).as_bool() {
        return Err(Miss::Gone);
    }
    let mut s = state();
    // Nothing hides while paused (spec 0039); the overview is closed then too.
    if !hiding_allowed(&s) {
        return Err(Miss::CantMove);
    }
    let raw = hwnd.0 as isize;
    let t = who(hwnd);
    let hidden = hidden_on(&s, raw);
    if hidden.as_ref().is_some_and(|(dev, w)| dev == device && *w == ws) {
        return Ok(());
    }
    let from = match &hidden {
        Some((id, _)) => s.monitors[id].device.clone().unwrap_or_else(|| id.clone()),
        None => device_name(win::monitor_from_window(hwnd, MONITOR_DEFAULTTONEAREST)),
    };
    let shell = Shell::new().map_err(|e| {
        warn!("overview: shell unavailable: {e}");
        Miss::CantMove
    })?;
    let id = id_of(&mut s, device);
    if hidden.is_none() {
        // Listed and saved before the cloak (spec 0034 AC-1). Front = top, as a send puts it.
        if list_then_save(&mut s, &[(id.clone(), ws, raw)]).is_err() {
            let active = mon_mut(&mut s, &id).active;
            if let Some(mon) = monitor_named(device) {
                indicator::show(mon, card(&s, &id, active, count).with(Says::NotSaved));
            }
            return Err(Miss::CantMove);
        }
        if let Err(e) = shell.hide(hwnd, true) {
            // The cloak can work and only the taskbar step fail: undo it, so nothing changes.
            if cloaked(hwnd) != 0 {
                let _ = shell.hide(hwnd, false);
            }
            if cloaked(hwnd) == 0 {
                unlist(&mut s, raw);
            }
            save(&mut s);
            warn!("overview: could not hide {t}: {e}");
            return Err(Miss::CantMove);
        }
    }
    if from != device {
        let moved = match (monitor_named(&from), monitor_named(device)) {
            (Some(old), Some(new)) => move_to(hwnd, old, new).map_err(|e| format!("could not move {t}: {e}")),
            _ => Err(format!("{from} or {device} not found")),
        };
        if let Err(e) = moved {
            if hidden.is_none() {
                // Shown again where it was, so off the list it was saved in.
                if shell.hide(hwnd, false).is_ok() {
                    unlist(&mut s, raw);
                }
                save(&mut s);
            }
            warn!("overview: {e}");
            return Err(Miss::CantMove);
        }
    }
    if let Some((dev, w)) = &hidden {
        s.monitors.get_mut(dev).expect("found above").hidden[*w].retain(|&x| x != raw);
        // Front = top, as a send puts it.
        mon_mut(&mut s, &id).hidden[ws].insert(0, raw);
    }
    if s.pinned.remove(&raw) {
        info!("overview: {t} unpinned, it goes to one workspace");
    }
    claim(&mut s, raw, "you dropped it on a workspace");
    info!("overview: {t} -> {device} workspace {}/{count}, hidden", ws + 1);
    save(&mut s);
    Ok(())
}

/// Move a window from one screen's work area to the same relative place on another's.
/// Maximized stays maximized; minimized comes back restored.
pub(super) fn move_to(hwnd: HWND, old: HMONITOR, new: HMONITOR) -> windows::core::Result<()> {
    let rect = map_rect(work_area(old), work_area(new), normal_rect(hwnd));
    put(hwnd, Spot { rect, state: if win::is_zoomed(hwnd).as_bool() { 'x' } else { 'n' } })
}

/// Place a window at `to`: its rect and normal, maximized or minimized (spec 0016). Never
/// takes focus: a maximize that does gives it straight back.
pub(super) fn put(hwnd: HWND, to: Spot) -> windows::core::Result<()> {
    let mut wp = WINDOWPLACEMENT { length: size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
    win::get_window_placement(hwnd, &mut wp)?;
    let fg = win::get_foreground_window();
    wp.rcNormalPosition = shift(to.rect, -1);
    wp.flags = WINDOWPLACEMENT_FLAGS(0);
    wp.showCmd = match to.state {
        'x' => SW_SHOWMAXIMIZED.0 as u32,
        'm' => SW_SHOWMINNOACTIVE.0 as u32,
        _ => SW_SHOWNOACTIVATE.0 as u32,
    };
    win::set_window_placement(hwnd, &wp)?;
    if to.state == 'x' {
        let new = win::monitor_from_rect(&to.rect, MONITOR_DEFAULTTONEAREST);
        if win::monitor_from_window(hwnd, MONITOR_DEFAULTTONEAREST) != new {
            // Some windows stay maximized where they were: restore onto the new screen, then maximize there.
            wp.showCmd = SW_SHOWNOACTIVATE.0 as u32;
            win::set_window_placement(hwnd, &wp)?;
            let _ = win::show_window(hwnd, SW_SHOWMAXIMIZED);
        }
        // Crossing to a screen of another scaling, Windows rescales the restore rect, and each
        // trip shrank it (1080x1872, 720x1248, 522x832; tried 2026-11-22). On its screen now, so no rescale.
        if normal_rect(hwnd) != to.rect {
            wp.showCmd = SW_SHOWMAXIMIZED.0 as u32;
            win::set_window_placement(hwnd, &wp)?;
        }
        // There is no maximize without activation.
        if win::get_foreground_window() != fg && !fg.is_invalid() {
            let _ = win::set_foreground_window(fg);
        }
    }
    if to.state == 'n' {
        // An app that rescales itself on the new screen's DPI can drift off the mapped size.
        let mut now = RECT::default();
        let _ = win::get_window_rect(hwnd, &mut now);
        if now != to.rect {
            let r = to.rect;
            let _ = win::set_window_pos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
    Ok(())
}

/// `r` at the same relative place and size in `to` as it had in `from`, kept inside `to`.
pub(super) fn map_rect(from: RECT, to: RECT, r: RECT) -> RECT {
    let (fw, fh) = ((from.right - from.left).max(1) as i64, (from.bottom - from.top).max(1) as i64);
    let (tw, th) = (to.right - to.left, to.bottom - to.top);
    let x = |v: i32| to.left + ((v - from.left) as i64 * tw as i64 / fw) as i32;
    let y = |v: i32| to.top + ((v - from.top) as i64 * th as i64 / fh) as i32;
    // A window partly off its old screen would land partly off the new one: pull it in.
    let (w, h) = ((x(r.right) - x(r.left)).min(tw), (y(r.bottom) - y(r.top)).min(th));
    let (left, top) = (x(r.left).clamp(to.left, to.right - w), y(r.top).clamp(to.top, to.bottom - h));
    RECT { left, top, right: left + w, bottom: top + h }
}

#[cfg(test)]
mod tests;
