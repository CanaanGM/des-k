// Tests for the screens a chooser lists.
use super::*;
use crate::settings_page::tests::loaded;

// The chooser lists every screen with a reason to be there: plugged in, remembered, or
// named by a table des-k never saw; All screens always first.
#[test]
fn the_chooser_lists_every_known_screen() {
    let r = RECT { left: 0, top: 0, right: 10, bottom: 10 };
    let view = crate::ws::views::View {
        present: vec![crate::ws::views::Present { id: "DEL-1".into(), device: "DISPLAY2".into(), name: "DELL U2720Q".into(), windows_name: "DELL U2720Q".into(), full: r, work: r }],
        waiting: vec![crate::ws::views::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }],
        ..Default::default()
    };
    let s = loaded("[indicator.screens.\"DEL-1\"]\nspot = \"top\"\n[indicator.screens.\"ACR-9\"]\nspot = \"left\"\n");
    let list = screen_choices(&view, s);
    let shown: Vec<(&str, &str)> = list.iter().map(|c| (c.name.as_str(), c.side)).collect();
    assert_eq!(shown, [("All screens", ""), ("DELL U2720Q", "Own place"), ("LG 27UL850", "Not plugged in"), ("ACR-9", "Unknown screen")]);
}
