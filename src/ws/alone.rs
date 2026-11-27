// Apps des-k leaves alone (spec 0041): whether a window's app is listed as "alone" in [apps], the
// app key cache behind that check, and the sweep that shows and drops a listed app's windows des-k
// still tracks. app_window asks `listed` last, so every path that lists windows skips them with no
// check of its own.
use super::*;

/// What des-k knows of a window's app key, by (hwnd, pid).
#[derive(Clone)]
enum Known {
    /// The key a rule names it by: the lowercase exe file name, or a Store app id.
    Key(String),
    /// Its process can't be read (a protected one): never listed (AC-10).
    Unreadable,
    /// A Store app's frame window whose app id couldn't be read at this sweep tick.
    Frame(u64),
}

// Its own lock, held only to read or insert: never across a Windows call and never while taking
// the ws lock, so it's safe both inside and outside that lock.
static KNOWN: LazyLock<Mutex<HashMap<(isize, u32), Known>>> = LazyLock::new(Default::default);
// One more each sweep: a frame window with no app id yet is asked again at most once a tick.
static TICK: AtomicU64 = AtomicU64::new(0);

fn known() -> MutexGuard<'static, HashMap<(isize, u32), Known>> {
    KNOWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `h`'s app is one des-k leaves alone: app_window's last check, so only windows des-k
/// would otherwise manage pay for it, and none while nothing is listed (AC-12).
pub(super) fn listed(h: HWND) -> bool {
    let set = settings::get();
    !set.alone.is_empty() && key_of(h, pid_of(h)).is_some_and(|k| set.alone.contains(&k))
}

/// `h`'s app key as rules match it (spec 0027 AC-16), never `who`'s, which keeps the name's case.
/// Its exe is read once per window and process; a frame window's app id at most once a tick
/// until it reads, and never from a hung window (AC-12).
fn key_of(h: HWND, pid: u32) -> Option<String> {
    let tick = TICK.load(Ordering::Relaxed);
    let was = known().get(&(h.0 as isize, pid)).cloned();
    let now = match was {
        Some(Known::Key(k)) => return Some(k),
        Some(Known::Unreadable) => return None,
        Some(Known::Frame(t)) if t == tick => return None,
        Some(Known::Frame(_)) => frame_key(h, tick),
        None => match crate::restore::exe_path(pid).map(|exe| crate::restore::program(&crate::restore::clean(&exe))) {
            Some(exe) if exe == "applicationframehost.exe" => frame_key(h, tick),
            Some(exe) => crate::apps::key_from(&exe).map_or(Known::Unreadable, Known::Key),
            None => Known::Unreadable,
        },
    };
    let key = if let Known::Key(k) = &now { Some(k.clone()) } else { None };
    known().insert((h.0 as isize, pid), now);
    key
}

/// A Store app frame window's key, its app id, once that can be read (AC-8).
fn frame_key(h: HWND, tick: u64) -> Known {
    // Reading the id asks the window's thread; a hung one would stall des-k.
    if win::is_hung_app_window(h).as_bool() {
        return Known::Frame(tick);
    }
    crate::restore::app_id(h).and_then(|id| crate::apps::key_from(&id)).map_or(Known::Frame(tick), Known::Key)
}

/// A send or pin pressed on `h`, a window of an app des-k leaves alone: nothing moves, the
/// indicator on its screen says why, and one line names the exe (AC-5).
pub(super) fn say_left_alone(s: &mut State, h: HWND, what: &str) {
    info!("{what}: {} is left alone, nothing done", who(h));
    let mon = win::monitor_from_window(h, MONITOR_DEFAULTTONEAREST);
    let id = id_of(s, &device_name(mon));
    let (active, count) = (mon_mut(s, &id).active, settings::get().count_for(&id));
    let app = crate::finder::names::app_name(h).unwrap_or_else(|| who(h));
    indicator::show(mon, card(s, &id, active, count).with(Says::LeftAlone { app }));
}

/// Main's 1 s timer, right after the settings watch, and once at start right after `load` (AC-6
/// to AC-8). Every window of a listed app that des-k tracks is shown if des-k hid it, top first,
/// and dropped from every list; the app's restore entries go too; then state.txt is saved. A
/// window whose show fails stays tracked to try again next time. Nothing while none is listed.
pub fn sweep() {
    let set = settings::get();
    if set.alone.is_empty() {
        known().clear();
        return;
    }
    TICK.fetch_add(1, Ordering::Relaxed);
    forget_closed();
    let mut guard = state();
    let s = &mut *guard;
    let found = plan(s, |raw| {
        let h = HWND(raw as *mut _);
        if !win::is_window(Some(h)).as_bool() {
            return None; // gone: the paths that track it drop it
        }
        let pid = pid_of(h);
        s.apps.get(&(raw, pid)).and_then(|app| crate::apps::key_from(app)).or_else(|| key_of(h, pid))
    }, &set.alone);
    let entries = drop_entries(s, &set.alone);
    if found.is_empty() {
        if entries > 0 {
            info!("apps: {entries} saved window(s) of apps left alone won't be restored");
            save(s);
        }
        return;
    }
    // In a hidden list, des-k hid it; anywhere else, only a shell cloak is des-k's kind.
    let listed: HashSet<isize> = s.monitors.values().flat_map(|m| m.hidden.iter().flatten().copied()).collect();
    let (mut shell, mut shown, mut stuck) = (None, std::collections::BTreeMap::<String, usize>::new(), HashSet::new());
    for (raw, key) in &found {
        let h = HWND(*raw as *mut _);
        if !listed.contains(raw) && cloaked(h) != 2 {
            continue;
        }
        if shell.is_none() {
            shell = match Shell::new() {
                Ok(sh) => Some(sh),
                Err(e) => return warn!("apps: shell unavailable, windows left alone are shown next time: {e}"),
            };
        }
        match uncloak(shell.as_ref().expect("made above"), *raw) {
            Some(true) => *shown.entry(key.clone()).or_default() += 1,
            Some(false) => _ = stuck.insert(*raw),
            None => {}
        }
    }
    let gone: HashSet<isize> = found.iter().map(|(raw, _)| *raw).filter(|raw| !stuck.contains(raw)).collect();
    forget(s, &gone);
    for (key, n) in shown {
        info!("apps: {key} left alone, {n} hidden window(s) shown");
    }
    save(s);
}

/// Closed windows leave the key cache, so a handle Windows reuses is read afresh.
fn forget_closed() {
    let ids: Vec<(isize, u32)> = known().keys().copied().collect();
    let closed: Vec<(isize, u32)> = ids
        .into_iter()
        .filter(|&(raw, pid)| {
            let h = HWND(raw as *mut _);
            !win::is_window(Some(h)).as_bool() || pid_of(h) != pid
        })
        .collect();
    if !closed.is_empty() {
        let mut k = known();
        for id in closed {
            k.remove(&id);
        }
    }
}

/// Every window des-k tracks, each once and with its app key, whose app is in `alone`: the ones
/// in hidden lists first, each list top first, so a sweep shows them top first (AC-6). `key`
/// gives a live window's key, None when it can't be known (yet).
pub(super) fn plan(s: &State, key: impl Fn(isize) -> Option<String>, alone: &HashSet<String>) -> Vec<(isize, String)> {
    tracked(s).into_iter().filter_map(|raw| Some((raw, key(raw).filter(|k| alone.contains(k))?))).collect()
}

/// Every window in a per window field of `s`, each once, hidden lists first. A field `forget`
/// clears is read here too: the test lists them.
pub(super) fn tracked(s: &State) -> Vec<isize> {
    let mut all: Vec<isize> = Vec::new();
    for m in s.monitors.values() {
        all.extend(m.hidden.iter().flatten());
    }
    for m in s.monitors.values() {
        all.extend(m.held.iter().flatten().map(|&(raw, _)| raw));
        all.extend(&m.desktop);
    }
    all.extend(s.restored.keys());
    all.extend(s.sent.map(|(raw, _)| raw));
    all.extend(s.away.keys());
    all.extend(s.spots.keys());
    all.extend(&s.pinned);
    all.extend(s.apps.keys().map(|&(raw, _)| raw));
    all.extend(&s.wants);
    all.extend(s.opened.iter().map(|&(raw, _)| raw));
    all.extend(s.settle.keys());
    all.extend(s.ruled.keys());
    let mut seen = HashSet::new();
    all.retain(|raw| seen.insert(*raw));
    all
}

/// Takes every window in `gone` out of each per window field of `s` (AC-6). A new field that
/// holds windows needs a line here and in `tracked`, or the field list test fails.
pub(super) fn forget(s: &mut State, gone: &HashSet<isize>) {
    let keep = |raw: &isize| !gone.contains(raw);
    for m in s.monitors.values_mut() {
        for l in &mut m.hidden {
            l.retain(keep);
        }
        for l in &mut m.held {
            l.retain(|(raw, _)| keep(raw));
        }
        m.desktop.retain(keep);
    }
    s.restored.retain(|raw, _| keep(raw));
    s.sent = s.sent.filter(|(raw, _)| keep(raw));
    s.away.retain(|raw, _| keep(raw));
    s.spots.retain(|raw, _| keep(raw));
    s.pinned.retain(keep);
    s.apps.retain(|(raw, _), _| keep(raw));
    s.wants.retain(keep);
    s.opened.retain(|(raw, _)| keep(raw));
    s.settle.retain(|raw, _| keep(raw));
    s.ruled.retain(|raw, _| keep(raw));
}

/// A running restore's entries for apps in `alone` go, split by key as rules split them (AC-6).
/// Returns how many went.
pub(super) fn drop_entries(s: &mut State, alone: &HashSet<String>) -> usize {
    let Some(r) = s.restore.as_mut() else { return 0 };
    let before = r.pending.len();
    r.pending.retain(|&i| !crate::apps::key_from(&r.saved[i].fp.app).is_some_and(|k| alone.contains(&k)));
    before - r.pending.len()
}

#[cfg(test)]
mod tests;
