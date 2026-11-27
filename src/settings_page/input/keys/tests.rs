// Tests for what Esc backs out of.
use super::*;
use crate::settings_page::recording::Prompt;

// Esc with the clash prompt open is Cancel, never close: closing would drop the prompt,
// and you expect Esc to back out of the question, not the page. Banners go next, one per
// Esc, and only an Esc with nothing left closes the page.
#[test]
fn esc_backs_out_one_layer_at_a_time() {
    let p = Prompt { key: "shortcuts.next", held: vec![], title: String::new(), sub: String::new(), edit: Edit::Gestures(vec![]) };
    assert_eq!(escape(Some(&p), true, true), Some(Ctl::Cancel("shortcuts.next")));
    assert_eq!(escape(None, true, true), Some(Ctl::Dismiss));
    assert_eq!(escape(None, false, true), Some(Ctl::DismissEdit));
    assert_eq!(escape(None, false, false), None);
}
