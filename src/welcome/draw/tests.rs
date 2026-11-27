// The first run's drawing tests: the Done step's taken line.
use super::*;

// The Done step's last line names exactly the Windows keys des-k holds: a key given back
// must leave the line, or you'd think Windows still lost it.
#[test]
fn the_taken_line_follows_what_des_k_holds() {
    let d = Settings { copilot_as_right_ctrl: true, ..Settings::default() };
    assert_eq!(taken_line(&d), "Taken over from Windows: Win Tab, Win F, Win Ctrl ← →, Win 1–9 (also with Shift or Ctrl) and the Copilot key. Change any in des-k settings › Shortcuts.");
    let s = Settings { finder: vec![], send_to: None, copilot_as_right_ctrl: false, ..Settings::default() };
    assert_eq!(taken_line(&s), "Taken over from Windows: Win Tab, Win Ctrl ← →, Win 1–9 and Win Ctrl 1–9. Change any in des-k settings › Shortcuts.");
    let none = Settings { finder: vec![], overview: vec![], previous: vec![], next: vec![], jump: None, send_to: None, send_to_stay: None, copilot_as_right_ctrl: false, ..Settings::default() };
    assert!(taken_line(&none).starts_with("des-k takes no keys"));
}
