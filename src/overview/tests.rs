// The overview's session tests: the close note, the speed line, the bar per screen, the
// refresh plan and the render states, plus the screen helpers the other tests share.
use super::*;
use super::drag::Aim;
use super::draw::{draw_x, paint_screen};
use super::layout::{Slot, Tile, intersect, place_in, spread, strip};
use crate::ui::{DARK, LIGHT, fonts::Fonts};
use windows::core::Error;

pub(super) fn screen(w: i32, h: i32, scale: f32, shown: usize, workspaces: usize) -> Screen {
    let rect = RECT { left: 0, top: 0, right: w, bottom: h };
    let tiles = strip(w, h, workspaces, scale).into_iter().map(|rect| Tile { rect, minis: Vec::new(), name: String::new(), file: None, picture: None }).collect();
    Screen { device: "D".into(), popup: HWND::default(), rect, scale, shown, tiles, slots: Vec::new(), cell_w: 0, labels: Labels::Hover, face: Face::Card, close_btn: HWND::default(), note: None, painted: false, tree: Tree::default(), wants: Vec::new() }
}

// AC-OVR-8: "admin" is a claim: say it only when Windows refused for that reason, else a
// note blames the wrong thing.
#[test]
fn only_access_denied_says_admin() {
    assert_eq!(close_note(&Error::from_hresult(ERROR_ACCESS_DENIED.to_hresult())), Note::CantCloseAdmin);
    assert_eq!(close_note(&Error::from_hresult(windows::Win32::Foundation::E_FAIL)), Note::CantClose);
    assert_eq!(Note::CantCloseAdmin.words().1, "Can't close an admin window");
    assert_eq!(Note::CantClose.words().1, "Can't close this window");
    // The come back note is information, not a warning: nothing failed.
    assert!(!Note::DidntClose.words().3 && Note::CantMove.words().3);
}

// AC-8: every open says how long it took, and one over the limit must be easy to find.
#[test]
fn the_speed_line_flags_an_open_over_the_limit() {
    assert_eq!(speed_line(3, 100), "overview: shown on 3 screens in 100 ms");
    assert_eq!(speed_line(3, 101), "overview: shown on 3 screens in 101 ms (over the 100 ms limit)");
}

// AC-OVR-7: the keys only on the screen the keys act on, a note on its own screen, and
// "Drop on a workspace" while dragging.
#[test]
fn the_bar_shows_on_the_selected_screen_and_a_note_on_its_own() {
    let mut s = Session {
        screens: vec![screen(1280, 720, 1.0, 0, 4), screen(1280, 720, 1.0, 0, 4)],
        came_from: HWND::default(),
        hover: None,
        tile_hover: None,
        selected: (1, None),
        closing: Vec::new(),
        drag: None,
        seen: Vec::new(),
        fading_in: None,
        alpha: 255,
        asked: 0,
        logged: false,
        wall_gen: 0,
    };
    assert_eq!((s.bar(0), s.bar(1)), (None, Some(Bar::Keys)));
    s.screens[0].note = Some(Note::CantMove);
    assert_eq!((s.bar(0), s.bar(1)), (Some(Bar::Note(Note::CantMove)), Some(Bar::Keys)));
    s.drag = Some(Drag { hwnd: HWND::default(), from: ("D".into(), 0), start: POINT::default(), scale: 1.0, stage: Stage::Cancelled, aim: None });
    assert_eq!(s.bar(1), Some(Bar::Keys), "a press that is not moving says nothing new");
}

// AC-10: live rebuilds only when the list really changed, since a rebuild registers every
// preview again and would blink four times a second; not live drops just what went and
// never reshuffles the rest, which is the whole point of turning live off.
#[test]
fn a_refresh_rebuilds_only_on_a_change_and_not_live_only_drops() {
    let h = |n: usize| HWND(std::ptr::without_provenance_mut(n));
    let w = |n: usize, ws: usize, on: bool| (h(n), "D".to_string(), ws, on);
    let before = [w(1, 0, true), w(2, 1, false), w(3, 0, true)];
    assert_eq!(refresh_plan(true, &before, &before), Refresh::Nothing);
    let moved = [w(1, 0, true), w(2, 2, false), w(3, 0, true)];
    assert_eq!(refresh_plan(true, &before, &moved), Refresh::Rebuild);
    let opened = [w(4, 0, true), w(1, 0, true), w(2, 1, false), w(3, 0, true)];
    assert_eq!(refresh_plan(true, &before, &opened), Refresh::Rebuild);
    assert_eq!(refresh_plan(false, &before, &opened), Refresh::Nothing, "new windows wait for the next open");
    assert_eq!(refresh_plan(false, &before, &moved), Refresh::Nothing, "the rest stay put");
    let went = [w(4, 0, true), w(3, 0, true)];
    assert_eq!(refresh_plan(false, &before, &went), Refresh::Drop(vec![h(1), h(2)]));
}

// The design's Built-in display: 1280 by 720 dip, its windows and where they sit (dip).
pub(super) fn design_windows() -> Vec<(&'static str, usize, RECT, bool)> {
    let r = |l: i32, t: i32, rr: i32, b: i32| RECT { left: l, top: t, right: rr, bottom: b };
    vec![
        ("Administrator: PowerShell", 0, r(0, 0, 1280, 672), false),
        ("Settings", 0, r(180, 70, 900, 610), false),
        ("des-k issues · GitHub - Google Chrome", 1, r(0, 0, 760, 672), false),
        ("overview.rs - des-k - Visual Studio Code", 1, r(600, 0, 1280, 672), false),
        ("Downloads - File Explorer", 1, r(120, 300, 660, 640), false),
        ("notes.txt - Notepad", 1, r(880, 360, 1240, 660), false),
        ("Spotify Premium", 1, r(240, 80, 1060, 600), false),
        ("Inbox - Outlook", 2, r(80, 30, 1200, 660), false),
    ]
}

/// A screen laid out as `build` would, from stand in windows: (title, workspace, real
/// rect in dip, pinned). `cards` draw nothing for Windows.
pub(super) fn laid_out(scale: f32, shown: usize, workspaces: usize, wins: &[(&str, usize, RECT, bool)], cards: &[&str], labels: Labels) -> Screen {
    let (w, h) = ((1280.0 * scale) as i32, (720.0 * scale) as i32);
    let mut sc = screen(w, h, scale, shown, workspaces);
    let px = |r: RECT| RECT { left: (r.left as f32 * scale) as i32, top: (r.top as f32 * scale) as i32, right: (r.right as f32 * scale) as i32, bottom: (r.bottom as f32 * scale) as i32 };
    let hwnd = |i: usize| HWND(std::ptr::without_provenance_mut(i + 10));
    for (i, (_, ws, r, pinned)) in wins.iter().enumerate() {
        for (k, t) in sc.tiles.iter_mut().enumerate() {
            if (*pinned && k == shown) || (!pinned && *ws == k) {
                let full = place_in(px(*r), sc.rect, t.rect);
                if let Some(rect) = intersect(full, t.rect) {
                    t.minis.push(Slot { hwnd: hwnd(i), rect, title: String::new(), icon: HICON::default(), pinned: false, card: cards.contains(&wins[i].0), thumb: None });
                }
            }
        }
    }
    let mine: Vec<usize> = (0..wins.len()).filter(|&i| wins[i].1 == shown || wins[i].3).collect();
    let band = if labels == Labels::None { 0 } else { sc.px(LABEL) };
    let (rects, cell_w) = spread(sc.spread_area(), &mine.iter().map(|&i| px(wins[i].2)).collect::<Vec<_>>(), sc.px(SLOT_GAP), band);
    sc.cell_w = cell_w;
    sc.labels = labels;
    sc.slots = mine.iter().zip(rects).map(|(&i, rect)| Slot { hwnd: hwnd(i), rect, title: wins[i].0.into(), icon: HICON::default(), pinned: wins[i].3, card: cards.contains(&wins[i].0), thumb: None }).collect();
    sc
}

/// `paint_screen` into a fresh w by h buffer whose top left is popup pixel `at`; its pixels.
pub(super) fn pixels(sc: &Screen, m: &Marks, t: Theme, (w, h): (i32, i32), at: (i32, i32)) -> Vec<u32> {
    unsafe {
        let mem = CreateCompatibleDC(None);
        let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
        let old = SelectObject(mem, bmp.into());
        paint_screen(mem, (w, h), sc, m, at, t);
        let _ = GdiFlush();
        let out = std::slice::from_raw_parts(bits.cast::<u32>(), (w * h) as usize).to_vec();
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        out
    }
}

/// Draws the states the design shows to target/overview/ PNGs, to compare by eye with
/// pdesk-design/restart/captures/ovr3-* (spec 0017 AC-1). DWM draws the live previews and
/// the wallpaper cannot be read here, so both are flat stand ins.
#[test]
#[ignore]
fn render() {
    ui::canvas::gdiplus();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("overview");
    std::fs::create_dir_all(&dir).unwrap();
    let code = HWND(std::ptr::without_provenance_mut(13));
    let web = HWND(std::ptr::without_provenance_mut(12));
    let note = HWND(std::ptr::without_provenance_mut(15));
    let mut many = design_windows();
    for (i, t) in ["Pull requests · des-k - Google Chrome", "des-k - File Explorer", "todo.txt - Notepad", "Calendar", "Photos"].iter().enumerate() {
        many.push((t, 1, RECT { left: 60 * i as i32, top: 40, right: 60 * i as i32 + 700, bottom: 560 }, false));
    }
    let mut pinned = design_windows();
    pinned.push(("Chat | Microsoft Teams", 0, RECT { left: 700, top: 120, right: 1240, bottom: 600 }, true));
    pinned.push(("Calculator", 1, RECT { left: 960, top: 60, right: 1280, bottom: 560 }, false));
    let one = vec![design_windows()[3]];
    let three = design_windows().into_iter().filter(|w| w.1 == 1).take(3).collect::<Vec<_>>();
    type Set = (Vec<(&'static str, usize, RECT, bool)>, usize, usize, Labels, &'static [&'static str]);
    let states: Vec<(&str, Set, Marks)> = vec![
        ("opened", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("hover", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), hover: Some(web), bar: Some(Bar::Keys), ..Marks::default() }),
        ("always", (design_windows(), 1, 4, Labels::Always, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("none", (design_windows(), 1, 4, Labels::None, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("tile-hover", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), tile_hover: Some(2), bar: Some(Bar::Keys), ..Marks::default() }),
        ("empty", (design_windows(), 3, 4, Labels::Hover, &[]), Marks { bar: Some(Bar::Keys), ..Marks::default() }),
        ("nine", (design_windows(), 1, 9, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("pinned-card", (pinned, 1, 4, Labels::Always, &["Calculator"]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("one", (one, 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("three", (three, 1, 4, Labels::Hover, &[]), Marks { selected: Some(web), bar: Some(Bar::Keys), ..Marks::default() }),
        ("twelve", (many, 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("drag-tile", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { src: Some(note), aim: Some(Aim::Tile(2)), bar: Some(Bar::Drag), ..Marks::default() }),
        ("drag-spread", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { aim: Some(Aim::Spread), bar: Some(Bar::Drag), ..Marks::default() }),
        ("cant-move", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Note(Note::CantMove)), ..Marks::default() }),
        ("cant-close-admin", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Note(Note::CantCloseAdmin)), ..Marks::default() }),
        ("cant-close", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Note(Note::CantClose)), ..Marks::default() }),
        ("didnt-close", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(note), bar: Some(Bar::Note(Note::DidntClose)), ..Marks::default() }),
        // Spec 0022: Outlook on 3 and the full screen PowerShell on 1 want you.
        ("wants", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("wants-drag", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { src: Some(note), aim: Some(Aim::Tile(2)), bar: Some(Bar::Drag), ..Marks::default() }),
        // Spec 0023: the handoff's o1 wide, o2 narrow, o4 marks under the chip.
        ("names", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("names-nine", (design_windows(), 1, 9, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
        ("names-wants", (design_windows(), 1, 4, Labels::Hover, &[]), Marks { selected: Some(code), bar: Some(Bar::Keys), ..Marks::default() }),
    ];
    let marked = |name: &str| if name.contains("wants") { vec![(0, HWND(std::ptr::without_provenance_mut(10))), (2, HWND(std::ptr::without_provenance_mut(17)))] } else { vec![] };
    let named = |name: &str| -> Vec<&str> {
        match name {
            "names-nine" => vec!["Mail", "Code", "Research and reading", "Music", "設計レビュー", "Chat 💬", "", "Admin and billing", "ألعاب الفيديو"],
            n if n.starts_with("names") => vec!["Mail", "Code", "", "Music"],
            _ => vec![],
        }
    };
    let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
    let themes: [(&str, Theme, f32, u32); 5] = [("light", LIGHT, 1.0, 0xCFD9EE), ("dark", DARK, 1.0, 0x1A2030), ("hc", aquatic, 1.0, 0x000000), ("desert", desert, 1.0, 0xFFFAEF), ("light-150", LIGHT, 1.5, 0xCFD9EE)];
    for (tname, t, scale, wall) in themes {
        for (name, (wins, shown, n, labels, cards), m) in &states {
            let mut sc = laid_out(scale, *shown, *n, wins, cards, *labels);
            sc.face = Face::Color(wall);
            sc.wants = marked(name);
            sc.tiles.iter_mut().zip(named(name)).for_each(|(t, n)| t.name = n.into());
            let (w, h) = sc.size();
            let path = dir.join(format!("{name}-{tname}.png"));
            ui::tests::save_png(w, h, &path, |dc| {
                paint_screen(dc, (w, h), &sc, m, (0, 0), t);
                // What DWM would draw: each live preview, as a flat stand in.
                let fonts = Fonts::new(scale);
                let c = Canvas::new(dc, true, scale, t, &fonts);
                let stand = if t.hc { 0x000000 } else if t.dark { 0x3A3F4A } else { 0xFFFFFF };
                for sl in sc.slots.iter().chain(sc.tiles.iter().flat_map(|t| &t.minis)).filter(|x| !x.card) {
                    let r = sl.rect;
                    let faded = m.src == Some(sl.hwnd) && !t.hc;
                    let color = if faded { mix(t.b.bg, stand, 0.4) } else { stand };
                    c.fill_rr((r.left as f32 / scale, r.top as f32 / scale, (r.right - r.left) as f32 / scale, (r.bottom - r.top) as f32 / scale), 0.0, op(color));
                    c.fill_rr((r.left as f32 / scale, r.top as f32 / scale, (r.right - r.left) as f32 / scale, ((r.bottom - r.top) as f32 / scale).min(12.0)), 0.0, op(mix(color, 0x808080, 0.25)));
                }
                // The hover X, its own popup on screen.
                if let Some(at) = m.hover.and_then(|h| sc.close_spot(h)) {
                    draw_x(&c, at.x as f32 / scale, at.y as f32 / scale, false);
                }
            });
        }
    }
}
