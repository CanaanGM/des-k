// Tests for the rows and their words: steps, the file round trip, greyed rows, problem titles.
use super::*;
use crate::settings_page::cards::changed;

// A hand written value off the grid is kept as is, and the next step lands on the grid,
// not one step away from the odd value.
#[test]
fn steps_land_on_the_grid() {
    assert_eq!(stepped(555, 0, 5000, 50, false), 550);
    assert_eq!(stepped(555, 0, 5000, 50, true), 600);
    assert_eq!(stepped(550, 0, 5000, 50, true), 600);
    assert_eq!(stepped(550, 0, 5000, 50, false), 500);
}

// A stepper never writes a value the parser would refuse and reset.
#[test]
fn steps_stay_in_range() {
    assert_eq!(stepped(0, 0, 5000, 50, false), 0);
    assert_eq!(stepped(5000, 0, 5000, 50, true), 5000);
    assert_eq!(stepped(5, 5, 50, 1, false), 5);
    assert_eq!(stepped(1000, 0, 1000, 50, true), 1000); // the overview's fade time
}

// Every row writes a value the parser reads back as the same setting: a typo in a key or
// a word here would save something des-k ignores. Covers the new fastest, style and
// overview fade time rows.
#[test]
fn every_row_round_trips_through_the_file() {
    for row in ROWS {
        let d = value(&DEFAULTS, row.key);
        let v = match row.kind {
            Kind::Toggle | Kind::Pair(..) => 1 - d,
            Kind::Step { lo, hi, .. } => if d == hi { lo } else { hi },
            Kind::Choice(words) => (d + 1) % words.len() as i64,
            Kind::Ladder(ladder) => if d == ladder[0] { ladder[1] } else { ladder[0] },
        };
        let text = settings::edit::edit_for_test(row.key, file_value(row, v));
        let (s, warn) = settings::parse::parse_for_test(&text);
        assert!(warn.is_empty(), "{}: {warn:?}", row.key);
        assert_eq!(value(&s, row.key), v, "{}", row.key);
    }
}

// AC-SET-8: the overview's fade time is greyed while the animation is None or Skip
// animations is on, naming each reason; the indicator's fade only with Skip animations
// (O-1). A greyed value is kept, so its stepper must not change it.
#[test]
fn greyed_rows_name_every_reason() {
    let mut s = Settings::default();
    assert_eq!(unused(&s, "overview.fade_ms"), vec![Why::AnimationNone]);
    assert!(unused(&s, "indicator.fade_ms").is_empty());
    s.fastest = true;
    assert_eq!(unused(&s, "overview.fade_ms"), vec![Why::AnimationNone, Why::Skip]);
    assert_eq!(unused(&s, "indicator.fade_ms"), vec![Why::Skip]);
    s.fastest = false;
    s.overview_animation = settings::Animation::Fade;
    assert!(unused(&s, "overview.fade_ms").is_empty());
    s.indicator_enabled = false;
    assert_eq!(unused(&s, "indicator.style"), vec![Why::IndicatorOff]);
    // Spec 0023 AC-14: Show screen name greys with the indicator, without words, and in
    // the Dots style, which shows no names; greyed, it keeps its value.
    assert_eq!(unused(&s, "indicator.screen_name"), vec![Why::IndicatorOff]);
    s.indicator_enabled = true;
    assert!(unused(&s, "indicator.screen_name").is_empty());
    s.indicator_style = settings::Style::Dots;
    assert_eq!(unused(&s, "indicator.screen_name"), vec![Why::Dots]);
    assert_eq!(value(&s, "indicator.screen_name"), 1);
}

// Spec 0026 AC-18: Dim reads "Off" at 0 and "40%" otherwise, on its row and in a hand
// edit's line; a hand typed value off the 5 grid steps onto it.
#[test]
fn dim_reads_off_or_a_percent() {
    let r = find_row("wallpaper.dim").unwrap();
    assert_eq!((shown(r, 0), shown(r, 40)), ("Off".to_string(), "40%".to_string()));
    assert_eq!(changed("wallpaper.dim", "0", "40"), "Dim pictures: Off → 40%");
    assert_eq!(changed("wallpaper.dim", "30", "40"), "Dim pictures: 30 → 40%");
    assert_eq!(changed("wallpaper.fit", "fill", "center"), "Fit: Fill → Center");
    assert_eq!((stepped(42, 0, 80, 5, true), stepped(42, 0, 80, 5, false)), (45, 40));
    let mut s = Settings::default();
    assert!(unused(&s, "wallpaper.transition").is_empty());
    s.fastest = true;
    assert_eq!(unused(&s, "wallpaper.transition"), vec![Why::Skip]);
}

// AC-SET-5: each kind of log line gets a plain title, and the bad values the design draws
// read as it does, so a problem never shows only des-k's own log words.
#[test]
fn problems_get_plain_titles() {
    let s = Settings::default();
    let p = |m: &str| {
        let key = m.split_once(": ").map_or("", |(k, _)| k);
        Problem { key: if key.contains(' ') { String::new() } else { key.to_string() }, message: m.to_string() }
    };
    assert_eq!(title(&p("unknown key workspace, ignored"), &s), "“workspace” isn't a setting, so des-k skips it");
    assert_eq!(title(&p("indicator.hold_ms: expected a whole number, got a string, using default 550"), &s), "des-k can't use this value, so it uses 550 ms");
    assert_eq!(title(&p("overview.fade_ms: 1500 is not between 0 and 1000, using default 250"), &s), "des-k can't use this value, so it uses 250 ms");
    assert_eq!(title(&p("fastest: expected true or false, got a string, using default false"), &s), "des-k can't use this value, so Skip animations is off");
    assert_eq!(title(&p("shortcuts: Win+F is in both finder and overview, overview turned off"), &s), "Win F was set for both Find a window and Open overview, so Open overview has no key now");
    assert_eq!(title(&p("gesture rule 3: fingers = 5, not 3 or 4, skipped"), &s), "Gesture rule 3 can't be used, so des-k skips it");
    assert!(!title(&p("something new des-k says"), &s).is_empty());
}

// A problem is shown under its own setting, or in its section, or in Notes; never lost.
// fastest's is under Motion, the Copilot key's under Shortcuts.
#[test]
fn problems_have_a_home() {
    assert_eq!(home("indicator.hold_ms"), Some(Section::Indicator));
    assert_eq!(home("shortcuts"), Some(Section::Shortcuts));
    assert_eq!(home("copilot_as_right_ctrl"), Some(Section::Shortcuts));
    assert_eq!(home("fastest"), Some(Section::Motion));
    assert_eq!(home("gesture"), Some(Section::Touchpad));
    assert_eq!(home(""), None); // unknown keys and gesture rule N
}

// AC-SET-4: Windows' own key is drawn first, but × and a recording act on the file's own
// index, so the drawn order never changes what is written.
#[test]
fn windows_key_first_maps_back_to_the_file() {
    let mut s = Settings::default();
    let win_f = Combo { mods: WIN, vk: 0x46 };
    let mine = Combo { mods: WIN | ALT, vk: 0x46 };
    s.finder = vec![mine, win_f];
    let sh = shortcut("shortcuts.finder").unwrap();
    assert_eq!(drawn_order(&s, sh), vec![1, 0]);
    assert!(has_win(&s, sh));
    s.finder = vec![mine];
    assert!(!has_win(&s, sh));
    assert_eq!(drawn_order(&s, sh), vec![0]);
}

// The Windows key of an action another action holds reads as taken by that one.
#[test]
fn a_taken_windows_key_names_its_holder() {
    let s = Settings { overview: vec![], finder: vec![Combo { mods: WIN, vk: 0x46 }, Combo { mods: WIN, vk: 0x09 }], ..Default::default() };
    let sh = shortcut("shortcuts.overview").unwrap();
    assert_eq!(took(&s, sh), Some("Find a window"));
}
