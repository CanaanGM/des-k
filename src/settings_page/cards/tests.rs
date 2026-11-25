// Tests for the hand edit lines.
use super::*;

// A hand edit shows in the page's words, as the design's banner does, never the file's.
#[test]
fn hand_edits_read_in_the_page_words() {
    assert_eq!(changed("indicator.hold_ms", "550", "800"), "Indicator stays: 550 → 800 ms");
    assert_eq!(changed("shortcuts.overview", "[Win+Tab]", "[]"), "Open overview: Win Tab → no key");
    assert_eq!(changed("fastest", "false", "true"), "Skip animations: off → on");
    assert_eq!(changed("overview.animation", "off", "fade"), "Overview animation: None → Fade");
    // App rules are not shortcut keys: they once read "notepad.exe: 3 or …" (tried 2026-11-22).
    assert_eq!(changed("apps", "[notepad.exe: 3]", "[notepad.exe: 3, mspaint.exe: 2 on XYZ-0000-1]"), "App rules: changed");
}
