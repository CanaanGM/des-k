// The page's shared test helpers and the render test.
use super::*;
use crate::settings_page::cards::colors::{ACCENT, Hex, ws_index};
use crate::settings_page::cards::names::Nm;
use crate::settings_page::cards::wallpaper::wall_wanted;
use crate::settings_page::picker::draw::{levels_of, place, room_of};
use super::menu::Menu;
use super::picker::{Anchor, ColorPick, Geo, PICK_H, PICK_W, Stop, file_color};
use super::recording::Prompt;
use super::rows::Section;

pub(super) fn loaded(text: &str) -> &'static Settings {
    Box::leak(Box::new(settings::parse::parse_for_test(text).0))
}

/// An edit in words, since toml values don't compare: "at colors.ws2 = \"#DDB43D\"".
pub(super) fn said(e: Option<Edit>) -> String {
    match e {
        Some(Edit::At(p, v)) => format!("at {} = {}", p.join("."), v.to_string().trim()),
        Some(Edit::Remove(p)) => format!("remove {}", p.join(".")),
        Some(_) => "other".into(),
        None => "none".into(),
    }
}

// The page tests that set settings::get() for every reader run one at a time.
pub(crate) static SETTINGS: Mutex<()> = Mutex::new(());

fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

/// Two screens as the render states draw them: a laptop on workspace 1 and a DELL on 2.
fn fake_pics() -> Pics {
    let r = rect;
    Pics {
        screens: vec![
            crate::ws::views::ScreenInfo { device: "DISPLAY1".into(), number: 1, name: "Built-in display".into(), rect: r(0, 600, 1440, 1500), scale: 200 },
            crate::ws::views::ScreenInfo { device: "DISPLAY2".into(), number: 2, name: "DELL U2720Q".into(), rect: r(1440, 0, 4000, 1440), scale: 150 },
        ],
        view: crate::ws::views::View {
            active: vec![("DISPLAY1".into(), 0), ("DISPLAY2".into(), 1)],
            windows: [("DISPLAY1", 0), ("DISPLAY1", 0), ("DISPLAY1", 0), ("DISPLAY2", 0), ("DISPLAY2", 1), ("DISPLAY2", 1), ("DISPLAY2", 3)].iter().map(|(d, w)| (d.to_string(), *w, r(0, 0, 10, 10))).collect(),
            names: vec![("DISPLAY1".into(), "Built-in display".into()), ("DISPLAY2".into(), "DELL U2720Q".into())],
            present: vec![
                crate::ws::views::Present { id: "internal".into(), device: "DISPLAY1".into(), name: "Built-in display".into(), windows_name: "Built-in display".into(), full: r(0, 600, 1440, 1500), work: r(0, 600, 1440, 1452) },
                crate::ws::views::Present { id: "DEL-A0F5-1234".into(), device: "DISPLAY2".into(), name: "DELL U2720Q".into(), windows_name: "DELL U2720Q".into(), full: r(1440, 0, 4000, 1440), work: r(1440, 0, 4000, 1392) },
            ],
            removals: vec![
                vec![
                    crate::ws::screens::Moved { name: "Built-in display".into(), device: Some("DISPLAY1".into()), unplugged: false, windows: 0, from: vec![] },
                    crate::ws::screens::Moved { name: "DELL U2720Q".into(), device: Some("DISPLAY2".into()), unplugged: false, windows: 1, from: vec![3] },
                ],
                vec![],
            ],
            waiting: vec![],
            open: vec![],
        },
        pad: Some(PadState { pad: true, enabled: true, rows: [PadRow::Nothing; 4], off: true, saved: true }),
        back: None,
        here: Some("DISPLAY2".into()),
        apps: vec![],
        wall_failed: false,
        wall_problems: vec![],
        wall_now: BTreeMap::new(),
        wall_counts: vec![],
    }
}

/// Apps with a window open, as the Apps render states show them: the three ruled ones
/// running (Spotify isn't), and, unless `ruled_only`, three more for the Add menu.
pub(super) fn fake_apps(w: &mut Win, ruled_only: bool) {
    let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
    let seen = |key: &str, name: &str, device: &str, ws: usize| crate::apps::Seen { key: key.into(), name: name.into(), icon, device: device.into(), ws };
    w.pics.apps = vec![
        seen("slack.exe", "Slack", "DISPLAY2", 1),
        seen("olk.exe", "Outlook", "DISPLAY1", 0),
        seen("microsoft.windowscalculator_8wekyb3d8bbwe!app", "Calculator", "DISPLAY1", 0),
    ];
    if !ruled_only {
        w.pics.apps.insert(1, seen("msedge.exe", "Microsoft Edge", "DISPLAY1", 0));
        w.pics.apps.push(seen("windowsterminal.exe", "Terminal", "DISPLAY2", 1));
        w.pics.apps.push(seen("code.exe", "Visual Studio Code", "DISPLAY2", 1));
    }
}

/// A picture value, or none for "".
pub(super) fn pic(name: &str) -> Pick {
    if name.is_empty() { Pick::None } else { Pick::Picture(name.into()) }
}

pub(super) fn fake_win(theme: Theme, scale: f32) -> Win {
    Win {
        hwnd: HWND::default(), scale, theme, fonts: Fonts::new(scale), scroll: 0.0, height: 0.0, fixed: 0.0, tops: vec![], cards: vec![], hits: vec![],
        focus: None, kb: true, hover: None, press: None, drag: None, vw: W, vh: H, error: None, rec: None, refused: None, prompt: None, menu: None,
        pics: fake_pics(), edited: None, tree: Arc::new(Mutex::new(Node::default())),
        hexes: vec![], own: None, screen: None, chooser: None, dragging: None, pick: None, geo: Geo::default(), ease: None, down_at: (0, 0), words: None,
        names_screen: None, typed: None, unsaved: vec![], wall_screen: None, wall_files: vec![], wall_dirs: vec![], wall_top: 0, wall_waiting: false,
    }
}

/// Decoded stand ins for the Wallpaper card's previews: a gradient per picture, and a blue
/// one for the screen's Windows wallpaper, as the approved design draws them.
fn fake_wall(w: &Win) {
    let Some((rect, size, files)) = wall_wanted(w, &settings::get()) else { return };
    let pic = |top: u32, bottom: u32| {
        let (pw, ph) = size;
        let px = (0..ph).flat_map(|y| std::iter::repeat_n(0xFF00_0000 | mix(top, bottom, y as f32 / (ph - 1).max(1) as f32), pw as usize)).collect();
        ui::canvas::Picture { w: pw, h: ph, px }
    };
    crate::wallpaper::put_for_test(rect, None, size, pic(0x9CD0FF, 0x0B2C6B));
    for f in files {
        // A slideshow's picture is "folder\name": colored by its name.
        let (a, b) = match f.rsplit('\\').next().unwrap_or_default() {
            "beach.jpg" => (0x7FD3F2, 0xF2DDA9),
            "forest.jpg" => (0x8FCBE6, 0x2F6B45),
            "desk.png" => (0xEAD9C3, 0x9A6644),
            "city.jpg" => (0x0E1A3F, 0x47306F),
            "dunes.jpg" => (0xF9D58C, 0xB65A2A),
            _ => (0xF4C7C3, 0x9CB6D6),
        };
        crate::wallpaper::put_for_test(rect, Some(&f), size, pic(a, b));
    }
}

/// One layout pass of the page, measuring only, as `lay` does.
pub(super) fn measure(win: &Win) -> Out {
    unsafe {
        let dc = GetDC(None);
        let out = compose(&mut Canvas::new(dc, false, win.scale, painted(win), &win.fonts), win);
        ReleaseDC(None, dc);
        out
    }
}

pub(super) fn take(win: &mut Win, out: Out) {
    (win.hits, win.height, win.fixed, win.tops, win.geo) = (out.hits, out.height, out.fixed, out.tops, out.geo);
}

/// An open picker for `key`, showing `color` and holding it in the file.
fn picking(key: &'static str, anchor: Anchor, color: u32) -> ColorPick {
    let start = file_color(&settings::get(), key).unwrap_or(color);
    ColorPick {
        key, from: None, anchor, before: None, start, file: color, color, hsv: ui::color::hsv_of(color, (0.0, 0.0, 0.0)), preview: None, written: None, failed: None,
        saved: false, held_edit: None, placed: None, scroll0: 0.0, guard: None, focus: Stop::Ring, last_swatch: 0, drag: None, held: None,
    }
}

/// Each render state, set up as the render test draws it: the settings, the scroll, an open
/// menu, chooser or picker. `skip_picks` leaves the picker's states out.
pub(super) fn each_state(skip_picks: bool, theme: Theme, scale: f32, f: &mut dyn FnMut(&str, &Win)) {
    // Name, section to scroll to, what to set up.
    type Setup = fn(&mut Win);
    // Spec 0023: names on every screen, and the laptop's own (renamed Left, Notes on 2, no name on 3).
    const NAMES: &str = "[names]
ws1 = \"Mail\"
ws2 = \"Code\"
ws4 = \"Research and reading\"
[names.screens.internal]
screen = \"Left\"
ws2 = \"Notes\"
ws3 = \"\"
";
    const DOWN_TO_NAMES: f32 = 380.0;
    // Spec 0026: the Wallpaper card as approved in design r3, with a screen of its own.
    const WALL: &str = "[wallpaper]
dim = 30
ws1 = \"forest.jpg\"
ws2 = \"desk.png\"
ws4 = \"dunes.jpg\"
[wallpaper.screens.\"DEL-A0F5-1234\"]
ws2 = \"city.jpg\"
ws4 = \"\"
";
    const DOWN_TO_WALL: f32 = 720.0;
    // Spec 0028: the slideshow rows of design r3 (frames k to p).
    const SLIDES: &str = "[wallpaper]
dim = 30
ws1 = \"forest.jpg\"
ws2 = \"beach\"
ws3 = [\"desk.png\", \"dunes.jpg\", \"forest.jpg\"]
ws4 = \"city nights\"
[wallpaper.intervals]
ws4 = 5
[wallpaper.screens.\"DEL-A0F5-1234\"]
ws4 = \"trips\"
";
    // What the backdrop shows now: each screen moves on its own.
    fn slides(w: &mut Win) {
        for (id, beach) in [("internal", "beach\\beach.jpg"), ("DEL-A0F5-1234", "beach\\lake at dawn.PNG")] {
            w.pics.wall_now.insert((id.into(), 1), beach.into());
            w.pics.wall_now.insert((id.into(), 2), "dunes.jpg".into());
            w.pics.wall_now.insert((id.into(), 3), "city nights\\city.jpg".into());
        }
        w.pics.wall_counts = vec![("beach".into(), 12), ("city nights".into(), 8), ("trips".into(), 0)];
    }
    const DIRS: [(&str, usize); 3] = [("beach", 12), ("city nights", 8), ("trips", 0)];
    const FILES: [&str; 5] = ["city.jpg", "desk.png", "dunes.jpg", "forest.jpg", "lake at dawn.PNG"];
    // The DELL turned on its side, so its previews are tall.
    fn portrait(w: &mut Win) {
        w.pics.view.present[1].full = RECT { left: 1440, top: 0, right: 2880, bottom: 2560 };
        w.wall_screen = Some("DEL-A0F5-1234".into());
    }
    // Spec 0027: four rules, as the approved design shows them.
    const APPS: &str = "[names]
ws1 = \"Mail\"
ws2 = \"Code\"
ws3 = \"Research\"
[apps]
\"slack.exe\" = { workspace = 3, screen = \"DEL-A0F5-1234\" }
\"olk.exe\" = { workspace = 1, screen = \"internal\" }
\"microsoft.windowscalculator_8wekyb3d8bbwe!app\" = { workspace = 4 }
\"spotify.exe\" = { workspace = 2 }
";
    let states: [(&str, Option<Section>, Setup); 71] = [
        ("wallpaper-alone-own", Some(Section::Workspaces), |w| {
            settings::store::use_for_test("[wallpaper]\nws1 = \"forest.jpg\"\nws2 = [\"desk.png\", \"dunes.jpg\"]\n[wallpaper.screens.internal]\nws2 = \"forest.jpg\"\n");
            w.pics.view.present.truncate(1);
            w.pics.view.waiting.clear();
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-unused", Some(Section::Workspaces), |w| {
            settings::store::use_for_test("[wallpaper]\nws1 = \"forest.jpg\"\nws2 = [\"desk.png\", \"dunes.jpg\"]\n[wallpaper.screens.internal]\nws2 = \"forest.jpg\"\n[wallpaper.screens.\"DEL-A0F5-1234\"]\nws1 = \"city.jpg\"\nws2 = \"city.jpg\"\n");
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-slides", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(SLIDES);
            slides(w);
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-slides-end", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(SLIDES);
            slides(w);
            w.focus = Some(Ctl::Stepper("wallpaper.interval"));
            w.scroll = DOWN_TO_WALL + 330.0;
        }),
        ("wallpaper-slides-folders", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(SLIDES);
            slides(w);
            w.wall_files = FILES.map(String::from).to_vec();
            w.wall_dirs = DIRS.map(|(f, n)| (f.to_string(), n)).to_vec();
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-slides-tick", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(SLIDES);
            slides(w);
            w.wall_files = FILES.map(String::from).to_vec();
            w.wall_dirs = DIRS.map(|(f, n)| (f.to_string(), n)).to_vec();
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-slides-screen", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(SLIDES);
            slides(w);
            portrait(w);
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-slides-gone", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(SLIDES);
            slides(w);
            w.pics.view.waiting = vec![crate::ws::views::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
            w.wall_screen = Some("LG-2".into());
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-slides-cant", Some(Section::Workspaces), |w| {
            settings::store::use_for_test("workspaces = 5
[wallpaper]
dim = 30
ws1 = \"beach\"
ws2 = \"trips\"
ws3 = \"scans\"
ws4 = [\"a.jpg\", \"b.jpg\", \"c.jpg\"]
ws5 = [\"old 1.png\", \"old 2.png\"]
[wallpaper.intervals]
ws3 = 10
");
            use crate::wallpaper::Problem;
            w.pics.wall_problems = vec![("beach".into(), Problem::Missing), ("trips".into(), Problem::Empty), ("scans".into(), Problem::Unreadable), ("a.jpg/b.jpg/c.jpg".into(), Problem::Missing), ("old 1.png/old 2.png".into(), Problem::Unreadable)];
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-screen", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            portrait(w);
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-menu", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            w.wall_files = FILES.map(String::from).to_vec();
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-menu-screen", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            portrait(w);
            w.wall_files = FILES.map(String::from).to_vec();
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-empty", Some(Section::Workspaces), |w| w.scroll = DOWN_TO_WALL),
        ("wallpaper-many", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            let many = ["alps.jpg", "aurora.png", "beach 2.jpg", "canyon.jpg", "city.jpg", "desk.png", "dunes.jpg", "fjord.jpg", "forest.jpg", "glacier.png", "harbor.jpg", "lake at dawn.PNG", "meadow.jpg", "moon.png"];
            w.wall_files = many.map(String::from).to_vec();
            w.wall_top = 3;
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-cant-show", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            w.pics.wall_problems = vec![("forest.jpg".into(), crate::wallpaper::Problem::Missing), ("desk.png".into(), crate::wallpaper::Problem::Undecodable)];
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-cant-draw", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            w.pics.wall_failed = true;
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-fit", Some(Section::Workspaces), |w| {
            settings::store::use_for_test("fastest = true
[wallpaper]
fit = \"center\"
transition = \"fade\"
ws1 = \"forest.jpg\"
ws2 = \"desk.png\"
");
            w.focus = Some(Ctl::Stepper("wallpaper.dim"));
            w.scroll = DOWN_TO_WALL + 120.0;
        }),
        ("wallpaper-chooser", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            w.pics.view.waiting = vec![crate::ws::views::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
            w.scroll = DOWN_TO_WALL;
        }),
        ("wallpaper-one", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(WALL);
            w.pics.screens.truncate(1);
            w.pics.view.present.truncate(1);
            w.pics.view.names.truncate(1);
            w.scroll = DOWN_TO_WALL - 200.0;
        }),
        ("apps-add-many", Some(Section::Apps), |w| {
            settings::store::use_for_test(APPS);
            fake_apps(w, false);
            let more: Vec<crate::apps::Seen> = (1..=27).map(|i| crate::apps::Seen { key: format!("app{i}.exe"), name: format!("App number {i}"), ..w.pics.apps[1].clone() }).collect();
            w.pics.apps.extend(more);
        }),
        ("apps", Some(Section::Apps), |w| {
            settings::store::use_for_test(APPS);
            fake_apps(w, false);
        }),
        ("apps-notes", Some(Section::Apps), |w| {
            settings::store::use_for_test("[apps]
\"spotify.exe\" = { workspace = 6 }
\"ms-teams.exe\" = { workspace = 2, screen = \"LG-2\" }
\"notepad.exe\" = { workspace = 1, screen = \"GSM-5B7F-12345\" }
\"olk.exe\" = { workspace = 1, screen = \"internal\" }
");
            fake_apps(w, false);
            w.pics.view.waiting = vec![crate::ws::views::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
            w.hover = Some(Ctl::AppX(3));
        }),
        ("apps-add", Some(Section::Apps), |w| {
            settings::store::use_for_test(APPS);
            fake_apps(w, false);
        }),
        ("apps-add-none", Some(Section::Apps), |w| {
            settings::store::use_for_test(APPS);
            fake_apps(w, true);
        }),
        ("apps-ws", Some(Section::Apps), |w| {
            settings::store::use_for_test(APPS);
            fake_apps(w, false);
        }),
        ("apps-screen", Some(Section::Apps), |w| {
            settings::store::use_for_test(APPS);
            fake_apps(w, false);
            w.pics.view.waiting = vec![crate::ws::views::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
        }),
        ("apps-empty", Some(Section::Apps), |w| fake_apps(w, false)),
        ("colors", Some(Section::Colors), |_| {}),
        ("colors-own", Some(Section::Colors), |w| {
            settings::store::use_for_test("[colors]\naccent = \"#E0457B\"\nws2 = \"#FFF4B0\"\nws3 = \"blue\"\n");
            w.focus = Some(Ctl::Hex(ACCENT));
            w.hexes = vec![Hex { key: ACCENT, digits: "2E8B5".into(), caret: 5, anchor: 5, line: None }];
            w.own = Some(0x2E8B57);
        }),
        ("place", Some(Section::Indicator), |w| {
            settings::store::use_for_test("[indicator]\nspot = \"top-right\"\n");
            w.scroll = 470.0;
            w.focus = Some(Ctl::Picture);
        }),
        ("place-screen", Some(Section::Indicator), |w| {
            settings::store::use_for_test("[indicator]\nspot = \"top-right\"\n[indicator.screens.\"DEL-A0F5-1234\"]\nspot = \"custom\"\nx = 70\ny = 120\n");
            w.screen = Some("DEL-A0F5-1234".into());
            w.scroll = 470.0;
        }),
        ("top", None, |_| {}),
        ("removing", None, |w| {
            w.hover = Some(Ctl::TileRemove);
            w.scroll = 140.0;
        }),
        ("clash", Some(Section::Shortcuts), |w| {
            let held = vec!["Win".to_string(), "Tab".to_string()];
            let sub = "“Open overview” uses it. Move it here and “Open overview” loses this key. Cancel changes nothing.".into();
            w.prompt = Some(Prompt { key: "shortcuts.finder", held, title: "Win Tab is already in use".into(), sub, edit: Edit::Gestures(vec![]) });
            w.focus = Some(Ctl::Cancel("shortcuts.finder"));
            w.scroll = 300.0;
        }),
        ("menu", Some(Section::Touchpad), |_| {}),
        ("keys", Some(Section::Shortcuts), |_| {}),
        ("indicator", Some(Section::Indicator), |_| {}),
        ("attention", Some(Section::Attention), |_| {}),
        ("attention-own", Some(Section::Attention), |w| {
            settings::store::use_for_test("[attention]\ntext = \"{{AppName}} has something for you\"\n[indicator]\nstyle = \"dots\"\n");
            w.focus = Some(Ctl::Words);
        }),
        ("attention-off", Some(Section::Attention), |_| settings::store::use_for_test("[attention]\nenabled = false\n")),
        ("overview", Some(Section::Overview), |w| w.focus = Some(Ctl::Stepper("overview.fade_ms"))),
        ("motion", Some(Section::Motion), |w| w.focus = Some(Ctl::Toggle("fastest"))),
        ("touchpad", Some(Section::Touchpad), |_| {}),
        // The color picker (spec 0021): the tile and picture cues, then the card in its states.
        ("tile-hover", None, |w| {
            w.hover = Some(Ctl::WsTile(1, 2));
            w.kb = false;
        }),
        ("overview-slots", Some(Section::Overview), |w| {
            settings::store::use_for_test("workspaces = 3\n");
            w.hover = Some(Ctl::ShowTile(0, 1));
            w.kb = false;
        }),
        ("pick-tile", None, |w| {
            w.pick = Some(picking("colors.ws2", Anchor::Tile("DISPLAY2".into(), 1), 0xDDB43C));
            w.kb = false;
        }),
        ("pick-swatch", Some(Section::Colors), |w| {
            // Workspace 3 moved to workspace 1's blue: the header shows start and now, and the duplicate line says so.
            let mut p = picking("colors.ws3", Anchor::Swatch("colors.ws3"), 0x79A4F3);
            (p.focus, p.from) = (Stop::Swatches(0), Some(Ctl::Swatch("colors.ws3")));
            w.pick = Some(p);
            w.focus = Some(Ctl::Swatch("colors.ws3"));
        }),
        ("pick-close", Some(Section::Colors), |w| {
            let mut p = picking("colors.ws3", Anchor::Swatch("colors.ws3"), 0x7AA5F2);
            p.focus = Stop::Square;
            w.pick = Some(p);
        }),
        ("pick-accent", Some(Section::Colors), |w| {
            settings::store::use_for_test("[colors]\naccent = \"#E0457B\"\n");
            let mut p = picking(ACCENT, Anchor::Swatch(ACCENT), 0x2E8B57);
            p.focus = Stop::Done;
            w.pick = Some(p);
        }),
        ("pick-failed", Some(Section::Colors), |w| {
            let mut p = picking("colors.ws2", Anchor::Swatch("colors.ws2"), 0xDDB43C);
            (p.failed, p.focus) = (Some("os error 32".into()), Stop::Cancel);
            w.pick = Some(p);
        }),
        ("pick-notes", None, |w| {
            // Both notes in the smallest window: the card opens over them, beside the tiles.
            w.vh = MIN_H;
            w.error = Some(("Workspace 4 color".into(), "os error 32".into()));
            w.edited = Some(("14:02".into(), vec!["Indicator stays: 550 → 800 ms".into()]));
            w.pick = Some(picking("colors.ws1", Anchor::Tile("DISPLAY1".into(), 0), 0x79A4F3));
            w.kb = false;
        }),
        ("pick-ring", None, |w| {
            let mut p = picking("colors.ws4", Anchor::Tile("DISPLAY2".into(), 3), 0x2E8B57);
            p.focus = Stop::Ring;
            w.pick = Some(p);
        }),
        ("pick-preset", None, |w| {
            let mut p = picking("colors.ws2", Anchor::Tile("DISPLAY2".into(), 1), 0x92BF4F);
            (p.focus, p.last_swatch) = (Stop::Swatches(10), 10);
            w.pick = Some(p);
            w.hover = Some(Ctl::Preset(4));
        }),
        ("pick-overview", Some(Section::Overview), |w| {
            w.pick = Some(picking("colors.ws2", Anchor::Show("DISPLAY1".into(), 1), 0xDDB43C));
            w.kb = false;
        }),
        // Spec 0023: the Names card in each of its states, then the Indicator's.
        ("names", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-empty", Some(Section::Workspaces), |w| w.scroll = DOWN_TO_NAMES),
        ("names-screen", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            w.pics.view.present[0].name = "Left".into();
            w.names_screen = Some("internal".into());
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-follow", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            w.pics.view.present[0].name = "Left".into();
            (w.names_screen, w.focus) = (Some("internal".into()), Some(Ctl::Name(Nm::Ws(0))));
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-own-emptied", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            w.pics.view.present[0].name = "Left".into();
            (w.names_screen, w.focus, w.typed) = (Some("internal".into()), Some(Ctl::Name(Nm::Ws(1))), Some(String::new()));
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-all-emptied", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            (w.focus, w.typed) = (Some(Ctl::Name(Nm::Ws(0))), Some(String::new()));
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-typing", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            (w.focus, w.typed) = (Some(Ctl::Name(Nm::Ws(2))), Some("Ideas 💡".into()));
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-failed", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            w.unsaved = vec![(None, Nm::Ws(1), "Codex".into())];
            w.error = Some(("Workspace 2 name".into(), "os error 32".into()));
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-one", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            w.pics.screens.truncate(1);
            w.pics.view.present.truncate(1);
            w.pics.view.names.truncate(1);
            w.scroll = DOWN_TO_NAMES - 120.0;
        }),
        ("names-menu", Some(Section::Workspaces), |w| {
            settings::store::use_for_test("[names.screens.internal]
screen = \"Desk\"
[names.screens.\"DEL-A0F5-1234\"]
screen = \"Desk\"
");
            for p in w.pics.view.present.iter_mut() {
                p.name = "Desk".into();
            }
            w.pics.view.waiting = vec![crate::ws::views::Waiting { id: "LG-2".into(), name: "LG 27UL850".into(), windows_name: "LG 27UL850".into(), windows: vec![] }];
            w.scroll = DOWN_TO_NAMES;
        }),
        ("names-readonly", Some(Section::Workspaces), |w| {
            settings::store::use_for_test(NAMES);
            settings::store::broken_for_test("expected `=`, found newline at line 3 column 9");
            w.names_screen = Some("internal".into());
            w.scroll = DOWN_TO_NAMES;
        }),
        ("indicator-names", Some(Section::Indicator), |_| settings::store::use_for_test(NAMES)),
        ("indicator-no-screen", Some(Section::Indicator), |w| {
            settings::store::use_for_test(&format!("{NAMES}[indicator]
screen_name = false
"));
            w.scroll = 200.0;
        }),
        ("indicator-dots", Some(Section::Indicator), |w| {
            settings::store::use_for_test("[indicator]
style = \"dots\"
");
            w.scroll = 200.0;
        }),
        ("indicator-off", Some(Section::Indicator), |w| {
            settings::store::use_for_test("[indicator]
enabled = false
");
            w.scroll = 200.0;
        }),
        ("attention-names", Some(Section::Attention), |_| settings::store::use_for_test(NAMES)),
    ];
    for (sname, sec, setup) in &states {
        if sname.starts_with("pick") && skip_picks {
            continue;
        }
        let mut win = fake_win(theme, scale);
        settings::store::use_for_test("");
        setup(&mut win);
        fake_wall(&win);
        let out = measure(&win);
        if let Some(top) = out.tops.iter().find(|(s, _)| *s == *sec).map(|(_, t)| *t) {
            win.scroll = (top - 8.0 + win.scroll).clamp(0.0, (out.height - (win.vh - out.fixed)).max(0.0));
        }
        let out = measure(&win);
        take(&mut win, out);
        if *sname == "menu"
            && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::Picker(2)).map(|h| h.at)
        {
            win.menu = Some(Menu { row: 2, times: 1, family: None, at, focus: 0 });
        }
        if *sname == "names-menu"
            && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::NameScreens).map(|h| h.at)
        {
            win.chooser = Some((Ctl::NameScreens, at, 2));
        }
        let apps_menu = match *sname {
            "apps-add" | "apps-add-none" | "apps-add-many" => Some((Ctl::AppAdd, 0)),
            "apps-ws" => Some((Ctl::AppWs(0), 1)),
            "apps-screen" => Some((Ctl::AppScreen(0), 3)),
            "wallpaper-menu" | "wallpaper-many" => Some((Ctl::WallPick(0), 5)),
            "wallpaper-menu-screen" => Some((Ctl::WallPick(1), 1)),
            "wallpaper-empty" => Some((Ctl::WallPick(0), 1)),
            "wallpaper-chooser" => Some((Ctl::WallScreens, 2)),
            "wallpaper-slides-folders" => Some((Ctl::WallPick(1), 7)),
            "wallpaper-slides-tick" => Some((Ctl::WallPick(2), 5)),
            _ => None,
        };
        if let Some((from, focus)) = apps_menu
            && let Some(at) = win.hits.iter().find(|h| h.ctl == from).map(|h| h.at)
        {
            win.chooser = Some((from, at, focus));
        }
        // An open picker: placed as on open, the page scrolled at once, the preview in place.
        if let Some((anchor, levels, beside)) = win.pick.as_ref().and_then(|p| levels_of(&p.anchor, &win.geo).map(|(a, l)| (a, l, !matches!(p.anchor, Anchor::Swatch(_))))) {
            let (at, d) = place(&room_of(&win), &levels, anchor, beside, (PICK_W, PICK_H), true);
            win.scroll += d;
            let p = win.pick.as_mut().unwrap();
            p.placed = Some(at);
            let shown = ws_index(p.key).map_or(theme.b.acc, |i| theme.b.ws[i]);
            p.preview = (shown != p.color).then(|| ui::theme_preview(!theme.dark, ui::tests::plain(), ws_index(p.key), p.color));
            let out = measure(&win);
            take(&mut win, out);
        }
        f(sname, &win);
    }
}

// Not a check that can fail: draws the page in the design's states into
// target/settings/*.png, to compare by eye with the approved captures (AC-1). Run with
// `cargo test render_settings -- --ignored`.
#[test]
#[ignore]
fn render_settings() {
    assert!(ui::canvas::gdiplus());
    let _one = SETTINGS.lock();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("settings");
    std::fs::create_dir_all(&dir).unwrap();
    let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let runs: Vec<(&str, Theme, f32)> = vec![("light", ui::LIGHT, 1.0), ("dark", ui::DARK, 1.0), ("hc", night_sky, 1.0), ("light-150", ui::LIGHT, 1.5), ("rose", ui::tests::rose(true), 1.0), ("rose-dark", ui::tests::rose(false), 1.0)];
    for (tname, theme, scale) in &runs {
        // The picker opens in neither high contrast nor (for brevity) the rose accent.
        each_state(theme.hc || tname.starts_with("rose"), *theme, *scale, &mut |sname, win| {
            let (pw, ph) = ((W * scale).round() as i32, (win.vh * scale).round() as i32);
            let t = painted(win);
            ui::tests::save_png(pw, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                let mut c = Canvas::new(dc, true, *scale, t, &win.fonts);
                c.boxed((0.0, 0.0, W + 1.0, win.vh + 1.0), 0.0, t.b.bg, None);
                compose(&mut c, win);
            });
        });
    }
}

// The click areas and what Narrator reads in every render state, kept from before the controls
// merge so the merge can't change them (spec 0035 AC-7). Run by hand, as the PNGs are:
// `cargo test snapshot -- --ignored`.
#[test]
#[ignore]
fn snapshot_settings() {
    use std::fmt::Write;
    assert!(ui::canvas::gdiplus());
    let _one = SETTINGS.lock();
    let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let mut got = String::new();
    for (tname, theme) in [("light", ui::LIGHT), ("hc", night_sky)] {
        each_state(theme.hc, theme, 1.0, &mut |sname, win| {
            let out = measure(win);
            let _ = writeln!(got, "== {sname} {tname}");
            for h in &out.hits {
                let _ = writeln!(got, "hit {:?} {} {}", h.ctl, h.group, crate::snapshot::bx(h.at));
            }
            for (ctl, n) in &out.said {
                let _ = writeln!(got, "say {ctl:?} {}", crate::snapshot::node(n));
            }
        });
    }
    crate::snapshot::check("settings_page", &got);
}
