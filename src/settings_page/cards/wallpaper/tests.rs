// Tests for what the Wallpaper card writes, lists and previews.
use super::*;
use crate::settings_page::choosers::chooser_lines;
use crate::settings_page::rows::{LADDER, ladder_step, minutes};
use crate::settings_page::tests::{fake_win, loaded, pic, said};

// Spec 0026 AC-15: a Wallpaper menu writes only what differs from what the row follows, so
// the file holds no key that changes nothing. On All screens, Windows wallpaper is no key at
// all. On a screen, choosing All screens' picture takes the screen's key out (and its table
// with its last key); Windows wallpaper there is "" only while All screens has a picture.
#[test]
fn a_wallpaper_choice_writes_only_what_differs() {
    let s = loaded("[wallpaper]\nws1 = \"forest.jpg\"\n[wallpaper.screens.internal]\nws2 = \"city.jpg\"\nws3 = \"\"\n");
    let e = |screen: Option<&str>, k: usize, pick: &str| said(wall_edit(s, screen, k, &pic(pick)));
    assert_eq!(e(None, 0, "forest.jpg"), "none");
    assert_eq!(e(None, 0, "desk.png"), "at wallpaper.ws1 = \"desk.png\"");
    assert_eq!(e(None, 0, ""), "remove wallpaper.ws1");
    assert_eq!(e(None, 3, ""), "none"); // nothing there to take out
    assert_eq!(e(Some("internal"), 0, "forest.jpg"), "none"); // it follows that already
    assert_eq!(e(Some("internal"), 0, ""), "at wallpaper.screens.internal.ws1 = \"\"");
    assert_eq!(e(Some("internal"), 0, "desk.png"), "at wallpaper.screens.internal.ws1 = \"desk.png\"");
    assert_eq!(e(Some("internal"), 1, "city.jpg"), "none");
    assert_eq!(e(Some("internal"), 1, ""), "remove wallpaper.screens.internal.ws2"); // All screens has none for it
    assert_eq!(e(Some("internal"), 2, ""), "remove wallpaper.screens.internal.ws3");
    assert_eq!(e(Some("internal"), 3, ""), "none");
    let own = "[wallpaper.screens.internal]\nws2 = \"city.jpg\"\n";
    let gone = settings::edit::apply_for_test(own, &wall_edit(loaded(own), Some("internal"), 1, &Pick::None).unwrap());
    assert!(!gone.contains("screens"), "{gone}");
    let (read, warn) = settings::parse::parse_for_test(&settings::edit::apply_for_test("", &wall_edit(loaded(""), Some("DEL#1.A"), 2, &pic("a b.png")).unwrap()));
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(read.wall_name("DEL#1.A", 2), Some("a b.png"));
}

// Spec 0026 AC-13: each row says how it reads, from the file's keys alone, and its menu
// checks the picture the screen shows; a picture the folder no longer has checks nothing.
#[test]
fn a_wallpaper_row_reads_its_keys() {
    let s = loaded("[wallpaper]\nws1 = \"forest.jpg\"\nws2 = \"gone.jpg\"\n[wallpaper.screens.internal]\nws2 = \"city.jpg\"\nws3 = \"\"\n");
    assert_eq!(wall_held(s, None, 0), (WallState::All, pic("forest.jpg")));
    assert_eq!(wall_held(s, Some("internal"), 0), (WallState::Follow, pic("forest.jpg")));
    assert_eq!(wall_held(s, Some("internal"), 1), (WallState::Own, pic("city.jpg")));
    assert_eq!(wall_held(s, Some("internal"), 2), (WallState::Windows, Pick::None));
    assert_eq!(wall_held(s, Some("internal"), 3), (WallState::Follow, Pick::None));
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.wall_files = vec!["city.jpg".into(), "desk.png".into(), "forest.jpg".into()];
    let (lines, cur) = chooser_lines(&w, s, Ctl::WallPick(0));
    let names: Vec<String> = lines.into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Windows wallpaper", "city.jpg", "desk.png", "forest.jpg", "Open folder"]);
    assert_eq!(cur, Some(3));
    assert_eq!(chooser_lines(&w, s, Ctl::WallPick(1)).1, None, "gone.jpg isn't in the folder");
    assert_eq!(chooser_lines(&w, s, Ctl::WallPick(3)).1, Some(0));
    w.wall_screen = Some("internal".into());
    assert_eq!(chooser_lines(&w, s, Ctl::WallPick(1)).1, Some(1), "the screen's own city.jpg");
}

// Spec 0028 AC-11: a tick adds a picture to the row's pictures or takes it out, and writes
// what the reader takes back as that same value: one picture a plain name, two or more a
// list by name, none your Windows wallpaper. On a screen, a row that follows all screens
// starts from their pictures, and a set equal to all screens' takes the screen's key out.
#[test]
fn a_tick_writes_the_rows_pictures_by_name() {
    let text = "[wallpaper]\nws1 = \"forest.jpg\"\nws2 = [\"city.jpg\", \"desk.png\"]\nws4 = \"beach\"\n[wallpaper.screens.internal]\nws3 = \"dunes.jpg\"\n";
    let s = loaded(text);
    let e = |screen: Option<&str>, k: usize, f: &str| said(wall_tick(s, screen, k, f));
    assert_eq!(e(None, 0, "desk.png"), "at wallpaper.ws1 = [\"desk.png\", \"forest.jpg\"]");
    assert_eq!(e(None, 0, "forest.jpg"), "remove wallpaper.ws1", "its last picture: Windows wallpaper");
    assert_eq!(e(None, 1, "city.jpg"), "at wallpaper.ws2 = \"desk.png\"", "one left is a plain name");
    assert_eq!(e(None, 1, "Alps.jpg"), "at wallpaper.ws2 = [\"Alps.jpg\", \"city.jpg\", \"desk.png\"]", "sorted with case ignored");
    assert_eq!(e(None, 3, "a.jpg"), "at wallpaper.ws4 = \"a.jpg\"", "a tick replaces a folder");
    assert_eq!(e(Some("internal"), 1, "dunes.jpg"), "at wallpaper.screens.internal.ws2 = [\"city.jpg\", \"desk.png\", \"dunes.jpg\"]");
    assert_eq!(e(Some("internal"), 2, "dunes.jpg"), "remove wallpaper.screens.internal.ws3", "none, as all screens has");
    let own = loaded("[wallpaper]\nws2 = [\"city.jpg\", \"desk.png\"]\n[wallpaper.screens.internal]\nws2 = [\"city.jpg\", \"desk.png\", \"x.jpg\"]\n");
    assert_eq!(said(wall_tick(own, Some("internal"), 1, "x.jpg")), "remove wallpaper.screens.internal.ws2", "the same set as all screens");
    // What it writes reads back as the set it meant.
    let (read, warn) = settings::parse::parse_for_test(&settings::edit::apply_for_test(text, &wall_tick(s, None, 0, "desk.png").unwrap()));
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(read.wallpaper[0], Pick::Set(vec!["desk.png".into(), "forest.jpg".into()]));
}

// Spec 0026 AC-13: with one known screen the card hides its chooser and writes All screens,
// but a screen with its own pictures keeps the chooser, or those pictures could be neither
// changed nor cleared and every change would go where that screen never looks (tried live
// 2026-11-21 on a laptop alone: nothing changed, whatever was picked).
#[test]
fn a_lone_screen_with_its_own_pictures_keeps_the_chooser() {
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.pics.view.present.truncate(1);
    w.pics.view.waiting.clear();
    w.wall_screen = Some("internal".into());
    let plain = loaded("[wallpaper]\nws2 = \"a.jpg\"\n");
    assert!(!wall_two(&w, plain));
    assert_eq!(wall_screen(&w, plain), None, "no chooser: the rows write All screens");
    let own = loaded("[wallpaper]\nws2 = \"a.jpg\"\n[wallpaper.screens.internal]\nws2 = \"b.jpg\"\n");
    assert!(wall_two(&w, own));
    assert_eq!(wall_screen(&w, own).as_deref(), Some("internal"));
    w.wall_screen = None;
    assert_eq!(wall_screen(&w, own), None, "All screens until you pick the screen");
}

// Spec 0026 AC-13 (grown 2026-11-21): Use this there too takes out the plugged in screens'
// own value for that row alone, so they follow All screens again; a screen not plugged in
// keeps its own, and other rows keep theirs, since you asked for one row on what you see.
#[test]
fn use_this_there_too_clears_only_plugged_in_screens_for_the_row() {
    let text = "[wallpaper]\nws2 = \"a.jpg\"\n[wallpaper.screens.internal]\nws1 = \"x.jpg\"\nws2 = \"b.jpg\"\n[wallpaper.screens.\"DEL-1\"]\nws2 = \"c.jpg\"\n[wallpaper.screens.\"LG-2\"]\nws2 = \"d.jpg\"\n";
    let present = ["internal".to_string(), "DEL-1".to_string()];
    let edit = wall_use_edit(loaded(text), &present, 1).expect("two screens to clear");
    let (read, warn) = settings::parse::parse_for_test(&settings::edit::apply_for_test(text, &edit));
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!(read.wallpaper_screens.get("internal").map(|o| (o[0].clone(), o[1].clone())), Some((Some(pic("x.jpg")), None)), "row 1 kept");
    assert!(!read.wallpaper_screens.contains_key("DEL-1"), "its last key gone, with its table");
    assert_eq!(read.wallpaper_screens.get("LG-2").map(|o| o[1].clone()), Some(Some(pic("d.jpg"))), "not plugged in");
    assert!(wall_use_edit(loaded(text), &present, 2).is_none(), "nothing to clear");
}

// Spec 0026 AC-15 (amended 2026-11-22): Windows wallpaper on All screens is a reset, so
// every screen shows your Windows wallpaper on that workspace: each screen's own value for
// the row goes, plugged in or not (one coming back must not bring a picture back), even
// when All screens had no picture. Other rows keep theirs, and a picture on All screens
// still leaves a screen's own alone (Use this there too is for that).
#[test]
fn windows_wallpaper_on_all_screens_resets_the_row_everywhere() {
    let text = "[wallpaper]\nws2 = \"a.jpg\"\n[wallpaper.screens.internal]\nws1 = \"x.jpg\"\nws2 = \"b.jpg\"\nws3 = \"\"\n[wallpaper.screens.\"LG-2\"]\nws2 = \"d.jpg\"\nws3 = \"e.jpg\"\n";
    let reset = |k: usize| {
        let edit = wall_edit(loaded(text), None, k, &Pick::None).expect("something to take out");
        let (read, warn) = settings::parse::parse_for_test(&settings::edit::apply_for_test(text, &edit));
        assert!(warn.is_empty(), "{warn:?}");
        read
    };
    let two = reset(1);
    assert_eq!(two.wallpaper[1], Pick::None);
    assert_eq!(two.wallpaper_screens.get("internal").map(|o| (o[0].clone(), o[1].clone())), Some((Some(pic("x.jpg")), None)), "row 1 kept");
    assert_eq!(two.wallpaper_screens.get("LG-2").map(|o| o[1].clone()), Some(None), "not plugged in, still reset");
    let three = reset(2);
    assert_eq!(three.wallpaper_screens.get("internal").map(|o| o[2].clone()), Some(None), "All screens had none: still reset");
    assert_eq!(three.wallpaper_screens.get("LG-2").map(|o| o[2].clone()), Some(None));
    assert_eq!(said(wall_edit(loaded(text), None, 3, &Pick::None)), "none", "nothing anywhere");
    assert_eq!(said(wall_edit(loaded(text), None, 1, &pic("c.jpg"))), "at wallpaper.ws2 = \"c.jpg\"", "a picture keeps screens' own");
}

// Spec 0028 AC-11: an All screens slideshow previews the picture its screen shows now, but
// only if that screen runs it: a screen with its own value must not lend its picture. With
// none running, a set previews its first picture rather than an empty tile (tried live
// 2026-11-21: the only screen had its own picture and the row looked broken); a folder
// keeps its tile, since naming a picture would mean reading the folder.
#[test]
fn a_slideshow_previews_only_what_its_screen_runs() {
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.pics.wall_now.insert(("internal".into(), 0), "x.jpg".into());
    let own = loaded("[wallpaper]\nws1 = [\"a.jpg\", \"b.jpg\"]\nws2 = \"beach\"\n[wallpaper.screens.internal]\nws1 = [\"x.jpg\", \"y.jpg\"]\nws2 = \"trips\"\n");
    assert_eq!(wall_shows(&w, own, None, 0).as_deref(), Some("a.jpg"), "not the screen's own slideshow");
    assert_eq!(wall_shows(&w, own, None, 1), None, "a folder keeps its tile");
    assert_eq!(wall_shows(&w, own, Some("internal"), 0).as_deref(), Some("x.jpg"), "the screen's own runs");
    w.pics.wall_now.insert(("internal".into(), 0), "b.jpg".into());
    let follows = loaded("[wallpaper]\nws1 = [\"a.jpg\", \"b.jpg\"]\n");
    assert_eq!(wall_shows(&w, follows, None, 0).as_deref(), Some("b.jpg"), "the one showing now");
}

// Spec 0028 AC-11: a menu lists the folder's pictures, then its slideshow folders, and checks
// the row's folder; a set checks no line, since its pictures show ticks. The list scrolls
// as one, the Slideshows heading counted, so a folder the keys move to is in view.
#[test]
fn a_wallpaper_menu_lists_folders_after_pictures() {
    let s = loaded("[wallpaper]\nws1 = \"beach\"\nws2 = [\"city.jpg\", \"desk.png\"]\n");
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.wall_files = vec!["city.jpg".into(), "desk.png".into()];
    w.wall_dirs = vec![("beach".into(), 12), ("trips".into(), 0)];
    let (lines, cur) = chooser_lines(&w, s, Ctl::WallPick(0));
    let names: Vec<String> = lines.into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["Windows wallpaper", "city.jpg", "desk.png", "beach", "trips", "Open folder"]);
    assert_eq!(cur, Some(3), "the beach folder");
    assert_eq!(chooser_lines(&w, s, Ctl::WallPick(1)).1, None, "a set");
    assert_eq!(wall_total(2, 2), 5);
    assert_eq!(wall_total(2, 0), 2, "no heading without folders");
    assert_eq!((wall_slot(2, 2, 2), wall_slot(3, 2, 2), wall_slot(5, 2, 2)), (2, 4, 6));
    assert_eq!(wall_reveal(0, wall_slot(4, 2, 2), wall_total(2, 2), 3), 2, "trips is the last line in view");
    assert_eq!(wall_reveal(2, wall_slot(5, 2, 2), wall_total(2, 2), 3), 2, "Open folder never moves the list");
}

// Spec 0028 AC-11: a workspace's time per picture moves on the ladder; a hand typed value
// off it steps onto it, the ends stay put, and the words read as the design shows them.
#[test]
fn time_per_picture_steps_on_its_ladder() {
    assert_eq!((ladder_step(LADDER, 30, true), ladder_step(LADDER, 30, false)), (45, 20));
    assert_eq!((ladder_step(LADDER, 25, true), ladder_step(LADDER, 25, false)), (30, 20));
    assert_eq!((ladder_step(LADDER, 1, false), ladder_step(LADDER, 1440, true)), (1, 1440));
    assert_eq!([minutes(45), minutes(60), minutes(90), minutes(1440)], ["45 min", "1 h", "1 h 30 min", "24 h"]);
    assert_eq!(changed("wallpaper.interval", "30", "60"), "Interval: 30 min → 1 h");
    assert_eq!(changed("wallpaper.intervals.ws3", "default", "10"), "Workspace 3 time per picture: default → 10 min");
}

// Spec 0028 AC-11: a workspace's own time is written only while it differs from Interval;
// stepping back to Interval's takes the key out with its table, so the file never keeps an
// empty [wallpaper.intervals] or a key that changes nothing.
#[test]
fn a_workspace_time_back_at_the_default_leaves_no_key() {
    let text = "[wallpaper]\ninterval = 30\nws3 = \"beach\"\n";
    assert_eq!(said(wall_interval_edit(loaded(text), 2, true)), "at wallpaper.intervals.ws3 = 45");
    let own = settings::edit::apply_for_test(text, &wall_interval_edit(loaded(text), 2, true).unwrap());
    assert_eq!(loaded(&own).interval_for(2), 45);
    assert_eq!(said(wall_interval_edit(loaded(&own), 2, false)), "remove wallpaper.intervals.ws3");
    let back = settings::edit::apply_for_test(&own, &wall_interval_edit(loaded(&own), 2, false).unwrap());
    assert!(!back.contains("intervals"), "{back}");
    assert_eq!(said(wall_interval_edit(loaded("[wallpaper]\ninterval = 1440\n"), 0, true)), "none", "already 24 h");
}

// Spec 0026 AC-13: a long folder scrolls inside the menu, and the line the keys move to is
// always in view; Windows wallpaper and Open folder never move the list.
#[test]
fn a_long_wallpaper_menu_keeps_its_focus_in_view() {
    assert_eq!(wall_rows(H, 40), 10);
    assert_eq!(wall_rows(MIN_H, 40), 10);
    assert_eq!(wall_rows(300.0, 40), 3);
    assert_eq!(wall_rows(H, 2), 2);
    assert_eq!(wall_reveal(0, 12, 40, 10), 2); // picture 12 is the last line in view
    assert_eq!(wall_reveal(5, 3, 40, 10), 2);
    assert_eq!(wall_reveal(5, 0, 40, 10), 5);
    assert_eq!(wall_reveal(5, 41, 40, 10), 5);
    assert_eq!(wall_reveal(35, 40, 40, 10), 30);
    assert_eq!(wall_reveal(0, 2, 2, 2), 0);
}
