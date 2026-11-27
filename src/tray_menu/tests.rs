// Tests for the tray menu: the chip lines, the screen map, the item words, and the render states.
use super::*;
use crate::settings::{CTRL, WIN};
use crate::ui::{DARK, LIGHT, fonts::Fonts};

fn screen(name: &str, (x, y, w, h): (i32, i32, i32, i32), active: usize, main: bool) -> TrayScreen {
    TrayScreen { name: name.into(), rect: RECT { left: x, top: y, right: x + w, bottom: y + h }, active, main, ws_name: String::new() }
}

fn named(s: TrayScreen, ws_name: &str) -> TrayScreen {
    TrayScreen { ws_name: ws_name.into(), ..s }
}

// Spec 0023 handoff trayCut: the screen is what the line is about, so a long workspace name
// gives way first, down to 10 letters; only then is the screen cut, at a word, keeping its
// first word and a twin's " #n". A line with no name is cut as before.
#[test]
fn a_chip_line_cuts_the_workspace_name_before_the_screen() {
    let fonts = Fonts::new(1.0);
    let dc = unsafe { windows::Win32::Graphics::Gdi::GetDC(None) };
    let c = Canvas::new(dc, false, 1.0, LIGHT, &fonts);
    let room = RIGHT - IN_X - 20.0 - 6.0;
    let fits = |(a, b): &(String, String)| c.width(a, F::Small) + c.width(SEP, F::Small) + c.width(b, F::Small) <= room;
    assert_eq!(tray_cut(&c, "DELL U2720Q", "Code", room), ("DELL U2720Q".into(), "Code".into()));
    let name_cut = tray_cut(&c, "DELL U2720Q #1", "Quarterly planning and budget 26", room);
    assert!(name_cut.0 == "DELL U2720Q #1" && name_cut.1.ends_with('…') && name_cut.1.chars().count() > 10 && fits(&name_cut), "{name_cut:?}");
    let both = tray_cut(&c, "Studio monitor above the desk 32 #2", "Quarterly planning and budget 26", room);
    assert!(both.0.starts_with("Studio") && both.0.ends_with("… #2") && both.1.chars().count() <= 11 && fits(&both), "{both:?}");
    let long = "LG ULTRAGEAR 27GR95QE OLED Gaming Monitor and then some more words";
    assert_eq!(tray_cut(&c, long, "", room), (c.fit(long, F::Small, room), String::new()));
    unsafe { let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, dc); }
}

fn menu(screens: Vec<TrayScreen>) -> Menu {
    Menu { version: "0.3.0".into(), screens, broken: false, hints_key: None, paused: false, swipes_off: false, log: true, check: Check::Ready, quit: Quit::Plain }
}

/// Screens on a grid of 16 × 9 units, like the design's: (x, y, w, h, workspace number).
fn wall(units: &[(i32, i32, i32, i32, usize)]) -> Vec<TrayScreen> {
    units.iter().enumerate().map(|(i, &(x, y, w, h, n))| screen(&format!("Screen {}", i + 1), (x * 240, y * 240, w * 240, h * 240), n - 1, i == 0)).collect()
}

// AC-TRY-4: the map must fit the menu's 264 × 88 however many screens you have, keep their
// arrangement, and never draw a screen too small to see or point at.
#[test]
fn the_map_fits_and_no_screen_is_too_small() {
    let twelve: Vec<RECT> = (0..12).map(|i| RECT { left: (i % 4) * 3840, top: (i / 4) * 2160, right: (i % 4 + 1) * 3840, bottom: (i / 4 + 1) * 2160 }).collect();
    let (boxes, h) = map(&twelve);
    assert!(h <= MAP_H);
    assert!(boxes.iter().all(|&(x, y, w, bh)| x >= 0.0 && x + w <= MAP_W && y + bh <= MAP_H && w >= MIN_SIDE && bh >= MIN_SIDE), "{boxes:?}");
    assert!(boxes[0].0 < boxes[1].0 && boxes[0].1 < boxes[4].1, "the arrangement is kept");
    // A small screen beside three huge ones: the map grows rather than lose it.
    let odd = [RECT { left: 0, top: 0, right: 20000, bottom: 8000 }, RECT { left: 20000, top: 0, right: 20400, bottom: 300 }];
    let (boxes, _) = map(&odd);
    assert!(boxes[1].2 >= MIN_SIDE - 0.5 && boxes[1].3 >= MIN_SIDE - 0.5, "{boxes:?}");
}

// Spec 0023 AC-10: a screen showing a named workspace says the name after its number, in
// the tooltip too; an unnamed one keeps today's tooltip, which has no workspace words.
#[test]
fn a_screen_says_its_workspaces_name() {
    let mut named = screen("Left", (0, 0, 1920, 1080), 1, true);
    named.ws_name = "Mail".into();
    let m = menu(vec![named, screen("Right", (1920, 0, 1920, 1080), 0, false)]);
    let s = Session { menu: m, lay: Lay::default(), scale: 1.0, theme: LIGHT, hot: None, focus: None, landed: None, tip: None, came_from: HWND::default() };
    assert_eq!((s.screen_words(0, true), s.screen_words(0, false)), ("Left, workspace 2, Mail, main display".into(), "Left, workspace 2, Mail, main display".into()));
    assert_eq!((s.screen_words(1, true), s.screen_words(1, false)), ("Right, workspace 1".into(), "Right".into()));
}

// AC-TRY-5, AC-TRY-7: a greyed item can't run; a newer release says where it goes; Q's
// letter only moves to Quit (the label starts with Q), and the hints key shows as caps.
#[test]
fn the_items_say_what_they_do_now() {
    let mut m = menu(vec![]);
    m.log = false;
    m.check = Check::Busy;
    m.hints_key = Some(Combo { mods: WIN | CTRL, vk: b'H' as u16 });
    let items = m.items();
    assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [Id::Hints, Id::Settings, Id::Log, Id::Update, Id::Pause, Id::Quit]);
    assert_eq!((items[2].on, items[3].on, items[3].label.as_str()), (false, false, "Checking for updates…"));
    assert_eq!(items[0].keys, ["Win", "Ctrl", "H"]);
    m.check = Check::Newer(Version(0, 4, 0));
    let up = &m.items()[3];
    assert_eq!((up.label.as_str(), up.line.as_str(), up.new, up.on), ("Update to 0.4.0", "Opens its page on github.com", true, true));
    // Spec 0018: the installed admin copy installs it, and the item can't start a second one.
    m.check = Check::Install(Version(0, 4, 2));
    let up = &m.items()[3];
    assert_eq!((up.label.as_str(), up.line.as_str(), up.new, up.on), ("Install 0.4.2", "Downloads and installs it", true, true));
    m.check = Check::Downloading(Version(0, 4, 2));
    assert_eq!((m.items()[3].label.as_str(), m.items()[3].on), ("Downloading des-k 0.4.2…", false));
    m.check = Check::Installing(Version(0, 4, 2));
    assert_eq!((m.items()[3].label.as_str(), m.items()[3].on), ("Installing des-k 0.4.2…", false));
    // Spec 0031 AC-5: a refused release says so and why, and the item stays clickable so a
    // check can find a newer one; it never reads as an offer (no New).
    for (f, line) in [
        (InstallFail::Unsigned, "Not signed by des-k's publisher"),
        (InstallFail::BadSignature, "Not signed by des-k's publisher"),
        (InstallFail::Mismatch, "Did not match its signed checksum"),
    ] {
        m.check = Check::Refused(Version(0, 9, 0), f);
        let up = &m.items()[3];
        assert_eq!((up.label.as_str(), up.line.as_str(), up.icon, up.new, up.on), ("des-k 0.9.0 refused", line, "\u{E7BA}", false, true), "{f:?}");
    }
    m.check = Check::Failed(Fail::NoConnection);
    assert_eq!(m.items()[3].line, "Couldn't check: no connection to GitHub.");
    m.check = Check::Failed(Fail::Timeout);
    assert_eq!(m.items()[3].line, "Couldn't check: GitHub did not answer in time.");
    m.quit = Quit::Stops;
    assert!(m.items()[5].line.contains("no gestures until you turn Windows' on"));
}

// Spec 0039 AC-1, AC-5: one item pauses and resumes, saying which it does now, and a paused des-k
// with Windows' gestures off says why a swipe does nothing, since nothing else on screen does.
#[test]
fn the_pause_item_says_pause_or_resume() {
    let mut m = menu(vec![]);
    let it = &m.items()[4];
    assert_eq!((it.id, it.label.as_str(), it.line.as_str(), it.on), (Id::Pause, "Pause des-k", "", true));
    m.paused = true;
    assert_eq!((m.items()[4].label.as_str(), m.items()[4].line.as_str()), ("Resume des-k", ""));
    m.swipes_off = true;
    assert_eq!(m.items()[4].line, "Swipes do nothing until you resume");
    m.paused = false;
    assert_eq!(m.items()[4].line, "", "running, swipes work: no line");
}

fn twelve_mixed() -> Vec<TrayScreen> {
    let mut s = wall(&[(9, 0, 16, 9, 1), (25, 0, 16, 9, 2), (41, 0, 16, 9, 3), (57, 0, 16, 9, 4), (9, 9, 16, 9, 5), (25, 9, 16, 9, 6), (41, 9, 16, 9, 7), (57, 9, 16, 9, 8), (0, 1, 9, 16, 9), (73, 1, 9, 16, 1), (26, 18, 14, 8, 2), (42, 18, 14, 8, 3)]);
    for (i, x) in s.iter_mut().enumerate() {
        x.main = i == 10;
    }
    s
}

/// Draws the design's states to target/tray/ PNGs, to compare by eye with
/// pdesk-design/restart/captures/trr4-* (spec 0017 AC-1, AC-TRY-1 to AC-TRY-8).
#[test]
#[ignore]
fn render() {
    ui::canvas::gdiplus();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("tray");
    std::fs::create_dir_all(&dir).unwrap();
    let two = || vec![screen("DELL U2720Q", (0, 0, 3840, 2160), 1, true), screen("Built-in display", (3840, 600, 2560, 1600), 0, false)];
    let three = wall(&[(0, 0, 16, 9, 2), (16, 0, 16, 9, 5), (32, 0, 16, 9, 1)]);
    let wall12 = wall(&[(0, 0, 16, 9, 3), (16, 0, 16, 9, 1), (32, 0, 16, 9, 7), (48, 0, 16, 9, 2), (0, 9, 16, 9, 4), (16, 9, 16, 9, 2), (32, 9, 16, 9, 6), (48, 9, 16, 9, 1), (0, 18, 16, 9, 5), (16, 18, 16, 9, 9), (32, 18, 16, 9, 3), (48, 18, 16, 9, 8)]);
    type Setup = fn(&mut Session);
    let states: Vec<(&str, Vec<TrayScreen>, Option<usize>, Setup)> = vec![
        ("pointed", two(), Some(0), |s| s.hot = Some(0)),
        ("hints-key", two(), Some(0), |s| {
            s.menu.hints_key = Some(Combo { mods: WIN | CTRL, vk: b'H' as u16 });
            s.hot = Some(0);
        }),
        ("checking", two(), Some(0), |s| s.menu.check = Check::Busy),
        ("broken-newer", two(), Some(0), |s| {
            s.menu.broken = true;
            s.menu.check = Check::Newer(Version(0, 4, 0));
            s.menu.log = false;
            s.hot = Some(3);
        }),
        ("broken-newest", two(), Some(0), |s| {
            s.menu.broken = true;
            s.menu.check = Check::Newest(Version(0, 3, 0));
        }),
        ("refused", two(), Some(0), |s| {
            s.menu.check = Check::Refused(Version(0, 9, 0), InstallFail::Unsigned);
            s.hot = Some(3);
        }),
        ("broken-failed", two(), Some(0), |s| {
            s.menu.broken = true;
            s.menu.check = Check::Failed(Fail::NoConnection);
        }),
        ("quit-saved", two(), Some(0), |s| {
            s.menu.quit = Quit::Saved;
            s.hot = Some(5);
        }),
        ("paused", two(), Some(0), |s| {
            s.menu.paused = true;
            s.menu.swipes_off = true;
            s.hot = Some(4);
        }),
        ("quit-stops", two(), Some(0), |s| s.menu.quit = Quit::Stops),
        ("keyboard", two(), Some(0), |s| s.focus = Some(0)),
        ("one", vec![screen("Built-in display", (0, 0, 2560, 1600), 0, true)], Some(0), |_| {}),
        ("three", three, Some(0), |s| s.hot = Some(1)),
        ("twelve-wall", wall12, Some(8), |s| s.hot = Some(1)),
        ("twelve-mixed", twelve_mixed(), Some(10), |_| {}),
        // Spec 0023: the handoff's t1 one, t2 two, t4 long twins, t4b a long rename, t3 map.
        ("names-one", vec![named(screen("Built-in display", (0, 0, 2560, 1600), 0, true), "Mail")], Some(0), |_| {}),
        ("names-two", vec![named(two()[0].clone(), "Code"), named(two()[1].clone(), "Mail")], Some(0), |_| {}),
        (
            "names-long",
            vec![
                named(screen("DELL U2720Q #1", (0, 0, 3840, 2160), 1, true), "Quarterly planning and budget 26"),
                named(screen("DELL U2720Q #2", (3840, 0, 3840, 2160), 0, false), "Reading list and papers to file"),
            ],
            Some(0),
            |_| {},
        ),
        ("names-long-rename", vec![named(screen("Studio monitor above the desk 32", (0, 0, 3840, 2160), 1, true), "Quarterly planning and budget 26")], Some(0), |_| {}),
        ("names-three", wall(&[(0, 0, 16, 9, 2), (16, 0, 16, 9, 5), (32, 0, 16, 9, 1)]).into_iter().enumerate().map(|(i, s)| named(s, ["Code", "", "Mail"][i])).collect(), Some(0), |s| s.hot = Some(0)),
    ];
    let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
    let (rose, rose_dark) = (ui::tests::rose(true), ui::tests::rose(false));
    let themes: [(&str, Theme, f32); 7] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5), ("rose", rose, 1.0), ("rose-dark", rose_dark, 1.0)];
    for (tname, t, scale) in themes {
        let fonts = Fonts::new(scale);
        for (name, screens, landed, setup) in &states {
            let m = menu(screens.clone());
            let lay = unsafe {
                let dc = GetDC(None);
                let l = layout(&Canvas::new(dc, false, scale, t, &fonts), &m);
                ReleaseDC(None, dc);
                l
            };
            let mut s = Session { menu: m, lay, scale, theme: t, hot: None, focus: None, landed: *landed, tip: None, came_from: HWND::default() };
            setup(&mut s);
            s.lay = unsafe {
                let dc = GetDC(None);
                let l = layout(&Canvas::new(dc, false, scale, t, &fonts), &s.menu);
                ReleaseDC(None, dc);
                l
            };
            let (w, h) = (s.px(W), s.px(s.lay.height));
            ui::tests::save_png(w, h, &dir.join(format!("{name}-{tname}.png")), |dc| {
                let c = Canvas::new(dc, true, scale, t, &fonts);
                draw(&c, &s);
                // Windows' 1 px edge, as ui::border asks for it.
                c.stroke_rr((0.5 / scale, 0.5 / scale, W - 1.0 / scale, s.lay.height - 1.0 / scale), 8.0, 1.0 / scale, op(t.b.edge));
            });
        }
    }
}
