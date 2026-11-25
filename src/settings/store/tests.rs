// Tests for the loaded copy and the change log lines.
use super::*;
use crate::settings::parse::{Problem, parse};

// Spec 0032 AC-1, AC-7: a save used to leak the copy it replaced, a few KB each time. Every
// replaced copy must be freed, except one a reader still holds, which must stay whole and
// readable until that reader lets go.
#[test]
fn a_save_frees_the_copy_it_replaced_once_no_reader_holds_it() {
    let cell = ArcSwap::from_pointee(Loaded { settings: Settings::default(), problems: Vec::new(), version: 0 });
    let held = Current(cell.load());
    let mut replaced = Vec::new();
    for i in 0..1000 {
        replaced.push(Arc::downgrade(&cell.load_full()));
        let (mut s, _) = parse("");
        s.twice_ms = 100 + i % 2;
        put(&cell, s, vec![Problem::new(format!("problem {i}"))]);
    }
    assert_eq!(cell.load().version, 1000);
    assert!(replaced[1..].iter().all(|w| w.upgrade().is_none()), "a replaced copy was never freed");
    assert!(replaced[0].upgrade().is_some(), "a copy was freed while a reader held it");
    assert_eq!((held.version(), held.twice_ms), (0, Settings::default().twice_ms));
    drop(held);
    assert!(replaced[0].upgrade().is_none(), "the held copy outlived its reader");
}

// Spec 0032 AC-2: before load(), every reader gets the defaults and no problems. Only the
// ignored render tests store into the real settings (use_for_test), never this run.
#[test]
fn get_before_load_gives_the_defaults() {
    assert_eq!(*get(), Settings::default());
    assert!(problems().is_empty());
    assert_eq!(get().version(), 0);
}

// The change log compares these one by one, so each must name its setting once.
#[test]
fn every_setting_is_logged_by_name() {
    let names: Vec<&str> = Settings::default().fields().into_iter().map(|(k, _)| k).collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!((names.len(), unique.len()), (71, 71));
}

// Spec 0027 AC-15: the change log names rules by key, workspace and screen id only.
#[test]
fn app_rules_log_by_key() {
    let (s, _) = parse("[apps]\n\"slack.exe\" = { workspace = 1, screen = \"GSM-1\" }\n\"spotify.exe\" = { workspace = 4 }\n");
    let line = s.fields().into_iter().find(|(k, _)| *k == "apps").map(|(_, v)| v);
    assert_eq!(line.as_deref(), Some("[slack.exe: 1 on GSM-1, spotify.exe: 4]"));
}

// Spec 0026 AC-16: the change line says which picture keys are set, never the file names, and a
// new picture for a key already set still counts as a change, or it would never show.
#[test]
fn wallpaper_change_lines_hold_keys_only() {
    let (old, _) = parse("[wallpaper]\nws1 = \"forest.jpg\"\n");
    let (new, _) = parse("[wallpaper]\nws1 = \"desk.png\"\nws3 = \"tall.jpg\"\n");
    assert_eq!(diff(&old, &new, Source::File), [Change { key: "wallpaper", was: "[ws1]".into(), now: "[ws1, ws3]".into() }]);
    let (swapped, _) = parse("[wallpaper]\nws1 = \"desk.png\"\n");
    assert_eq!(diff(&old, &swapped, Source::File), [Change { key: "wallpaper", was: "[ws1]".into(), now: "[ws1]".into() }]);
    let (screen, _) = parse("[wallpaper]\nws1 = \"forest.jpg\"\n[wallpaper.screens.internal]\nws2 = \"desk.png\"\nws3 = \"\"\n");
    assert_eq!(diff(&old, &screen, Source::File), [Change { key: "wallpaper", was: "[ws1]".into(), now: "[ws1; screens: internal (ws2, ws3)]".into() }]);
    let (moved, _) = parse("[wallpaper]\nws1 = \"forest.jpg\"\n[wallpaper.screens.internal]\nws2 = \"tall.jpg\"\nws3 = \"\"\n");
    assert_eq!(diff(&screen, &moved, Source::File).len(), 1, "a new picture on one screen is a change");
}

// Spec 0026 AC-16: fit, transition and dim say nothing about you, so each logs as its own
// line with its value, apart from the keys only wallpaper line.
#[test]
fn picture_look_changes_log_their_values() {
    let (old, _) = parse("[wallpaper]\nws1 = \"forest.jpg\"\n");
    let (new, _) = parse("[wallpaper]\nws1 = \"forest.jpg\"\nfit = \"fit\"\ntransition = \"fade\"\ndim = 40\n");
    let lines: Vec<(&str, String, String)> = diff(&old, &new, Source::File).into_iter().map(|c| (c.key, c.was, c.now)).collect();
    assert_eq!(lines, [("wallpaper.fit", "fill".into(), "fit".into()), ("wallpaper.transition", "instant".into(), "fade".into()), ("wallpaper.dim", "0".into(), "40".into())]);
}

// Spec 0028 AC-12: interval, shuffle and a workspace's own interval log with their values; a
// workspace interval added or taken out says default.
#[test]
fn slideshow_settings_log_their_values() {
    let (old, _) = parse("[wallpaper]\n[wallpaper.intervals]\nws1 = 10\n");
    let (new, _) = parse("[wallpaper]\ninterval = 10\nshuffle = true\n[wallpaper.intervals]\nws3 = 5\n");
    let lines: Vec<(&str, String, String)> = diff(&old, &new, Source::File).into_iter().map(|c| (c.key, c.was, c.now)).collect();
    assert_eq!(lines, [
        ("wallpaper.interval", "30".into(), "10".into()),
        ("wallpaper.shuffle", "false".into(), "true".into()),
        ("wallpaper.intervals.ws1", "10".into(), "default".into()),
        ("wallpaper.intervals.ws3", "default".into(), "5".into()),
    ]);
}

// Spec 0023 AC-15: the change line says which name keys are set, never what they say, and a
// new name for a key already set still counts as a change, or it would never take effect.
#[test]
fn names_change_lines_hold_keys_only() {
    let (old, _) = parse("[names]\nws1 = \"Mail\"\n");
    let (new, _) = parse("[names]\nws1 = \"Post\"\nws2 = \"Code\"\n[names.screens.internal]\nscreen = \"Kitchen\"\nws2 = \"\"\n");
    let changes = diff(&old, &new, Source::File);
    assert_eq!(changes, [Change { key: "names", was: "[ws1]".into(), now: "[ws1, ws2; screens: internal (screen, ws2)]".into() }]);
    let (renamed, _) = parse("[names]\nws1 = \"Post\"\n");
    let changes = diff(&old, &renamed, Source::File);
    assert_eq!(changes.len(), 1);
    assert!(!format!("{changes:?}{new}").contains("Post") && !format!("{new}").contains("Kitchen"));
}
