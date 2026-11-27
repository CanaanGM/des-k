// Tests for writing a change into the file.
use super::*;
use crate::settings::parse::{action, parse};

// Spec 0020: a screen's place is written as its own [indicator.screens."<id>"] table and
// read back; taking out its last key takes out the table, leaving no empty headers behind.
#[test]
fn screen_tables_come_and_go_whole() {
    let at = |k: &str| vec!["indicator".to_string(), "screens".into(), "DEL-A1B2-12345".into(), k.into()];
    let one = edit_text(DEFAULT_FILE, &Edit::At(at("spot"), "top-right".into())).unwrap();
    assert!(one.contains("[indicator.screens.\"DEL-A1B2-12345\"]
spot = \"top-right\""), "{one}");
    assert!(!one.contains("[indicator.screens]"));
    let (s, w) = parse(&one);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(s.indicator_screens["DEL-A1B2-12345"].spot, Some(Spot::TopRight));
    let two = edit_text(&one, &Edit::At(at("gap"), 40.into())).unwrap();
    let back = edit_text(&two, &Edit::Remove(at("gap"))).unwrap();
    assert_eq!(back, one);
    let gone = edit_text(&one, &Edit::Remove(at("spot"))).unwrap();
    assert_eq!(gone, DEFAULT_FILE);
    let whole = edit_text(&two, &Edit::Remove(at("spot")[..3].to_vec())).unwrap();
    assert_eq!(whole, DEFAULT_FILE);
    let colors = edit_text(DEFAULT_FILE, &Edit::At(vec!["colors".into(), "ws2".into()], "#123456".into())).unwrap();
    assert_eq!(parse(&colors).0.ws_colors[1], Some(0x123456));
}

// Spec 0027 AC-14: a rule is an inline table, and the page changes one key inside it, so
// whatever else you wrote on that line, and the comments around it, stay as they were.
#[test]
fn rule_keys_change_inside_their_inline_table() {
    let file = "[apps]
# mine
\"Slack.exe\" = { workspace = 1, screen = \"GSM-1\" } # chat
";
    let at = |k: &str| vec!["apps".to_string(), "Slack.exe".into(), k.into()];
    let moved = edit_text(file, &Edit::At(at("workspace"), 3.into())).unwrap();
    assert_eq!(moved, "[apps]
# mine
\"Slack.exe\" = { workspace = 3, screen = \"GSM-1\" } # chat
");
    let any = edit_text(&moved, &Edit::Remove(at("screen"))).unwrap();
    assert_eq!(parse(&any).0.apps, vec![("slack.exe".to_string(), Rule { workspace: 2, screen: None })]);
    assert!(any.contains("# mine") && any.contains("# chat"), "{any}");
    // The last rule out leaves [apps] itself for the next one.
    let none = edit_text(&any, &Edit::Remove(at("screen")[..2].to_vec())).unwrap();
    assert_eq!(none, "[apps]
");
    assert!(parse(&none).0.apps.is_empty());
}

// Every upgrade writes first_run_seen into a 0.3.0 file that never had it. The key must
// land in the root table (above [shortcuts], or TOML would read it as a shortcut), the
// comment over [shortcuts] must stay with its header, and the new key has no comment.
#[test]
fn first_run_seen_lands_above_the_first_table() {
    let old = "# des-k settings.\n\nworkspaces = 4\nfastest = false\n\n# Your keys.\n[shortcuts]\nfinder = [\"Win+F\"]\n";
    let new = edit_text(old, &Edit::Value("first_run_seen", true.into())).unwrap();
    let (key, header, comment) = (new.find("first_run_seen = true").unwrap(), new.find("[shortcuts]").unwrap(), new.find("# Your keys.").unwrap());
    assert!(key < comment && comment < header, "{new}");
    assert_eq!(new.lines().find(|l| l.starts_with("first_run_seen")), Some("first_run_seen = true"));
    assert_eq!(parse(&new).0.first_run_seen, Some(true));
    assert_eq!(parse(&new).0.finder, Settings::default().finder);
}

// Written at the first start after an update into a file that never had it: like
// first_run_seen, it must land above [shortcuts] or TOML would read it as a shortcut.
#[test]
fn whats_new_seen_lands_above_the_first_table() {
    let old = "workspaces = 4\nfirst_run_seen = true\n\n# Your keys.\n[shortcuts]\nfinder = [\"Win+F\"]\n";
    let new = edit_text(old, &Edit::Value("whats_new_seen", "0.9.0".into())).unwrap();
    assert!(new.find("whats_new_seen = \"0.9.0\"").unwrap() < new.find("# Your keys.").unwrap(), "{new}");
    let (s, warn) = parse(&new);
    assert_eq!((s.whats_new_seen, warn.len()), (Some(crate::update::Version(0, 9, 0)), 0));
}

// The page and your hand edits share one file, so a click must change only its value and
// keep every comment, both yours and the ones that explain each setting (spec 0012 AC-3).
#[test]
fn a_page_edit_changes_only_that_value() {
    let edit = |name, value| edit_text(DEFAULT_FILE, &Edit::Value(name, value)).unwrap();
    let off = edit("indicator.enabled", toml_edit::Value::from(false));
    assert_eq!(off, DEFAULT_FILE.replacen("[indicator]\nenabled = true", "[indicator]\nenabled = false", 1));
    assert!(!parse(&off).0.indicator_enabled);
    let hold = edit("indicator.hold_ms", toml_edit::Value::from(600));
    assert_eq!(hold, DEFAULT_FILE.replacen("hold_ms = 550               # 0 to 5000", "hold_ms = 600               # 0 to 5000", 1));
    let top = edit("workspaces", toml_edit::Value::from(6));
    assert_eq!(top, DEFAULT_FILE.replacen("workspaces = 4              # 2 to 9", "workspaces = 6              # 2 to 9", 1));
    // A key the file left out is added, and nothing else moves.
    let added = edit_text("[indicator]\nenabled = true # mine\n", &Edit::Value("indicator.hold_ms", toml_edit::Value::from(100))).unwrap();
    assert_eq!(added, "[indicator]\nenabled = true # mine\nhold_ms = 100\n");
}

// A recorded combo replaces only its own line, keeping the column the file lined up.
#[test]
fn a_combo_edit_changes_only_that_list() {
    let c = |mods, vk: u8| Combo { mods, vk: vk as u16 };
    let text = edit_text(DEFAULT_FILE, &Edit::Combos("shortcuts.finder", vec![c(WIN, b'F'), c(WIN | ALT, b'G')])).unwrap();
    assert_eq!(text, DEFAULT_FILE.replacen("finder = [\"Win+F\"]", "finder = [\"Win+F\", \"Win+Alt+G\"]", 1));
    assert_eq!(parse(&text).0.finder, [c(WIN, b'F'), c(WIN | ALT, b'G')]);
    let off = edit_text(DEFAULT_FILE, &Edit::Digits("shortcuts.jump", None)).unwrap();
    assert_eq!(off, DEFAULT_FILE.replacen("jump         = \"Win\"", "jump         = \"\"", 1));
    let on = edit_text(&off, &Edit::Digits("shortcuts.jump", Some(WIN | ALT))).unwrap();
    assert_eq!(parse(&on).0.jump, Some(WIN | ALT));
}

// Move it here saves both entries in one write, so the file never holds the clash.
#[test]
fn a_pair_writes_both_entries() {
    let next = Combo { mods: WIN, vk: b'F' as u16 };
    let pair = Edit::Pair(Box::new(Edit::Combos("shortcuts.next", vec![next])), Box::new(Edit::Combos("shortcuts.finder", vec![])));
    let (s, warn) = parse(&edit_text(DEFAULT_FILE, &pair).unwrap());
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!((s.next, s.finder), (vec![next], vec![]));
}

// The page rewrites every block from its 10 rows. Written over the default file with the
// default map, nothing may change, so the comment above the blocks and their layout stay.
#[test]
fn the_gesture_rewrite_keeps_the_file() {
    let d = Settings::default();
    let map = full_map(|f, m, t| d.rule(f, m, t));
    assert_eq!(map, d.gestures);
    assert_eq!(edit_text(DEFAULT_FILE, &Edit::Gestures(map)).unwrap(), DEFAULT_FILE);
}

// A twice rule and a key combo survive the rewrite, and a row the file had no rule for is
// written as "none" (AC-12).
#[test]
fn the_gesture_rewrite_parses_back() {
    let (hand, _) = parse("[[gesture]]\nfingers = 3\ntap = true\ndo = \"search\"\n");
    let combo = Action::Combo(Combo { mods: CTRL | SHIFT, vk: b'T' as u16 });
    let map = full_map(|f, m, t| match (f, m, t) {
        (4, Motion::Left, 2) => Some(combo),
        _ => hand.rule(f, m, t),
    });
    let text = edit_text("# mine\nworkspaces = 3\n", &Edit::Gestures(map.clone())).unwrap();
    assert!(text.starts_with("# mine\nworkspaces = 3\n"), "{text}");
    let (s, warn) = parse(&text);
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.gestures, map);
    assert_eq!(s.rule(3, Motion::Tap, 1), Some(action("search").unwrap()));
    assert_eq!(s.rule(4, Motion::Left, 1), Some(Action::Nothing));
    assert_eq!(s.rule(4, Motion::Left, 2), Some(combo));
    assert_eq!(s.gestures.len(), 11);
}
