// The first run's render and snapshot tests, and the render states they draw.
use super::*;

/// Each render state, set up as the render test draws it: the step, the scroll, an open menu.
pub(super) fn each_state(theme: Theme, scale: f32, f: &mut dyn FnMut(&str, &Win)) {
    let r = |l, t, rr, b| RECT { left: l, top: t, right: rr, bottom: b };
    let screens = vec![
        crate::ws::ScreenInfo { device: "DISPLAY1".into(), number: 1, name: "Built-in display".into(), rect: r(0, 540, 1440, 1440), scale: 150 },
        crate::ws::ScreenInfo { device: "DISPLAY2".into(), number: 2, name: "DELL U2720Q".into(), rect: r(1440, 0, 4000, 1440), scale: 150 },
    ];
    let view = crate::ws::View {
        active: vec![("DISPLAY1".into(), 0), ("DISPLAY2".into(), 0)],
        windows: vec![("DISPLAY1".into(), 0, r(0, 0, 9, 9)), ("DISPLAY2".into(), 0, r(0, 0, 9, 9)), ("DISPLAY2".into(), 3, r(0, 0, 9, 9))],
        names: vec![("DISPLAY1".into(), "Built-in display".into()), ("DISPLAY2".into(), "DELL U2720Q".into())],
        removals: vec![vec![
            crate::ws::Moved { name: "DELL U2720Q".into(), device: Some("DISPLAY2".into()), unplugged: false, windows: 1, from: vec![3] },
            crate::ws::Moved { name: "Built-in display".into(), device: Some("DISPLAY1".into()), unplugged: false, windows: 0, from: vec![] },
        ]],
        waiting: vec![],
        present: vec![],
        open: vec![],
    };
    let (n, o) = (PadRow::Nothing, PadRow::StillOn);
    let pad = |rows: [PadRow; 4], saved| Some(PadState { pad: true, enabled: true, rows, off: rows == [n; 4], saved });
    type Setup = fn(&mut Win);
    let states: Vec<(&str, Setup)> = vec![
        ("f0-try", |_| {}),
        ("f1-switched", |w| {
            w.tried = [true, false, false];
            w.switched = Some(("DELL U2720Q".into(), 1, false));
            w.pics.view.active[1].1 = 1;
        }),
        ("f2-keys", |w| {
            w.step = Step::Keys;
            w.focus = Some(Ctl::Side(2));
        }),
        ("f3-behavior", |w| w.step = Step::Behavior),
        ("f4-pad-first", |w| {
            w.step = Step::Pad;
            w.pics.pad = Some(PadState { pad: true, enabled: true, rows: [PadRow::StillOn; 4], off: false, saved: false });
        }),
        ("f5-pad-saved", |w| {
            w.step = Step::Pad;
            w.pics.pad = Some(PadState { pad: true, enabled: true, rows: [PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing], off: false, saved: true });
        }),
        ("f6-pad-all", |w| w.step = Step::Pad),
        ("f7-done", |w| {
            w.step = Step::Done;
            w.pics.pad = Some(PadState { pad: true, enabled: true, rows: [PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing], off: false, saved: true });
        }),
        ("f8-keys-more", |w| {
            w.step = Step::Keys;
            w.more[1] = true;
            w.to_end = true;
        }),
        ("f10-keys-rec", |w| {
            w.step = Step::Keys;
            w.rec = Some(Rec::Digits("shortcuts.jump"));
            w.focus = Some(Ctl::Cancel);
        }),
        ("f12-keys-clash", |w| {
            w.step = Step::Keys;
            let held = vec!["Win".to_string(), "Ctrl".to_string(), "1–9".to_string()];
            let sub = "“Send window to 1–9 and stay” uses it. Move it here and that action has no key. Cancel changes nothing.".into();
            w.prompt = Some(Prompt { key: "shortcuts.jump", held, title: "Win Ctrl 1–9 is already in use".into(), sub, edit: Edit::Gestures(vec![]) });
            w.focus = Some(Ctl::Cancel);
        }),
        ("f14-behavior-more", |w| {
            w.step = Step::Behavior;
            w.more[2] = true;
            w.focus = Some(Ctl::Minus("workspaces"));
        }),
        ("r15-f0-behavior-more-end", |w| {
            w.step = Step::Behavior;
            w.more[2] = true;
            w.to_end = true;
        }),
        ("f15-try-all", |w| {
            w.tried = [true; 3];
            w.switched = Some(("DELL U2720Q".into(), 1, false));
        }),
        ("f24-pad-menu", |w| w.step = Step::Pad),
        ("f26-done-hints-rec", |w| {
            w.step = Step::Done;
            w.rec = Some(Rec::List("shortcuts.hints", None));
            w.focus = Some(Ctl::Cancel);
        }),
    ];
    for (sname, setup) in &states {
        let mut win = Win {
            hwnd: HWND::default(), scale, theme, fonts: Fonts::new(scale), step: Step::Try, more: [false; 5], tried: [false; 3], switched: None,
            scroll: 0.0, to_end: false, content: 0.0, mid: (0.0, 0.0), hits: vec![], focus: Some(Ctl::Next), kb: true, hover: None, press: None, vw: W, vh: SHORT, cap: TALL,
            grow: None, rec: None, refused: None, prompt: None, menu: None, error: None,
            pics: Pics { screens: screens.clone(), view: view.clone(), pad: pad([n; 4], true), mouse: "DISPLAY2".into() },
            tree: Arc::new(Mutex::new(Node::default())),
        };
        let _ = o;
        setup(&mut win);
        win.kb = win.focus != Some(Ctl::Next); // the ring shows after a key, so only where a state sets it
        win.vh = win.step.height();
        let measure = |win: &Win| unsafe {
            let dc = GetDC(None);
            let out = compose(&mut Canvas::new(dc, false, scale, theme, &win.fonts), win);
            ReleaseDC(None, dc);
            out
        };
        let out = measure(&win);
        let view_h = out.mid.1 - out.mid.0;
        if win.to_end {
            win.scroll = (out.content - view_h).max(0.0);
        } else if let Some(at) = out.hits.iter().find(|h| Some(h.ctl.stop()) == win.focus && h.group == 1).map(|h| h.at)
            && at.1 + at.3 + 12.0 > out.mid.1
        {
            win.scroll = (at.1 + at.3 + 12.0 - out.mid.1).min((out.content - view_h).max(0.0));
        }
        win.hits = measure(&win).hits;
        if *sname == "f24-pad-menu"
            && let Some(at) = win.hits.iter().find(|h| h.ctl == Ctl::Picker(settings::GESTURE_ROWS.iter().position(|r| *r == (3, Motion::Up)).unwrap())).map(|h| h.at)
        {
            let row = settings::GESTURE_ROWS.iter().position(|r| *r == (3, Motion::Up)).unwrap();
            win.menu = Some(Menu { row, times: 1, family: None, at, focus: 0 });
        }
        f(sname, &win);
    }
}

// Not a check that can fail: draws the design file's states offscreen into
// target/welcome/*.png, so the drawing can be compared with the approved design without
// opening the window. Run with `cargo test render -- --ignored`.
#[test]
#[ignore]
fn render_first_run() {
    assert!(ui::gdiplus());
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("welcome");
    std::fs::create_dir_all(&dir).unwrap();
    let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let runs: Vec<(&str, Theme, f32)> = vec![("light", ui::LIGHT, 1.0), ("dark", ui::DARK, 1.0), ("hc", night_sky, 1.0), ("light-150", ui::LIGHT, 1.5), ("rose", ui::tests::rose(true), 1.0), ("rose-dark", ui::tests::rose(false), 1.0)];
    for (tname, theme, scale) in &runs {
        each_state(*theme, *scale, &mut |sname, win| {
            let (pw, ph) = ((W * scale).round() as i32, (win.vh * scale).round() as i32);
            ui::tests::save_png(pw, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                let mut c = Canvas::new(dc, true, *scale, *theme, &win.fonts);
                c.boxed((0.0, 0.0, W + 1.0, win.vh + 1.0), 0.0, theme.b.bg, None);
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
fn snapshot_first_run() {
    use std::fmt::Write;
    assert!(ui::gdiplus());
    let _one = crate::settings_page::tests::SETTINGS.lock();
    settings::store::use_for_test("");
    let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let mut got = String::new();
    for (tname, theme) in [("light", ui::LIGHT), ("hc", night_sky)] {
        each_state(theme, 1.0, &mut |sname, win| {
            let out = unsafe {
                let dc = GetDC(None);
                let out = compose(&mut Canvas::new(dc, false, win.scale, win.theme, &win.fonts), win);
                ReleaseDC(None, dc);
                out
            };
            let _ = writeln!(got, "== {sname} {tname}");
            for h in &out.hits {
                let _ = writeln!(got, "hit {:?} {} {}", h.ctl, h.group, crate::snapshot::bx(h.at));
            }
            for (ctl, n) in &out.said {
                let _ = writeln!(got, "say {ctl:?} {}", crate::snapshot::node(n));
            }
            for n in &out.texts {
                let _ = writeln!(got, "text {}", crate::snapshot::node(n));
            }
        });
    }
    crate::snapshot::check("welcome", &got);
}
