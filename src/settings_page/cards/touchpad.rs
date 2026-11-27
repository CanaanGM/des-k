// The Touchpad section: Windows' setup notes and each gesture row with its picker (spec 0011,
// spec 0012).
use super::*;
use crate::settings_page::menu::{once_label, twice_label};
use crate::settings_page::recording::Rec;

impl Ctx<'_, '_> {
    // ---- Touchpad ----

    pub(super) fn touchpad(&mut self, y: f32) -> f32 {
        let st = self.w.pics.pad.unwrap_or(PadState { pad: true, enabled: self.s.touchpad_enabled, rows: [PadRow::Nothing; 4], off: true, saved: false });
        let back = self.w.pics.back;
        let (note, head, chips) = pad_state(&st, back);
        let mut h = 0.0;
        if let Some(n) = note {
            h += self.cx.note(X0, y, self.cw, &n) + 12.0;
        }
        for (g, fingers) in [(0usize, 3u8), (1, 4)] {
            h += self.card(y + h, |c, y| {
                let cap = format!("{head} · click an action to change it");
                let mut h = c.header(y, &format!("{fingers} fingers"), Some(fingers), &cap);
                for (k, tap) in [(0usize, false), (1, true)] {
                    if k > 0 {
                        c.sep(y + h);
                    }
                    h += c.gesture_row(y + h, fingers, tap, chips.map(|ch| ch[g * 2 + k]));
                }
                h
            }) + 12.0;
        }
        let give = st.saved && back.is_none_or(|b| b.failed() || !b.cleared) && !matches!(head, "Kept for a touchpad");
        h += self.card(y + h, |c, y| {
            let mut h = c.setting("touchpad.enabled", y);
            c.sep(y + h);
            h += c.setting("touchpad.swipe_mm", y + h);
            c.sep(y + h);
            h += c.setting("touchpad.twice_ms", y + h);
            if give {
                c.sep(y + h);
                let cap = [t("Windows uses its own 3- and 4-finger gestures again from your next sign-in. Until then, none work. Quit does the same.")];
                let bw = c.cx.btn_w("Give back", None);
                h += c.row(y + h, "Give back to Windows", &cap, (bw, 32.0), |c, x, y| { c.cx.button(Ctl::Pad(Pad::GiveBack), x, y, "Give back", None, false, 1); }, None);
            }
            h
        });
        h
    }

    /// One gesture row: Swipe or Tap, Windows' state for it, and its pickers.
    fn gesture_row(&mut self, y: f32, fingers: u8, tap: bool, chip: Option<Chip>) -> f32 {
        let b = self.cx.c.t.b;
        let motions: &[Motion] = if tap { &[Motion::Tap] } else { &[Motion::Left, Motion::Right, Motion::Up, Motion::Down] };
        let (x, px) = (X0 + 16.0, X0 + 16.0 + 116.0 + 16.0);
        let pw_all = X0 + self.cw - 16.0 - px;
        let cols = if tap { 1 } else { 2 };
        let pw = (pw_all - 8.0 * (cols as f32 - 1.0)) / cols as f32;
        let mut ph: f32 = 0.0;
        for (i, m) in motions.iter().enumerate() {
            let Some(row) = settings::GESTURE_ROWS.iter().position(|r| *r == (fingers, *m)) else { continue };
            let (cx, cy) = (px + (i % cols) as f32 * (pw + 8.0), y + 8.0 + (i / cols) as f32 * 48.0);
            let h = self.picker(row, cx, cy, pw);
            ph = ph.max(cy - y - 8.0 + h);
        }
        self.cx.c.text(x, y + 8.0 + 6.0, 20.0, if tap { "Tap" } else { "Swipe" }, F::Bold, b.tx);
        if let Some(ch) = chip {
            let (icon, text, col) = match ch {
                Chip::Ok => (Some(I_OK), "Off in Windows", b.good),
                Chip::On => (None, "Windows uses it", b.tx2),
                Chip::Err => (Some(I_WARN), "Can't read it", b.warn),
                Chip::Off => (None, "Off in Windows", b.tx2),
                Chip::Back => (None, "Back at next sign-in", b.tx2),
                Chip::Not => (Some(I_WARN), "Not written back", b.warn),
            };
            let mut cx = x;
            if let Some(i) = icon {
                self.cx.icon(cx, y + 36.0, 16.0, i, F::IconSmall, col);
                cx += 16.0;
            }
            self.cx.c.text(cx, y + 36.0, 16.0, text, F::Small, b.tx2);
        }
        8.0 + ph.max(52.0) + 8.0
    }

    /// A gesture picker: its direction mark, what it does, its twice action and default, ▾.
    fn picker(&mut self, row: usize, x: f32, y: f32, w: f32) -> f32 {
        let t = self.cx.c.t;
        let b = t.b;
        let (fingers, motion) = settings::GESTURE_ROWS[row];
        let s = self.s;
        let once = s.rule(fingers, motion, 1);
        let twice = s.rule(fingers, motion, 2);
        let def = DEFAULTS.rule(fingers, motion, 1);
        let ctl = Ctl::Picker(row);
        let open = self.w.menu.is_some_and(|m| m.row == row);
        let waiting = self.w.rec == Some(Rec::Gesture(row, 1)) || self.w.rec == Some(Rec::Gesture(row, 2));
        let text = if waiting { "Press the key combo".to_string() } else { once_label(once.unwrap_or(Action::Nothing)) };
        let mut sub = vec![];
        if twice.is_some() {
            sub.push(format!("Twice: {}", twice_label(twice)));
        }
        if once != def && !(once.is_none() && def == Some(Action::Nothing)) {
            sub.push(format!("Default: {}", once_label(def.unwrap_or(Action::Nothing))));
        }
        let sub = sub.join(" · ");
        let h = if sub.is_empty() { 32.0 } else { 40.0 };
        if open || waiting {
            self.cx.field((x, y, w, h), b.card, Some(b.acc));
        } else {
            let fill = if self.cx.hovered(ctl) && !self.cx.ro { mix(b.track, b.tx, 0.05) } else { b.track };
            self.cx.field((x, y, w, h), fill, None);
        }
        let glyph = match motion {
            Motion::Left => "\u{E72B}",
            Motion::Right => "\u{E72A}",
            Motion::Up => "\u{E74A}",
            Motion::Down => "\u{E74B}",
            Motion::Tap => "\u{E815}",
        };
        let my = y + (h - 22.0) / 2.0;
        if t.hc {
            self.cx.c.stroke_rr((x + 6.5, my + 0.5, 21.0, 21.0), 4.0, 1.0, op(b.acc));
        } else {
            self.cx.c.fill_rr((x + 6.0, my, 22.0, 22.0), 4.0, op(b.held));
        }
        let mark = if t.dark && !t.hc { mix(b.acc_b, 0xFFFFFF, 0.28) } else { b.acc_b };
        let gw = self.cx.c.width(glyph, F::IconTiny);
        self.cx.icon(x + 6.0 + (22.0 - gw) / 2.0, my, 22.0, glyph, F::IconTiny, mark);
        let lx = x + 36.0;
        let lw = w - 36.0 - 26.0;
        let ink = if self.cx.ro { b.tx3 } else { b.tx };
        if sub.is_empty() {
            let shown = self.cx.c.fit(&text, F::Body, lw);
            self.cx.c.text(lx, y, 32.0, &shown, F::Body, ink);
        } else {
            let shown = self.cx.c.fit(&text, F::Body, lw);
            self.cx.c.text(lx, y + 2.0, 20.0, &shown, F::Body, ink);
            let s2 = self.cx.c.fit(&sub, F::Small, lw);
            self.cx.c.text(lx, y + 21.0, 16.0, &s2, F::Small, b.tx2);
        }
        let cw = self.cx.c.width(I_CHEV, F::IconTiny);
        self.cx.icon(x + w - 10.0 - cw, y, h, I_CHEV, F::IconTiny, b.tx2);
        self.cx.ring((x, y, w, h), 6.0, ctl);
        self.cx.hit(ctl, (x, y, w, h), 1);
        let dir = match motion {
            Motion::Left => "swipe left",
            Motion::Right => "swipe right",
            Motion::Up => "swipe up",
            Motion::Down => "swipe down",
            Motion::Tap => "tap",
        };
        let name = format!("{fingers} fingers, {dir}");
        self.cx.say(ctl, UIA_ComboBoxControlTypeId, &name, &sub, self.cx.ro, (None, Some(text)));
        h
    }
}

/// Windows' state per gesture row (setup::ROWS' order), as each row's small line shows it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Chip {
    Ok,
    On,
    Err,
    Off,
    Back,
    Not,
}

/// The touchpad section's note, its cards' header words and each row's state, in the first
/// run's words (spec 0011's states, AC-SET-6).
fn pad_state(st: &PadState, back: Option<GaveBack>) -> (Option<NoteSpec>, &'static str, Option<[Chip; 4]>) {
    let note = |tone, icon, title: &str, line: String, buttons: Vec<(Ctl, &'static str)>| Some(NoteSpec { tone, icon, title: title.into(), lines: vec![t(line)], buttons, group: 1 });
    const LOG: &str = "Details: tray icon › Open log.";
    let windows = (Ctl::Pad(Pad::Windows), "Open Windows Settings");
    if let Some(b) = back.filter(|b| b.failed()) {
        let none = b.wrote.iter().all(|w| *w == Wrote::NotOne);
        let (title, line) = if none { ("Give back failed", format!("None went back. des-k keeps your old values to try again. {LOG}")) } else { ("Give back partly failed", format!("Some went back, some didn't. des-k keeps your old values to try again. {LOG}")) };
        let chips = b.wrote.map(|w| if w == Wrote::All { Chip::Back } else { Chip::Not });
        return (note(Tone::Warn, I_WARN, title, line, vec![(Ctl::Pad(Pad::GiveBack), "Try again")]), "des-k runs these", Some(chips));
    }
    if back.is_some() && !st.off {
        let cleared = back.is_some_and(|b| b.cleared);
        let line = if cleared {
            "des-k gave Windows its settings back and runs no gestures now. Until you sign in again, none work. To use them now, set them in Windows Settings."
        } else {
            "Nothing is wrong now, but des-k couldn't delete its copy of your old values. Give back again to clean it up."
        };
        return (note(Tone::Info, I_INFO, "Windows gets its gestures back at your next sign-in", line.into(), vec![windows]), "des-k would run", Some([Chip::Back; 4]));
    }
    if !st.enabled {
        return if st.off {
            (note(Tone::Warn, I_WARN, "No 3- or 4-finger gesture works now", "Windows' own gestures are off too. Turn reading on, or turn them back on in Windows Settings.".into(), vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on"), windows]), "des-k would run", Some([Chip::Off; 4]))
        } else {
            (note(Tone::Info, I_INFO, "Touchpad reading is off", "Windows runs its own 3- and 4-finger gestures. des-k runs none of these.".into(), vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on")]), "des-k would run", Some([Chip::On; 4]))
        };
    }
    if !st.pad {
        return (note(Tone::Info, I_INFO, "No precision touchpad found", "Your gestures are kept. They run once one is plugged in and set up.".into(), vec![]), "Kept for a touchpad", None);
    }
    if let Some(i) = st.rows.iter().position(|r| *r == PadRow::CantRead) {
        let row = setup::ROWS[i].0;
        let one = row.trim_end_matches('s');
        let chips = st.rows.map(|r| match r {
            PadRow::Nothing => Chip::Ok,
            PadRow::StillOn => Chip::On,
            PadRow::CantRead => Chip::Err,
        });
        let line = format!("Until des-k can read it, it runs no gestures, and the ones you turned off don't work. Set {row} again in Windows Settings. {LOG}");
        return (note(Tone::Warn, I_WARN, &format!("Can't read Windows' {one} setting"), line, vec![windows]), "des-k would run", Some(chips));
    }
    let on: Vec<usize> = (0..4).filter(|&i| st.rows[i] == PadRow::StillOn).collect();
    let chips = st.rows.map(|r| if r == PadRow::StillOn { Chip::On } else { Chip::Ok });
    match on.len() {
        0 => (note(Tone::Good, I_OK, "des-k runs your 3- and 4-finger gestures", "All four are off in Windows, so each gesture does one thing, also in admin windows.".into(), vec![]), "des-k runs these", Some(chips)),
        4 => (
            note(Tone::Info, I_SETUP, "Turn off four gestures in Windows", "Settings › Touchpad: under Three-finger and Four-finger gestures, set Swipes and Taps to “Nothing”. des-k runs these once all four are off.".into(), vec![(Ctl::Pad(Pad::Windows), "Set up in Windows Settings")]),
            "des-k would run",
            Some(chips),
        ),
        n => {
            let names: Vec<String> = on.iter().map(|&i| spelled_row(setup::ROWS[i].0)).collect();
            let off = 4 - n;
            let line = format!("Still on: {}. des-k runs gestures once all four are off; until then, the {off} you turned off don't work.", names.join(", "));
            (note(Tone::Warn, I_WARN, &format!("{n} of 4 {} still on in Windows", if n == 1 { "is" } else { "are" }), line, vec![windows]), "des-k would run", Some(chips))
        }
    }
}

/// "3-finger swipes" → "three-finger swipes", as a sentence has it.
fn spelled_row(row: &str) -> String {
    row.replacen("3-", "three-", 1).replacen("4-", "four-", 1)
}
