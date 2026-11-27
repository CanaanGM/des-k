// The finder's session tests: filtering, Ctrl+Backspace, what Narrator hears, workspace names
// and the render states, plus the row helpers the other tests share.
use super::*;
use super::draw::draw;
use crate::ui::{DARK, LIGHT, Theme, fonts::Fonts};

pub(super) fn row(on_screen: bool, screen: usize, workspace: usize, used: Option<Instant>) -> Row {
    Row {
        hwnd: HWND::default(),
        title: String::new(),
        program: String::new(),
        icon: HICON::default(),
        screen,
        place: String::new(),
        workspace,
        on_screen,
        pinned: false,
        unplugged: false,
        ws_name: String::new(),
        used,
        rank: 0,
        gone: false,
        marks: vec![],
    }
}

pub(super) fn named(program: &str, title: &str, rank: usize) -> Row {
    Row { program: program.into(), title: title.into(), rank, ..row(true, 1, 1, None) }
}

pub(super) fn session(rows: Vec<Row>) -> Session {
    Session::new(rows, HWND::default(), String::new(), 2, "Built-in display".into(), 1.0)
}

pub(super) fn filter(s: &mut Session, q: &str) {
    s.query = q.into();
    s.refilter(false);
}

// AC-5: each typed word must appear, letters in order, any case, in "program title"; the
// best match is selected, and an empty result says why.
#[test]
fn refilter_matches_every_word_fuzzily_and_selects_the_best() {
    let mut s = session(vec![named("explorer", "Downloads", 0), named("Code", "finder.rs - pdesk - Visual Studio Code", 1), named("chrome", "pdesk issues", 2)]);
    let firsts = |s: &mut Session, q: &str| {
        filter(s, q);
        (s.shown.iter().map(|&(i, _)| i).collect::<Vec<_>>(), s.selected, s.empty_text())
    };
    assert_eq!(firsts(&mut s, "vsc"), (vec![1], 0, None));
    assert_eq!(firsts(&mut s, "PDESK visual").0, [1]);
    assert_eq!(firsts(&mut s, "pdesk").0.len(), 2);
    assert_eq!(firsts(&mut s, "zzzq"), (vec![], 0, Some("No match")));
    assert_eq!(s.count().as_deref(), Some("0 of 3"));
    s.rows.iter_mut().for_each(|r| r.gone = true);
    assert_eq!(firsts(&mut s, "").2, Some("No windows"));
    assert_eq!(s.count(), None);
}

// Ctrl+Backspace removes one word, like everywhere else in Windows, not the whole query or one letter.
#[test]
fn delete_word_stops_at_a_space_or_punctuation_boundary() {
    let cut = |s: &str| {
        let w: Vec<u16> = s.encode_utf16().collect();
        String::from_utf16(&w[..delete_word(&w)]).unwrap()
    };
    assert_eq!(cut("code pdesk"), "code ");
    assert_eq!(cut("code pdesk  "), "code ");
    assert_eq!(cut("main.rs"), "main.");
    assert_eq!(cut("main."), "main");
    assert_eq!(cut("word"), "");
    assert_eq!(cut(""), "");
}

// AC-FND-9: Narrator hears the same words the row paints, plus where the window is; and it
// hears each change once, not again on every repaint.
#[test]
fn narrator_reads_the_chosen_row_once_per_change() {
    let mut rows = vec![named("Google Chrome", "des-k issues", 0), named("Notepad", "notes.txt", 1)];
    rows[0].place = "DELL U2720Q".into();
    rows[0].hwnd = HWND(10 as *mut _);
    rows[1].hwnd = HWND(11 as *mut _);
    rows[1].on_screen = false;
    rows[1].workspace = 3;
    rows[1].place = "Built-in display".into();
    let mut s = session(rows);
    assert_eq!(s.speech(), Some(("2 windows, notes.txt, Notepad, Built-in display, workspace 3, hidden".into(), true)));
    assert_eq!(s.speech(), None);
    s.selected = 0;
    assert_eq!(s.speech(), Some(("des-k issues, Google Chrome, DELL U2720Q, workspace 1, shown".into(), false)));
    s.note = Some(Note::Unplugged);
    assert_eq!(s.speech().unwrap().0, "Its screen was just unplugged. In a moment it moves to Built-in display; try again then.");
    let tree = s.tree();
    let list = tree.kids.iter().find(|n| n.role == UIA_ListControlTypeId).unwrap();
    assert_eq!(list.kids.iter().map(|k| k.selected).collect::<Vec<_>>(), [Some(true), Some(false)]);
}

// Spec 0023 AC-9: typing a workspace's name finds its windows, but never above a window
// whose own title or app matches; name only rows keep the AC-4 order, with no bold letters,
// and a pinned window (on every workspace) matches no name.
#[test]
fn a_workspace_name_matches_after_every_title_and_app() {
    let on = |program: &str, title: &str, rank: usize, ws_name: &str| Row { ws_name: ws_name.into(), ..named(program, title, rank) };
    let mut rows = vec![on("Notepad", "todo.txt", 1, "Mail"), on("Outlook", "Mail", 2, ""), on("Code", "main.rs", 0, "Mail"), on("Teams", "Chat", 3, "")];
    rows[3].pinned = true;
    rows[3].ws_name = "Mail".into();
    let mut s = session(rows);
    filter(&mut s, "mail");
    assert_eq!(s.shown.iter().map(|&(i, _)| i).collect::<Vec<_>>(), [1, 2, 0]);
    assert!(s.rows[0].marks.is_empty() && s.rows[2].marks.is_empty() && !s.rows[1].marks.is_empty());
    filter(&mut s, "post");
    assert!(s.shown.is_empty());
}

// Spec 0023 AC-8, AC-15: a row on a named workspace reads its name before its screen, as the
// place column paints them.
#[test]
fn narrator_reads_a_rows_workspace_name_before_its_screen() {
    let mut r = named("Notepad", "notes.txt", 0);
    (r.place, r.ws_name, r.workspace, r.on_screen) = ("DELL U2720Q".into(), "Mail".into(), 2, false);
    let s = session(vec![r]);
    assert_eq!(s.words(0), "notes.txt, Notepad, Mail, DELL U2720Q, workspace 2, hidden");
}

/// A window list like the design's: [icon, program, title, screen, workspace, flag].
fn design_rows(set: &str) -> Vec<Row> {
    const DELL: &str = "DELL U2720Q";
    const BUILT: &str = "Built-in display";
    let all: Vec<(&str, &str, &str, usize, &str)> = match set {
        // Spec 0023 handoff f2: names and screens that do not fit the place column.
        "cut" => vec![
            ("Excel", "Budget 2026.xlsx - Excel", "DELL U2720Q #2", 2, "shown"),
            ("Word", "Plan.docx - Word", "DELL U2720Q #2", 2, ""),
            ("Zotero", "My Library - Zotero", BUILT, 3, "away"),
            ("Edge", "Papers - Microsoft Edge", BUILT, 3, "away"),
            ("Notepad", "todo.txt - Notepad", "Studio monitor above the desk 32", 1, "shown"),
        ],
        "long" => vec![
            ("Visual Studio Code", "settings_page.rs - des-k - a very long workspace folder name that goes on - Visual Studio Code", "LG ULTRAGEAR 27GR95QE OLED Gaming Monitor #1", 9, ""),
            ("Microsoft Edge WebView2", "A web page title that is far too long to fit in the row at all", "LG ULTRAGEAR 27GR95QE OLED Gaming Monitor #2", 7, "shown"),
            ("Windows Terminal", "Administrator: PowerShell", BUILT, 6, ""),
        ],
        _ => vec![
            ("Visual Studio Code", "finder.rs - des-k - Visual Studio Code", DELL, 2, "shown"),
            ("Google Chrome", "des-k issues · GitHub - Google Chrome", DELL, 3, if set == "closing" { "closing" } else if set == "away" { "away" } else { "" }),
            ("File Explorer", "Downloads", BUILT, 1, "shown"),
            ("Windows Terminal", "PowerShell", DELL, 1, ""),
            ("Notepad", "notes.txt - Notepad", BUILT, 3, if set == "away" { "away" } else { "" }),
            ("Microsoft Teams", "Chat | Microsoft Teams", BUILT, 1, "pin"),
            ("Outlook", "Inbox - Outlook", DELL, 4, ""),
            ("Settings", "Display", BUILT, 2, ""),
            ("Paint", "Untitled - Paint", DELL, 2, "shown"),
            ("Google Chrome", "Pull requests · des-k - Google Chrome", BUILT, 4, ""),
            ("Spotify", "Spotify Premium", BUILT, 2, ""),
            ("File Explorer", "des-k", DELL, 4, ""),
        ],
    };
    let app = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
    all.into_iter()
        .enumerate()
        .map(|(i, (program, title, place, ws, flag))| Row {
            hwnd: HWND((i + 1) as *mut _),
            place: if flag == "away" { "LG HDR 4K".into() } else { place.into() },
            on_screen: matches!(flag, "shown" | "pin"),
            pinned: flag == "pin",
            unplugged: flag == "away",
            icon: app,
            workspace: ws,
            ..named(program, title, i)
        })
        .collect()
}

/// Workspace N's name is `names[N - 1]` on every row that is not pinned, as `ws` fills
/// `Place.ws_name`; the search runs again, since a name can match it (spec 0023 AC-9).
fn name_rows(s: &mut Session, names: &[&str]) {
    for r in s.rows.iter_mut().filter(|r| !r.pinned) {
        r.ws_name = names.get(r.workspace - 1).copied().unwrap_or_default().into();
    }
    s.refilter(true);
}

/// Draws every state the design shows to target/finder/ PNGs, to compare by eye with
/// pdesk-design/restart/captures/fdr3-* (spec 0017 AC-1, AC-FND-1 to AC-FND-7).
#[test]
#[ignore]
fn render() {
    ui::canvas::gdiplus();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("finder");
    std::fs::create_dir_all(&dir).unwrap();
    type Setup = fn(&mut Session);
    let states: Vec<(&str, &str, &str, Setup)> = vec![
        ("opened", "all", "", |_| {}),
        ("chrome", "all", "chrome", |_| {}),
        ("two-words", "all", "code des", |_| {}),
        ("no-match", "all", "zzzq", |_| {}),
        ("no-windows", "none", "", |_| {}),
        ("scrolled", "all", "", |s| {
            s.selected = 11;
            s.scroll_to_selection();
        }),
        ("one-screen", "all", "", |s| s.screens = 1),
        ("hover", "all", "", |s| s.hover = Some(4)),
        ("long", "long", "", |s| s.selected = 0),
        ("closing", "closing", "", |s| s.closing.push(HWND(2 as *mut _))),
        ("closed", "all", "", |s| {
            s.rows[1].gone = true;
            s.refilter(true);
        }),
        ("cant-move", "long", "", |s| {
            s.selected = 2;
            s.note = Some(Note::CantMove);
        }),
        ("cant-close", "long", "", |s| {
            s.selected = 2;
            s.note = Some(Note::CantClose);
        }),
        ("gone", "all", "", |s| {
            s.rows[1].gone = true;
            s.refilter(true);
            s.note = Some(Note::Gone);
        }),
        ("unplugged", "away", "", |_| {}),
        ("unplugged-note", "away", "", |s| s.note = Some(Note::Unplugged)),
        // Spec 0023: the handoff's f1 rows, f2 cut, f3 search, f4 one screen.
        ("names", "away", "", |s| name_rows(s, &["Mail", "Code", "Research", "بريد"])),
        ("names-cut", "cut", "", |s| {
            name_rows(s, &["", "Quarterly planning", "Reading list"]);
            (s.rows[1].ws_name, s.rows[3].ws_name) = ("Quarterly planning and budget 26".into(), "Reading list and papers to file".into());
            s.selected = 1;
        }),
        ("names-search", "all", "mail", |s| name_rows(s, &["Mail", "Code", "Research", "Music"])),
        ("names-one-screen", "all", "", |s| {
            name_rows(s, &["Mail", "Code", "", "Quarterly planning and budget 26"]);
            s.screens = 1;
        }),
    ];
    let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
    let themes: [(&str, Theme, f32); 5] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5)];
    for (tname, t, scale) in themes {
        let fonts = Fonts::new(scale);
        for (name, set, query, setup) in &states {
            let rows = if *set == "none" { vec![] } else { design_rows(set) };
            let mut s = Session::new(rows, HWND::default(), String::new(), 2, "Built-in display".into(), scale);
            filter(&mut s, query);
            setup(&mut s);
            let (w, h) = (s.px(WIDTH), s.px(s.height()));
            let path = dir.join(format!("{name}-{tname}.png"));
            ui::tests::save_png(w, h, &path, |dc| {
                let c = Canvas::new(dc, true, scale, t, &fonts);
                draw(&c, &s, Some(query));
                // Windows' 1 px edge, as ui::border asks for it.
                c.stroke_rr((0.5 / scale, 0.5 / scale, WIDTH - 1.0 / scale, s.height() - 1.0 / scale), 8.0, 1.0 / scale, op(t.b.edge));
            });
        }
    }
}
