// Tests for pause (spec 0039): which workspace each screen resumes on, which windows hide again,
// held lists through a resize, and the guard every hiding path must ask.
use super::*;

fn held(id: &str, ws: usize, raw: isize) -> Held {
    Held { id: id.into(), ws, raw, open: true, pinned: false, now: Some(id.into()) }
}

fn shown(pairs: &[(&str, usize)]) -> HashMap<String, usize> {
    pairs.iter().map(|&(id, ws)| (id.to_string(), ws)).collect()
}

// AC-6: resume puts every screen back on the workspace it showed when you paused, and hides only
// what belongs elsewhere there. A window that closed, or whose handle a new process took, is
// nothing of yours to hide; one you dragged to another screen stays where you put it; a pinned
// one is on every workspace.
#[test]
fn resume_hides_only_what_still_belongs_on_another_workspace() {
    let active = shown(&[("A", 0), ("B", 2)]);
    let mut list = vec![held("A", 1, 10), held("A", 0, 11), held("B", 0, 20), held("B", 2, 21)];
    list.push(Held { open: false, ..held("A", 1, 12) });
    list.push(Held { now: Some("B".into()), ..held("A", 1, 13) });
    list.push(Held { now: None, ..held("A", 1, 14) });
    list.push(Held { pinned: true, ..held("A", 1, 15) });
    let targets = resume_targets(&active, &list, None, true);
    assert_eq!(targets, active, "nothing switched while paused");
    let hides: Vec<isize> = resume_hides(&list, &targets).iter().map(|h| h.raw).collect();
    assert_eq!(hides, [10, 20]);
}

// AC-7: the window you were using when you resumed must not vanish from under your hands: its
// screen goes to its workspace. Only with follow on, only for the screen it sits on, and not for
// a window you dragged to another screen (that screen did not hold it).
#[test]
fn the_window_you_use_keeps_its_screen_on_its_workspace() {
    let active = shown(&[("A", 0), ("B", 0)]);
    let list = vec![held("A", 2, 10), held("B", 1, 20), Held { now: Some("B".into()), ..held("A", 3, 30) }];
    let t = resume_targets(&active, &list, Some(10), true);
    assert_eq!(t, shown(&[("A", 2), ("B", 0)]));
    let hides: Vec<isize> = resume_hides(&list, &t).iter().map(|h| h.raw).collect();
    assert_eq!(hides, [20], "the window in front stays in view");
    assert_eq!(resume_targets(&active, &list, Some(10), false), active, "follow off: it hides like the rest");
    assert_eq!(resume_targets(&active, &list, Some(30), true), active, "moved to B, which never held it");
    assert_eq!(resume_targets(&active, &list, Some(99), true), active, "a new window: nobody's workspace");
}

// Spec 0041 AC-14: a window des-k leaves alone is never held, so z order alone would skip it and
// follow the managed window under it, moving the screen away from the window you were using. With
// one focused, resume follows nothing; any other focus keeps AC-7's follow.
#[test]
fn a_focused_window_left_alone_stops_the_follow() {
    let active = shown(&[("A", 0)]);
    let list = vec![held("A", 2, 10)];
    let follow = |left_alone| resume_targets(&active, &list, resume_top(left_alone, Some(10)), true);
    assert_eq!(follow(true), active, "the listed window you use keeps the screen where it is");
    assert_eq!(follow(false), shown(&[("A", 2)]), "any other focus: AC-7 follows as before");
    assert_eq!(resume_top(true, None), None);
}

// AC-7: when resume moves a screen to the window you use, the windows in view there at the pause
// must go back to the workspace the screen showed then, as a follow would leave them, or they
// quietly join the new one. The window you use and pinned ones stay; held windows of any screen
// (one dragged here while paused, too) are AC-6's to decide; a layout des-k does not know hides
// nothing and is not even read.
#[test]
fn a_followed_resume_hides_what_was_in_view_onto_the_old_workspace() {
    let h = |raw: isize| HWND(raw as *mut _);
    let (pinned, held): (HashSet<isize>, HashSet<isize>) = ([50].into(), [60, 70].into());
    let out = follow_hides("A", 1, false, || vec![h(10), h(40), h(50), h(60), h(80)], h(10), &pinned, &held);
    let not_held = |raw| Hide { id: "A".into(), ws: 1, raw, held: false };
    assert_eq!(out, [not_held(40), not_held(80)], "top first, onto workspace 1, counted apart from held ones");
    let read = || -> Vec<HWND> { panic!("windows read on an unknown layout") };
    assert!(follow_hides("A", 1, true, read, h(10), &pinned, &held).is_empty());
}

// AC-12: lowering the count while paused must not lose a held window's memory: it moves to the new
// last workspace, as a hidden one does, so resume still hides it.
#[test]
fn a_held_window_of_a_removed_workspace_moves_to_the_last_one() {
    let mut m = Monitor::with(4);
    m.held[3].push((30, 3));
    m.held[1].push((10, 1));
    assert!(m.resize(2).is_empty());
    assert_eq!(m.held, vec![vec![], vec![(10, 1), (30, 3)]]);
    m.resize(5);
    assert_eq!(m.held.len(), 5, "held keeps one list per workspace, like hidden");
}

// AC-9: a held window's line must carry everything a W line does, under its own letter, so the
// next start adopts it by pid and fingerprint, and a restore after a reboot can match it.
#[test]
fn a_held_line_reads_back() {
    let sp = crate::ws::tests::spot(-1920, 'x');
    let line = held_line("DEL-4123-ABC", 2, 77, 44, &sp.text(), &fp_text(&Fp { app: "a.exe".into(), class: "c".into(), title: "t".into() }, true));
    let lines = parse_state(&format!("L 1a 1700000000\nZ\n{line}"), |_| None);
    assert_eq!(lines[1], Line::Z);
    let fp = Some(Fp { app: "a.exe".into(), class: "c".into(), title: "t".into() });
    assert_eq!(lines[2], Line::H { id: "DEL-4123-ABC".into(), ws: 2, raw: 77, pid: 44, spot: Some(sp), fp });
}

// AC-9: a held window is in view, so save_lines meets it among the screen's windows; a V or P line
// as well as its H line would let a restore match it twice.
#[test]
fn a_held_window_gets_no_view_line() {
    let s = State::default();
    let (listed, held): (HashSet<isize>, HashSet<isize>) = ([1].into(), [2].into());
    assert!(!needs_view_line(&s, 1, &listed, &held), "hidden: its W line");
    assert!(!needs_view_line(&s, 2, &listed, &held), "held: its H line");
    assert!(needs_view_line(&s, 3, &listed, &held), "an ordinary window in view");
}

// AC-10, AC-11: only a file a pause wrote holds windows. Delete its Z line by hand and the next
// start runs with every window an ordinary one in view; adopting the H lines anyway left a window
// you see held, and the next resume hid it on the workspace it was held on long ago (found live
// 2026-11-26).
#[test]
fn only_a_paused_file_holds_its_windows() {
    let h = "H DEL-4123-ABC 2 77 44 0 0 10 10 n\ta\tc\tt\n";
    let read = |text: &str| held_in(&parse_state(text, |_| None));
    let (paused, held) = read(&format!("L 1a 1700000000\nZ\n{h}H DEL-4123-ABC 1 78 45 0 0 10 10 n\n"));
    assert!(paused);
    let got: Vec<(&str, usize, isize, u32)> = held.iter().map(|x| (x.0.as_str(), x.1, x.2, x.3)).collect();
    assert_eq!(got, [("DEL-4123-ABC", 2, 77, 44)], "a line with no fingerprint can't be matched to its window");
    assert_eq!(read(&format!("L 1a 1700000000\n{h}")), (false, vec![]), "Z deleted by hand: running, nothing held");
    assert_eq!(read(&format!("L 1a 1700000000\n{h}Z\n")).1.len(), 1, "wherever the Z sits");
}

// AC-9, AC-10: --quit after a crash while paused must show a held window still cloaked, yet keep
// the pause for the next start: the Z line, every H line and each screen's shown workspace.
#[test]
fn rescue_while_paused_keeps_the_pause() {
    let text = "L 1a 1700000000\nZ\nM DEL 2 0 0 100 100\tDell\n\
        H DEL 1 100 7 0 0 10 10 n\ta\tc\tt\n\
        H DEL 0 200 8 0 0 10 10 n\ta\tc\tt\n\
        W DEL 1 300 9 0 0 10 10 n\ta\tc\tt\n";
    let (out, shown, left) = rescued(text, |raw, _| match raw {
        100 => Some(true), // cloaked by a crash mid pause: shown
        200 => None,       // in view, as a held window should be
        _ => Some(true),
    });
    assert_eq!((shown, left), (2, 0));
    assert_eq!(out, "L 1a 1700000000\nZ\nM DEL 2 0 0 100 100\tDell\nH DEL 1 100 7 0 0 10 10 n\ta\tc\tt\nH DEL 0 200 8 0 0 10 10 n\ta\tc\tt\n");
}

// AC-11 and spec 0034: a file with no Z line is rescued exactly as before the pause existed.
#[test]
fn rescue_without_a_pause_resets_the_shown_workspace() {
    let (out, ..) = rescued("M DEL 2 0 0 100 100\tDell\n", |_, _| None);
    assert_eq!(out, "M DEL 0 0 0 100 100\tDell\n");
}

/// Every function in src/ws (tests aside) whose body cloaks a window (`.hide(…, true)`), and
/// whether it asks `hiding_allowed(` before that line.
fn cloak_sites() -> Vec<(String, bool)> {
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).expect("src/ws").flatten() {
            let p = e.path();
            if p.is_dir() {
                files(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") && p.file_name().is_some_and(|n| n != "tests.rs") {
                out.push(p);
            }
        }
    }
    let mut paths = Vec::new();
    files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join("ws"), &mut paths);
    let mut out = Vec::new();
    for p in paths {
        let text = std::fs::read_to_string(&p).expect("readable");
        let (mut name, mut body) = (String::new(), String::new());
        for line in text.lines() {
            let t = line.trim_start();
            if let Some(rest) = ["fn ", "pub fn ", "pub(super) fn ", "pub(crate) fn "].iter().find_map(|k| t.strip_prefix(k)) {
                name = rest.split(['(', '<']).next().unwrap_or_default().to_string();
                body.clear();
            }
            body.push_str(line);
            body.push('\n');
            if line.contains(".hide(") && line.contains(", true)") {
                out.push((name.clone(), body.contains("hiding_allowed(")));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// Spec 0039's invariant: a held window is never hidden, so every path that cloaks must ask the
// guard first. A new path that cloaks fails here until it asks, and is then listed.
#[test]
fn every_path_that_hides_asks_whether_it_may() {
    let sites = cloak_sites();
    let unguarded: Vec<&String> = sites.iter().filter(|(_, ok)| !ok).map(|(n, _)| n).collect();
    assert!(unguarded.is_empty(), "these hide without asking hiding_allowed: {unguarded:?}");
    let names: Vec<&str> = sites.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["cloak_listed", "give_back", "hide_new", "place", "restore_tick", "resync", "send"]);
}

// Spec 0039 AC-5: a paused start's note waits for your first input, so you are looking when it
// shows. Input from before the start (your password at logon) must not fire it, nor must the tick
// count's wrap after 49 days; a resume first, or a busy screen, holds it back.
#[test]
fn the_start_reminder_waits_for_input_after_the_start() {
    assert!(!reminder_due(5000, 4000, true, false), "input before the start");
    assert!(!reminder_due(5000, 5000, true, false), "the start's own tick");
    assert!(reminder_due(5000, 5001, true, false));
    assert!(reminder_due(u32::MAX - 10, 20, true, false), "input after the tick count wrapped");
    assert!(!reminder_due(20, u32::MAX - 10, true, false), "input from before a wrap is old");
    assert!(!reminder_due(5000, 6000, false, false), "resumed first");
    assert!(!reminder_due(5000, 6000, true, true), "full screen on the screen under the mouse");
}
