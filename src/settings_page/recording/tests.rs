// Tests for the clash prompt a recorded key gets.
use super::*;
use crate::settings_page::tests::SETTINGS;

// Back's key given to another action asks first and names back by its row, like every clash,
// so a recorded key never silently turns back off (spec 0024 AC-6).
#[test]
fn a_key_back_has_gets_the_clash_prompt() {
    let _one = SETTINGS.lock();
    settings::store::use_for_test("workspaces = 4\n");
    let combo = settings::Combo { mods: settings::WIN, vk: 0x08 };
    let Plan::Ask(p) = plan_combo("shortcuts.finder", Some(0), combo) else { panic!("took back's key without asking") };
    assert_eq!(p.title, "Win Backspace is already in use");
    assert!(p.sub.contains("“Back to the last workspace” uses it"), "{}", p.sub);
}
