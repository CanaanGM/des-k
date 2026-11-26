// Tests for screens: resizing the count, the gone and wake checks, settling, claims, spots and
// returns, twins and ids, folds and give back.
use super::*;
use crate::ws::tests::{rect, with_hidden, spot};

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

// Spec 0039 AC-12 (and AC-2, AC-6 through it): pause in the 10 s after a screen leaves, and a
// window hidden there shows on no screen, so Windows minimizes it. It is held, not hidden and
// not away, yet its spot from before must stay frozen: else the fold reads it minimized, it
// comes home minimized off its screen, and resume leaves it in view (found live 2026-11-26).
#[test]
fn a_held_window_on_no_screen_keeps_its_spot_while_its_screen_is_missing() {
    // A handle no real window has: app_windows never lists it, as it never listed the minimized one.
    let raw: isize = 0x7FFF_FFF0_0000;
    let mut s = State::default();
    let mut lg = Monitor::with(4);
    lg.held[1].push((raw, 7));
    s.monitors.insert("LG".into(), lg);
    s.spots.insert(raw, ("LG".into(), spot(-756, 'n')));
    let now: HashSet<String> = ["internal".to_string()].into();

    refresh_spots(&mut s, &now);

    assert_eq!(s.spots.get(&raw), Some(&("LG".to_string(), spot(-756, 'n'))), "the fold needs the spot it had on LG, normal, not minimized");
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

// A screen with more workspaces than you now have folds its extra ones onto the main display's
// last workspace, so no window lands on a workspace that does not exist (spec 0016 AC-2).
#[test]
fn a_fold_caps_the_workspace_at_the_last_one() {
    assert_eq!(fold_to(4, 3, 0), FoldTo::Hide(2));
    assert_eq!(fold_to(4, 3, 2), FoldTo::Show(2));
}

// A window from the workspace the main display shows comes into view; one from any other
// workspace stays hidden there, as it was on its own screen.
#[test]
fn a_fold_shows_only_the_workspace_the_main_display_shows() {
    assert_eq!(fold_to(1, 4, 1), FoldTo::Show(1));
    assert_eq!(fold_to(3, 4, 1), FoldTo::Hide(3));
}

// Windows may move a window off a screen before des-k hears the screen left, so the spot stored
// at the last check is its true home spot. One stored for another screen says nothing about this
// one, so the spot is read now.
#[test]
fn a_fold_uses_the_spot_stored_for_the_screen_that_left() {
    let stored = ("DEL-1".to_string(), spot(10, 'n'));
    assert_eq!(fold_spot(Some(&stored), "DEL-1", || panic!("read although stored")), spot(10, 'n'));
    assert_eq!(fold_spot(Some(&stored), "DEL-2", || spot(20, 'z')), spot(20, 'z'));
    assert_eq!(fold_spot(None, "DEL-1", || spot(30, 'n')), spot(30, 'n'));
}

// Spec 0039 AC-12: a screen that folds while paused must not lose which workspace each held
// window belongs to. Each one still open gets an away record carrying its held workspace and its
// spot there, so it goes home held on that workspace and resume hides it there. A closed window,
// or one whose handle another process took, is nothing of yours to bring back.
#[test]
fn a_fold_while_paused_gives_each_held_window_an_away_record_with_its_workspace() {
    let held = vec![vec![], vec![(10, 1)], vec![(20, 2), (30, 3), (40, 99)]];
    let open = |raw: isize, pid: u32| raw != 30 && pid != 99; // 30 closed, 40's handle now another pid's
    let out = held_aways("LG", held, open, |raw| spot(raw as i32, 'n'));
    let rec = |ws: usize, raw: isize, pid: u32| (raw, Away { home: "LG".into(), ws, spot: spot(raw as i32, 'n'), pinned: false, pid });
    assert_eq!(out, [rec(1, 10, 1), rec(2, 20, 2)]);
}

fn away(ws: usize, pinned: bool) -> Away {
    Away { home: "DEL-1".into(), ws, spot: spot(100, 'n'), pinned, pid: 7 }
}

// Pinned means in view on every workspace, so a pinned window comes home in view even when its
// workspace is hidden there; anything else goes home in view only on the workspace shown.
#[test]
fn a_return_brings_into_view_only_pinned_windows_and_the_shown_workspace() {
    assert_eq!(goes_in_view(&away(2, true), 4, 0, false), (2, true));
    assert_eq!(goes_in_view(&away(2, false), 4, 2, false), (2, true));
    assert_eq!(goes_in_view(&away(2, false), 4, 0, false), (2, false));
}

// You may have lowered the count while the screen was away: its windows go to the last
// workspace, never to one that no longer exists.
#[test]
fn a_return_caps_the_workspace_at_the_last_one() {
    assert_eq!(goes_in_view(&away(5, false), 3, 0, false), (2, false));
    assert_eq!(goes_in_view(&away(5, false), 3, 2, false), (2, true));
}

// On a Windows layout des-k does not know nothing can be hidden, so every window goes home in
// view rather than being left on the wrong screen (spec 0030 AC-6).
#[test]
fn a_return_on_an_unknown_layout_brings_everything_into_view() {
    assert_eq!(goes_in_view(&away(3, false), 4, 0, true), (3, true));
}

// A screen that comes back as it was gets each window back at its exact spot; one whose work
// area changed (a new resolution, the taskbar moved) gets the same relative place and size.
#[test]
fn a_return_keeps_the_exact_spot_unless_the_area_changed() {
    let area = rect(0, 0, 1000, 1000);
    assert_eq!(home_spot(&away(0, false), area, area), spot(100, 'n'));
    let bigger = rect(0, 0, 2000, 2000);
    assert_eq!(home_spot(&away(0, false), area, bigger), Spot { rect: rect(200, 0, 400, 200), state: 'n' });
}

// Each way home keeps a window from flashing or being lost: one already in place is not touched,
// one in view is moved, a hidden one moves before it shows, a hidden one that stays hidden is
// moved and listed again, and one in view that goes home hidden is hidden before it moves.
// Where a window is now is read only for one in view that stays in view.
#[test]
fn each_way_home_follows_from_where_it_is_and_where_it_goes() {
    let unread = || panic!("read where it is although it moves anyway");
    assert_eq!(way_home(true, false, || true), Way::InPlace);
    assert_eq!(way_home(true, false, || false), Way::Move);
    assert_eq!(way_home(true, true, unread), Way::MoveShow);
    assert_eq!(way_home(false, true, unread), Way::MoveRelist);
    assert_eq!(way_home(false, false, unread), Way::HideMove);
}
