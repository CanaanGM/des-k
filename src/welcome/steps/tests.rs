// The first run's step tests: the shortcut rows' words and keys.
use super::*;
use crate::settings::ALT;

// A row's words must say where its Windows key went, so giving one back never leaves you
// guessing whether Windows has it (AC-FR-4): des-k's, given back bare, your own key, taken.
#[test]
fn a_shortcut_row_says_where_its_windows_key_is() {
    let d = Settings::default();
    assert_eq!(held(&d, TAKES[2].keys), Held::Desk);
    let bare = Settings { finder: vec![], ..Settings::default() };
    assert_eq!(held(&bare, TAKES[2].keys), Held::Bare);
    let own = Settings { finder: vec![Combo { mods: WIN | ALT, vk: b'F' as u16 }], ..Settings::default() };
    assert_eq!(held(&own, TAKES[2].keys), Held::Own(None));
    // Go to workspace took Win Ctrl 1–9 from Send window to 1–9 and stay.
    let moved = Settings { jump: Some(WIN | CTRL), send_to_stay: None, ..Settings::default() };
    assert_eq!(held(&moved, TAKES[3].keys), Held::Own(Some("Send window to 1–9 and stay")));
    assert!(matches!(held(&moved, TAKES[5].keys), Held::Took("Go to workspace 1–9")));
    assert_eq!(take_caption(&moved, 5), "No key. Win Ctrl 1–9 now goes to workspace 1–9.");
    // The ← → pair is des-k's only while both arrows are.
    let half = Settings { next: vec![], ..Settings::default() };
    assert_ne!(held(&half, TAKES[0].keys), Held::Desk);
}

// A pair row shows one key only when both arrows share their modifiers, as you set them
// together; otherwise it must not pretend they match.
#[test]
fn a_pair_shows_as_one_key_only_when_it_is_one() {
    let d = Settings::default();
    assert_eq!(row_caps(&d, TAKES[0].keys).map(|c| words(&c)), Some("Win Ctrl ← →".into()));
    let odd = Settings { next: vec![Combo { mods: WIN | ALT, vk: VK_RIGHT.0 }], ..Settings::default() };
    assert_eq!(row_caps(&odd, TAKES[0].keys).map(|c| words(&c)), Some("Win Ctrl ←".into()));
}

// Spec 0038 AC-11: step 1 tells you how to aim the keys, so its words must match the rule in
// force, or the first thing you try hits the other screen.
#[test]
fn step_1_says_how_to_aim_the_switch_keys() {
    let focus = Settings { act_on: settings::ActOn::Focus, ..Settings::default() };
    let (switch, overview) = (TRIES[0].2, TRIES[1].2);
    assert_eq!(try_how(0, switch, &Settings::default()), "Move your mouse to a screen, then press the keys.");
    assert_eq!(try_how(0, switch, &focus), "Click a window on a screen, then press the keys.");
    assert_eq!(try_how(1, overview, &focus), overview, "only the switch row changes");
}
