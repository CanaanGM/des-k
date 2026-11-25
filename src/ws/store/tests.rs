// Tests for state.txt: lines read back as written, old files upgrade, and the rescue keeps what it
// could not show.
use super::*;
use crate::ws::tests::{rect, spot};

// --quit after a crash must never lose a window: one it could not show keeps its W line,
// so the next start or --quit still finds it. Shown windows and V lines go and every
// screen shows workspace 1, as after a quit.
#[test]
fn rescue_keeps_the_line_of_a_window_it_could_not_show() {
    let text = "L 1a2b 1700000000\nM DISPLAY1 2\n\
        W DISPLAY1 1 100 7\tapp\tclass\tshown\n\
        W DISPLAY1 2 200 8\tapp\tclass\tstuck\n\
        W DISPLAY1 1 300 9\tapp\tclass\tclosed\n\
        W DISPLAY1 1 0 0\tapp\tclass\tpending\n\
        V DISPLAY1\tapp\tclass\tvisible\n";
    let mut asked = Vec::new();
    let (out, shown, left) = rescued(text, |raw, pid| {
        asked.push((raw, pid));
        match raw {
            100 => Some(true),
            200 => Some(false),
            _ => None,
        }
    });
    assert_eq!((shown, left), (1, 1));
    assert_eq!(out, "L 1a2b 1700000000\nM DISPLAY1 0\nW DISPLAY1 2 200 8\tapp\tclass\tstuck\n");
    assert_eq!(asked, vec![(100, 7), (200, 8), (300, 9)], "a restore's hwnd 0 entry is no window to show");
}

// Upgrading while docked keeps the layout: each device name becomes the id of the screen it
// names. Lines of a screen not plugged in go once, except a hidden window's, which keeps
// its device so load shows it rather than lose it (AC-15).
#[test]
fn an_old_state_file_upgrades_to_screen_ids() {
    let text = "M DISPLAY1 2\nM DISPLAY9 1\nW DISPLAY1 1 100 7\ta\tc\tt\nW DISPLAY9 0 200 8\ta\tc\tu\nV DISPLAY9\ta\tc\tv\nP DISPLAY1 300 9\ta\tc\tp\n";
    let up = |dev: &str| (dev == "DISPLAY1").then(|| "DEL-1".to_string());
    let fp = |t: &str| Some(Fp { app: "a".into(), class: "c".into(), title: t.into() });
    assert_eq!(parse_state(text, up), vec![
        Line::M { id: "DEL-1".into(), active: 2, area: RECT::default(), name: String::new() },
        Line::W { id: "DEL-1".into(), ws: 1, raw: 100, pid: 7, spot: None, fp: fp("t") },
        Line::W { id: "DISPLAY9".into(), ws: 0, raw: 200, pid: 8, spot: None, fp: fp("u") },
        Line::P { id: "DEL-1".into(), raw: 300, pid: 9, spot: None, fp: fp("p") },
    ]);
}

// What save writes, load reads back: the away record above all, since it is what brings a
// window home after a crash or a power cut (AC-8).
#[test]
fn new_lines_read_back() {
    let away = Away { home: "DEL-4123-ABC".into(), ws: 2, spot: spot(-1920, 'x'), pinned: true, pid: 44 };
    let text = format!(
        "M DEL-4123-ABC 1 0 0 2560 1400\tDELL U2720Q\nA {} 2 77 44 {} 1\ta\tc\tt\nV internal {}\ta\tc\tv\n",
        away.home,
        away.spot.text(),
        Spot { rect: RECT::default(), state: 'n' }.text()
    );
    let lines = parse_state(&text, |_| None);
    assert_eq!(lines[0], Line::M { id: "DEL-4123-ABC".into(), active: 1, area: rect(0, 0, 2560, 1400), name: "DELL U2720Q".into() });
    assert_eq!(lines[1], Line::A { raw: 77, away, fp: Some(Fp { app: "a".into(), class: "c".into(), title: "t".into() }) });
    assert!(matches!(&lines[2], Line::V { spot: None, .. }), "a zero size is a spot nobody knew");
}

// A layout saved by an older des-k, with full exe paths, must still restore after the upgrade
// and still adopt its away window: read back, a path is its file name, which is what a live
// window gives now (spec 0033 AC-2).
#[test]
fn an_old_full_path_reads_back_as_its_file_name() {
    let sp = spot(0, 'n').text();
    let text = format!(
        "W DEL-1 2 0 0 {sp}\tc:\\users\\x\\appdata\\local\\programs\\microsoft vs code\\code.exe\tChrome_WidgetWin_1\tnotes.md\nA DEL-1 1 77 44 {sp} 0\tC:\\Windows\\notepad.exe\tNotepad\tt\n"
    );
    let lines = parse_state(&text, |_| None);
    let Line::W { fp: Some(saved), .. } = &lines[0] else { panic!("{:?}", lines[0]) };
    assert_eq!(saved.app, "code.exe");
    assert!(matches!(&lines[1], Line::A { fp: Some(f), .. } if f.app == "notepad.exe"));
    let entry = crate::restore::Entry { dev: "DEL-1".into(), ws: Some(2), pin: false, away: false, spot: None, fp: saved.clone() };
    let live = Fp { app: "code.exe".into(), class: "Chrome_WidgetWin_1".into(), title: "notes.md".into() };
    assert_eq!(crate::restore::pick(&[entry], &[0], &live, 1), crate::restore::Pick::Take(0));
}

// Nothing reads a title with Restore off, so none is kept (spec 0033 AC-4). The line still has
// all three fields: with fewer it would read as an older des-k's line with no fingerprint,
// and a restore turned back on, or an away window's adoption, would lose the window.
#[test]
fn restore_off_keeps_no_title_but_a_whole_fingerprint() {
    let f = Fp { app: "code.exe".into(), class: "Chrome_WidgetWin_1".into(), title: "salary 2027.xlsx".into() };
    assert!(fp_text(&f, true).contains("salary"));
    let off = fp_text(&f, false);
    assert!(!off.contains("salary"));
    let line = format!("V DEL-1 {}{off}", spot(0, 'n').text());
    let read = parse_state(&line, |_| None);
    assert!(matches!(&read[..], [Line::V { fp: Some(Fp { app, class, title }), .. }] if app == "code.exe" && class == "Chrome_WidgetWin_1" && title.is_empty()));
}

// --quit after a crash shows what it can, but an away window's line stays: its screen may
// still come back, after the next start or a reboot (AC-8).
#[test]
fn rescue_keeps_away_lines() {
    let text = "M DEL-1 2 0 0 100 100\tDell\nA DEL-1 1 100 7 0 0 10 10 n 0\ta\tc\tt\n";
    let (out, shown, left) = rescued(text, |_, _| Some(true));
    assert_eq!((shown, left), (1, 0));
    assert_eq!(out, "M DEL-1 0 0 0 100 100\tDell\nA DEL-1 1 100 7 0 0 10 10 n 0\ta\tc\tt\n");
}
