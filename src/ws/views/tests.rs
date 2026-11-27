// Tests for what the finder and the settings page read: screens numbered as you see them.
use super::*;
use crate::ws::tests::rect;

// "S · W" numbers in the finder and the settings page must match what you see: screens
// counted from the left, then from the top.
#[test]
fn screens_are_numbered_left_to_right_then_top_down() {
    let r = |left, top| rect(left, top, left + 100, top + 100);
    let got = in_order(vec![("D1".into(), r(0, 0)), ("D2".into(), r(-1920, 0)), ("D3".into(), r(0, -1080)), ("D4".into(), r(2560, 200))]);
    assert_eq!(got, ["D2", "D3", "D1", "D4"]);
}
