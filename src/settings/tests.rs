// Tests for the model: the defaults, the gesture rules, clashes, and the per screen values.
use super::*;
use super::parse::parse;

// Spec 0022 AC-9: your words come back with the app named wherever you put the tag, in any
// case; text without it shows as typed; a nameless app is still "An app", never blank.
#[test]
fn attention_text_names_the_app() {
    assert_eq!(fill(ATTENTION_TEXT, "Slack"), "Slack misses you");
    assert_eq!(fill("{{AppName}} rang, {{appname}}!", "Teams"), "Teams rang, Teams!");
    assert_eq!(fill("Someone wants you", "Slack"), "Someone wants you");
    assert_eq!(fill("{{appname}} misses you", ""), "An app misses you");
    assert_eq!(fill("Hey\n{{appname}}\tnow", "Ölçü"), "Hey Ölçü now");
    assert_eq!(fill("{{appname", "Slack"), "{{appname");
}

// The gestures you get without writing a rule are spec 0011 AC-2, spelled out here on
// purpose, apart from default_gestures(), so a slip in that table fails this test.
#[test]
fn the_default_gestures_are_ac_2() {
    let (s, warn) = parse("");
    assert!(warn.is_empty(), "{warn:?}");
    let got: Vec<String> = s.gestures.iter().map(|g| format!("{g}: {}", g.action)).collect();
    assert_eq!(
        got,
        [
            "4 left: next", "4 right: previous", "4 up: overview", "4 down: show_desktop", "3 left: app_next",
            "3 right: app_previous", "3 up: overview", "3 down: show_desktop", "3 tap: search", "4 tap: notifications",
        ]
    );
}

// AC-5: the reader must find both rules of a gesture, so it knows to wait for the second go.
#[test]
fn a_twice_rule_is_kept_beside_the_once_rule() {
    let (s, warn) = parse("[[gesture]]\nfingers = 4\nswipe = \"left\"\ndo = \"next\"\n[[gesture]]\nfingers = 4\nswipe = \"left\"\ntimes = 2\ndo = \"jump_1\"\n[[gesture]]\nfingers = 3\ntap = true\ntimes = 2\ndo = \"finder\"\n");
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.rules(4, Motion::Left), (Some(s.gestures[0]), Some(s.gestures[1])));
    assert_eq!(s.rules(3, Motion::Tap), (None, Some(s.gestures[2])));
    assert_eq!(s.rules(4, Motion::Right), (None, None));
}

// The page's clash scan must agree with the hook: a list beats a set, a set only loses
// the one digit, and two sets with the same keys would make a digit do two things.
#[test]
fn clashes_are_found_like_the_hook_finds_combos() {
    let d = Settings::default();
    let c = |mods, vk: u8| Combo { mods, vk: vk as u16 };
    assert_eq!(d.combo_owner("next", c(WIN, b'F')), Some(Owner::List("finder")));
    assert_eq!(d.combo_owner("finder", c(WIN, b'F')), None); // its own
    assert_eq!(d.combo_owner("finder", c(WIN, b'3')), Some(Owner::Set("jump")));
    assert_eq!(d.combo_owner("finder", c(WIN, b'0')), None); // no workspace 0
    assert_eq!(d.combo_owner("finder", c(WIN | ALT, b'3')), None);
    assert_eq!(d.set_owner("send_to_stay", WIN), Some("jump"));
    assert_eq!(d.set_owner("jump", WIN), None);
    assert_eq!(d.set_owner("jump", WIN | ALT), None);
    assert!(d.skips("jump").is_empty());
}

// Spec 0023 AC-1, AC-3: a screen's own name beats every screen's, "" there hides the name on
// that screen only, and a workspace past the count shows no name until the count grows back.
// Every surface asks ws_name, so this order is the one you see everywhere.
#[test]
fn a_screen_name_beats_every_screens_name() {
    let file = "workspaces = 3\n[names]\nws2 = \"Code\"\nws3 = \"\"\nws4 = \"Later\"\n\
                [names.screens.internal]\nws2 = \"Notes\"\nws1 = \"\"\n\
                [names.screens.\"DEL-1\"]\nws2 = \"\"\n";
    let (s, warn) = parse(file);
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.ws_name("internal", 1), Some("Notes"));
    assert_eq!(s.ws_name("SAM-9", 1), Some("Code"));
    assert_eq!(s.ws_name("DEL-1", 1), None);
    assert_eq!((s.ws_name("SAM-9", 0), s.ws_name("internal", 0)), (None, None));
    assert_eq!(s.ws_name("SAM-9", 2), None, "\"\" in [names] is no name");
    assert_eq!(s.ws_name("SAM-9", 3), None, "past the count");
    let (more, _) = parse(&file.replace("workspaces = 3", "workspaces = 4"));
    assert_eq!(more.ws_name("SAM-9", 3), Some("Later"));
}

// Spec 0023 AC-11: a rename names its screen; "" or no key leaves Windows' name.
#[test]
fn an_empty_rename_is_no_rename() {
    let (s, warn) = parse("[names.screens.internal]\nscreen = \"Left\"\n[names.screens.\"DEL-1\"]\nscreen = \"  \"\nws1 = \"Mail\"\n");
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!((s.screen_rename("internal"), s.screen_rename("DEL-1"), s.screen_rename("SAM-9")), (Some("Left"), None, None));
}

// Spec 0026 AC-1, AC-10: a screen's own picture beats every screen's, "" there shows your
// Windows wallpaper on that screen only, and a workspace past the count shows nothing until
// the count grows back. The backdrop, the overview and the page all ask wallpaper_for, so
// this order is what you see everywhere.
#[test]
fn a_screen_picture_beats_every_screens_picture() {
    let file = "workspaces = 3\n[wallpaper]\nws2 = \"forest.jpg\"\nws3 = \"\"\nws4 = \"later.png\"\n\
                [wallpaper.screens.internal]\nws2 = \"desk.png\"\nws1 = \"\"\nws3 = \"tall.jpg\"\n\
                [wallpaper.screens.\"DEL-1\"]\nws2 = \"\"\n";
    let (s, warn) = parse(file);
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(s.wall_name("internal", 1), Some("desk.png"));
    assert_eq!(s.wall_name("SAM-9", 1), Some("forest.jpg"));
    assert_eq!(s.wall_name("DEL-1", 1), None, "\"\" on a screen is your Windows wallpaper there");
    assert_eq!((s.wall_name("SAM-9", 0), s.wall_name("internal", 0)), (None, None));
    assert_eq!((s.wall_name("SAM-9", 2), s.wall_name("internal", 2)), (None, Some("tall.jpg")), "\"\" in [wallpaper] is no picture");
    assert_eq!(s.wall_name("SAM-9", 3), None, "past the count");
    let (more, _) = parse(&file.replace("workspaces = 3", "workspaces = 4"));
    assert_eq!(more.wall_name("SAM-9", 3), Some("later.png"));
}

// Spec 0026 AC-4: fastest means every des-k fade is instant, the picture fade included,
// whatever transition says.
#[test]
fn fastest_keeps_pictures_instant() {
    assert!(parse("[wallpaper]\ntransition = \"fade\"\n").0.wallpaper_fades());
    assert!(!parse("fastest = true\n[wallpaper]\ntransition = \"fade\"\n").0.wallpaper_fades());
}

// Spec 0020 AC-9: a screen's table changes only the keys it sets; the rest, and every screen
// without a table (or one not plugged in), follow [indicator].
#[test]
fn a_screen_table_overrides_only_its_own_keys() {
    let text = "[indicator]
spot = \"top-right\"
gap = 10
[indicator.screens.\"DEL-A1B2-1\"]
spot = \"custom\"
x = 80.5
[indicator.screens.internal]
gap = 0
";
    let (s, warn) = parse(text);
    assert!(warn.is_empty(), "{warn:?}");
    let d = Pos { spot: Spot::TopRight, gap: 10, x: 50.0, y: 50.0 };
    assert_eq!(s.indicator_pos, d);
    assert_eq!(s.indicator_pos_for("DEL-A1B2-1"), Pos { spot: Spot::Custom, x: 80.5, ..d });
    assert_eq!(s.indicator_pos_for("internal"), Pos { gap: 0, ..d });
    assert_eq!(s.indicator_pos_for("LG-unplugged"), d);
}

// Spec 0040 AC-1, AC-2: a laptop with two workspaces next to a big screen with six. A good line
// gives its screen its own count, a line for a screen des-k never saw is kept, and each bad line
// is dropped alone with a line naming its key, so that screen just follows `workspaces`.
#[test]
fn each_screen_can_have_its_own_count() {
    let (s, warn) = parse("workspaces = 6\n[workspaces_per_screen]\n\"internal\" = 2\n\"GSM-OLD-1\" = 3\n\"#1\" = 12\n\"#2\" = \"two\"\n\"\" = 3\n");
    assert_eq!((s.count_for("internal"), s.count_for("GSM-OLD-1"), s.count_for("DEL-A0F5-1234")), (2, 3, 6));
    assert_eq!((s.count_for("#1"), s.count_for("#2")), (6, 6), "bad lines follow workspaces");
    assert_eq!(s.count_by_screen.len(), 2, "only the good lines are kept");
    assert_eq!(warn, [
        "workspaces_per_screen.\"\": an empty screen id, ignored",
        "workspaces_per_screen.\"#1\": 12 is outside 2 to 9, ignored",
        "workspaces_per_screen.\"#2\": expected a whole number, got a string, ignored",
    ]);
    // Nothing else resets (spec 0001).
    assert_eq!(s.shared_count, 6);
}

// AC-2: a value that is not a table is ignored with one line, and every screen has workspaces.
#[test]
fn a_per_screen_value_that_is_not_a_table_is_ignored() {
    let (s, warn) = parse("workspaces = 5\nworkspaces_per_screen = 2\n");
    assert!(s.count_by_screen.is_empty());
    assert_eq!(s.count_for("internal"), 5);
    assert_eq!(warn.len(), 1, "{warn:?}");
    assert!(warn[0].starts_with("workspaces_per_screen: expected a table"));
}

// AC-1, AC-11: a line equal to workspaces is the same as following it, and the cards that list
// workspaces for every screen go up to the highest count any screen has, never past it.
#[test]
fn most_is_the_highest_count_and_an_equal_line_follows() {
    let (s, _) = parse("workspaces = 4\n[workspaces_per_screen]\n\"internal\" = 4\n");
    assert_eq!((s.count_for("internal"), s.most()), (4, 4));
    let (s, _) = parse("workspaces = 3\n[workspaces_per_screen]\n\"internal\" = 2\n\"DEL-1\" = 7\n");
    assert_eq!(s.most(), 7);
    let (s, _) = parse("workspaces = 5\n[workspaces_per_screen]\n\"internal\" = 2\n");
    assert_eq!(s.most(), 5, "a lower line never shortens the lists");
    assert_eq!(s.counts_said(), "5 workspaces each; internal 2");
    assert_eq!(Settings::default().counts_said(), "4 workspaces each");
}

// AC-5: a workspace past a screen's own count has no name and no picture there, while a screen
// with more still shows them.
#[test]
fn names_and_pictures_stop_at_each_screens_count() {
    let (s, _) = parse("workspaces = 6\n[workspaces_per_screen]\n\"internal\" = 2\n[names]\nws3 = \"Mail\"\n[wallpaper]\nws3 = \"forest.jpg\"\n");
    assert_eq!((s.ws_name("internal", 2), s.ws_name("DEL-1", 2)), (None, Some("Mail")));
    assert_eq!((s.wall_name("internal", 2), s.wall_name("DEL-1", 2)), (None, Some("forest.jpg")));
}
