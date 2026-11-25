// Tests for what the Names card writes and lists.
use super::*;
use crate::settings_page::tests::{fake_win, loaded, said};

// Spec 0023 AC-12: a name box writes its own key and nothing else. An emptied All screens
// box takes out names.wsN; an emptied own row takes out the screen's key, so the row
// follows all screens again; Enter on an empty following row writes "" (no name there) only
// when all screens has a name to hide, and leaving it writes nothing. Text is cleaned as the
// file is read, so unchanged text, give or take spaces, writes nothing.
#[test]
fn a_name_box_writes_only_its_own_key() {
    let s = loaded("[names]\nws1 = \"Mail\"\nws2 = \"Code\"\n[names.screens.internal]\nscreen = \"Left\"\nws2 = \"Notes\"\nws3 = \"\"\n");
    let e = |screen: Option<&str>, row: Nm, text: &str, enter: bool| said(name_edit(s, screen, row, text, enter));
    assert_eq!(e(None, Nm::Ws(0), "", false), "remove names.ws1");
    assert_eq!(e(None, Nm::Ws(0), " Mail ", true), "none");
    assert_eq!(e(None, Nm::Ws(0), "Post", false), "at names.ws1 = \"Post\"");
    assert_eq!(e(None, Nm::Ws(2), "", true), "none"); // nothing there to take out
    assert_eq!(e(Some("internal"), Nm::Ws(1), "", false), "remove names.screens.internal.ws2");
    assert_eq!(e(Some("internal"), Nm::Ws(0), "", true), "at names.screens.internal.ws1 = \"\"");
    assert_eq!(e(Some("internal"), Nm::Ws(0), "", false), "none");
    assert_eq!(e(Some("internal"), Nm::Ws(3), "", true), "none"); // all screens has no name for it
    assert_eq!(e(Some("internal"), Nm::Ws(2), "", true), "none"); // no name stays no name
    assert_eq!(e(Some("internal"), Nm::Ws(2), "Mail", false), "at names.screens.internal.ws3 = \"Mail\"");
    assert_eq!(e(Some("internal"), Nm::Screen, "", false), "remove names.screens.internal.screen");
    assert_eq!(e(Some("DEL-1"), Nm::Screen, "Right", true), "at names.screens.DEL-1.screen = \"Right\"");
    assert_eq!(e(None, Nm::Screen, "Right", true), "none"); // All screens has no screen name
}

// Spec 0023 AC-12: a screen id holding # or . gets its own quoted table, never a table
// path made from its parts, and taking out its last key takes the table with it.
#[test]
fn a_screen_id_with_odd_letters_keeps_its_own_table() {
    let id = "DEL#1.A";
    let at = name_edit(loaded(""), Some(id), Nm::Ws(1), "Notes", false).unwrap();
    let text = settings::edit::apply_for_test("[names]\nws2 = \"Code\"\n", &at);
    let (read, warn) = settings::parse::parse_for_test(&text);
    assert!(warn.is_empty(), "{warn:?}");
    assert_eq!((read.ws_name(id, 1), read.ws_name("internal", 1)), (Some("Notes"), Some("Code")));
    let gone = settings::edit::apply_for_test(&text, &Edit::Remove(name_path(Some(id), Nm::Ws(1)).unwrap()));
    assert!(!gone.contains("screens"), "{gone}");
    assert!(gone.contains("ws2 = \"Code\""), "{gone}");
}

// Spec 0023 AC-12: the Names chooser lists the screens plugged in, then remembered ones, and
// tells two screens you gave the same name apart by Windows' own name.
#[test]
fn the_names_chooser_tells_same_names_apart() {
    let r = RECT::default();
    let p = |id: &str, name: &str, windows: &str| crate::ws::Present { id: id.into(), device: String::new(), name: name.into(), windows_name: windows.into(), full: r, work: r };
    let view = crate::ws::View {
        present: vec![p("DEL-1", "Left", "DELL U2720Q"), p("LG-2", "Left", "LG 27UL850"), p("internal", "Built-in display", "Built-in display")],
        waiting: vec![crate::ws::Waiting { id: "HP-3".into(), name: "HP E24".into(), windows_name: "HP E24".into(), windows: vec![] }],
        ..Default::default()
    };
    let shown: Vec<(String, String)> = name_choices(&view).into_iter().map(|c| (c.name, c.side)).collect();
    let want = [("All screens", ""), ("Left", "DELL U2720Q"), ("Left", "LG 27UL850"), ("Built-in display", ""), ("HP E24", "Not plugged in")];
    assert_eq!(shown, want.map(|(a, b)| (a.to_string(), b.to_string())));
}

// Spec 0023 AC-13: after a failed save the box keeps your text, whatever a reload brings,
// until a good save or Esc; the same workspace on a screen is another box.
#[test]
fn a_name_that_failed_to_save_outlives_a_reload() {
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.unsaved = vec![(None, Nm::Ws(1), "Codex".into())];
    let l = name_look(&w, loaded("[names]\nws2 = \"Code\"\n"), Nm::Ws(1));
    assert!(l.unsaved);
    assert_eq!((l.shown.as_str(), l.saved.as_str()), ("Codex", "Code"));
    let edited = loaded("[names]\nws2 = \"Mail\"\n");
    assert_eq!(name_look(&w, edited, Nm::Ws(1)).shown, "Codex");
    assert_eq!(name_look(&w, edited, Nm::Ws(0)).shown, "");
    w.names_screen = Some("internal".into());
    assert!(!name_look(&w, edited, Nm::Ws(1)).unsaved);
}

// Spec 0023 AC-12: with one known screen the card has no chooser: its Screen name row
// renames that screen, with Windows' name as the hint, and the workspace rows name every
// screen, whatever screen was chosen before.
#[test]
fn one_screen_renames_itself_and_names_all_screens() {
    let mut w = fake_win(ui::LIGHT, 1.0);
    w.pics.view.present.truncate(1);
    w.names_screen = Some("DEL-A0F5-1234".into());
    let s = loaded("");
    let screen = name_look(&w, s, Nm::Screen);
    assert_eq!((screen.screen.as_deref(), screen.hint.as_str()), (Some("internal"), "Built-in display"));
    assert_eq!(name_look(&w, s, Nm::Ws(0)).screen, None);
}
