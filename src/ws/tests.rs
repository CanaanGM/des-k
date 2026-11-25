// Tests for workspaces: resizing and back, the screen checks and returns, screen ids, state.txt lines and the rescue.
use super::*;

fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT { left, top, right, bottom }
}

// Shift+Enter promises the same relative place and size: a left half stays a left half,
// even on a screen with another size and position.
#[test]
fn map_rect_keeps_the_relative_place_and_size() {
    let laptop = rect(0, 0, 1920, 1140);
    let external = rect(1920, -200, 1920 + 2560, -200 + 1400);
    let left_half = rect(0, 0, 960, 1140);
    assert_eq!(map_rect(laptop, external, left_half), rect(1920, -200, 1920 + 1280, 1200));
}

fn with_hidden(active: usize, hidden: &[&[isize]]) -> Monitor {
    let mut m = Monitor::with(hidden.len());
    m.active = active;
    m.hidden = hidden.iter().map(|l| l.to_vec()).collect();
    m
}

// Lowering the count must never lose a window: the windows of every removed workspace end
// up hidden on the new last one, and the workspace you look at does not change (AC-6).
#[test]
fn lowering_moves_removed_windows_to_the_new_last_workspace() {
    let mut m = with_hidden(0, &[&[], &[1], &[2, 3], &[4]]);
    assert_eq!(m.resize(2), Vec::<isize>::new());
    assert_eq!(m.active, 0);
    assert_eq!(m.hidden, vec![vec![], vec![1, 2, 3, 4]]);
}

// A screen showing a removed workspace switches to the new last one, and the windows that
// land there are shown, not left hidden behind a workspace you are already on (AC-6).
#[test]
fn lowering_below_the_shown_workspace_shows_what_lands_there() {
    let mut m = with_hidden(3, &[&[9], &[1], &[2], &[]]);
    m.last_switch = Some((Instant::now(), 2));
    assert_eq!(m.resize(2), vec![1, 2]);
    assert_eq!(m.active, 1);
    assert_eq!(m.hidden, vec![vec![9], vec![]]);
    assert!(m.last_switch.is_none(), "a back jump to a removed workspace would index past the end");
}

// Back must land on a real workspace other than the shown one, or do nothing but the card:
// a fresh start, a workspace a smaller count removed, and a resize that moved the screen onto
// its own last workspace all have nowhere to go (spec 0024 AC-4).
#[test]
fn back_goes_only_to_a_workspace_that_still_exists() {
    let mut m = with_hidden(2, &[&[], &[], &[], &[]]);
    assert_eq!(m.back_target(4), None, "des-k just started: no switch yet");
    m.last_switch = Some((Instant::now(), 0));
    assert_eq!(m.back_target(4), Some(0));
    m.last_switch = Some((Instant::now(), 3));
    assert_eq!(m.back_target(3), None, "workspace 4 is gone");
    m.last_switch = Some((Instant::now(), 2));
    assert_eq!(m.back_target(4), None, "already shown");
}

// A smaller count that moves the screen onto its last workspace leaves back nowhere to go,
// never a jump to where you already are (spec 0024 state transitions).
#[test]
fn a_resize_onto_the_last_workspace_leaves_no_back() {
    let mut m = with_hidden(3, &[&[], &[], &[], &[]]);
    m.last_switch = Some((Instant::now(), 1));
    m.resize(2);
    assert_eq!(m.active, 1);
    assert_eq!(m.back_target(2), None);
}

// Raising adds empty workspaces and leaves windows where an earlier lowering put them (AC-6).
#[test]
fn raising_adds_empty_workspaces_only() {
    let mut m = with_hidden(1, &[&[5], &[]]);
    assert_eq!(m.resize(4), Vec::<isize>::new());
    assert_eq!(m.active, 1);
    assert_eq!(m.hidden, vec![vec![5], vec![], vec![], vec![]]);
}

// A screen that drops out for a moment (wake from sleep, a DisplayPort relink) keeps its
// workspaces: only GONE_CHECKS one second checks in a row count, and display change
// events alone never do, however many arrive.
#[test]
fn a_screen_must_stay_gone_before_its_windows_are_shown() {
    let mut gone = HashMap::new();
    let dp = ["DISPLAY2".to_string()];
    for _ in 0..50 {
        assert!(count_gone(&mut gone, &dp, false, GONE_CHECKS).is_empty());
    }
    for _ in 1..GONE_CHECKS {
        assert!(count_gone(&mut gone, &dp, true, GONE_CHECKS).is_empty());
    }
    assert_eq!(count_gone(&mut gone, &dp, true, GONE_CHECKS), vec![("DISPLAY2".to_string(), GONE_CHECKS)]);
}

// Two short drop outs never add up to one long one: a screen that comes back starts at zero.
#[test]
fn a_screen_that_comes_back_starts_counting_again() {
    let mut gone = HashMap::new();
    let dp = ["DISPLAY2".to_string()];
    for _ in 1..GONE_CHECKS {
        count_gone(&mut gone, &dp, true, GONE_CHECKS);
    }
    assert!(count_gone(&mut gone, &[], true, GONE_CHECKS).is_empty());
    assert!(gone.is_empty());
    for _ in 1..GONE_CHECKS {
        assert!(count_gone(&mut gone, &dp, true, GONE_CHECKS).is_empty());
    }
}

// --quit after a crash must never lose a window: one it could not show keeps its W line,
// so the next start or --quit still finds it. Shown windows and V lines go and every
// screen shows workspace 1, as after a quit.
#[test]
fn rescue_keeps_the_line_of_a_window_it_could_not_show() {
    let text = "L 1a2b 1700000000\nM DISPLAY1 2\n\
        W DISPLAY1 1 100 7\tapp\tclass\tshown\n\
        W DISPLAY1 2 200 8\tapp\tclass\tstuck\n\
        W DISPLAY1 1 300 9\tapp\tclass\tclosed\n\
        W DISPLAY1 1 0 0\tapp\tclass\tpending\n\
        V DISPLAY1\tapp\tclass\tvisible\n";
    let mut asked = Vec::new();
    let (out, shown, left) = rescued(text, |raw, pid| {
        asked.push((raw, pid));
        match raw {
            100 => Some(true),
            200 => Some(false),
            _ => None,
        }
    });
    assert_eq!((shown, left), (1, 1));
    assert_eq!(out, "L 1a2b 1700000000\nM DISPLAY1 0\nW DISPLAY1 2 200 8\tapp\tclass\tstuck\n");
    assert_eq!(asked, vec![(100, 7), (200, 8), (300, 9)], "a restore's hwnd 0 entry is no window to show");
}

// "S · W" numbers in the finder and the settings page must match what you see: screens
// counted from the left, then from the top.
#[test]
fn screens_are_numbered_left_to_right_then_top_down() {
    let r = |left, top| rect(left, top, left + 100, top + 100);
    let got = in_order(vec![("D1".into(), r(0, 0)), ("D2".into(), r(-1920, 0)), ("D3".into(), r(0, -1080)), ("D4".into(), r(2560, 200))]);
    assert_eq!(got, ["D2", "D3", "D1", "D4"]);
}

// A window that fails to show is never dropped: it may still be cloaked, and only a list
// lets a later switch, quit or panic show it. A closed one goes. Shown windows keep their
// top first order, so focus lands on the old top one.
#[test]
fn a_window_that_fails_to_show_stays_tracked() {
    let (shown, stuck) = show_all(vec![1, 2, 3, 4], |raw| match raw {
        2 => Some(false),
        3 => None,
        _ => Some(true),
    });
    assert_eq!(shown, vec![1, 4]);
    assert_eq!(stuck, vec![2]);
}

// A window hanging off its old screen must still land fully on the new one, never lost off its edge.
#[test]
fn map_rect_stays_inside_the_new_work_area() {
    let a = rect(0, 0, 1000, 1000);
    let b = rect(1000, 0, 2000, 1000);
    assert_eq!(map_rect(a, b, rect(800, 900, 1200, 1300)), rect(1600, 600, 2000, 1000));
    assert_eq!(map_rect(a, b, rect(-50, -50, 1100, 1100)), rect(1000, 0, 2000, 1000));
}

// A slow screen after a wake must not fold and then come back a moment later (spec 0016 AC-18).
#[test]
fn after_a_wake_a_screen_must_stay_gone_longer() {
    let mut gone = HashMap::new();
    let dp = ["DEL-4123-1".to_string()];
    for _ in 0..GONE_CHECKS {
        assert!(count_gone(&mut gone, &dp, true, WAKE_GONE_CHECKS).is_empty());
    }
    for _ in GONE_CHECKS + 1..WAKE_GONE_CHECKS {
        assert!(count_gone(&mut gone, &dp, true, WAKE_GONE_CHECKS).is_empty());
    }
    assert_eq!(count_gone(&mut gone, &dp, true, WAKE_GONE_CHECKS).len(), 1);
}

fn spot(x: i32, state: char) -> Spot {
    Spot { rect: rect(x, 0, x + 100, 100), state }
}

// Windows moves windows off a screen before des-k hears it left, and no check runs while
// the PC sleeps: only the spot from before can say where a window goes back (invariant 2).
#[test]
fn a_window_whose_screen_is_missing_keeps_its_spot() {
    let now: HashSet<String> = ["internal".to_string()].into();
    let old = Some(("DEL-1".to_string(), spot(3000, 'x')));
    assert_eq!(next_spot(old.clone(), "internal".into(), &now, false, || spot(0, 'n')), ("DEL-1".to_string(), spot(3000, 'x')));
    // Once away, its record holds that spot, and a claim needs where it is now.
    assert_eq!(next_spot(old, "internal".into(), &now, true, || spot(0, 'n')), ("internal".to_string(), spot(0, 'n')));
}

// Unplugging must never look like your move, nor Windows putting a window back on its own
// screen; a drag between two screens that stayed is yours (spec 0016 AC-6).
#[test]
fn only_a_move_between_screens_that_stayed_is_a_claim() {
    let all: HashSet<String> = ["L".to_string(), "A".to_string(), "B".to_string()].into();
    let no_a: HashSet<String> = ["L".to_string(), "B".to_string()].into();
    assert!(is_claim("L", "B", "A", &no_a, &no_a), "dragged from the laptop to B while A is away");
    assert!(!is_claim("A", "L", "A", &all, &no_a), "Windows moved it off A as A left");
    assert!(!is_claim("L", "A", "A", &all, &all), "Windows put it back on its home");
    assert!(!is_claim("L", "L", "A", &no_a, &no_a), "moved on the screen it is on");
    assert!(!is_claim("B", "L", "A", &all, &no_a.iter().filter(|i| *i != "B").cloned().collect()), "B left too");
}

// A dock drops and brings back its screens one by one, and Windows moves windows after each.
// A spot read between two drops is half a move (the portrait screen came back with its
// stack squeezed, 2026-11-10), and a return between two arrivals is moved again by Windows.
// So nothing settles until the layout held SETTLE_CHECKS one second checks.
#[test]
fn the_layout_settles_only_after_it_held_three_checks() {
    let mut calm = count_calm(SETTLE_CHECKS, false, false); // the LG leaves
    assert!(calm < SETTLE_CHECKS, "spots read now would be half a move");
    calm = count_calm(calm, true, true);
    calm = count_calm(calm, false, false); // the portrait screen leaves 0.9 s later
    assert_eq!(calm, 0, "a second change starts the count again");
    for _ in 0..10 {
        calm = count_calm(calm, true, false);
    }
    assert!(calm < SETTLE_CHECKS, "display change messages alone never settle it");
    for _ in 1..SETTLE_CHECKS {
        calm = count_calm(calm, true, true);
    }
    assert!(calm < SETTLE_CHECKS);
    assert!(count_calm(calm, true, true) >= SETTLE_CHECKS);
}

// A return is tried BACK_CHECKS checks after the screen is back, then once a check, and
// stops after RETURN_TRIES tries so a frozen window is not moved forever (AC-5, AC-12).
#[test]
fn a_return_waits_then_tries_ten_times() {
    assert_eq!(try_return(1), None, "a screen that flickers on wake is not back yet");
    assert_eq!(try_return(BACK_CHECKS), Some(false));
    assert_eq!(try_return(BACK_CHECKS + RETURN_TRIES - 1), Some(true));
    assert_eq!(try_return(BACK_CHECKS + RETURN_TRIES), None);
    assert_eq!((0..100).filter_map(try_return).count(), RETURN_TRIES as usize);
}

// A screen whose work area changed while it was away (another resolution or taskbar) gets
// its windows at the same relative spot; an area nobody knew still lands on the screen.
#[test]
fn a_return_maps_the_spot_when_the_area_changed() {
    let old = rect(0, 0, 1000, 1000);
    let new = rect(0, 0, 2000, 1000);
    assert_eq!(remap(old, new, rect(500, 0, 1000, 500)), rect(1000, 0, 2000, 500));
    assert_eq!(remap(RECT::default(), new, rect(-500, 0, 100, 100)), rect(0, 0, 600, 100));
}

// Two monitors of one model with no serial must stay apart, numbered left to right (AC-1).
#[test]
fn twins_are_numbered_left_to_right() {
    let at = |id: &str, left| Screen { device: String::new(), id: id.into(), name: String::new(), area: rect(left, 0, left + 10, 10), full: rect(left, 0, left + 10, 10), primary: false };
    let mut list = vec![at("internal", -100), at("DEL-4123", 0), at("DEL-4123", 100)];
    number_twins(&mut list);
    assert_eq!(list.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["internal", "DEL-4123#1", "DEL-4123#2"]);
}

// Spec 0023 AC-11: a rename shows exactly as typed, with no number, and the twin it leaves
// behind loses its " #2" (nothing left to tell apart). An empty rename keeps both numbers.
#[test]
fn a_renamed_twin_leaves_the_other_unnumbered() {
    let at = |id: &str, left| Screen { device: String::new(), id: id.into(), name: "DELL U2720Q".into(), area: rect(left, 0, left + 10, 10), full: rect(left, 0, left + 10, 10), primary: false };
    let s = State { present: vec![at("DEL-4123#1", 0), at("DEL-4123#2", 100)], ..State::default() };
    let labels = |file: &str| {
        let set = settings::parse::parse_for_test(file).0;
        (label_by(&set, &s, "DEL-4123#1"), label_by(&set, &s, "DEL-4123#2"))
    };
    assert_eq!(labels(""), ("DELL U2720Q #1".into(), "DELL U2720Q #2".into()));
    assert_eq!(labels("[names.screens.\"DEL-4123#1\"]
screen = \"Left\"
"), ("Left".into(), "DELL U2720Q".into()));
    assert_eq!(labels("[names.screens.\"DEL-4123#1\"]
screen = \"\"
"), ("DELL U2720Q #1".into(), "DELL U2720Q #2".into()));
    let both = "[names.screens.\"DEL-4123#1\"]
screen = \"Desk\"
[names.screens.\"DEL-4123#2\"]
screen = \"Desk\"
";
    assert_eq!(labels(both), ("Desk".into(), "Desk".into()));
}

// An EDID serial or instance id with spaces or tabs would break the state.txt line.
#[test]
fn a_screen_id_is_one_safe_token() {
    assert_eq!(clean_id(r"DISPLAY\DEL4123\5&2a&0 UID\t"), "DISPLAY_DEL4123_5_2a_0_UID_t");
}

// Upgrading while docked keeps the layout: each device name becomes the id of the screen it
// names. Lines of a screen not plugged in go once, except a hidden window's, which keeps
// its device so load shows it rather than lose it (AC-15).
#[test]
fn an_old_state_file_upgrades_to_screen_ids() {
    let text = "M DISPLAY1 2\nM DISPLAY9 1\nW DISPLAY1 1 100 7\ta\tc\tt\nW DISPLAY9 0 200 8\ta\tc\tu\nV DISPLAY9\ta\tc\tv\nP DISPLAY1 300 9\ta\tc\tp\n";
    let up = |dev: &str| (dev == "DISPLAY1").then(|| "DEL-1".to_string());
    let fp = |t: &str| Some(Fp { app: "a".into(), class: "c".into(), title: t.into() });
    assert_eq!(parse_state(text, up), vec![
        Line::M { id: "DEL-1".into(), active: 2, area: RECT::default(), name: String::new() },
        Line::W { id: "DEL-1".into(), ws: 1, raw: 100, pid: 7, spot: None, fp: fp("t") },
        Line::W { id: "DISPLAY9".into(), ws: 0, raw: 200, pid: 8, spot: None, fp: fp("u") },
        Line::P { id: "DEL-1".into(), raw: 300, pid: 9, spot: None, fp: fp("p") },
    ]);
}

// What save writes, load reads back: the away record above all, since it is what brings a
// window home after a crash or a power cut (AC-8).
#[test]
fn new_lines_read_back() {
    let away = Away { home: "DEL-4123-ABC".into(), ws: 2, spot: spot(-1920, 'x'), pinned: true, pid: 44 };
    let text = format!(
        "M DEL-4123-ABC 1 0 0 2560 1400\tDELL U2720Q\nA {} 2 77 44 {} 1\ta\tc\tt\nV internal {}\ta\tc\tv\n",
        away.home,
        away.spot.text(),
        Spot { rect: RECT::default(), state: 'n' }.text()
    );
    let lines = parse_state(&text, |_| None);
    assert_eq!(lines[0], Line::M { id: "DEL-4123-ABC".into(), active: 1, area: rect(0, 0, 2560, 1400), name: "DELL U2720Q".into() });
    assert_eq!(lines[1], Line::A { raw: 77, away, fp: Some(Fp { app: "a".into(), class: "c".into(), title: "t".into() }) });
    assert!(matches!(&lines[2], Line::V { spot: None, .. }), "a zero size is a spot nobody knew");
}

// A layout saved by an older des-k, with full exe paths, must still restore after the upgrade
// and still adopt its away window: read back, a path is its file name, which is what a live
// window gives now (spec 0033 AC-2).
#[test]
fn an_old_full_path_reads_back_as_its_file_name() {
    let sp = spot(0, 'n').text();
    let text = format!(
        "W DEL-1 2 0 0 {sp}\tc:\\users\\x\\appdata\\local\\programs\\microsoft vs code\\code.exe\tChrome_WidgetWin_1\tnotes.md\nA DEL-1 1 77 44 {sp} 0\tC:\\Windows\\notepad.exe\tNotepad\tt\n"
    );
    let lines = parse_state(&text, |_| None);
    let Line::W { fp: Some(saved), .. } = &lines[0] else { panic!("{:?}", lines[0]) };
    assert_eq!(saved.app, "code.exe");
    assert!(matches!(&lines[1], Line::A { fp: Some(f), .. } if f.app == "notepad.exe"));
    let entry = restore::Entry { dev: "DEL-1".into(), ws: Some(2), pin: false, away: false, spot: None, fp: saved.clone() };
    let live = Fp { app: "code.exe".into(), class: "Chrome_WidgetWin_1".into(), title: "notes.md".into() };
    assert_eq!(restore::pick(&[entry], &[0], &live, 1), restore::Pick::Take(0));
}

// Nothing reads a title with Restore off, so none is kept (spec 0033 AC-4). The line still has
// all three fields: with fewer it would read as an older des-k's line with no fingerprint,
// and a restore turned back on, or an away window's adoption, would lose the window.
#[test]
fn restore_off_keeps_no_title_but_a_whole_fingerprint() {
    let f = Fp { app: "code.exe".into(), class: "Chrome_WidgetWin_1".into(), title: "salary 2027.xlsx".into() };
    assert!(fp_text(&f, true).contains("salary"));
    let off = fp_text(&f, false);
    assert!(!off.contains("salary"));
    let line = format!("V DEL-1 {}{off}", spot(0, 'n').text());
    let read = parse_state(&line, |_| None);
    assert!(matches!(&read[..], [Line::V { fp: Some(Fp { app, class, title }), .. }] if app == "code.exe" && class == "Chrome_WidgetWin_1" && title.is_empty()));
}

// --quit after a crash shows what it can, but an away window's line stays: its screen may
// still come back, after the next start or a reboot (AC-8).
#[test]
fn rescue_keeps_away_lines() {
    let text = "M DEL-1 2 0 0 100 100\tDell\nA DEL-1 1 100 7 0 0 10 10 n 0\ta\tc\tt\n";
    let (out, shown, left) = rescued(text, |_, _| Some(true));
    assert_eq!((shown, left), (1, 0));
    assert_eq!(out, "M DEL-1 0 0 0 100 100\tDell\nA DEL-1 1 100 7 0 0 10 10 n 0\ta\tc\tt\n");
}

// Win+Ctrl+arrows cycle: past the last workspace you land on the first, and before the first
// on the last, so either key reaches every workspace.
#[test]
fn a_step_wraps_both_ways() {
    assert_eq!(step_target(3, 1, 4), 0);
    assert_eq!(step_target(0, -1, 4), 3);
    assert_eq!(step_target(1, 1, 4), 2);
    assert_eq!(step_target(2, -1, 4), 1);
}

// With one workspace a step lands where it started, so switch only shows the indicator and
// hides nothing.
#[test]
fn a_step_with_one_workspace_stays_put() {
    assert_eq!(step_target(0, 1, 1), 0);
    assert_eq!(step_target(0, -1, 1), 0);
}

fn shown_on(active: usize, last: Option<usize>) -> Monitor {
    let mut m = Monitor::with(4);
    m.active = active;
    m.last_switch = last.map(|ws| (Instant::now(), ws));
    m
}

// A digit past your workspace count must not switch anywhere: the indicator says there is no
// such workspace, whatever jump_back says.
#[test]
fn a_jump_past_the_count_is_missing() {
    let m = shown_on(1, Some(0));
    assert_eq!(jump_target(&m, 4, 4, true), Jump::Missing);
    assert_eq!(jump_target(&m, 8, 4, false), Jump::Missing);
}

// Pressing the shown workspace's digit is a back key only when you turned jump_back on and
// there is a last workspace; otherwise it just shows where you are (spec 0024 AC-8).
#[test]
fn a_jump_to_the_shown_workspace_goes_back_only_with_jump_back_on() {
    let m = shown_on(2, Some(0));
    assert_eq!(jump_target(&m, 2, 4, true), Jump::Back(0));
    assert_eq!(jump_target(&m, 2, 4, false), Jump::Stay);
    assert_eq!(jump_target(&shown_on(2, None), 2, 4, true), Jump::Stay);
}

// Any other digit goes to its workspace, even with a back target waiting: jump_back changes
// only the shown workspace's digit.
#[test]
fn a_jump_elsewhere_goes_there() {
    let m = shown_on(2, Some(0));
    assert_eq!(jump_target(&m, 3, 4, true), Jump::Go(3));
    assert_eq!(jump_target(&m, 0, 4, false), Jump::Go(0));
}
