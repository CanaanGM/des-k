// The first run's touchpad tests: spec 0011's states in its words.
use super::*;

// spec 0011's states each get their own words, and Give back hides while a failed one
// offers Try again (AC-FR-5).
#[test]
fn the_touchpad_states_word_each_case() {
    let (n, o) = (PadRow::Nothing, PadRow::StillOn);
    let st = |rows: [PadRow; 4], saved| PadState { pad: true, enabled: true, rows, off: rows == [n; 4], saved };
    assert!(pad_view(&st([o; 4], false), None).note.is_some_and(|x| x.title == "Turn off four gestures in Windows"));
    assert_eq!(pad_view(&st([o, n, o, n], true), None).sub, "2 of 4 done. des-k starts once all four are off in Windows. Until then, the 2 you turned off don't work.");
    assert!(pad_view(&st([n; 4], true), None).ready);
    let failed = GaveBack { wrote: [Wrote::NotOne; 4], cleared: false };
    let v = pad_view(&st([n; 4], true), Some(failed));
    assert!(!v.give && v.note.is_some_and(|x| x.buttons[0].1 == "Try again"));
}
