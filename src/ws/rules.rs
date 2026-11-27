// [apps] rules: placing a new window on its app's workspace and screen, hiding it there, and
// watching it while its app may move it back (spec 0027).
use super::*;

/// A window a rule moved to another screen, watched for `SETTLE_FOR` (spec 0027 AC-6).
pub(super) struct Settle {
    pub(super) target: String, // screen id
    pub(super) spot: Spot,
    pub(super) until: Instant,
    pub(super) puts: u8,
}

/// A window a rule hid that has not been shown since (spec 0027 AC-5, AC-17).
pub(super) struct Ruled {
    pub(super) at: Instant,
    pub(super) recloaks: u8,
    pub(super) from: String, // the screen it opened on, whose top window takes focus off it
}

/// Marks every top level window there is now as shown once: a rule then acts only on windows
/// that show for the first time from here on (spec 0027 AC-3). Called as the show hook goes in.
pub fn seed_opened() {
    let all: Vec<(isize, u32)> = top_level().into_iter().map(|h| (h.0 as isize, pid_of(h))).collect();
    state().opened.extend(all);
}

/// The show hook went out: nothing is watched any more.
pub fn forget_opened() {
    let mut s = state();
    s.opened.clear();
    s.settle.clear();
}

/// Whether `h` shows for the first time; it counts as shown from now on either way (AC-3).
pub fn first_show(h: HWND) -> bool {
    state().opened.insert((h.0 as isize, pid_of(h)))
}

/// Puts a new window of an app with a rule where the rule says (spec 0027): on the rule's
/// screen (or the one it opened on), in the rule's workspace, hidden if that workspace is not
/// showing. Never switches a screen. `name` is the app's name for the note, `shown` the tick
/// count when Windows showed it, None for a window open before des-k started (AC-11).
pub fn place_new(h: HWND, rule: &settings::Rule, name: String, shown: Option<u32>) {
    let note = place_in(&mut state(), h, rule, name, shown);
    if let Some((mon, card)) = note {
        indicator::show(mon, card);
    }
}

pub(super) fn place_in(s: &mut State, h: HWND, rule: &settings::Rule, name: String, shown: Option<u32>) -> Option<(HMONITOR, Card)> {
    let (app, k, raw) = (who(h), rule.workspace, h.0 as isize);
    // Paused: a new window opens where Windows puts it, and nothing logs per window (spec 0039 AC-4).
    if !hiding_allowed(s) {
        return None;
    }
    if crate::cloak::unknown() {
        info!("apps: {app} opened where Windows put it: hiding is off on this Windows");
        return None;
    }
    let from_mon = win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST);
    let from = id_of(s, &device_name(from_mon));
    let target = rule.screen.clone().unwrap_or_else(|| from.clone());
    // Past the count of the screen it would land on, it waits (spec 0040 AC-6).
    let count = settings::get().count_for(&target);
    if k >= count {
        info!("apps: {app} opened where Windows put it: its rule's workspace {} waits, {target} has {count}", k + 1);
        return None;
    }
    // Where it lands now: the rule's screen, or the main display while that one is unplugged (AC-7).
    let (land, away) = match s.present.iter().find(|p| p.id == target) {
        Some(p) => (p.clone(), false),
        None if !s.monitors.contains_key(&target) => {
            warn!("apps: {app} opened where Windows put it: its rule's screen {target} is one des-k has never seen");
            return None;
        }
        None => match s.present.iter().find(|p| p.primary) {
            Some(p) => (p.clone(), true),
            None => {
                warn!("apps: {app} opened where Windows put it: no main display to wait on");
                return None;
            }
        },
    };
    // On the main display while its screen is away: the same workspace, or its last (AC-7).
    let land_count = settings::get().count_for(&land.id);
    let land_ws = if away { fold_ws(k, land_count) } else { k };
    let now = spot_of(h);
    let from_area = work_area(from_mon);
    let spot = Spot { rect: remap(from_area, land.area, now.rect), state: now.state };
    let was_fg = win::get_foreground_window() == h;
    let hide = s.monitors.entry(land.id.clone()).or_insert_with(|| Monitor::of(&land)).active != land_ws;
    let mut how = String::from("visible");
    if hide {
        let Ok(shell) = Shell::new() else {
            warn!("apps: {app} opened where Windows put it: the shell is not available");
            return None;
        };
        // Listed and saved before the cloak, so every exit path shows it again (spec 0034 AC-1,
        // spec 0027 AC-12). Front = top. Refused: it stays where Windows put it.
        if list_then_save(s, &[(land.id.clone(), land_ws, raw)]).is_err() {
            return None;
        }
        match hide_new(s, &shell, h) {
            Ok(retries) => {
                how = match shown {
                    Some(t) => format!("hidden {} ms after it showed, {retries} retries", win::get_tick_count().wrapping_sub(t)),
                    None => format!("hidden, open since before des-k started, {retries} retries"),
                }
            }
            Err(e) => {
                // hide_new showed it again if it could; one that may still be cloaked stays listed.
                if cloaked(h) == 0 {
                    unlist(s, raw);
                }
                save(s);
                warn!("apps: {app} opened where Windows put it: could not hide it ({e})");
                return None;
            }
        }
        s.ruled.insert(raw, Ruled { at: Instant::now(), recloaks: 0, from: from.clone() });
        if was_fg {
            focus_off(&app, from_mon);
        }
    }
    // Hidden first, so it never crosses screens where you can see it.
    if land.id != from {
        if !hung(h)
            && let Err(e) = put(h, spot)
        {
            warn!("apps: could not move {app} to {}: {e}", land.name);
        }
        s.settle.insert(raw, Settle { target: land.id.clone(), spot, until: Instant::now() + SETTLE_FOR, puts: 0 });
    }
    if away && let Some(home) = s.monitors.get(&target) {
        let spot = Spot { rect: remap(from_area, home.area, now.rect), state: now.state };
        s.away.insert(raw, Away { home: target.clone(), ws: k, spot, pinned: false, pid: pid_of(h) });
    }
    let waits = if away { format!(", waiting for {}", s.monitors.get(&target).map_or(target.as_str(), |m| m.name.as_str())) } else { String::new() };
    info!("apps: {app} -> {} workspace {}/{land_count}, {how}{waits}", land.name, land_ws + 1);
    save(s);
    let active = s.monitors.get(&land.id).map_or(0, |m| m.active);
    let mon = monitor_named(&land.device)?;
    hide.then(|| (mon, card(s, &land.id, active, land_count).with(Says::Opened { app: name, to: land_ws })))
}

/// Moves focus off a window a rule hid to the top window on `mon`, the screen it opened on, or
/// the desktop (AC-5). It is cloaked now, so `app_windows_on` skips it. Windows lets only the
/// process that got the last input move focus, so des-k types its masked key first.
pub(super) fn focus_off(app: &str, mon: HMONITOR) {
    let next = app_windows_on(mon).first().copied().unwrap_or(win::get_shell_window());
    keys::mask();
    // Its return, not GetForegroundWindow: the other app's thread may take a moment to activate.
    if !win::set_foreground_window(next).as_bool() {
        warn!("apps: could not move focus off {app}");
    }
}

/// Holds a rule's hide (`WM_UNCLOAKED`, spec 0027 AC-17): a window a rule hid under `HOLD_FOR`
/// ago that something else uncloaked while des-k still lists it hidden is cloaked again, up to
/// `RECLOAKS` times. After that, or if the cloak fails, it stays visible and leaves its lists, so
/// des-k never lists a visible window as hidden. des-k's own uncloaks take a window off its list
/// first, under the lock, so this never undoes them.
pub fn recloak(h: HWND) {
    // Nothing a rule hid on this layout (spec 0030 AC-6).
    if crate::cloak::unknown() {
        return;
    }
    let mut guard = state();
    let s = &mut *guard;
    if !hiding_allowed(s) {
        return;
    }
    let raw = h.0 as isize;
    let Some(r) = s.ruled.get(&raw) else { return };
    if r.at.elapsed() >= HOLD_FOR || !win::is_window(Some(h)).as_bool() || cloaked(h) != 0 {
        return;
    }
    let Some((id, k)) = hidden_on(s, raw) else { return };
    let (recloaks, from, app) = (r.recloaks, r.from.clone(), who(h));
    let held = recloaks < RECLOAKS
        && match Shell::new() {
            Ok(shell) => hide_new(s, &shell, h).inspect_err(|e| warn!("apps: could not hide {app} again: {e}")).is_ok(),
            Err(e) => {
                warn!("apps: could not hide {app} again: {e}");
                false
            }
        };
    if !held {
        // A window that may still be cloaked never leaves the lists (AGENTS.md).
        if cloaked(h) != 0 {
            return;
        }
        if let Some(m) = s.monitors.get_mut(&id) {
            m.hidden[k].retain(|&w| w != raw);
        }
        s.ruled.remove(&raw);
        s.away.remove(&raw);
        if recloaks >= RECLOAKS {
            warn!("apps: Windows showed {app} again a fourth time, left visible and off workspace {}", k + 1);
        } else {
            warn!("apps: {app} left visible and off workspace {}", k + 1);
        }
        return save(s);
    }
    if let Some(r) = s.ruled.get_mut(&raw) {
        r.recloaks += 1;
    }
    let fg = win::get_foreground_window();
    if fg == h || win::get_ancestor(fg, GA_ROOTOWNER) == h {
        let mon = mon_of(s, &from).unwrap_or_else(|| win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST));
        focus_off(&app, mon);
    }
    info!("apps: Windows showed {app} again, hidden again ({}/{RECLOAKS})", recloaks + 1);
}

/// Cloaks a window that just showed, retrying every `NEW_RETRY` while the shell has no view for
/// it yet (AGENTS.md gotcha). Returns how many retries it took. Refuses while paused (spec 0039).
pub(super) fn hide_new(s: &State, shell: &Shell, h: HWND) -> windows::core::Result<u32> {
    const NO_VIEW: windows::core::HRESULT = windows::core::HRESULT(0x8002802Bu32 as i32);
    if !hiding_allowed(s) {
        return Err(windows::Win32::Foundation::E_ABORT.into());
    }
    let mut retries = 0;
    loop {
        match shell.hide(h, true) {
            Ok(()) => return Ok(retries),
            // The cloak can work and only the taskbar step fail: undo it, or nothing tracks it.
            Err(e) if cloaked(h) != 0 => {
                let _ = shell.hide(h, false);
                return Err(e);
            }
            Err(e) if e.code() == NO_VIEW && retries < NEW_TRIES && win::is_window(Some(h)).as_bool() => {
                std::thread::sleep(NEW_RETRY);
                retries += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Whether a window a rule moved is still watched, so main keeps its settle timer.
pub fn settling() -> bool {
    !state().settle.is_empty()
}

/// Puts a window a rule moved back on its screen when its app moves it off within
/// `SETTLE_FOR`, `SETTLE_PUTS` times at most (AC-6). Returns whether any window is still watched.
pub fn settle_tick() -> bool {
    let mut guard = state();
    let s = &mut *guard;
    // Paused: no rule acts, so its timer stops (spec 0039 AC-4).
    if !hiding_allowed(s) {
        s.settle.clear();
        return false;
    }
    let raws: Vec<isize> = s.settle.keys().copied().collect();
    for raw in raws {
        let h = HWND(raw as *mut _);
        let Some(st) = s.settle.get(&raw) else { continue };
        let (target, spot, puts) = (st.target.clone(), st.spot, st.puts);
        if Instant::now() >= st.until || !win::is_window(Some(h)).as_bool() {
            s.settle.remove(&raw);
            continue;
        }
        let on = win::monitor_from_window(h, MONITOR_DEFAULTTONULL);
        if on.is_invalid() || id_of(s, &device_name(on)) == target {
            continue;
        }
        let app = who(h);
        if puts == SETTLE_PUTS {
            warn!("apps: {app} keeps moving itself off its rule's screen, left where it is");
            s.settle.remove(&raw);
            continue;
        }
        if hung(h) {
            continue;
        }
        match put(h, spot) {
            Ok(()) => info!("apps: {app} moved itself off its rule's screen, put back ({}/{SETTLE_PUTS})", puts + 1),
            Err(e) => warn!("apps: could not put {app} back: {e}"),
        }
        if let Some(st) = s.settle.get_mut(&raw) {
            st.puts += 1;
        }
    }
    !s.settle.is_empty()
}
