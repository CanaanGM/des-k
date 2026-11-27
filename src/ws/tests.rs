// Tests for ws's core (back's target), and the helpers the other ws tests share.
use super::*;

pub(super) fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT { left, top, right, bottom }
}

pub(super) fn with_hidden(active: usize, hidden: &[&[isize]]) -> Monitor {
    let mut m = Monitor::with(hidden.len());
    m.active = active;
    m.hidden = hidden.iter().map(|l| l.to_vec()).collect();
    m
}

// Back must land on a real workspace other than the shown one, or do nothing but the card:
// a fresh start, a workspace a smaller count removed, and a resize that moved the screen onto
// its own last workspace all have nowhere to go (spec 0024 AC-4).
#[test]
fn back_goes_only_to_a_workspace_that_still_exists() {
    let mut m = with_hidden(2, &[&[], &[], &[], &[]]);
    assert_eq!(m.back_target(4), None, "des-k just started: no switch yet");
    m.last_switch = Some((Instant::now(), 0));
    assert_eq!(m.back_target(4), Some(0));
    m.last_switch = Some((Instant::now(), 3));
    assert_eq!(m.back_target(3), None, "workspace 4 is gone");
    m.last_switch = Some((Instant::now(), 2));
    assert_eq!(m.back_target(4), None, "already shown");
}

pub(super) fn spot(x: i32, state: char) -> Spot {
    Spot { rect: rect(x, 0, x + 100, 100), state }
}

pub(super) fn hwnd(raw: isize) -> HWND {
    HWND(raw as *mut _)
}
