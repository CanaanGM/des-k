// Tests for what the color rows write and what a paste takes.
use super::*;
use crate::settings_page::tests::said;

// AC-12: the file only holds colors that differ, so typing des-k's own gold takes ws2 out
// and the workspace follows des-k's palette again.
#[test]
fn a_default_color_takes_its_key_out() {
    assert_eq!(said(Some(color_edit("colors.ws2", 0xDDB43C))), "remove colors.ws2");
    assert_eq!(said(Some(color_edit("colors.ws2", 0xDDB43D))), "at colors.ws2 = \"#DDB43D\"");
    assert_eq!(said(Some(color_edit(ACCENT, 0xE0457B))), "at colors.accent = \"#E0457B\"");
}

// The hex box's paste takes exactly six hex digits, trimming spaces and a leading #, so a
// stray paste never saves half a color.
#[test]
fn paste_takes_exactly_six_hex_digits() {
    assert_eq!(pasted(" #2e8b57 "), Some("2E8B57".into()));
    assert_eq!(pasted("2E8B57"), Some("2E8B57".into()));
    assert_eq!(pasted("#2E8B5"), None);
    assert_eq!(pasted("#2E8B577"), None);
    assert_eq!(pasted("seagreen"), None);
    assert_eq!(hex_color("2E8B57"), Some(0x2E8B57));
    assert_eq!(hex_color("2E8B5"), None);
}
