// The hints window's render test, and the settings and touchpad states its tests share.
use super::*;
use super::rows::read;
use crate::settings::{ALT, CTRL, Gesture, WIN};
use crate::setup::Wrote;
use crate::ui::{DARK, LIGHT};

pub(super) fn c(mods: u8, vk: u16) -> Combo {
    Combo { mods, vk }
}

pub(super) fn st() -> State {
    State { pad: true, enabled: true, rows: [PadRow::Nothing; 4], off: true, saved: true }
}

/// The design's own setup (frame 4): two combos for switching, a split send pair, Win 1–9
/// given back, Find a window on Win Alt F, a twice action, Nothing, a typed combo, a workspace.
pub(super) fn mine() -> Settings {
    let mut s = Settings::default();
    s.previous.push(c(CTRL | ALT, VK_LEFT.0));
    s.next.push(c(CTRL | ALT, VK_RIGHT.0));
    s.send_previous.clear();
    s.jump = None;
    s.finder = vec![c(WIN | ALT, b'F' as u16)];
    s.copilot_as_right_ctrl = false;
    s.hints = vec![c(WIN | CTRL, b'H' as u16)];
    let set = |s: &mut Settings, f, m, t, a| {
        s.gestures.retain(|g: &Gesture| (g.fingers, g.motion, g.times) != (f, m, t));
        s.gestures.push(Gesture { fingers: f, motion: m, times: t, action: a });
    };
    set(&mut s, 4, Motion::Up, 2, Action::Finder);
    set(&mut s, 3, Motion::Down, 1, Action::Nothing);
    set(&mut s, 3, Motion::Tap, 1, Action::Combo(c(CTRL | ALT, b'T' as u16)));
    set(&mut s, 4, Motion::Tap, 1, Action::Jump(2));
    s
}

/// A render state: its name, the settings, Windows' touchpad, a give back, the recorder, the height.
type Shot<'a> = (&'a str, &'a Settings, State, Option<GaveBack>, Rec, f32);

/// Draws the design's states to target/hints/ PNGs, to compare by eye with
/// pdesk-design/restart/captures/hr3-* (spec 0017 AC-1, AC-HNT-1 to AC-HNT-5).
#[test]
#[ignore]
fn render() {
    ui::canvas::gdiplus();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("hints");
    std::fs::create_dir_all(&dir).unwrap();
    let d = Settings::default();
    let keyed = Settings { hints: vec![c(WIN | CTRL, b'H' as u16)], ..Settings::default() };
    let rec = Rec::Recording(None);
    let clash = recorded_rec(&d, c(WIN, b'F' as u16)).unwrap();
    let rows = |r: [PadRow; 4]| State { rows: r, ..st() };
    let wait = rows([PadRow::StillOn, PadRow::Nothing, PadRow::StillOn, PadRow::Nothing]);
    let cant = rows([PadRow::Nothing, PadRow::CantRead, PadRow::Nothing, PadRow::Nothing]);
    let back = Some(GaveBack { wrote: [Wrote::All; 4], cleared: true });
    let my = mine();
    // Every line under the keyboard cards: the rule, the remote app line and the Copilot line (spec 0038).
    let all = Settings { copilot_as_right_ctrl: true, act_on: crate::settings::ActOn::Focus, ..Settings::default() };
    let states: Vec<Shot> = vec![
        ("defaults", &d, st(), None, Rec::Idle, H),
        ("key-set", &keyed, st(), None, Rec::Idle, H),
        ("recording", &d, st(), None, rec, H),
        ("clash", &d, st(), None, clash, H),
        ("mine", &my, st(), None, Rec::Idle, H),
        ("still-on", &d, wait, None, Rec::Idle, H),
        ("cant-read", &d, cant, None, Rec::Idle, H),
        ("given-back", &d, State { rows: [PadRow::StillOn; 4], off: false, saved: false, ..st() }, back, Rec::Idle, H),
        ("reading-off", &d, State { enabled: false, ..st() }, None, Rec::Idle, H),
        ("reading-off-windows", &d, State { enabled: false, off: false, ..st() }, None, Rec::Idle, H),
        ("no-touchpad", &d, State { pad: false, ..st() }, None, Rec::Idle, H),
        ("short", &d, st(), None, Rec::Idle, 528.0),
        ("tall", &all, st(), None, Rec::Idle, 860.0),
    ];
    let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
    let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
    let themes: [(&str, Theme, f32); 5] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5)];
    for (tname, t, scale) in themes {
        let fonts = ui::fonts::fonts(scale);
        for (name, s, state, back, rec, height) in &states {
            let v = read(s, state, *back);
            let (w, h) = ((W * scale).round() as i32, (height * scale).round() as i32);
            ui::tests::save_png(w, h, &dir.join(format!("{name}-{tname}.png")), |dc| {
                let c = Canvas::new(dc, true, scale, t, &fonts);
                let mut places = vec![];
                compose(&c, &mut places, &v, rec, None, *height, 0.0);
                // The buttons are windows of their own: drawn here where they sit.
                for p in &places {
                    let focus = p.id == ID_CANCEL && matches!(rec, Rec::Clash { .. });
                    c.fill_rr((p.bx.0 - RING, p.bx.1 - RING, p.bx.2 + 2.0 * RING, p.bx.3 + 2.0 * RING), 0.0, op(p.back));
                    paint_button(&c, p, p.bx, false, focus);
                }
            });
        }
    }
}
