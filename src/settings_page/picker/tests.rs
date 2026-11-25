// Tests for the color picker's undo, closing, guard and keys.
use super::*;
use crate::settings_page::cards::colors::{color_title, path_of};
use crate::settings_page::tests::said;

// AC-6: Esc and Cancel put back exactly what the file had. No key then: it goes again (des-k's
// own gold stays des-k's). des-k's gold typed by hand: it is written back as typed, not
// taken out, since the file is yours. Nothing is written when the file holds it already.
#[test]
fn undo_writes_back_exactly_what_the_file_had() {
    let k = |t: &str| settings::edit::raw_for_test(t, &path_of("colors.ws2"));
    let saved = k("[colors]\nws2 = \"#92BF4F\"\n");
    assert_eq!(said(undo_edit("colors.ws2", None, saved.as_ref())), "remove colors.ws2");
    let typed = k("[colors]\nws2 = \"#DDB43C\" # mine\n");
    assert_eq!(said(undo_edit("colors.ws2", typed.as_ref(), saved.as_ref())), "at colors.ws2 = \"#DDB43C\" # mine");
    assert_eq!(said(undo_edit("colors.ws2", typed.as_ref(), k("[colors]\nws2 = \"#DDB43C\"\n").as_ref())), "none", "a comment is no difference");
    assert_eq!(said(undo_edit("colors.ws2", None, None)), "none");
    // An inline table reads the same as a section.
    assert_eq!(said(undo_edit("colors.ws2", None, k("colors = { ws2 = \"#92BF4F\" }\n").as_ref())), "remove colors.ws2");
}

// AC-12: the picker's own write reloads before it returns, so the value in flight is never an
// outside edit; a save that works, then one that fails, then the 1 s reload (the file still
// holding the first) leaves the picker open; anything else closes it, and Windows' accent always does.
#[test]
fn only_a_value_the_picker_did_not_write_closes_it() {
    assert!(!outside(Some(0x92BF4F), 0xDDB43C, Some(0x92BF4F)), "its own write, in flight");
    assert!(!outside(Some(0x92BF4F), 0x92BF4F, None), "the reload after a save that worked");
    assert!(!outside(Some(0x92BF4F), 0x92BF4F, None), "a failed save left the file as it was");
    assert!(outside(Some(0x123456), 0x92BF4F, None), "an edit by hand");
    assert!(outside(None, 0xE0457B, None), "the accent switched to Windows");
}

// AC-13: the page's notes hold still while the picker is open, and catch up on any close.
#[test]
fn notes_catch_up_when_the_picker_closes() {
    let why = Some("os error 32".to_string());
    let title = || color_title("colors.ws2");
    let old = || Some(("Workspace 2 color".to_string(), "os error 5".to_string()));
    // A failure, then Esc: undone, no note.
    assert_eq!(note_after(why.as_ref(), false, How::Undo, title(), None), None);
    // A failure, then Done: the note, with Windows' words, under the color's title.
    assert_eq!(note_after(why.as_ref(), false, How::Keep(true), title(), None), Some(("Workspace 2 color".into(), "os error 32".into())));
    // A failure, then a re-pick of the file's color (Not saved gone, nothing written), then Done.
    assert_eq!(note_after(None, false, How::Keep(true), title(), None), None);
    // A note from before it opened, and a save in it that worked: the note goes.
    assert_eq!(note_after(None, true, How::Keep(true), title(), old()), None);
    // A failure, then an outside edit to its key: the page caused the close, the note shows.
    assert_eq!(note_after(why.as_ref(), false, How::Quiet, title(), None).map(|n| n.0), Some("Workspace 2 color".into()));
    // Nothing saved, nothing failed: the note stays as it was.
    assert_eq!(note_after(None, false, How::Undo, title(), old()), old());
}

// AC-15: a double click on a tile opens the picker and does nothing else, but a quick pick
// right after opening still works.
#[test]
fn the_opening_click_guards_only_its_own_spot_and_moment() {
    let now = Instant::now();
    let g = Some(Guard { at: (500, 300), until: now + Duration::from_millis(500) });
    let soon = now + Duration::from_millis(200);
    assert!(guarded(g, (501, 300), soon, (4, 4)), "the second press of a double click");
    assert!(!guarded(g, (503, 300), soon, (4, 4)), "3 px away: a press of its own");
    assert!(!guarded(g, (501, 300), now + Duration::from_millis(600), (4, 4)), "after the double click time");
    assert!(!guarded(None, (500, 300), soon, (4, 4)), "opened from the keyboard: no guard");
}

// AC-9: the picker's keys.
#[test]
fn the_keys_move_pick_keep_and_undo() {
    let h = |a: Option<Arrow>| match a {
        Some(Arrow::Hsv(hsv)) => hsv,
        other => panic!("{other:?}"),
    };
    assert_eq!(h(arrow(Stop::Ring, VK_LEFT, false, (0.0, 0.5, 0.5))).0, 359.0, "Left at hue 0 wraps");
    assert_eq!(h(arrow(Stop::Ring, VK_RIGHT, true, (355.0, 0.5, 0.5))).0, 5.0, "Shift turns 10");
    assert_eq!(h(arrow(Stop::Square, VK_UP, true, (10.0, 0.5, 0.95))).2, 1.0, "Shift+Up stops at full value");
    assert_eq!(h(arrow(Stop::Square, VK_LEFT, false, (10.0, 0.0, 0.5))).1, 0.0, "saturation stops at 0");
    assert_eq!(arrow(Stop::Swatches(2), VK_RIGHT, false, (0.0, 0.0, 0.0)), Some(Arrow::Swatch(3)), "the third of a row goes on to the next row");
    assert_eq!(arrow(Stop::Swatches(11), VK_RIGHT, false, (0.0, 0.0, 0.0)), None, "the last stays");
    assert_eq!(arrow(Stop::Swatches(10), VK_DOWN, false, (0.0, 0.0, 0.0)), None, "the bottom row stays");
    assert_eq!(arrow(Stop::Swatches(4), VK_UP, false, (0.0, 0.0, 0.0)), Some(Arrow::Swatch(1)));
    assert_eq!(arrow(Stop::Done, VK_LEFT, false, (0.0, 0.0, 0.0)), None);
    assert_eq!(key_does(VK_RETURN, Stop::Ring, false), KeyDoes::Close(How::Keep(true)), "Enter on the ring keeps");
    assert_eq!(key_does(VK_RETURN, Stop::Swatches(3), false), KeyDoes::Close(How::Keep(true)), "Enter never picks");
    assert_eq!(key_does(VK_RETURN, Stop::Cancel, false), KeyDoes::Close(How::Undo), "Enter on Cancel undoes");
    assert_eq!(key_does(VK_SPACE, Stop::Done, false), KeyDoes::Close(How::Keep(true)));
    assert_eq!(key_does(VK_SPACE, Stop::Swatches(3), false), KeyDoes::Pick(3));
    assert_eq!(key_does(VK_ESCAPE, Stop::Square, false), KeyDoes::Close(How::Undo));
    assert_eq!(key_does(VK_RETURN, Stop::Ring, true), KeyDoes::Nothing, "a held Enter acts once");
    assert_eq!(key_does(VK_ESCAPE, Stop::Ring, true), KeyDoes::Nothing);
    // Tab wraps through five stops; back on the swatches, the color shown, else the last.
    assert_eq!(tab(Stop::Cancel, false, 0x123456, 4), Stop::Swatches(4));
    assert_eq!(tab(Stop::Cancel, false, ui::PRESETS[7], 4), Stop::Swatches(7));
    assert_eq!(tab(Stop::Swatches(7), true, 0, 7), Stop::Cancel);
    assert_eq!(tab(Stop::Ring, false, 0, 0), Stop::Square);
}

// AC-8: the duplicate line names the lowest other workspace in use with the same or a very
// close color, counted as it shows (its default with no key), and never one you don't have.
#[test]
fn the_duplicate_line_names_the_lowest_workspace_in_use() {
    let mut ws = ui::WS;
    assert_eq!(dup_of(0x79A4F3, 1, &ws, 4), Some((0, true)), "workspace 1's own blue");
    assert_eq!(dup_of(0x7AA5F2, 1, &ws, 4), Some((0, false)), "a step off is very close");
    assert_eq!(dup_of(0x92BF4F, 1, &ws, 9), None, "lime is nobody's");
    assert_eq!(dup_of(0xDDB43C, 1, &ws, 4), None, "its own color is no duplicate");
    ws[2] = 0x92BF4F;
    ws[5] = 0x92BF4F;
    assert_eq!(dup_of(0x92BF4F, 1, &ws, 9), Some((2, true)), "the lowest");
    assert_eq!(dup_of(0xF97676, 1, &ws, 8), None, "workspace 9 isn't in use");
}
