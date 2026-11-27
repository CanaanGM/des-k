// The motion tests: a fade eases out and ends at full strength.
use super::*;

// The fade must end exactly at full strength, or the overview would stay a little see
// through for as long as it is open; and exactly at nothing on close (AC-11).
#[test]
fn a_fade_eases_out_and_ends_exactly_at_full_strength() {
    assert_eq!((ease(0.0), ease(1.0)), (0.0, 1.0));
    assert!(ease(0.5) > 0.5, "ease out covers most of the way early, then settles");
}
