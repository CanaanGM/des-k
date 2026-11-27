// Tests for moving windows: map_rect keeps a window's relative place inside the new work area.
use super::*;
use crate::ws::tests::rect;

// Shift+Enter promises the same relative place and size: a left half stays a left half,
// even on a screen with another size and position.
#[test]
fn map_rect_keeps_the_relative_place_and_size() {
    let laptop = rect(0, 0, 1920, 1140);
    let external = rect(1920, -200, 1920 + 2560, -200 + 1400);
    let left_half = rect(0, 0, 960, 1140);
    assert_eq!(map_rect(laptop, external, left_half), rect(1920, -200, 1920 + 1280, 1200));
}

// A window hanging off its old screen must still land fully on the new one, never lost off its edge.
#[test]
fn map_rect_stays_inside_the_new_work_area() {
    let a = rect(0, 0, 1000, 1000);
    let b = rect(1000, 0, 2000, 1000);
    assert_eq!(map_rect(a, b, rect(800, 900, 1200, 1300)), rect(1600, 600, 2000, 1000));
    assert_eq!(map_rect(a, b, rect(-50, -50, 1100, 1100)), rect(1000, 0, 2000, 1000));
}
