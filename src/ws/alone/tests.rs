// Tests for the sweep's plain rules (spec 0041): which tracked windows belong to an app left
// alone, and that dropping one leaves it in no list.
use super::*;
use crate::ws::tests::{spot, with_hidden};

fn alone(keys: &[&str]) -> HashSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

/// A layout with one window of teams.exe (raws 1 to 15) in every per window field, and one of
/// slack.exe (100) hidden beside them.
fn everywhere() -> State {
    let mut s = State::default();
    let mut m = with_hidden(0, &[&[1, 100, 2], &[3]]);
    m.held[1].push((4, 40));
    m.desktop.push(5);
    s.monitors.insert("DEL-1".into(), m);
    s.restored.insert(6, Instant::now());
    s.sent = Some((7, Instant::now()));
    s.away.insert(8, Away { home: "DEL-2".into(), ws: 1, spot: spot(0, 'n'), pinned: false, pid: 80 });
    s.spots.insert(9, ("DEL-1".into(), spot(0, 'n')));
    s.pinned.insert(10);
    s.apps.insert((11, 110), "teams.exe".into());
    s.wants.insert(12);
    s.opened.insert((13, 130));
    s.settle.insert(14, rules::Settle { target: "DEL-1".into(), spot: spot(0, 'n'), until: Instant::now(), puts: 0 });
    s.ruled.insert(15, rules::Ruled { at: Instant::now(), recloaks: 0, from: "DEL-1".into() });
    s
}

fn key(raw: isize) -> Option<String> {
    Some(if raw == 100 { "slack.exe" } else { "teams.exe" }.to_string())
}

// AC-6: a listed app's window can sit in any of des-k's lists, and one the sweep misses is hidden
// again, followed, or saved. Hidden ones come first, each list top first, so they show top first
// and nothing behind flashes.
#[test]
fn the_sweep_finds_a_listed_app_in_every_list() {
    let s = everywhere();
    let found: Vec<isize> = plan(&s, key, &alone(&["teams.exe"])).into_iter().map(|(raw, _)| raw).collect();
    assert_eq!(&found[..3], &[1, 2, 3], "hidden first, top first: {found:?}");
    let mut all = found.clone();
    all.sort();
    assert_eq!(all, (1..=15).collect::<Vec<_>>());
    assert!(plan(&s, key, &alone(&["zoom.exe"])).is_empty());
}

// AC-6, AC-9: dropping a listed app's windows leaves them in no field, and every other window
// exactly where it was, so nothing else moves or shows.
#[test]
fn a_dropped_window_is_in_no_list_and_others_stay() {
    let mut s = everywhere();
    let gone: HashSet<isize> = (1..=15).collect();
    forget(&mut s, &gone);
    assert!(tracked(&s).iter().all(|raw| !gone.contains(raw)), "{:?}", tracked(&s));
    assert_eq!(s.monitors["DEL-1"].hidden, vec![vec![100], vec![]]);
}

// AC-8: a Store app's frame window whose id can't be read yet is managed as ordinary: the
// sweep leaves it until its key is known.
#[test]
fn a_window_with_no_key_yet_stays_managed() {
    let s = everywhere();
    let found = plan(&s, |raw| (raw != 1).then(|| "teams.exe".to_string()), &alone(&["teams.exe"]));
    assert!(found.iter().all(|(raw, _)| *raw != 1), "{found:?}");
}

// AC-6: a restore that's running drops the entries of a listed app, keyed as rules key them, so
// it never hides that app's reopened windows.
#[test]
fn a_running_restore_drops_entries_of_apps_left_alone() {
    let entry = |app: &str| crate::restore::Entry { dev: "DEL-1".into(), ws: Some(1), pin: false, away: false, spot: None, fp: crate::restore::Fp { app: app.into(), class: "C".into(), title: String::new() } };
    let restore = Restore {
        saved: vec![entry("teams.exe"), entry("slack.exe"), entry("microsoft.teams_8wekyb3d8bbwe!app"), entry("teams.exe")],
        pending: vec![0, 1, 2, 3],
        resolved: HashSet::new(),
        unclear_logged: HashSet::new(),
        cache: HashMap::new(),
        deadline: Instant::now(),
        hidden: 0,
        kept: 0,
    };
    let mut s = State { restore: Some(restore), ..State::default() };
    assert_eq!(drop_entries(&mut s, &alone(&["teams.exe", "microsoft.teams_8wekyb3d8bbwe!app"])), 3);
    assert_eq!(s.restore.as_ref().map(|r| r.pending.clone()), Some(vec![1]));
}

/// The fields of `struct <name>` in src/ws.rs, by name.
fn fields(name: &str) -> Vec<String> {
    let src = include_str!("../../ws.rs").replace("\r\n", "\n");
    let body = src.split_once(&format!("struct {name} {{\n")).and_then(|(_, rest)| rest.split_once("\n}")).map(|(b, _)| b).expect("struct found");
    body.lines().map(str::trim).filter(|l| !l.starts_with("//") && l.contains(':')).map(|l| l.split(':').next().unwrap_or_default().to_string()).collect()
}

/// The body of `fn <name>(` in alone.rs.
fn body(name: &str) -> String {
    let src = include_str!("../alone.rs").replace("\r\n", "\n");
    src.split_once(&format!("fn {name}(")).and_then(|(_, rest)| rest.split_once("\n}\n")).map(|(b, _)| b.to_string()).expect("fn found")
}

// Spec 0041 invariant: a listed app's window is in no list after one sweep. A new field that
// holds windows must be read by `tracked` and cleared by `forget`, or say here why not, so the
// sweep can never miss a list nobody told it about (as pause's test lists hiding sites).
#[test]
fn every_field_that_holds_windows_is_swept() {
    // Hold no window, or (restore) its entries go by key in `drop_entries`.
    const NOT_WINDOWS: [&str; 18] = [
        "monitors", "logon", "restore", "gone", "present", "seen", "back", "layout", "calm", "spots_fresh", "last_acted", "paused", // State
        "active", "last_follow", "last_switch", "device", "area", "name", // Monitor
    ];
    let (tracked, forget) = (body("tracked"), body("forget"));
    let missed: Vec<String> = fields("State")
        .into_iter()
        .chain(fields("Monitor"))
        .filter(|f| !NOT_WINDOWS.contains(&f.as_str()))
        .filter(|f| !tracked.contains(&format!(".{f}")) || !forget.contains(&format!(".{f}")))
        .collect();
    assert!(missed.is_empty(), "the sweep misses these fields: {missed:?}");
    // The reader itself works: a broken one would find no field and pass anything.
    assert!(fields("State").iter().any(|f| f == "pinned") && fields("Monitor").iter().any(|f| f == "held"));
}
