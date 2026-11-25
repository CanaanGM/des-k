// Tests for what the Apps rows write and say.
use super::*;
use crate::settings_page::choosers::chooser_lines;
use crate::settings_page::tests::{fake_apps, fake_win, loaded};

// Spec 0027 AC-14: Add writes one inline table where the app's window is, a menu writes its
// rule's one key, Any screen takes the screen out, and remove takes the rule out, the last
// one leaving [apps] for the next. Each reads back as the rule the page showed.
#[test]
fn app_rules_write_one_thing_each() {
    let file = "[apps]\n# mine\n";
    let added = settings::edit::apply_for_test(file, &app_edit("slack.exe", AppChange::Add(2, Some("DEL-1".into()))));
    // It goes in right under [apps]; the comment stays where it was, after it.
    assert_eq!(added, "[apps]\n\"slack.exe\" = { workspace = 3, screen = \"DEL-1\" }\n# mine\n");
    let rule = |text: &str| settings::parse::parse_for_test(text).0.apps;
    let moved = settings::edit::apply_for_test(&added, &app_edit("slack.exe", AppChange::Ws(0)));
    assert_eq!(rule(&moved), vec![("slack.exe".to_string(), settings::Rule { workspace: 0, screen: Some("DEL-1".into()) })]);
    let other = settings::edit::apply_for_test(&moved, &app_edit("slack.exe", AppChange::Screen(Some("internal".into()))));
    assert!(other.contains("screen = \"internal\""), "{other}");
    let any = settings::edit::apply_for_test(&other, &app_edit("slack.exe", AppChange::Screen(None)));
    assert_eq!(rule(&any), vec![("slack.exe".to_string(), settings::Rule { workspace: 0, screen: None })]);
    let gone = settings::edit::apply_for_test(&any, &app_edit("slack.exe", AppChange::Remove));
    assert_eq!(gone, file);
    // Added with no screen when its window's screen isn't plugged in.
    let bare = settings::edit::apply_for_test(file, &app_edit("spotify.exe", AppChange::Add(0, None)));
    assert!(bare.contains("\"spotify.exe\" = { workspace = 1 }\n"), "{bare}");
}

// Spec 0027 AC-13: a row says why its rule waits or is skipped; Add lists only apps with no
// rule; a workspace past the count is never offered, and nothing is chosen in that menu.
#[test]
fn app_rows_say_what_waits() {
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.pics.view.waiting = vec![crate::ws::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
    let s = loaded("[apps]\n\"slack.exe\" = { workspace = 6 }\n\"olk.exe\" = { workspace = 1, screen = \"LG-2\" }\n\"code.exe\" = { workspace = 1, screen = \"GSM-9\" }\n\"msedge.exe\" = { workspace = 2, screen = \"internal\" }\n");
    let notes: Vec<Option<(String, bool)>> = s.apps.iter().map(|(_, r)| app_note(&w.pics.view, s, r)).collect();
    assert_eq!(notes, vec![
        Some(("Waits: you have 4 workspaces".to_string(), false)),
        Some(("Screen not plugged in, windows wait on the main display".to_string(), false)),
        Some(("des-k hasn't seen this screen yet".to_string(), true)),
        None,
    ]);
    fake_apps(&mut w, false);
    let add: Vec<String> = chooser_lines(&w, s, Ctl::AppAdd).0.into_iter().map(|c| c.name).collect();
    assert_eq!(add, ["Calculator", "Terminal"]);
    let (lines, cur) = chooser_lines(&w, s, Ctl::AppWs(0));
    assert_eq!((lines.len(), cur), (4, None));
    let screens: Vec<(String, String)> = chooser_lines(&w, s, Ctl::AppScreen(2)).0.into_iter().map(|c| (c.name, c.side)).collect();
    let want = [("Any screen", "Where it opens"), ("Built-in display", ""), ("DELL U2720Q", ""), ("LG 27UL850", "Not plugged in"), ("GSM-9", "Not seen yet")];
    assert_eq!(screens, want.map(|(a, b)| (a.to_string(), b.to_string())));
}
