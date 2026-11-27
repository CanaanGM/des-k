// Tests for what Where it shows writes.
use super::*;
use crate::settings_page::tests::{loaded, said};

// AC-13: a screen key equal to All screens' goes, so the screen follows All screens again,
// but never the table's last key: walking the spots must not silently drop a kept place.
#[test]
fn a_screen_key_equal_to_all_screens_goes_but_never_the_last() {
    let id = "DEL-A0F5-1234";
    let two = loaded("[indicator]\ngap = 24\n[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"top-right\"\ngap = 40\n");
    assert_eq!(said(place_edit(two, Some(id), GAP, 24.0)), "remove indicator.screens.DEL-A0F5-1234.gap");
    let one = loaded("[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"top-right\"\n");
    let center = spot_index(settings::Spot::Center) as f64;
    assert_eq!(said(place_edit(one, Some(id), SPOT, center)), "at indicator.screens.DEL-A0F5-1234.spot = \"center\"");
    assert_eq!(said(place_edit(one, Some(id), GAP, 24.0)), "none", "it follows All screens' gap already");
    assert_eq!(said(place_edit(one, Some(id), GAP, 40.0)), "at indicator.screens.DEL-A0F5-1234.gap = 40");
    let none = loaded("");
    assert_eq!(said(place_edit(none, Some(id), SPOT, center)), "none");
    assert_eq!(said(place_edit(none, None, SPOT, 2.0)), "at indicator.spot = \"top-right\"");
    assert_eq!(said(place_edit(none, None, ACROSS, 70.0)), "at indicator.x = 70");
    // And what the screen then uses: its spot from its table, the rest from All screens.
    let text = "[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"top-right\"\n";
    let after = settings::parse::parse_for_test(&settings::edit::apply_for_test(text, &place_edit(one, Some(id), GAP, 40.0).unwrap())).0;
    assert_eq!(pos_of(&after, Some(id)).gap, 40);
    assert_eq!(pos_of(&after, Some(id)).spot, settings::Spot::TopRight);
}

// Spots, from Custom, picks the spot nearest the card: the third it sits in.
#[test]
fn spots_from_custom_is_the_nearest_third() {
    assert_eq!(SPOT_NAMES[nearest(70.0, 20.0)], "Top right");
    assert_eq!(SPOT_NAMES[nearest(50.0, 50.0)], "Center");
    assert_eq!(SPOT_NAMES[nearest(0.0, 100.0)], "Bottom left");
}
