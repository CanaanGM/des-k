// Tests for reading the file: each bad value resets alone, clashes, gestures, names, pictures, apps.
use super::*;
use crate::settings::edit::{Edit, edit_text};

// The file des-k writes for you and the values it falls back to are two copies of the
// defaults. If they drift, a fresh install behaves differently from a broken file.
#[test]
fn default_file_is_the_defaults() {
    let (s, warn) = parse(DEFAULT_FILE);
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s, Settings::default());
    // parse() starts from the defaults, so also prove every key is spelled out in the file.
    let top: Table = DEFAULT_FILE.parse().unwrap();
    let (sc, ind, res) = (top["shortcuts"].as_table().unwrap(), top["indicator"].as_table().unwrap(), top["restore"].as_table().unwrap());
    // whats_new_seen has no default to write: the first start writes the running version.
    assert!(TOP_KEYS.iter().filter(|k| **k != "whats_new_seen").all(|k| top.contains_key(*k)));
    assert!(SHORTCUT_KEYS.iter().all(|k| sc.contains_key(*k)));
    // Screen tables and workspace colors have no default to write; the file shows an example.
    assert!(INDICATOR_KEYS.iter().filter(|k| **k != "screens").all(|k| ind.contains_key(*k)));
    assert!(top["colors"].as_table().unwrap().contains_key("accent"));
    assert!(RESTORE_KEYS.iter().all(|k| res.contains_key(*k)));
    assert!(OVERVIEW_KEYS.iter().all(|k| top["overview"].as_table().unwrap().contains_key(*k)));
    assert!(TOUCHPAD_KEYS.iter().all(|k| top["touchpad"].as_table().unwrap().contains_key(*k)));
    assert!(ATTENTION_KEYS.iter().all(|k| top["attention"].as_table().unwrap().contains_key(*k)));
}

// Spec 0022 AC-9: a bad text resets only itself, and the log line never quotes it (your
// words could name a person). 100 UTF-16 units fit: an emoji counts as two, as the box counts.
#[test]
fn a_bad_attention_text_resets_only_itself() {
    let hundred = "é".repeat(98) + "😀";
    let (s, warn) = parse(&format!("[attention]\nenabled = false\ntext = \"{hundred}\"\n"));
    assert_eq!((s.attention_enabled, s.attention_text.as_str(), warn.len()), (false, hundred.as_str(), 0));
    for (text, why) in [("\"\"", "it is empty"), ("\"   \"", "it is empty"), ("7", "expected text"), (&format!("\"{hundred}x\""), "101 characters")] {
        let (s, warn) = parse(&format!("[attention]\nenabled = false\ntext = {text}\n"));
        assert_eq!((s.attention_enabled, s.attention_text.as_str()), (false, ATTENTION_TEXT), "{text}");
        assert!(warn.len() == 1 && warn[0].contains(why) && !warn[0].contains("é"), "{warn:?}");
    }
}

// "none" is how you get a spread with no text at all, so it must stick; a typo costs only
// the labels, back to titles on hover.
#[test]
fn a_bad_overview_labels_only_resets_itself() {
    let (s, warn) = parse("[overview]\nlabels = \"None\"\n");
    assert_eq!(s.overview_labels, Labels::None);
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("workspaces = 6\n[overview]\nlabels = \"sometimes\"\n");
    assert_eq!((s.workspaces, s.overview_labels), (6, Labels::Hover));
    assert_eq!(warn, ["overview.labels: \"sometimes\" is not one of \"hover\", \"always\", \"none\", using default \"hover\""]);
}

// live = false is how you stop the spread moving as you aim (AC-10), so it must stick; a
// bad value costs only that key, back to live.
#[test]
fn a_bad_overview_live_only_resets_itself() {
    let (s, warn) = parse("[overview]\nlive = false\n");
    assert!(!s.overview_live);
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("[overview]\nlabels = \"none\"\nlive = \"no\"\n");
    assert_eq!((s.overview_labels, s.overview_live), (Labels::None, true));
    assert_eq!(warn.len(), 1, "{warn:?}");
    assert!(warn[0].starts_with("overview.live:"), "{warn:?}");
}

// The indicator style and `fastest` are read like every key: a bad value resets only itself
// (spec 0017), so a typo never turns the card into the dots or the fades off.
#[test]
fn style_and_fastest_reset_only_themselves() {
    let (s, warn) = parse("fastest = true\n[indicator]\nstyle = \"Dots\"\n");
    assert_eq!((s.fastest, s.indicator_style), (true, Style::Dots));
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("fastest = \"yes\"\n[indicator]\nstyle = \"pill\"\nhold_ms = 900\n");
    assert_eq!((s.fastest, s.indicator_style, s.hold_ms), (false, Style::Card, 900));
    assert_eq!(warn.len(), 2, "{warn:?}");
    assert!(warn[1].starts_with("indicator.style: \"pill\" is not one of \"card\", \"dots\""), "{warn:?}");
}

// A fresh file says false and an upgrader's lacks the key: both must see the first run once.
#[test]
fn first_run_seen_tells_fresh_from_upgrade() {
    assert_eq!(parse(DEFAULT_FILE).0.first_run_seen, Some(false));
    assert_eq!(parse("workspaces = 3\n").0.first_run_seen, None);
    assert_eq!(parse("first_run_seen = true\n").0.first_run_seen, Some(true));
    // A typo shows the first run again rather than never.
    let (s, warn) = parse("first_run_seen = \"yes\"\n");
    assert_eq!((s.first_run_seen, warn.len()), (Some(false), 1));
}

// What's new opens for releases newer than this key (spec 0029 AC-9): a missing key is an
// upgrader from 0.8.0, and anything that isn't X.Y.Z must count the same, with one warning,
// or a typo would hide What's new for good.
#[test]
fn whats_new_seen_reads_a_version_or_counts_as_missing() {
    use crate::update::Version;
    assert_eq!(parse("workspaces = 3\n").0.whats_new_seen, None);
    assert_eq!(parse("whats_new_seen = \"0.9.0\"\n").0.whats_new_seen, Some(Version(0, 9, 0)));
    for bad in ["whats_new_seen = \"banana\"\n", "whats_new_seen = \"0.9\"\n", "whats_new_seen = 9\n"] {
        let (s, warn) = parse(bad);
        assert_eq!((s.whats_new_seen, warn.len()), (None, 1), "{bad}");
    }
}

// The overview's fade time is yours to shorten (0 is instant) but never past a second; a
// bad value goes back to 250, and only it, so a typo never costs the animation itself
// (spec 0017 AC-7).
#[test]
fn a_bad_overview_fade_time_only_resets_itself() {
    assert_eq!(parse("").0.overview_fade_ms, 250);
    let (s, warn) = parse("[overview]\nfade_ms = 0\n");
    assert_eq!(s.overview_fade_ms, 0);
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("[overview]\nanimation = \"fade\"\nfade_ms = 1500\n");
    assert_eq!((s.overview_animation, s.overview_fade_ms), (Animation::Fade, 250));
    assert_eq!(warn.len(), 1, "{warn:?}");
    assert!(warn[0].starts_with("overview.fade_ms:"), "{warn:?}");
}

// The hints window has no key until you set one (your words: "nothing by default"), a key
// you set must stick, and one that another shortcut already has must not steal it: the
// earlier list keeps it and only hints turns off (spec 0017 AC-HNT-4).
#[test]
fn the_hints_key_is_off_by_default_and_loses_a_clash() {
    assert!(parse("").0.hints.is_empty());
    let (s, warn) = parse("[shortcuts]
hints = [\"Win+Ctrl+H\"]
");
    assert_eq!(s.hints, [Combo { mods: WIN | CTRL, vk: b'H' as u16 }]);
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("[shortcuts]
hints = [\"Win+F\"]
");
    assert_eq!((s.hints.is_empty(), s.finder.clone()), (true, Settings::default().finder));
    assert_eq!(warn, ["shortcuts: Win+F is in both finder and hints, hints turned off"]);
    assert_eq!(s.combo_owner("hints", Combo { mods: WIN, vk: b'F' as u16 }), Some(Owner::List("finder")));
}

// Off must stay the default (the overview opens at once unless you ask), a chosen style
// must stick, and a typo, or the "zoom" an older file may still hold, costs only the
// animation, never the keys next to it (AC-11, AC-14).
#[test]
fn a_bad_overview_animation_only_resets_itself() {
    assert_eq!(parse("").0.overview_animation, Animation::Off);
    let (s, warn) = parse("[overview]\nanimation = \"Fade\"\n");
    assert_eq!(s.overview_animation, Animation::Fade);
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("[overview]\nanimation = \"zoom\"\nlive = false\n");
    assert_eq!((s.overview_animation, s.overview_live), (Animation::Off, false));
    assert_eq!(warn, ["overview.animation: \"zoom\" is not one of \"off\", \"fade\", using default \"off\""]);
}

// minutes = 0 would end the restore before any app reopens; it must not also switch restore off.
#[test]
fn a_bad_restore_minutes_only_resets_itself() {
    let (s, warn) = parse("[restore]\nenabled = false\nminutes = 0\n");
    assert!(!s.restore_enabled);
    assert_eq!(s.restore_minutes, 3);
    assert_eq!(warn.len(), 1, "{warn:?}");
}

// The whole point of reading key by key: a typo costs one setting, never the rest.
#[test]
fn a_bad_value_only_resets_itself() {
    let text = "workspaces = 12\nfollow_activation = \"yes\"\ntypo = 1\n[shortcuts]\nprevious = [\"alt+CTRL+left\", \"Ctrl+Banana\", 7]\n[indicator]\nhold_ms = \"long\"\nfade_ms = 0\n";
    let (s, warn) = parse(text);
    assert_eq!(s.workspaces, 4);
    // A typo must not silently switch follow off: on is the safe side, typing never goes invisible.
    assert!(s.follow_activation);
    assert_eq!(s.previous, [Combo { mods: ALT | CTRL, vk: VK_LEFT.0 }]);
    assert_eq!(s.hold_ms, 550);
    assert_eq!(s.fade_ms, 0);
    assert_eq!(warn.len(), 6, "{warn:?}"); // workspaces, follow_activation, typo, Banana, 7, hold_ms
}

// follow_activation = false is the escape from an app that keeps stealing focus, so it must stick.
#[test]
fn jump_back_can_be_turned_off() {
    let (s, warn) = parse("jump_back = false\n");
    assert!(!s.jump_back, "off must give back spec 0005's jump, which only shows the card");
    assert!(warn.is_empty(), "{warn:?}");
    let (s, warn) = parse("jump_back = \"no\"\n");
    assert!(s.jump_back, "a bad value keeps the default");
    assert_eq!(warn.len(), 1, "{warn:?}");
}

#[test]
fn follow_activation_can_be_turned_off() {
    let (s, warn) = parse("follow_activation = false\n");
    assert!(!s.follow_activation);
    assert!(warn.is_empty(), "{warn:?}");
}

// One combo in both lists would make a key do two things; neither list can be trusted.
#[test]
fn a_clash_resets_both_lists() {
    let (s, warn) = parse("[shortcuts]\nprevious = [\"Win+Ctrl+Right\"]\n");
    assert_eq!((s.previous, s.next), (Settings::default().previous, Settings::default().next));
    assert_eq!(warn.len(), 1, "{warn:?}");
}

// Win+Ctrl+Left/Right and the swipe must keep switching, whatever the send lists say.
#[test]
fn a_send_list_clashing_with_a_switch_list_is_turned_off() {
    let (s, warn) = parse("[shortcuts]
send_next_stay = [\"Win+Ctrl+Right\"]
");
    let d = Settings::default();
    assert!(s.send_next_stay.is_empty());
    assert_eq!((&s.previous, &s.next), (&d.previous, &d.next));
    assert_eq!((&s.send_previous, &s.send_next, &s.send_previous_stay), (&d.send_previous, &d.send_next, &d.send_previous_stay));
    assert_eq!(warn, ["shortcuts: Win+Ctrl+Right is in both next and send_next_stay, send_next_stay turned off"]);
}

// Moving a switch shortcut onto a default send combo must not make one key do two things.
#[test]
fn a_switch_list_beats_a_default_send_list() {
    let (s, warn) = parse("[shortcuts]
previous = [\"Win+Ctrl+Shift+Left\"]
");
    assert_eq!(s.previous, [Combo { mods: WIN | CTRL | SHIFT, vk: VK_LEFT.0 }]);
    assert!(s.send_previous.is_empty());
    assert_eq!(warn.len(), 1, "{warn:?}");
}

// A stay combo that is also a follow combo would send and switch at once; the earlier list keeps it.
#[test]
fn a_stay_list_clashing_with_a_follow_list_is_turned_off() {
    let (s, warn) = parse("[shortcuts]
send_previous_stay = [\"Win+Ctrl+Shift+Right\"]
");
    assert_eq!(s.send_next, Settings::default().send_next);
    assert!(s.send_previous_stay.is_empty());
    assert_eq!(warn.len(), 1, "{warn:?}");
}

// A typo in one send list costs that list only, back to its default.
#[test]
fn a_bad_send_entry_only_resets_its_list() {
    let (s, warn) = parse("[shortcuts]
send_next = [\"Win+Banana\"]
send_previous = [\"Win+Alt+N\"]
");
    let d = Settings::default();
    assert_eq!(s.send_next, d.send_next);
    assert_eq!(s.send_previous, [Combo { mods: WIN | ALT, vk: b'N' as u16 }]);
    assert_eq!((&s.send_previous_stay, &s.send_next_stay), (&d.send_previous_stay, &d.send_next_stay));
    assert_eq!(warn.len(), 2, "{warn:?}"); // Banana dropped, then no valid entry left
}

// "" is how you give Win+N back to the taskbar, so it must turn only that set off.
#[test]
fn an_empty_digit_set_is_off() {
    let (s, warn) = parse("[shortcuts]\njump = \"\"\n");
    let d = Settings::default();
    assert_eq!(s.jump, None);
    assert_eq!((s.send_to, s.send_to_stay), (d.send_to, d.send_to_stay));
    assert!(warn.is_empty(), "{warn:?}");
}

// Shift alone would swallow every typed ! @ #, so it is refused; a typo costs only its own key.
#[test]
fn a_bad_digit_set_only_resets_itself() {
    let (s, warn) = parse("[shortcuts]\njump = \"Shift\"\nsend_to = \"Win+Hyper\"\nsend_to_stay = \"alt+WIN\"\n");
    let d = Settings::default();
    assert_eq!((s.jump, s.send_to), (d.jump, d.send_to));
    assert_eq!(s.send_to_stay, Some(WIN | ALT));
    assert_eq!(warn.len(), 2, "{warn:?}");
}

// One modifier set for two actions would make a digit both jump and send; the earlier set keeps it.
#[test]
fn a_digit_set_clashing_with_an_earlier_one_is_turned_off() {
    let (s, warn) = parse("[shortcuts]\nsend_to_stay = \"win\"\n");
    assert_eq!(s.jump, Some(WIN));
    assert_eq!(s.send_to_stay, None);
    assert_eq!(warn, ["shortcuts: Win is in both jump and send_to_stay, send_to_stay turned off"]);
}

// An explicit list on one digit must not cost the set its other eight digits. It is a
// setting the page makes on purpose (Move it here), so it is not logged as a problem.
#[test]
fn a_list_on_a_digit_only_takes_that_digit() {
    let (s, warn) = parse("[shortcuts]\nnext = [\"Win+Ctrl+2\"]\n");
    assert_eq!(s.next, [Combo { mods: WIN | CTRL, vk: b'2' as u16 }]);
    assert_eq!(s.send_to_stay, Some(WIN | CTRL));
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.skips("send_to_stay"), [('2', "next")]);
}

// Removing the last combo on the page writes [], and that must turn the shortcut off, not
// bring the default back. A list whose every entry is bad still falls back (spec 0001 AC-6).
#[test]
fn an_empty_list_is_off() {
    let (s, warn) = parse("[shortcuts]\nfinder = []\nprevious = []\n");
    assert!(s.finder.is_empty() && s.previous.is_empty());
    assert_eq!(s.next, Settings::default().next);
    assert!(warn.is_empty(), "{warn:?}");
}

// Win+Ctrl+Right must keep switching (and the swipe with it) even if the finder is set onto it.
#[test]
fn a_finder_combo_clashing_with_a_switch_list_turns_the_finder_off() {
    let (s, warn) = parse("[shortcuts]
finder = [\"Win+Ctrl+Right\"]
");
    assert_eq!(s.next, Settings::default().next);
    assert!(s.finder.is_empty());
    assert_eq!(warn, ["shortcuts: Win+Ctrl+Right is in both next and finder, finder turned off"]);
}

// The hook tries the lists first, so a finder on Win+3 really does take 3 from jump; the log must say so.
#[test]
fn a_finder_on_a_digit_takes_that_digit_from_its_set() {
    let (s, warn) = parse("[shortcuts]
finder = [\"Win+3\"]
");
    assert_eq!(s.finder, [Combo { mods: WIN, vk: b'3' as u16 }]);
    assert_eq!(s.jump, Some(WIN));
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.skips("jump"), [('3', "finder")]);
}

// Win+F must keep opening the finder if the overview is set onto it: the earlier list keeps
// the combo, so one key never opens both.
#[test]
fn an_overview_combo_clashing_with_the_finder_turns_the_overview_off() {
    let (s, warn) = parse("[shortcuts]
overview = [\"Win+F\"]
");
    assert_eq!(s.finder, Settings::default().finder);
    assert!(s.overview.is_empty());
    assert_eq!(warn, ["shortcuts: Win+F is in both finder and overview, overview turned off"]);
}

// Writing one rule means you chose your own map: none of the defaults may still act.
#[test]
fn a_written_rule_replaces_the_whole_default_map() {
    let (s, warn) = parse("[[gesture]]\nfingers = 3\ntap = true\ndo = \"Play_Pause\"\n");
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.gestures, [Gesture { fingers: 3, motion: Motion::Tap, times: 1, action: action("play_pause").unwrap() }]);
    assert_eq!(s.rules(4, Motion::Left), (None, None));
}

// AC-4: one typo must never cost the other gestures; each bad rule says why it was skipped.
#[test]
fn a_bad_rule_only_skips_itself() {
    let text = r#"
[[gesture]]
fingers = 5
swipe = "left"
do = "next"
[[gesture]]
fingers = 4
swipe = "left"
do = "explode"
[[gesture]]
fingers = 4
swipe = "left"
tap = true
do = "next"
[[gesture]]
fingers = 4
do = "next"
[[gesture]]
fingers = 4
swipe = "left"
times = 3
do = "next"
[[gesture]]
fingers = 4
swipe = "left"
do = "next"
speed = 2
[[gesture]]
fingers = 3
tap = false
do = "search"
[[gesture]]
fingers = 4
swipe = "sideways"
do = "next"
[[gesture]]
fingers = 4
swipe = "right"
do = "previous"
"#;
    let (s, warn) = parse(text);
    assert_eq!(s.gestures, [Gesture { fingers: 4, motion: Motion::Right, times: 1, action: Action::Switch(-1) }]);
    assert_eq!(warn.len(), 8, "{warn:?}");
    for (i, w) in warn.iter().enumerate() {
        assert!(w.starts_with(&format!("gesture rule {}: ", i + 1)) && w.ends_with(", skipped"), "{w}");
    }
    assert!(warn[1].contains("\"explode\""), "{}", warn[1]);
    assert!(warn[5].contains("speed"), "{}", warn[5]);
}

// Two rules for one gesture would make it do two things; the first one keeps it.
#[test]
fn a_repeated_gesture_keeps_the_first_rule() {
    let (s, warn) = parse("[[gesture]]\nfingers = 4\nswipe = \"up\"\ndo = \"finder\"\n[[gesture]]\nfingers = 4\nswipe = \"UP\"\ndo = \"overview\"\n");
    assert_eq!(s.gestures.len(), 1);
    assert_eq!(s.gestures[0].action, Action::Finder);
    assert_eq!(warn, ["gesture rule 2: 4 up is already set by an earlier rule, skipped"]);
}

// A combo is how you reach any key Windows or an app has; an action name always wins over
// one, so "next" can never be read as something else.
#[test]
fn do_takes_a_name_or_a_combo() {
    assert_eq!(action("NEXT"), Ok(Action::Switch(1)));
    assert_eq!(action("back"), Ok(Action::Back));
    assert_eq!(action("send_to_stay_3"), Ok(Action::SendTo(2, false)));
    assert_eq!(action("send_to_9"), Ok(Action::SendTo(8, true)));
    assert_eq!(action("jump_1"), Ok(Action::Jump(0)));
    assert_eq!(action("Win+Ctrl+Right"), Ok(Action::Combo(Combo { mods: WIN | CTRL, vk: VK_RIGHT.0 })));
    for bad in ["jump_0", "jump_10", "Left", "explode"] {
        assert!(action(bad).is_err(), "{bad}");
    }
    // Every name prints back as itself, so the log shows what you wrote.
    for (name, a) in ACTIONS {
        assert_eq!(a.to_string(), *name);
    }
    // The welcome screen shows these words; a name with no label would show raw there.
    for (name, _) in ACTIONS {
        assert!(ACTION_LABELS.iter().any(|(n, _)| n == name), "{name} has no label");
    }
    assert_eq!(Action::SendTo(2, false).label(), "send window to 3, stay");
    assert_eq!(Action::Combo(Combo { mods: WIN | CTRL, vk: VK_RIGHT.0 }).label(), "Win+Ctrl+Right");
    let (_, warn) = parse("[[gesture]]\nfingers = 4\ntap = true\ndo = \"Win+L\"\n");
    assert_eq!(warn, ["gesture rule 1: Windows never lets a program type Win+L, so it does nothing"]);
}

// A bad [touchpad] value costs that key only; a gesture list that is not [[gesture]] blocks
// falls back to the default map, never to no gestures at all.
#[test]
fn a_bad_touchpad_value_only_resets_itself() {
    let (s, warn) = parse("gesture = 4\n[touchpad]\nenabled = true\nswipe_mm = 60\ntwice_ms = 500\n");
    let d = Settings::default();
    assert!(s.touchpad_enabled);
    assert_eq!((s.swipe_mm, s.twice_ms), (15, 500));
    assert_eq!(s.gestures, d.gestures);
    assert_eq!(warn.len(), 2, "{warn:?}");
}

#[test]
fn combo_text_round_trips() {
    for text in ["Win+Ctrl+Left", "Ctrl+Shift+A", "Alt+F24", "Win+7", "Ctrl+PageDown"] {
        assert_eq!(parse_combo(text).unwrap().to_string(), text);
    }
    for bad in ["Left", "Win+Win+Left", "Win+A+B", "Win+", "Win+F25", "Win+Ctrl"] {
        assert!(parse_combo(bad).is_err(), "{bad}");
    }
}

// A file you are halfway through typing must not become all defaults on a live reload:
// that would remap every shortcut mid edit (AC-14). At start, defaults stay the fallback.
#[test]
fn a_broken_file_is_an_error_on_reload_not_defaults() {
    let why = reparse("workspaces = \n").unwrap_err();
    assert!(why.starts_with("not valid TOML (") && why.contains("line 1"), "{why}");
    assert!(edit_text("workspaces = \n", &Edit::Value("workspaces", toml_edit::Value::from(3))).is_err());
    let (s, warn) = parse("workspaces = \n");
    assert_eq!((s, warn.len()), (Settings::default(), 1));
    assert_eq!(reparse("workspaces = 6\n").unwrap().0.workspaces, 6);
}

// The page finds a setting's note by its key, so the key must come from the log words.
#[test]
fn a_problem_knows_its_setting() {
    let (_, warn) = parse("typo = 1\n[indicator]\nhold_ms = \"x\"\n");
    let keys: Vec<String> = warn.into_iter().map(|w| Problem::new(w).key).collect();
    assert_eq!(keys, ["", "indicator.hold_ms"]);
}

// The recorder refuses keys the file can't name, so every key it takes must round trip.
#[test]
fn every_savable_key_round_trips() {
    for vk in (0..=255u16).filter(|vk| savable(*vk)) {
        let combo = Combo { mods: CTRL, vk };
        assert_eq!(parse_combo(&combo.to_string()), Ok(combo), "vk {vk:#x}");
    }
    assert!(!savable(VK_NUMPAD3.0) && !savable(VK_OEM_1.0) && !savable(VK_VOLUME_UP.0) && !savable(VK_LWIN.0));
}

// Back's default key must be one the file can name, read in any case and written back the
// same way, or the page's recorder could never save it again (spec 0024 AC-6).
#[test]
fn backspace_is_a_key_name() {
    let combo = Combo { mods: WIN, vk: VK_BACK.0 };
    assert_eq!(parse_combo("win+BACKSPACE"), Ok(combo));
    assert_eq!(combo.to_string(), "Win+Backspace");
    assert!(savable(VK_BACK.0));
}

// Spec 0027 AC-10: one bad rule never costs the others, each problem is one line, and a key
// with a path in it is never echoed, so a shared log can't leak where your files live.
#[test]
fn a_bad_app_rule_only_drops_itself() {
    let file = r#"[apps]
"Slack.exe" = { workspace = 1, screen = "GSM-5B7F-12345" }
"C:\\x\\notepad.exe" = { workspace = 2 }
"spotify" = { workspace = 2 }
"zero.exe" = { workspace = 0 }
"text.exe" = { workspace = "1" }
"empty.exe" = { workspace = 1, screen = "" }
"colour.exe" = { workspace = 1, colour = 1 }
"plain.exe" = 3
"slack.exe" = { workspace = 3 }
"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App" = { workspace = 4 }
"spotify.exe" = { workspace = 4 }
"#;
    let (s, warn) = parse(file);
    let rule = |w, sc: Option<&str>| Rule { workspace: w, screen: sc.map(str::to_string) };
    assert_eq!(s.apps, vec![
        ("slack.exe".to_string(), rule(0, Some("GSM-5B7F-12345"))),
        ("microsoft.windowscalculator_8wekyb3d8bbwe!app".to_string(), rule(3, None)),
        ("spotify.exe".to_string(), rule(3, None)),
    ]);
    assert_eq!(warn.len(), 8, "{warn:?}");
    assert!(warn.iter().all(|w| !w.contains("C:") && !w.contains("\\x")), "{warn:?}");
    assert!(warn.iter().any(|w| w.contains("\"slack.exe\" differs from an earlier key only in case")), "{warn:?}");
}

// Spec 0027 AC-10: past 64 rules the rest are dropped with one line, not one each.
#[test]
fn app_rules_stop_at_64() {
    let file = (0..70).fold("[apps]\n".to_string(), |f, i| f + &format!("\"app{i}.exe\" = {{ workspace = 1 }}\n"));
    let (s, warn) = parse(&file);
    assert_eq!(s.apps.len(), APPS_MAX);
    assert_eq!(s.apps.last().map(|(k, _)| k.as_str()), Some("app63.exe"));
    assert_eq!(warn, vec!["apps: more than 64 rules, the last 6 ignored".to_string()]);
}

// Spec 0023 AC-2, AC-15: a bad name resets only its own key, and the warn line names the key
// but never the text (a name can say anything). 32 units fit after cleanup; an emoji is two.
#[test]
fn a_bad_name_resets_only_itself_and_is_never_logged() {
    let full = "é".repeat(30) + "😀";
    let (s, warn) = parse(&format!("[names]\nws1 = \"  {full}\\t\"\nws2 = \"Mail\\nRoom\"\n"));
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!((s.names[0].as_str(), s.names[1].as_str()), (full.as_str(), "Mail Room"));
    let long = format!("\"{full}x\"");
    for (key, value, why) in [("ws1", long.as_str(), "33 characters"), ("ws1", "7", "expected text"), ("ws10", "\"Secret\"", "unknown key"), ("foo", "\"Secret\"", "unknown key")] {
        let (s, warn) = parse(&format!("[names]\nws2 = \"Code\"\n{key} = {value}\n"));
        assert_eq!((s.names[0].as_str(), s.names[1].as_str()), ("", "Code"), "{key} = {value}");
        assert!(warn.len() == 1 && warn[0].contains(why) && !warn[0].contains('é') && !warn[0].contains("Secret") && !warn[0].contains("Code"), "{warn:?}");
    }
    let (s, warn) = parse(&format!("[names.screens.internal]\nscreen = {long}\nws2 = \"Notes\"\nws12 = \"x\"\n"));
    assert_eq!(warn.len(), 2, "{warn:?}");
    assert_eq!((s.screen_rename("internal"), s.ws_name("internal", 1)), (None, Some("Notes")));
}

// Spec 0026 AC-2: the admin des-k reads a picture by joining the wallpapers folder with this
// value, so anything that could reach outside the folder, or isn't a picture type, resets
// only its own key. The warn line names the key, never the value (a file name can say
// anything about you, AC-16).
#[test]
fn a_bad_picture_resets_only_itself_and_is_never_logged() {
    let (s, warn) = parse("[wallpaper]\nws1 = \"Forest.JPG\"\nws2 = \"\"\nws3 = \"a b.jpeg\"\n");
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!((s.wall_name("x", 0), s.wall_name("x", 1), s.wall_name("x", 2)), (Some("Forest.JPG"), None, Some("a b.jpeg")));
    let long = format!("\"{}.png\"", "s".repeat(252));
    let outside = "not a plain file name";
    for (key, value, why) in [
        ("ws1", r#""..\\secret.jpg""#, outside),
        ("ws1", r#""C:\\secret.jpg""#, outside),
        ("ws1", r#""secret/b.jpg""#, outside),
        ("ws1", r#""secret.jpg:x.png""#, outside),
        ("ws1", r#""secret..jpg""#, outside),
        ("ws1", r#"".""#, outside),
        ("ws1", "5", "expected a picture, a folder or a list"),
        ("ws1", long.as_str(), "256 characters"),
        ("ws10", r#""secret.jpg""#, "unknown key"),
    ] {
        let (s, warn) = parse(&format!("[wallpaper]\nws2 = \"desk.png\"\n{key} = {value}\n"));
        assert_eq!((s.wall_name("x", 0), s.wall_name("x", 1)), (None, Some("desk.png")), "{key} = {value}");
        assert!(warn.len() == 1 && warn[0].contains(why) && warn[0].contains(key), "{warn:?}");
        assert!(!warn[0].contains("secret") && !warn[0].contains("desk") && !warn[0].contains("sss"), "{warn:?}");
    }
}

// Spec 0026 AC-2: a screen table holds the same plain file names; a bad one, or an unknown
// key, resets only itself and is logged by key, never by value.
#[test]
fn a_bad_screen_picture_resets_only_itself() {
    let (s, warn) = parse("[wallpaper]\nws2 = \"forest.jpg\"\n[wallpaper.screens.internal]\nws1 = \"desk.png\"\nws2 = \"C:\\\\secret.jpg\"\nfit = \"secret\"\n");
    assert_eq!((s.wall_name("internal", 0), s.wall_name("internal", 1)), (Some("desk.png"), Some("forest.jpg")));
    assert_eq!(warn.len(), 2, "{warn:?}");
    assert!(warn.iter().any(|w| w.contains("wallpaper.screens.\"internal\".ws2") && w.contains("not a plain file name")), "{warn:?}");
    assert!(warn.iter().all(|w| !w.contains("secret") && !w.contains("desk")), "{warn:?}");
    let (s, warn) = parse("[wallpaper]\nws2 = \"forest.jpg\"\nscreens = \"secret\"\n");
    assert!(warn.len() == 1 && warn[0].contains("wallpaper.screens") && !warn[0].contains("secret"), "{warn:?}");
    assert_eq!(s.wall_name("internal", 1), Some("forest.jpg"));
}

// SECURITY.md (review H-02): the admin des-k reads settings.toml, which anything running as
// you can rewrite, so no value in it may steer des-k to a program or a file outside its own
// folders. Every value des-k turns into a name it acts on gets a program and a path here;
// one that kept any of them would be a way in.
#[test]
fn no_setting_names_a_program_or_a_path() {
    let cmd = r"C:\\Windows\\System32\\cmd.exe";
    let text = format!(
        r#"
[wallpaper]
ws1 = "{cmd}"
ws2 = "..\\..\\secret.jpg"
ws3 = ["{cmd}", "\\\\server\\share\\a.png", "C:/x/b.jpg", "..\\c.png"]
ws4 = "C:"
[wallpaper.screens.internal]
ws1 = "{cmd}"
ws5 = "/etc/d.png"
[apps]
"{cmd}" = {{ workspace = 2 }}
"\\\\server\\share\\slack.exe" = {{ workspace = 2 }}
"../slack.exe" = {{ workspace = 2 }}
"C:slack.exe" = {{ workspace = 2 }}
[[gesture]]
fingers = 3
swipe = "up"
do = "{cmd}"
[[gesture]]
fingers = ["{cmd}"]
swipe = "{cmd}"
"#
    );
    let (s, warn) = parse(&text);
    assert!((0..9).all(|i| s.wallpaper_for("internal", i).is_none() && s.wallpaper_for("other", i).is_none()), "{:?} {:?}", s.wallpaper, s.wallpaper_screens);
    assert!(s.apps.is_empty(), "{:?}", s.apps);
    assert!(s.gestures.is_empty(), "{:?}", s.gestures);
    assert!(warn.iter().any(|w| w.starts_with("gesture rule 1:") && w.ends_with("skipped")), "{warn:?}");
    // A refusal names the key, never the path: a log you share never carries one (it could
    // name your user folder). Typed paths into every kind of value a warn line could repeat.
    let more = format!(
        r#"
[shortcuts]
next = ["{cmd}", 7, ["{cmd}"]]
jump = "{cmd}"
[indicator]
style = "{cmd}"
"{cmd}" = 1
[colors]
accent = "{cmd}"
"#
    );
    let (s2, warn2) = parse(&more);
    assert_eq!(s2.next, Settings::default().next);
    let all: Vec<&String> = warn.iter().chain(&warn2).collect();
    assert!(all.len() == 20 && all.iter().all(|w| !w.contains("cmd.exe") && !w.contains("server") && !w.contains("System32")), "{all:#?}");
}

// Spec 0026 AC-4, AC-5, AC-18: fit, transition and dim are one value for every screen, and
// a bad one resets only itself. dim resets to 0, never to the last good value, so a typo
// can't leave pictures darker than the file says; past 80 a picture is close to black.
#[test]
fn fit_transition_and_dim_reset_only_themselves() {
    let s = parse("").0;
    assert_eq!((s.wallpaper_fit, s.wallpaper_fade, s.wallpaper_dim), (Fit::Fill, false, 0), "fill, instant, no dim");
    let (s, warn) = parse("[wallpaper]\nfit = \"Center\"\ntransition = \"fade\"\ndim = 80\nws1 = \"forest.jpg\"\n");
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!((s.wallpaper_fit, s.wallpaper_fade, s.wallpaper_dim), (Fit::Center, true, 80));
    for bad in ["dim = 81", "dim = -5", "dim = \"40\"", "dim = 40.5"] {
        let (s, warn) = parse(&format!("[wallpaper]\nfit = \"fit\"\nws1 = \"forest.jpg\"\n{bad}\n"));
        assert_eq!((s.wallpaper_dim, s.wallpaper_fit, s.wall_name("x", 0)), (0, Fit::Whole, Some("forest.jpg")), "{bad}");
        assert!(warn.len() == 1 && warn[0].starts_with("wallpaper.dim"), "{bad}: {warn:?}");
    }
    let (s, warn) = parse("[wallpaper]\nfit = \"tile\"\ntransition = \"slide\"\ndim = 40\n");
    assert_eq!((s.wallpaper_fit, s.wallpaper_fade, s.wallpaper_dim), (Fit::Fill, false, 40));
    assert!(warn.len() == 2 && warn[0].starts_with("wallpaper.fit") && warn[1].starts_with("wallpaper.transition"), "{warn:?}");
}

// Spec 0028 AC-1, AC-14: a string without a picture's extension names a subfolder (so
// notes.txt is a folder, not a reset), and a list is a set: entries that aren't picture
// names are skipped with one line giving their count, never their names, repeats count
// once, one left is a plain picture and none left is no wallpaper.
#[test]
fn folders_and_sets_parse_by_their_shape() {
    let file = r#"[wallpaper]
ws1 = "beach"
ws2 = "notes.txt"
ws3 = ["c.jpg", "A.png", "secret", 5, "c.jpg", "..\\secret.jpg"]
ws4 = ["only.jpg", "trips"]
ws5 = []
[wallpaper.screens.internal]
ws1 = ["y.png", "x.jpg"]
ws2 = []
"#;
    let (s, warn) = parse(file);
    assert_eq!(s.wallpaper_for("x", 0), Some(&Pick::Folder("beach".into())));
    assert_eq!(s.wallpaper_for("x", 1), Some(&Pick::Folder("notes.txt".into())));
    assert_eq!(s.wallpaper_for("x", 2), Some(&Pick::Set(vec!["A.png".into(), "c.jpg".into()])), "in name order, no repeats");
    assert_eq!(s.wallpaper_for("x", 3), Some(&Pick::Picture("only.jpg".into())));
    assert_eq!(s.wallpaper[4], Pick::None);
    assert_eq!(s.wallpaper_for("internal", 0), Some(&Pick::Set(vec!["x.jpg".into(), "y.png".into()])));
    assert_eq!(s.wallpaper_for("internal", 1), None, "[] on a screen is your Windows wallpaper there");
    assert_eq!(s.wallpaper_screens["internal"][1], Some(Pick::None));
    assert_eq!(warn, ["wallpaper.ws3: 3 entries skipped, not a picture name", "wallpaper.ws4: 1 entry skipped, not a picture name"]);
    assert_eq!(s.wallpaper[2].key(), "A.png/c.jpg", "a set's note key");
    // The page lists a subfolder only when the reader would take its name as a folder.
    assert!(is_folder_name("beach") && is_folder_name("notes.txt"));
    assert!(!is_folder_name("trip.png") && !is_folder_name(".") && !is_folder_name("a\\b"));
}

// Spec 0028 AC-2: a workspace's own interval wins on every screen; a bad one, or an unknown
// key, resets only itself with one line, and the workspace follows `interval` again.
#[test]
fn intervals_follow_interval_unless_their_own_is_good() {
    let (s, warn) = parse("[wallpaper]\n");
    assert_eq!((s.wallpaper_interval, s.interval_for(2), s.wallpaper_shuffle), (30, 30, false));
    assert!(warn.is_empty());
    let (s, warn) = parse("[wallpaper]\ninterval = 10\nshuffle = true\n[wallpaper.intervals]\nws3 = 5\nws4 = 0\nws5 = \"x\"\nws12 = 3\n");
    assert_eq!((s.interval_for(0), s.interval_for(2), s.interval_for(3), s.interval_for(4)), (10, 5, 10, 10));
    assert!(s.wallpaper_shuffle);
    assert_eq!(warn.len(), 3, "{warn:?}");
    assert!(warn[0].contains("wallpaper.intervals.ws12") && warn[1].starts_with("wallpaper.intervals.ws4") && warn[2].starts_with("wallpaper.intervals.ws5"), "{warn:?}");
    let (s, warn) = parse("[wallpaper]\ninterval = 1441\nshuffle = 1\n");
    assert_eq!((s.wallpaper_interval, s.wallpaper_shuffle), (30, false));
    assert!(warn.len() == 2 && warn[0].starts_with("wallpaper.interval") && warn[1].starts_with("wallpaper.shuffle"), "{warn:?}");
}

// AC-11: one bad value costs only that key, and in a screen table it falls back to the
// default's value rather than to center, so a typo never jumps the card across the screen.
#[test]
fn a_bad_place_or_color_resets_only_itself() {
    let text = "[indicator]
spot = \"middle\"
x = 120
gap = 24
y = 10
[indicator.screens.s1]
spot = \"top\"
gap = 900
width = 3
[colors]
accent = \"#abc\"
ws1 = \"#1e2a78\"
ws3 = \"blue\"
";
    let (s, warn) = parse(text);
    assert_eq!(s.indicator_pos, Pos { y: 10.0, ..Pos::default() });
    assert_eq!(s.indicator_pos_for("s1"), Pos { spot: Spot::Top, y: 10.0, ..Pos::default() });
    assert_eq!(s.accent, Accent::Windows);
    assert_eq!(s.ws_colors[..3], [Some(0x1E2A78), None, None]);
    let keys: Vec<&str> = warn.iter().map(|w| w.split(": ").next().unwrap()).collect();
    assert_eq!(keys, ["indicator.spot", "indicator.x", "unknown key indicator.screens.\"s1\".width, ignored", "indicator.screens.\"s1\".gap", "colors.accent", "colors.ws3"]);
    // Not tables at all: the whole screens key goes, the rest of [indicator] stays.
    let (s, warn) = parse("[indicator]
spot = \"left\"
screens = 5
");
    assert_eq!((s.indicator_pos.spot, s.indicator_screens.len()), (Spot::Left, 0));
    assert!(warn[0].starts_with("indicator.screens: expected"), "{warn:?}");
}

// AC-2: "windows" in any case follows Windows; a typed color is kept exactly.
#[test]
fn the_accent_is_windows_or_your_color() {
    assert_eq!(parse("[colors]
accent = \"Windows\"
").0.accent, Accent::Windows);
    assert_eq!(parse("[colors]
accent = \"#E0457b\"
").0.accent, Accent::Rgb(0xE0457B));
    assert_eq!(hex(0xE0457B), "#E0457B");
}
