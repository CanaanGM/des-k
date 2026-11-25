// The hints window's rows tests: the rows, the gestures and the touchpad words follow what they
// read, and a recorded key saves or asks first.
use super::*;
use crate::hints::tests::{c, mine, st};
use crate::settings::{CTRL, SHIFT, WIN};
use crate::setup::Wrote;

// AC-HNT-3: every row must show what the file holds now: a pair with the same modifiers is
// one row, a split pair is two (next first), an empty list says No key, and every combo
// of a list shows, so the sheet never hides a key you set.
#[test]
fn the_rows_follow_the_settings() {
    let d = read(&Settings::default(), &st(), None);
    assert_eq!(d.around[0], KeyRow { title: "Switch workspace".into(), keys: vec![vec!["Win".into(), "Ctrl".into(), "Left".into(), "Right".into()]] });
    assert_eq!(d.around[1].keys, [["Win", "1–9"]]);
    assert_eq!(d.around[2], KeyRow { title: "Back to the last workspace".into(), keys: vec![vec!["Win".into(), "Backspace".into()]] }, "right after Go to workspace (spec 0024 AC-6)");
    assert_eq!(d.windows.len(), 5);
    let v = read(&mine(), &st(), None);
    assert_eq!(v.around[0].keys.len(), 2, "both switch combos");
    assert!(v.around[1].keys.is_empty(), "Win 1–9 given back: No key");
    assert_eq!(v.around[4].keys, [["Win", "Alt", "F"]]);
    let titles: Vec<&str> = v.windows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles[..2], ["Send window to next workspace", "Send window to previous workspace"]);
    assert!(v.windows[1].keys.is_empty());
    assert!(!v.copilot);
    assert_eq!(v.key, [c(WIN | CTRL, b'H' as u16)]);
}

// AC-HNT-3: a gesture shows its action, a twice action under it, Nothing, or the keys it types.
#[test]
fn the_gestures_say_what_each_one_does() {
    let v = read(&mine(), &st(), None);
    let (three, four) = (&v.cards[0].1, &v.cards[1].1);
    assert_eq!(three[0].once, Does::Name("Next app".into()));
    assert_eq!(three[3].once, Does::Nothing);
    assert_eq!(three[4].once, Does::Types(vec!["Ctrl".into(), "Alt".into(), "T".into()]));
    assert_eq!((four[2].once.clone(), four[2].twice.as_deref()), (Does::Name("Overview".into()), Some("Twice: Find a window")));
    assert_eq!(four[4].once, Does::Name("Go to workspace 3".into()));
}

// AC-HNT-5: each of spec 0011's touchpad states gets its own words, so you always know
// why a gesture does nothing, and the button that fixes it.
#[test]
fn every_touchpad_state_says_why() {
    assert_eq!(pad(&st(), None), Pad::On);
    let wait = State { rows: [PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing], ..st() };
    let (status, words, button) = pad_words(pad(&wait, None));
    let (_, title, line, warn) = words.unwrap();
    assert_eq!((status, title.as_str(), warn, button), ("Not running", "2 of 4 are still on in Windows", true, Some("Touchpad setup")));
    assert_eq!(line, "Still on: three-finger swipes, four-finger swipes. des-k runs these once all four are off; until then, the 2 you turned off don't work.");
    let cant = State { rows: [PadRow::Nothing, PadRow::CantRead, PadRow::Nothing, PadRow::Nothing], ..st() };
    assert!(pad_words(pad(&cant, None)).1.unwrap().2.starts_with("It is the 3-finger tap setting."));
    // A give back writes your values back, so Windows reads them on again until you sign in.
    let back = GaveBack { wrote: [Wrote::All; 4], cleared: true };
    let given = State { rows: [PadRow::StillOn; 4], off: false, saved: false, ..st() };
    assert_eq!(pad(&given, Some(back)), Pad::GaveBack);
    assert_eq!(pad(&st(), Some(back)), Pad::On, "Windows' gestures read off again: the give back is over");
    assert_eq!(pad(&State { enabled: false, ..st() }, None), Pad::Off);
    assert_eq!(pad(&State { enabled: false, off: false, ..st() }, None), Pad::OffWin);
    assert_eq!(pad_words(Pad::OffWin).2, Some("Turn reading on"));
    assert_eq!(pad(&State { pad: false, ..st() }, None), Pad::Missing);
    assert_eq!(pad_words(Pad::Missing).2, None);
}

// AC-HNT-4: a free key saves at once; one another shortcut has asks first and names it,
// and Move it here takes it from that one only; a digit of a set only costs that digit.
#[test]
fn a_recorded_key_saves_or_asks_first() {
    let s = Settings::default();
    assert_eq!(recorded_rec(&s, c(WIN | CTRL, b'H' as u16)), Some(Rec::Idle));
    match recorded_rec(&s, c(WIN, b'F' as u16)) {
        Some(Rec::Clash { title, sub, other, .. }) => {
            assert_eq!(title, "Win F is already in use");
            assert_eq!(sub, "“Find a window” uses it. Move it here and “Find a window” loses this key. Cancel changes nothing.");
            assert_eq!(other, Some(("shortcuts.finder", vec![])));
        }
        r => panic!("{r:?}"),
    }
    match recorded_rec(&s, c(WIN, b'3' as u16)) {
        Some(Rec::Clash { sub, other: None, .. }) => assert!(sub.contains("skips 3"), "{sub}"),
        r => panic!("{r:?}"),
    }
    let set = Settings { hints: vec![c(WIN | SHIFT, b'H' as u16)], ..Settings::default() };
    assert_eq!(recorded_rec(&set, c(WIN | SHIFT, b'H' as u16)), None, "already its key");
}
