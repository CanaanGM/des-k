// Tests for the gesture menu's lines and clicks.
use super::*;
use crate::settings_page::input::keys::hit_at;
use crate::settings_page::rows::Section;
use crate::settings_page::tests::{SETTINGS, fake_win, measure, take};

// No twice rule and a twice rule that does nothing behave apart (one waits for a second
// go), so their labels must differ and say which one waits.
#[test]
fn twice_labels_tell_waiting_apart() {
    let (none, nothing) = (twice_label(None), twice_label(Some(Action::Nothing)));
    assert_ne!(none, nothing);
    assert!(nothing.contains("waits") && !none.contains("waits"));
}

// Every action a gesture can do is in the menu exactly once, so none is out of reach.
#[test]
fn the_menu_offers_every_action_once() {
    let s = Settings::default();
    let m = Menu { row: 0, times: 1, family: None, at: (0.0, 0.0, 0.0, 0.0), focus: 0 };
    let lines = menu_lines(&m, &s);
    for a in settings::actions() {
        let n = lines.iter().filter(|l| l.does == Does::Set(Some(a))).count();
        let in_family = FAMILIES.iter().any(|(_, f)| (0..9).any(|i| f(i) == a));
        assert_eq!(n, if in_family { 0 } else { 1 }, "{a:?}");
    }
}

// A click on a line of an open menu picks that line: the menu's own hit, which keeps a
// click between lines from reaching the page under it, must never swallow its lines. It
// did, for the Names chooser (spec 0023 AC-12) and the gesture menu alike.
#[test]
fn a_click_on_a_menu_line_lands_on_that_line() {
    let _one = SETTINGS.lock();
    settings::store::use_for_test("workspaces = 4
");
    let mut win = fake_win(ui::LIGHT, 1.0);
    let out = measure(&win);
    take(&mut win, out);
    let mid = |a: Bx| (a.0 + a.2 / 2.0, a.1 + a.3 / 2.0);
    let open = |win: &mut Win, ctl: Ctl, sec: Section| {
        let top = win.tops.iter().find(|t| t.0 == Some(sec)).unwrap().1;
        win.scroll = top;
        let out = measure(win);
        take(win, out);
        win.hits.iter().find(|h| h.ctl == ctl).map(|h| h.at).unwrap()
    };

    let at = open(&mut win, Ctl::NameScreens, Section::Workspaces);
    win.chooser = Some((Ctl::NameScreens, at, 0));
    let out = measure(&win);
    take(&mut win, out);
    let lines: Vec<(Ctl, Bx)> = win.hits.iter().filter(|h| matches!(h.ctl, Ctl::ScreenItem(i) if i != usize::MAX)).map(|h| (h.ctl, h.at)).collect();
    assert!(lines.len() >= 3, "All screens and the two fake screens");
    for (c, a) in lines {
        assert_eq!(hit_at(&win, mid(a)), Some(c));
    }
    win.chooser = None;

    let at = open(&mut win, Ctl::Picker(2), Section::Touchpad);
    win.menu = Some(Menu { row: 2, times: 1, family: None, at, focus: 0 });
    let out = measure(&win);
    take(&mut win, out);
    let lines: Vec<(Ctl, Bx)> = win.hits.iter().filter(|h| matches!(h.ctl, Ctl::Item(i) if i != usize::MAX)).map(|h| (h.ctl, h.at)).collect();
    assert!(!lines.is_empty());
    for (c, a) in lines {
        assert_eq!(hit_at(&win, mid(a)), Some(c));
    }
}
