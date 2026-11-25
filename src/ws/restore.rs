// Coming back: load at start, the restore ticks after a new sign in, showing every window at exit
// or after a panic, and the resync after explorer restarts.
use super::*;

/// Whether `load` started a restore, and why (spec 0003): after a new sign in, windows open
/// before des-k started also go by their app's rule (spec 0027 AC-11).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Restoring {
    No,
    Resume,
    NewSignIn,
}

/// Matching reopened windows to saved entries after a new sign in (spec 0003).
pub(super) struct Restore {
    pub(super) saved: Vec<Entry>,   // as loaded, never changed
    pub(super) pending: Vec<usize>, // indexes into saved not used yet
    pub(super) resolved: HashSet<(isize, u32)>, // windows restore is finished with
    pub(super) unclear_logged: HashSet<(isize, u32)>,
    pub(super) cache: HashMap<(isize, u32), (String, String)>, // app and class; only titles change
    pub(super) deadline: Instant,
    pub(super) hidden: usize,
    pub(super) kept: usize,
}

/// A fresh Shell for an exit path, waiting out an explorer restart: the exit gets one chance.
pub(super) fn shell_for_exit() -> Option<Shell> {
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

pub(super) fn restore_in(s: &mut State) {
    let shell = shell_for_exit();
    let mut left = 0;
    for m in s.monitors.values_mut() {
        for list in &mut m.hidden {
            // No shell: every window still exists and stays hidden, so every one is kept.
            let (_, stuck) = show_all(std::mem::take(list), |raw| match &shell {
                Some(sh) => uncloak(sh, raw),
                None => win::is_window(Some(HWND(raw as *mut _))).as_bool().then_some(false),
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
    if let Err(e) = save_lines(s, left > 0) {
        error!("state.txt not saved: {e}");
    }
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
/// restore runs, and for a new sign in; main then drives it with `restore_tick`.
pub fn load() -> Restoring {
    let mut guard = state();
    let s = &mut *guard;
    s.logon = crate::restore::logon();
    s.present = screen_ids();
    s.seen = s.present.iter().map(|p| p.id.clone()).collect();
    // Screens at start count as settled: a screen that leaves in the first seconds keeps its windows' spots.
    s.layout = layout_of(&s.present);
    s.calm = SETTLE_CHECKS;
    log_screens(&s.present);
    let Ok(text) = std::fs::read_to_string(state_file()) else { return Restoring::No };
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
    let restoring = match st.restore_enabled && if new_sign_in { !late } else { cut_short } {
        false => Restoring::No,
        true if new_sign_in => Restoring::NewSignIn,
        true => Restoring::Resume,
    };
    // A restart in the same sign in: the same window, same pid, is still pinned.
    if !new_sign_in {
        let same = |&(raw, pid): &(isize, u32)| win::is_window(Some(HWND(raw as *mut _))).as_bool() && pid_of(HWND(raw as *mut _)) == pid;
        s.pinned.extend(pins.iter().filter(|p| same(p)).map(|&(raw, _)| raw));
        // Still that window, still open: it still waits for its screen (spec 0016 AC-8).
        for (raw, a, fp) in adopt_away {
            let h = HWND(raw as *mut _);
            let it = same(&(raw, a.pid)) && crate::restore::app(h, a.pid) == fp.app && crate::restore::clean(&class_name(h)) == fp.class;
            if it && s.monitors.contains_key(&a.home) {
                if a.pinned {
                    s.pinned.insert(raw);
                }
                s.away.insert(raw, a);
            }
        }
    }
    if restoring != Restoring::No {
        // A new sign in: every saved handle is dead. A resume: only the cut entries are left.
        if new_sign_in {
            adopt.clear();
        }
        let cut = |(e, cut): (Entry, bool)| (new_sign_in || cut).then_some(e);
        let all = hidden.into_iter().filter_map(cut).chain(visible).chain(pinned.into_iter().filter_map(cut)).chain(aways.into_iter().filter_map(cut));
        // An app with a rule goes by its rule, not by restore (spec 0027 AC-11).
        let (ruled, saved): (Vec<Entry>, Vec<Entry>) = all.partition(|e| crate::apps::key_from(&e.fp.app).is_some_and(|key| st.apps.iter().any(|(k, _)| *k == key)));
        let by_rule = if ruled.is_empty() { String::new() } else { format!(" ({} more left to their app's rule)", ruled.len()) };
        info!("restore: {} {} saved window(s){by_rule}, matching for {} min", if new_sign_in { "new sign in," } else { "resuming," }, saved.len(), st.restore_minutes);
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
        let (mut kept, mut shown, mut stuck) = (0, 0, 0);
        let mut fold_now = HashSet::new();
        for (dev, ws, raw, pid) in adopt {
            let h = HWND(raw as *mut _);
            let on_current = unsafe { vdm.IsWindowOnCurrentVirtualDesktop(h) }.is_ok_and(|b| b.as_bool());
            if !(win::is_window(Some(h)).as_bool() && pid_of(h) == pid && cloaked(h) == 2 && on_current) {
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
                        // On an unknown layout the one line below says it (spec 0030 AC-10).
                        if crate::cloak::unknown() {
                            stuck += 1;
                        } else {
                            error!("could not show saved window {raw:#x}: {e}, kept to try again");
                        }
                        let m = mon_mut(s, &dev);
                        let ws = ws.min(m.hidden.len() - 1);
                        m.hidden[ws].push(raw);
                        crate::cloak::adopt(h);
                    }
                },
            }
        }
        if crate::cloak::unknown() && kept + stuck > 0 {
            error!("loaded saved layout: {} an earlier des-k hid stay hidden, this Windows' layout is unknown: signing out and in shows them", words(kept + stuck));
        } else {
            info!("loaded saved layout: {kept} hidden window(s) adopted, {shown} shown again");
        }
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
    // A restore only hides, so on a layout des-k does not know it ends here (spec 0030 AC-6).
    if Instant::now() >= r.deadline || r.pending.is_empty() || crate::cloak::unknown() {
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
                let ac = (crate::restore::app(h, key.1), crate::restore::clean(&class_name(h)));
                // A Store window's app id may not be readable yet: ask again next tick.
                if ac.0 != "applicationframehost.exe" {
                    r.cache.insert(key, ac.clone());
                }
                ac
            }
        };
        left.push((h, key, Fp { app, class, title: crate::restore::clean(&title(h)) }));
    }

    let mut changed = false;
    // The windows this tick hides: listed and saved once, before any is cloaked (spec 0034 AC-1).
    let mut plan: Vec<Hide> = Vec::new();
    for pass in [1, 2] {
        let mut rest = Vec::new();
        for (h, key, fp) in left {
            let i = match crate::restore::pick(&r.saved, &r.pending, &fp, pass) {
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
                if e.pin {
                    s.pinned.insert(raw);
                }
                s.away.insert(raw, Away { home: e.dev.clone(), ws, spot, pinned: e.pin, pid: key.1 });
                r.pending.retain(|&p| p != i);
                if here.is_none()
                    && !(e.pin || e.ws.is_none() || ws == home_active)
                    && let Some(main) = main
                {
                    let to = Spot { rect: remap(from, main.area, spot.rect), state: spot.state };
                    if s.monitors.entry(main.id.clone()).or_insert_with(|| Monitor::of(&main)).active != ws {
                        // Moved once hidden, so it never crosses screens where you can see it.
                        plan.push(Hide { h, key, i, app: fp.app.clone(), to: (main.id.clone(), ws), then: After::Away { put: to, name } });
                        continue;
                    }
                    if !hung(h)
                        && let Err(err) = put(h, to)
                    {
                        warn!("restore: could not place {}: {err}", fp.app);
                    }
                }
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
                _ if device_name(win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST)) != p.device => {}
                _ => {
                    r.pending.retain(|&p| p != i);
                    plan.push(Hide { h, key, i, app: fp.app.clone(), to: (dev.clone(), ws), then: After::Home { was_fg: unsafe { GetForegroundWindow() } == h } });
                }
            }
        }
        left = rest;
    }

    let list: Vec<(String, usize, isize)> = plan.iter().map(|p| (p.to.0.clone(), p.to.1, p.h.0 as isize)).collect();
    if list_then_save(s, &list).is_err() {
        // Nothing hidden, nothing used up: a later tick tries again (spec 0034 AC-2).
        for p in &plan {
            unplan(s, p);
        }
        plan.clear();
    }
    for p in plan {
        let raw = p.h.0 as isize;
        // A brand new window fails to cloak for a moment: nothing used up, next tick retries.
        if shell.hide(p.h, true).is_err() {
            // The cloak can work and only the taskbar step fail: undo it, or nothing tracks it.
            if cloaked(p.h) != 0 {
                let _ = shell.hide(p.h, false);
            }
            // One that may still be cloaked stays listed, as hidden (AGENTS.md).
            if cloaked(p.h) == 0 {
                unlist(s, raw);
                unplan(s, &p);
                changed = true;
                continue;
            }
        }
        #[cfg(feature = "drill")]
        crate::drill::at(crate::drill::Stop::KillMidRestore);
        s.restored.retain(|_, t| t.elapsed() < RESTORED_IGNORE);
        s.restored.insert(raw, Instant::now());
        let r = s.restore.as_mut().expect("this tick's restore");
        r.resolved.insert(p.key);
        changed = true;
        let (dev, ws) = p.to;
        match p.then {
            After::Home { was_fg } => {
                r.hidden += 1;
                info!("restore: {} -> {dev} workspace {}/{count}", p.app, ws + 1);
                if was_fg {
                    let mon = win::monitor_from_window(p.h, MONITOR_DEFAULTTONEAREST);
                    let next = app_windows_on(mon).first().copied().unwrap_or(win::get_shell_window());
                    unsafe {
                        let _ = SetForegroundWindow(next);
                        if GetForegroundWindow() != next {
                            warn!("restore: could not move focus");
                        }
                    }
                }
            }
            After::Away { put: to, name } => {
                r.kept += 1;
                if !hung(p.h)
                    && let Err(err) = put(p.h, to)
                {
                    warn!("restore: could not place {}: {err}", p.app);
                }
                info!("restore: {} waits for {name}, workspace {}/{count}", p.app, ws + 1);
            }
        }
    }
    let r = s.restore.as_ref().expect("this tick's restore");
    if r.pending.is_empty() {
        finish_restore(s);
        return false;
    }
    if changed {
        save(s);
    }
    true
}

/// A window a restore tick hides (spec 0034 AC-1): listed on `to` (screen id, workspace) and its
/// entry `i` out of `pending` before the one save, cloaked after.
pub(super) struct Hide {
    pub(super) h: HWND,
    pub(super) key: (isize, u32),
    pub(super) i: usize,
    pub(super) app: String,
    pub(super) to: (String, usize),
    pub(super) then: After,
}

/// What follows a restore's cloak: home, on its own screen; or away, onto the main display's
/// workspace while its screen is not plugged in, moved there once hidden.
pub(super) enum After {
    Home { was_fg: bool },
    Away { put: Spot, name: String },
}

/// A planned restore hide that did not happen: its entry goes back to `pending` (in order, as
/// it started), and an away one loses the record it was given, so a later tick retries. A
/// pinned entry is never planned: it stays in view.
pub(super) fn unplan(s: &mut State, p: &Hide) {
    if let After::Away { .. } = p.then {
        s.away.remove(&(p.h.0 as isize));
    }
    let r = s.restore.as_mut().expect("this tick's restore");
    let at = r.pending.partition_point(|&x| x < p.i);
    r.pending.insert(at, p.i);
}

/// Unmatched entries are dropped: a window that reopens later is never hidden (AC-6).
pub(super) fn finish_restore(s: &mut State) {
    let Some(r) = s.restore.take() else { return };
    info!("restore: done, {} hidden, {} kept visible, {} saved window(s) not matched", r.hidden, r.kept, r.pending.len());
    save(s);
}

// Tries left to hide every tracked window again after explorer restarted (resync).
pub(super) static RESYNC: AtomicU32 = AtomicU32::new(0);
pub(super) const RESYNC_TRIES: u32 = 10;

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
    // Nothing to hide again on a layout des-k does not know; the listed stay listed (spec 0030 AC-6).
    if crate::cloak::unknown() {
        RESYNC.store(0, Ordering::Relaxed);
        return info!("explorer restarted: hiding is off on this Windows, nothing hidden again");
    }
    let Ok(shell) = Shell::new() else { return }; // explorer not up yet
    let mut s = state();
    let (mut hidden, mut failed, mut in_view) = (0, 0, 0);
    for m in s.monitors.values_mut() {
        for list in &mut m.hidden {
            list.retain(|&raw| {
                let h = HWND(raw as *mut _);
                if !win::is_window(Some(h)).as_bool() {
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
