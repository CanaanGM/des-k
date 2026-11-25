// Switching a screen's workspace: the screen an action hits, switch, jump, back, follow, the
// one switch_to every way goes through, and showing a list of hidden windows.
use super::*;

/// The indicator's card for screen `id`: named only when two or more screens are plugged in and
/// `[indicator] screen_name` is on (spec 0023 AC-5), with this screen's workspace names, and
/// carrying the id so the indicator finds that screen's own place (spec 0020 AC-9).
pub(super) fn card(s: &State, id: &str, active: usize, count: usize) -> Card {
    let set = settings::get();
    let wants = s.monitors.get(id).filter(|_| set.attention_enabled).map_or_else(Vec::new, |m| {
        (0..m.hidden.len()).filter(|&i| m.hidden[i].iter().any(|h| s.wants.contains(h))).collect()
    });
    let screen = (s.present.len() > 1 && set.indicator_screen_name).then(|| label(s, id));
    Card { screen_id: id.to_string(), wants, names: names_of(id, count), ..Card::new(active, count, screen) }
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

pub(crate) fn under_mouse() -> (HMONITOR, String) {
    let mut pt = POINT::default();
    unsafe { let _ = GetCursorPos(&mut pt); }
    let mon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
    (mon, device_name(mon))
}

/// Where an action came from (spec 0038): a shortcut, or the touchpad, whose hand is on the pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Keys,
    Pointer,
}

/// The screen an action acts on (spec 0038 AC-5 to AC-8). Swipes, and keys with `act_on = "mouse"`,
/// take the mouse's; keys with `"focus"` take the focused window's, else the screen des-k last
/// acted on, else the mouse's. Lazy, so the mouse mode reads nothing more than it always did.
pub(super) fn pick<T>(from: Source, act_on: settings::ActOn, focus: impl FnOnce() -> Option<T>, last: impl FnOnce() -> Option<T>, mouse: impl FnOnce() -> T) -> T {
    if from == Source::Pointer || act_on == settings::ActOn::Mouse {
        return mouse();
    }
    focus().or_else(last).unwrap_or_else(mouse)
}

/// The focused window's screen, or None when nothing counts as focused: nothing in front, the
/// desktop, a taskbar, or a window on no screen (AC-7).
pub(super) fn focused_screen() -> Option<HMONITOR> {
    let fg = unsafe { GetForegroundWindow() };
    let class = class_name(fg);
    if is_desktop(fg, win::get_shell_window(), &class) || matches!(class.as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return None;
    }
    let mon = unsafe { MonitorFromWindow(fg, MONITOR_DEFAULTTONULL) };
    (!mon.is_invalid()).then_some(mon)
}

/// The screen an action from `from` acts on, as `under_mouse` gives it: its monitor and device name.
pub(crate) fn target(from: Source) -> (HMONITOR, String) {
    let on = |mon: HMONITOR| (mon, device_name(mon));
    let last = || {
        let s = state();
        s.last_acted.as_deref().and_then(|id| mon_of(&s, id))
    };
    pick(from, settings::get().act_on, || focused_screen().map(on), || last().map(on), under_mouse)
}

/// The workspace a step of `dir` (-1 / +1) from `active` lands on, wrapping both ways.
pub(super) fn step_target(active: usize, dir: isize, count: usize) -> usize {
    (active as isize + dir).rem_euclid(count as isize) as usize
}

/// Switch the workspace of the screen `target` picks by `dir` (-1 / +1), wrapping like a send.
pub fn switch(dir: isize, from: Source) {
    let (mon, dev) = target(from);
    let count = settings::get().workspaces;
    let mut s = state();
    let name = id_of(&mut s, &dev);
    let active = mon_mut(&mut s, &name).active;
    let target = step_target(active, dir, count);
    if target == active {
        indicator::show(mon, card(&s, &name, active, count)); // nowhere to go: still show where we are
        return;
    }
    switch_to(&mut s, mon, &name, target, None, None, None);
}

/// What a jump to one workspace does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Jump {
    /// Past the count: only the indicator, saying so.
    Missing,
    /// Already shown and nothing to go back to: only the indicator.
    Stay,
    /// Already shown with `jump_back` on: back to the last workspace.
    Back(usize),
    Go(usize),
}

/// Where a jump to workspace `index` (0 based) goes on screen `m`.
pub(super) fn jump_target(m: &Monitor, index: usize, count: usize, jump_back: bool) -> Jump {
    if index >= count {
        return Jump::Missing;
    }
    if index != m.active {
        return Jump::Go(index);
    }
    match jump_back.then(|| m.back_target(count)).flatten() {
        Some(ws) => Jump::Back(ws),
        None => Jump::Stay,
    }
}

/// Show workspace `index` (0 based) on the screen `target` picks. Past the count, or
/// already shown: only the indicator, so you see where you are (spec 0005). Already shown with
/// `jump_back` on, it goes back to the last workspace when there is one (spec 0024 AC-8).
pub fn jump(index: usize, from: Source) {
    let (mon, dev) = target(from);
    let count = settings::get().workspaces;
    let mut s = state();
    let name = id_of(&mut s, &dev);
    let m = mon_mut(&mut s, &name);
    let active = m.active;
    match jump_target(m, index, count, settings::get().jump_back) {
        Jump::Missing => {
            info!("jump: no workspace {} ({count} workspaces)", index + 1);
            indicator::show(mon, card(&s, &name, active, count).with(Says::NoWorkspace { n: index + 1 }));
        }
        Jump::Stay => indicator::show(mon, card(&s, &name, active, count)),
        Jump::Back(target) => {
            if switch_to(&mut s, mon, &name, target, None, None, None) == Switch::Switched {
                info!("jump: {name} already on workspace {}, back to {}/{count}", index + 1, target + 1);
            }
        }
        Jump::Go(target) => {
            switch_to(&mut s, mon, &name, target, None, None, None);
        }
    }
}

/// Show the workspace the screen `target` picks showed before its last switch; pressed again,
/// it comes back (spec 0024). Nothing to go back to: only the indicator, as jump does.
pub fn back(from: Source) {
    let (mon, dev) = target(from);
    let count = settings::get().workspaces;
    let mut s = state();
    let name = id_of(&mut s, &dev);
    let m = mon_mut(&mut s, &name);
    let active = m.active;
    let Some(target) = m.back_target(count) else {
        info!("back: {name} has no last workspace");
        return indicator::show(mon, card(&s, &name, active, count));
    };
    if switch_to(&mut s, mon, &name, target, None, None, None) == Switch::Switched {
        info!("back: {name} -> workspace {}/{count}", target + 1);
    }
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
    let owner = win::get_ancestor(hwnd, GA_ROOTOWNER);
    let found = [hwnd, owner].into_iter().find_map(|h| {
        s.monitors.iter().find_map(|(dev, m)| Some((h, dev.clone(), m.hidden.iter().position(|l| l.contains(&(h.0 as isize)))?)))
    });
    let Some((h, name, ws)) = found else { return };
    let mon = mon_of(&s, &name);
    let wins = [hwnd.0 as isize, owner.0 as isize];
    match follow_verdict(&s, wins, &name, ws, cloaked(h), mon.is_some(), Instant::now()) {
        Follow::RuleHid { from } => {
            let app = who(hwnd);
            info!("follow: a rule hid {app} and it has not been shown yet, ignored");
            let mon = mon_of(&s, &from).unwrap_or_else(|| unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) });
            focus_off(&app, mon);
        }
        Follow::NotOurs => {}
        Follow::JustRestored => info!("follow: restore just hid {}, ignored", who(hwnd)),
        Follow::JustSent => info!("follow: just sent {}, ignored", who(hwnd)),
        Follow::NoScreen => warn!("follow: {name} not found, ignored {}", who(hwnd)),
        Follow::JustLeft => info!("follow: {name} just left workspace {}, ignored {}", ws + 1, who(hwnd)),
        Follow::Cooling => info!("follow: {name} cooling down, ignored {}", who(hwnd)),
        Follow::Go => {
            let Some(mon) = mon else { return }; // Go comes only with its screen found
            let app = who(hwnd);
            // Says why the screen moved; the name stays off the log line (spec 0038 AC-12, AC-13).
            let note = Says::Followed { app: crate::finder::names::app_name(hwnd).unwrap_or_default() };
            if switch_to(&mut s, mon, &name, ws, Some(hwnd), None, Some(note)) == Switch::Switched {
                s.monitors.get_mut(&name).expect("switch_to keeps the monitor").last_follow = Some(Instant::now());
                info!("follow: {name} -> workspace {}/{} for {app}", ws + 1, settings::get().workspaces);
            }
        }
    }
}

/// What follow does with a hidden window that came to the front: ignore it for the first
/// reason that holds, in this order, or Go.
#[derive(Clone, PartialEq, Debug)]
pub(super) enum Follow {
    /// A rule hid it and it has not been shown yet: focus moves off it on screen `from`.
    RuleHid { from: String },
    /// Not des-k's cloak.
    NotOurs,
    JustRestored,
    JustSent,
    NoScreen,
    JustLeft,
    Cooling,
    Go,
}

/// Follow's ignore rules. `wins` is the window and its root owner, hidden on workspace `ws` of
/// screen `name`; `cloak` is its `DWMWA_CLOAKED` value and `found` whether that screen is present.
pub(super) fn follow_verdict(s: &State, wins: [isize; 2], name: &str, ws: usize, cloak: u32, found: bool, now: Instant) -> Follow {
    // Apps take focus just after a rule hid them, or long after (Teams): your screen stays, and
    // the keyboard leaves the hidden window (spec 0027 AC-5). Before the check below: a new
    // Chrome window can carry its own cloak too, which reads 3, not 2.
    if cloak != 0
        && let Some(r) = wins.iter().find_map(|w| s.ruled.get(w))
    {
        return Follow::RuleHid { from: r.from.clone() };
    }
    if cloak != 2 {
        return Follow::NotOurs;
    }
    let within = |t: Instant, limit: Duration| now.saturating_duration_since(t) < limit;
    let cooldown = Duration::from_millis(FOLLOW_COOLDOWN_MS);
    if wins.iter().any(|w| s.restored.get(w).is_some_and(|&t| within(t, RESTORED_IGNORE))) {
        return Follow::JustRestored;
    }
    // Windows can hand focus back to a window a stay send just hid.
    if s.sent.is_some_and(|(sent, t)| within(t, cooldown) && wins.contains(&sent)) {
        return Follow::JustSent;
    }
    if !found {
        return Follow::NoScreen;
    }
    let m = &s.monitors[name];
    // Windows can hand focus to a window mid cloak; going straight back would undo your switch.
    if m.last_switch.is_some_and(|(t, left)| within(t, cooldown) && left == ws) {
        return Follow::JustLeft;
    }
    if m.last_follow.is_some_and(|t| within(t, cooldown)) {
        return Follow::Cooling;
    }
    Follow::Go
}

/// The windows a switch hides, top first: `windows()` (the screen's app windows) minus `keep`
/// and pinned ones. A pinned window stays in view on every workspace (spec 0014). On a Windows
/// layout des-k does not know, nothing is hidden and the switch only changes the number (spec
/// 0030 AC-5), and the windows are not even read.
pub(super) fn to_hide(unknown: bool, windows: impl FnOnce() -> Vec<HWND>, keep: Option<HWND>, pinned: &HashSet<isize>) -> Vec<HWND> {
    if unknown {
        return Vec::new();
    }
    windows().into_iter().filter(|&h| Some(h) != keep && !pinned.contains(&(h.0 as isize))).collect()
}

/// Whether the focused window `fg` is the desktop: none, the shell's own window, or explorer's
/// desktop (class `Progman` or `WorkerW`).
pub(super) fn is_desktop(fg: HWND, shell: HWND, class: &str) -> bool {
    fg.is_invalid() || fg == shell || matches!(class, "Progman" | "WorkerW")
}

/// Where focus goes once a switch has shown its windows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum FocusAfter {
    Leave,
    /// The window asked for, which is not in front.
    Bring(HWND),
    /// The first window shown.
    Next(HWND),
    /// Nothing was shown: the desktop.
    Desktop,
}

/// `focus` is the window asked for and `fg_now` the one in front now; `fg_hidden` says the switch
/// hid the window that had focus, `fg_desktop` that the desktop had it, and `first` is the first
/// window shown.
pub(super) fn focus_after(focus: Option<HWND>, fg_now: HWND, fg_hidden: bool, fg_desktop: bool, first: Option<HWND>) -> FocusAfter {
    match focus {
        // Normally it already has focus: it is what came to the front.
        Some(h) if fg_now != h => FocusAfter::Bring(h),
        Some(_) => FocusAfter::Leave,
        // Don't leave keyboard focus in a window we just hid.
        None if fg_hidden || fg_desktop => first.map_or(FocusAfter::Desktop, FocusAfter::Next),
        None => FocusAfter::Leave,
    }
}

/// After a hide failed: the cloak can work and only the taskbar step fail, so a window that reads
/// cloaked is shown again with `undo`, or nothing would track it. Returns whether it comes off its
/// list: only when it then reads not cloaked, since one that may still be cloaked is never
/// dropped (AGENTS.md).
pub(super) fn failed_hide(mut cloak: impl FnMut() -> u32, undo: impl FnOnce()) -> bool {
    if cloak() != 0 {
        undo();
    }
    cloak() == 0
}

/// Show workspace `target` (already valid) on one monitor. `focus` is the window to leave
/// in front; `None` focuses the first shown window, but only if we hid the focused one or the
/// desktop has focus.
/// `keep` is a visible window that comes along instead of being hidden (a follow send), and
/// `note` a line for the card (spec 0038 AC-12).
pub(super) fn switch_to(s: &mut State, mon: HMONITOR, name: &str, target: usize, focus: Option<HWND>, keep: Option<HWND>, note: Option<Says>) -> Switch {
    s.last_acted = Some(name.to_string());
    let count = settings::get().workspaces;
    let active = mon_mut(s, name).active;
    if target == active {
        return Switch::Already;
    }
    let shell = match Shell::new() {
        Ok(sh) => sh,
        Err(e) => {
            warn!("shell unavailable: {e}");
            return Switch::Refused;
        }
    };

    let fg = unsafe { GetForegroundWindow() };
    // Leaving an empty workspace left focus on the desktop; nobody's focus to keep there.
    let fg_desktop = is_desktop(fg, win::get_shell_window(), &class_name(fg));
    let mut fg_hidden = false;
    let hides = to_hide(crate::cloak::unknown(), || app_windows_on(mon), keep, &s.pinned);
    // Top first, as a hidden list keeps them, so showing them later is top-down (no flash).
    let list: Vec<(String, usize, isize)> = hides.iter().map(|h| (name.to_string(), active, h.0 as isize)).collect();
    if list_then_save(s, &list).is_err() {
        indicator::show(mon, card(s, name, active, count).with(Says::NotSaved));
        return Switch::Refused;
    }
    // Hide bottom-up: hiding the top window first briefly uncovers the ones behind it.
    #[cfg(feature = "drill")]
    let (mut done, half) = (0, hides.len().div_ceil(2));
    for (i, &h) in hides.iter().enumerate().rev() {
        match shell.hide(h, true) {
            Ok(()) => {
                fg_hidden |= h == fg;
                #[cfg(feature = "drill")]
                {
                    done += 1;
                    if done == 1 {
                        crate::drill::at(crate::drill::Stop::KillAfterHide);
                    }
                    if done == half {
                        crate::drill::at(crate::drill::Stop::PauseMidSwitch);
                    }
                }
            }
            // This very hide found the layout unknown, and cloak.rs said so: the rest stay in view.
            Err(_) if crate::cloak::unknown() => {
                for h in &hides[..=i] {
                    unlist(s, h.0 as isize);
                }
                break;
            }
            Err(e) => {
                let undo = || {
                    let _ = shell.hide(h, false);
                };
                if failed_hide(|| cloaked(h), undo) {
                    unlist(s, h.0 as isize);
                }
                warn!("cloak {:?} failed: {e}", h.0);
            }
        }
    }
    let m = s.monitors.get_mut(name).expect("made above");

    // uncloak keeps each one listed, silently, on an unknown layout.
    let (shown, stuck) = show_all(std::mem::take(&mut m.hidden[target]), |raw| uncloak(&shell, raw));
    // Still hidden, so still tracked: the next switch back here, quit or panic tries again.
    m.hidden[target] = stuck;
    m.last_switch = Some((Instant::now(), m.active));
    m.active = target;
    shown_changed();
    // Those windows now belong to the workspace just left; show_desktop must not bring them back here.
    m.desktop.clear();

    let first = shown.first().map(|&raw| HWND(raw as *mut _));
    match focus_after(focus, unsafe { GetForegroundWindow() }, fg_hidden, fg_desktop, first) {
        FocusAfter::Leave => {}
        FocusAfter::Bring(h) => unsafe {
            let _ = SetForegroundWindow(h);
            if GetForegroundWindow() != h {
                warn!("follow: could not focus {}", who(h));
            }
        },
        FocusAfter::Next(h) => unsafe { let _ = SetForegroundWindow(h); },
        FocusAfter::Desktop => unsafe { let _ = SetForegroundWindow(win::get_shell_window()); },
    }
    if crate::cloak::unknown() {
        info!("{name}: workspace {}/{count} (hiding is off on this Windows)", target + 1);
    } else {
        info!("{name}: workspace {}/{count} ({} shown)", target + 1, shown.len());
    }
    let card = card(s, name, target, count);
    indicator::show(mon, match note {
        Some(line) => card.with(line),
        None => card,
    });
    s.sent = None;
    save(s);
    Switch::Switched
}

/// What `switch_to` did. Refused: nothing was shown or hidden, because the windows it would hide
/// could not be saved first, or the shell was not there (spec 0034 AC-2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Switch {
    Switched,
    Already,
    Refused,
}

/// Shows the windows of `list`, top first. `show` gets each one: None if it is closed, else
/// whether it is in view now. Returns those shown and those still hidden: a window that may
/// still be cloaked is never dropped, so a later switch, quit or panic tries again.
pub(super) fn show_all(list: Vec<isize>, mut show: impl FnMut(isize) -> Option<bool>) -> (Vec<isize>, Vec<isize>) {
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
pub(super) fn uncloak(shell: &Shell, raw: isize) -> Option<bool> {
    let h = HWND(raw as *mut _);
    if !win::is_window(Some(h)).as_bool() {
        return None;
    }
    // On an unknown layout nothing can be shown: kept, and the caller says so once (spec 0030 AC-5).
    if crate::cloak::unknown() {
        return Some(false);
    }
    Some(shell.hide(h, false).inspect_err(|e| if !crate::cloak::unknown() { error!("uncloak {raw:#x} failed: {e}, kept to try again") }).is_ok())
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
    switch_to(&mut s, mon, &id, ws, None, None, None);
}

#[cfg(test)]
mod tests;
