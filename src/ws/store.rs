// The data folder and state.txt: where des-k may write (never through a link), saving the layout,
// reading it back, and --quit's rescue after a crash.
use super::*;

/// %LOCALAPPDATA%\des-k, worked out once: the 1 s settings check asks for it.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or(".".into()));
        let dir = base.join("des-k");
        // The admin des-k writes here, and anything running as you could make this folder a
        // junction to one only an admin may write: drop the link, never follow it.
        if is_link(&dir) {
            warn!("the des-k data folder is a link, not a folder: removed, a real folder takes its place");
            let _ = std::fs::remove_dir(&dir);
        }
        // Before the rename it was pdesk: carry its settings, layout and saved gestures over once.
        let old = base.join("pdesk");
        if !dir.exists() && old.is_dir() && !is_link(&old) && let Err(e) = std::fs::rename(&old, &dir) {
            warn!("could not move the old pdesk data folder to des-k: {e}");
        }
        let _ = std::fs::create_dir_all(&dir);
        dir
    })
    .clone()
}

pub(super) fn is_link(path: &Path) -> bool {
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

pub(super) fn state_file() -> PathBuf {
    data_dir().join("state.txt")
}

/// Persist the layout so a restart, hard kill, or reboot keeps it. Lines (spec 0016 for ids,
/// spots and A lines; a spot is `x y w h state`):
/// `L <logon id> <logon time>`, `M <screen id> <active> <left top right bottom>` TAB name,
/// `W <screen id> <workspace> <hwnd> <pid> <spot>` TAB app TAB class TAB title (a hidden window),
/// `P <screen id> <hwnd> <pid> <spot>` TAB fingerprint (a pinned one, spec 0014),
/// `V <screen id> <spot>` TAB fingerprint (a visible one), and
/// `A <home id> <workspace> <hwnd> <pid> <spot> <pinned>` TAB fingerprint (an away one).
/// While a restore runs, its pending entries are written too (hwnd 0), so another restart
/// still has them. The closing save of an action: a failure is logged, the action stands.
pub(super) fn save(s: &mut State) {
    if let Err(e) = try_save(s) {
        error!("state.txt not saved: {e}");
    }
}

/// `save`, telling the caller whether state.txt was written, and why not (spec 0034 AC-3).
pub(super) fn try_save(s: &mut State) -> Result<(), String> {
    // Every change to where windows live ends here, so a mark leaves with its window on any path
    // (spec 0022 AC-4).
    let wants = std::mem::take(&mut s.wants);
    if settings::get().attention_enabled {
        s.wants = wants.into_iter().filter(|&h| hidden_on(s, h).is_some()).collect();
    }
    // Shown by a switch, the finder or the overview, or gone: the rule is done with it.
    let ruled = std::mem::take(&mut s.ruled);
    s.ruled = ruled.into_iter().filter(|&(h, _)| hidden_on(s, h).is_some()).collect();
    // A closed window's handle can come back for a new window, which must count as new.
    s.opened.retain(|&(h, pid)| win::is_window(Some(HWND(h as *mut _))).as_bool() && pid_of(HWND(h as *mut _)) == pid);
    save_lines(s, true)
}

/// Listed before cloaked (spec 0034 AC-1): puts each window (screen id, workspace, hwnd) in front
/// of its workspace's hidden list and saves, before any of them is cloaked, so a kill at any
/// moment after leaves each one where `--quit` and the next start look. If the save fails they
/// come off the lists again and nothing may be hidden: the reason is logged once here, and the
/// next save that works lets hiding work again. An empty list saves nothing.
pub(super) fn list_then_save(s: &mut State, list: &[(String, usize, isize)]) -> Result<(), String> {
    if list.is_empty() {
        return Ok(());
    }
    // In reverse, so each workspace's list keeps `list`'s order, top first.
    for (id, ws, raw) in list.iter().rev() {
        mon_mut(s, id).hidden[*ws].insert(0, *raw);
    }
    try_save(s).inspect_err(|e| {
        for (_, _, raw) in list {
            unlist(s, *raw);
        }
        error!("state.txt not saved, nothing hidden: {e}");
        CANT_SAVE.fetch_max(1, Ordering::Relaxed);
    })
}

/// Takes a window listed by `list_then_save` off again, after its hide failed.
pub(super) fn unlist(s: &mut State, raw: isize) {
    for m in s.monitors.values_mut() {
        for l in &mut m.hidden {
            l.retain(|&w| w != raw);
        }
    }
}

// A refused hide sets 1 under the lock; main's 1 s timer shows the tray's balloon and sets 2, so
// it shows once a run (spec 0034 AC-2).
pub(super) static CANT_SAVE: AtomicU32 = AtomicU32::new(0);

/// Whether the tray's "can't save your layout" balloon is due: true once a run, after the first
/// refused hide.
pub fn cant_save_due() -> bool {
    CANT_SAVE.compare_exchange(1, 2, Ordering::Relaxed, Ordering::Relaxed).is_ok()
}

/// Without fingerprints: only L, M and A lines (an exit that showed everything). A lines stay,
/// so windows still go home to a screen that returns after a quit (spec 0016 AC-8).
pub(super) fn save_lines(s: &mut State, fingerprints: bool) -> Result<(), String> {
    let mut out = String::new();
    if let Some((id, time)) = s.logon {
        out += &format!("L {id:x} {time}\n");
    }
    let restore_on = settings::get().restore_enabled;
    let line = |f: &Fp| fp_text(f, restore_on);
    // Only the apps of windows still listed carry over, so the cache never outgrows them.
    let (mut old, mut apps) = (std::mem::take(&mut s.apps), HashMap::new());
    let mut fp = |h: HWND, pid: u32| {
        let key = (h.0 as isize, pid);
        let app = old.remove(&key).unwrap_or_else(|| crate::restore::app(h, pid));
        // A Store window's app id may not be readable yet: ask again next save.
        if !app.is_empty() && app != "applicationframehost.exe" {
            apps.insert(key, app.clone());
        }
        line(&Fp { app, class: crate::restore::clean(&class_name(h)), title: crate::restore::clean(&title(h)) })
    };
    s.pinned.retain(|&raw| win::is_window(Some(HWND(raw as *mut _))).as_bool());
    // The spot of the last check: a hidden window of a screen just unplugged keeps its old one.
    let spot = |raw: isize| s.spots.get(&raw).map_or_else(|| spot_of(HWND(raw as *mut _)), |x| x.1).text();
    // A window listed but not cloaked yet (spec 0034 AC-1) is still in view: its W line is its only one.
    let listed: HashSet<isize> = s.monitors.values().flat_map(|m| m.hidden.iter().flatten().copied()).collect();
    for (id, m) in &s.monitors {
        let a = m.area;
        out += &format!("M {id} {} {} {} {} {}\t{}\n", m.active, a.left, a.top, a.right, a.bottom, crate::restore::clean(&m.name));
        if !fingerprints {
            continue;
        }
        for (ws, list) in m.hidden.iter().enumerate() {
            for &h in list {
                let hwnd = HWND(h as *mut _);
                let pid = pid_of(hwnd);
                out += &format!("W {id} {ws} {h} {pid} {}{}", spot(h), fp(hwnd, pid));
            }
        }
        if let Some(mon) = mon_of(s, id) {
            // An away window's A line says where it belongs.
            for h in app_windows_on(mon).into_iter().filter(|h| !s.away.contains_key(&(h.0 as isize)) && !listed.contains(&(h.0 as isize))) {
                let (raw, pid) = (h.0 as isize, pid_of(h));
                if s.pinned.contains(&raw) {
                    out += &format!("P {id} {raw} {pid} {}{}", spot(raw), fp(h, pid));
                } else {
                    out += &format!("V {id} {}{}", spot(raw), fp(h, pid));
                }
            }
        }
    }
    for (&raw, a) in &s.away {
        out += &format!("A {} {} {raw} {} {} {}{}", a.home, a.ws, a.pid, a.spot.text(), a.pinned as u8, fp(HWND(raw as *mut _), a.pid));
    }
    if let Some(r) = s.restore.as_ref().filter(|_| fingerprints) {
        for e in r.pending.iter().map(|&i| &r.saved[i]) {
            // A line from an older des-k had no spot: a zero size reads back as none.
            let sp = e.spot.unwrap_or(Spot { rect: RECT::default(), state: 'n' }).text();
            match (e.away, e.ws, e.pin) {
                (true, Some(ws), pin) => out += &format!("A {} {ws} 0 0 {sp} {}{}", e.dev, pin as u8, line(&e.fp)),
                (_, Some(ws), _) => out += &format!("W {} {ws} 0 0 {sp}{}", e.dev, line(&e.fp)),
                (_, None, true) => out += &format!("P {} 0 0 {sp}{}", e.dev, line(&e.fp)),
                (_, None, false) => out += &format!("V {} {sp}{}", e.dev, line(&e.fp)),
            }
        }
    }
    s.apps = apps;
    write_state(out)
}

/// A line's fingerprint, after its first tab: app, class and title. Only a restore reads a title,
/// so with Restore off the title field stays empty (spec 0033 AC-4); the line keeps all three
/// fields, so it still reads back as a line with a fingerprint.
pub(super) fn fp_text(f: &Fp, restore_on: bool) -> String {
    format!("\t{}\t{}\t{}\n", f.app, f.class, if restore_on { f.title.as_str() } else { "" })
}

/// One state.txt line, read. Lines from an older des-k name Windows' device (DISPLAY2) and
/// carry no spots: `upgrade` turns such a name into the id of the present screen it names, or
/// None (spec 0016 AC-15). Their M, P and V lines then go; a W line keeps the device name, so
/// load shows that window rather than lose it.
#[derive(Debug, PartialEq)]
pub(super) enum Line {
    L(u64, i64),
    M { id: String, active: usize, area: RECT, name: String },
    W { id: String, ws: usize, raw: isize, pid: u32, spot: Option<Spot>, fp: Option<Fp> },
    P { id: String, raw: isize, pid: u32, spot: Option<Spot>, fp: Option<Fp> },
    V { id: String, spot: Option<Spot>, fp: Option<Fp> },
    A { raw: isize, away: Away, fp: Option<Fp> },
}

pub(super) fn parse_state(text: &str, upgrade: impl Fn(&str) -> Option<String>) -> Vec<Line> {
    text.lines().filter_map(|line| parse_line(line, &upgrade)).collect()
}

pub(super) fn parse_line(line: &str, upgrade: &dyn Fn(&str) -> Option<String>) -> Option<Line> {
    let (head, tail) = line.split_once('\t').unwrap_or((line, ""));
    // The fingerprint follows the first tab; a line without one is from an older des-k.
    let fp = match tail.splitn(3, '\t').collect::<Vec<_>>().as_slice() {
        // An older des-k wrote the full path: read as the file name, it still matches (spec 0033 AC-2).
        [app, class, title] => Some(Fp { app: crate::restore::program(app), class: class.to_string(), title: title.to_string() }),
        _ => None,
    };
    let n = |v: &str| v.parse::<i64>().ok();
    Some(match head.split(' ').collect::<Vec<_>>().as_slice() {
        ["L", id, time] => Line::L(u64::from_str_radix(id, 16).ok()?, time.parse().ok()?),
        ["M", id, active, l, t, r, b] => {
            let area = RECT { left: n(l)? as i32, top: n(t)? as i32, right: n(r)? as i32, bottom: n(b)? as i32 };
            Line::M { id: id.to_string(), active: active.parse().ok()?, area, name: tail.to_string() }
        }
        ["M", dev, active] => Line::M { id: upgrade(dev)?, active: active.parse().ok()?, area: RECT::default(), name: String::new() },
        ["W", id, ws, raw, pid, spot @ ..] if spot.len() == 5 || spot.is_empty() => {
            let id = if spot.is_empty() { upgrade(id).unwrap_or(id.to_string()) } else { id.to_string() };
            Line::W { id, ws: ws.parse().ok()?, raw: raw.parse().ok()?, pid: pid.parse().ok()?, spot: Spot::parse(spot), fp }
        }
        ["P", id, raw, pid, spot @ ..] if spot.len() == 5 || spot.is_empty() => {
            let id = if spot.is_empty() { upgrade(id)? } else { id.to_string() };
            Line::P { id, raw: raw.parse().ok()?, pid: pid.parse().ok()?, spot: Spot::parse(spot), fp }
        }
        ["V", id, spot @ ..] if spot.len() == 5 || spot.is_empty() => {
            let id = if spot.is_empty() { upgrade(id)? } else { id.to_string() };
            Line::V { id, spot: Spot::parse(spot), fp }
        }
        ["A", home, ws, raw, pid, x, y, w, h, st, pinned] => {
            let away = Away { home: home.to_string(), ws: ws.parse().ok()?, spot: Spot::parse(&[x, y, w, h, st])?, pinned: *pinned == "1", pid: pid.parse().ok()? };
            Line::A { raw: raw.parse().ok()?, away, fp }
        }
        _ => return None,
    })
}

/// Writes state.txt whole; the caller logs a failure (spec 0034 AC-3).
pub(super) fn write_state(text: String) -> Result<(), String> {
    // A crash or power loss mid write must never leave half a file.
    let tmp = data_dir().join("state.tmp");
    write_new(&tmp, &text).and_then(|()| std::fs::rename(&tmp, state_file())).map_err(|e| e.to_string())
}

/// `des-k --quit` with no des-k running (after a crash or kill, or from the uninstaller):
/// show every window state.txt says des-k hid, if it is still that window and still
/// cloaked on this Windows desktop (the same check as `load`). Then the file is left as a
/// quit leaves it, plus the W lines of any window it could not show.
pub fn rescue() {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return info!("no saved layout, nothing to show") };
    let vdm = unsafe { CoCreateInstance::<_, IVirtualDesktopManager>(&VirtualDesktopManager, None, CLSCTX_ALL) }.ok();
    let Some(shell) = shell_for_exit() else { return };
    let (out, shown, left) = rescued(&text, |raw, pid| {
        let h = HWND(raw as *mut _);
        let on_current = vdm.as_ref().is_some_and(|v| unsafe { v.IsWindowOnCurrentVirtualDesktop(h) }.is_ok_and(|b| b.as_bool()));
        if !(win::is_window(Some(h)).as_bool() && pid_of(h) == pid && cloaked(h) == 2 && on_current) {
            return None; // closed, reused handle, or not hidden by des-k
        }
        // On an unknown layout it stays listed; cloak.rs logged why once (spec 0030 AC-10).
        if crate::cloak::unknown() {
            return Some(false);
        }
        Some(shell.hide(h, false).inspect_err(|e| if !crate::cloak::unknown() { error!("could not show saved window {raw:#x}: {e}") }).is_ok())
    });
    info!("des-k was not running: {shown} hidden window(s) shown, {left} could not be");
    if let Err(e) = write_state(out) {
        error!("state.txt not saved: {e}");
    }
}

/// rescue's pass over state.txt, apart so it can be tested. `show` gets each saved hidden
/// window (hwnd, pid): None if it is not des-k's any more, else whether it is shown now.
/// Returns the new file, as a quit leaves it plus the W lines of windows still hidden, and
/// how many were shown and left. A lines stay, as a quit keeps them.
pub(super) fn rescued(text: &str, mut show: impl FnMut(isize, u32) -> Option<bool>) -> (String, usize, usize) {
    let (mut out, mut shown, mut left) = (String::new(), 0, 0);
    for line in text.lines() {
        let (head, tail) = line.split_once('\t').map_or((line, String::new()), |(h, t)| (h, format!("\t{t}")));
        match head.split(' ').collect::<Vec<_>>().as_slice() {
            ["L", ..] => out += &format!("{line}\n"),
            ["M", id, _, rest @ ..] => out += &format!("M {id} 0{}{tail}\n", rest.iter().map(|v| format!(" {v}")).collect::<String>()),
            // An away window's line stays whatever happens: its screen may still return (spec 0016 AC-8).
            ["A", _, _, raw, pid, ..] => {
                out += &format!("{line}\n");
                if let (Ok(raw), Ok(pid)) = (raw.parse::<isize>(), pid.parse::<u32>())
                    && raw != 0
                {
                    match show(raw, pid) {
                        Some(true) => shown += 1,
                        Some(false) => left += 1,
                        None => {}
                    }
                }
            }
            ["W", _, _, raw, pid, ..] => {
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

#[cfg(test)]
mod tests;
