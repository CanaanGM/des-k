// The first run's touchpad step: spec 0011's states in its words, and the step's rows, gesture
// pickers and their menu.
use super::*;
use super::draw::{Ctx, NoteSpec, P};
use super::steps::Rec;

// ---- The touchpad's state, in the first run's words (spec 0011's states) ----

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Chip {
    Ok,
    Off,
    On,
    Err,
    Back,
    Part,
    Not,
}

/// One of spec 0011's touchpad states as step 4 draws it.
pub(super) struct PadView {
    pub(super) sub: String,
    pub(super) note: Option<NoteSpec>,
    head: &'static str,
    chips: [Chip; 4],
    give: bool,  // the Give back row shows
    quit: bool,  // its caption adds "Quit does the same."
    pub(super) ready: bool, // des-k runs the gestures now
    pub(super) done_line: String, // Done's touchpad row
    pub(super) done_button: bool, // and its Open Windows Settings
}

pub(super) fn pad_view(st: &PadState, back: Option<GaveBack>) -> PadView {
    const LOG: &str = "Details: tray icon › Open log.";
    let note = |tone, icon, title: &str, line: String, buttons: Vec<(Ctl, &'static str)>| Some(NoteSpec { tone, icon, title: title.into(), lines: vec![P::T(line)], buttons, group: 0 });
    let windows = |label| vec![(Ctl::Pad(Pad::Windows), label)];
    let rows = st.rows.map(|r| match r {
        PadRow::Nothing => Chip::Ok,
        PadRow::StillOn => Chip::On,
        PadRow::CantRead => Chip::Err,
    });
    let mut v = PadView { sub: String::new(), note: None, head: "des-k would run", chips: rows, give: false, quit: false, ready: false, done_line: String::new(), done_button: false };
    if let Some(b) = back.filter(|b| b.failed()) {
        let none = b.wrote.iter().all(|w| *w == Wrote::NotOne);
        v.chips = b.wrote.map(|w| match w {
            Wrote::All => Chip::Back,
            Wrote::Part => Chip::Part,
            Wrote::NotOne => Chip::Not,
        });
        let (title, sub, line) = if none {
            ("Give back failed", "Giving the gestures back failed.", "None went back.")
        } else {
            ("Give back partly failed", "Giving the gestures back partly failed.", "Some went back, some didn't.")
        };
        v.sub = sub.into();
        v.head = if none { "des-k runs" } else { "des-k would run" };
        v.note = note(Tone::Warn, I_WARN, title, format!("{line} des-k keeps your old values to try again. Windows Settings is open. {LOG}"), vec![(Ctl::Pad(Pad::GiveBack), "Try again")]);
        v.ready = none && st.off;
        v.done_line = format!("{title}. Try again on the Touchpad step.");
        return v;
    }
    if let Some(b) = back.filter(|_| !st.off) {
        v.chips = [Chip::Back; 4];
        v.sub = "You gave the gestures back.".into();
        v.note = if b.cleared {
            note(Tone::Info, I_INFO, "Windows gets its gestures back at your next sign-in", "des-k stopped reading them. Windows Settings is open: set them there to use them now.".into(), vec![])
        } else {
            note(Tone::Info, I_INFO, "Windows gets its gestures back at your next sign-in", format!("Nothing is wrong now, but des-k couldn't delete its copy of your old values. Give back again to clean it up. {LOG}"), vec![])
        };
        v.give = st.saved;
        v.done_line = "Windows gets its gestures back at your next sign-in.".into();
        return v;
    }
    v.give = st.saved;
    if !st.enabled {
        v.sub = "des-k's touchpad reading is off.".into();
        if st.off {
            v.chips = [Chip::Off; 4];
            v.quit = true;
            v.note = Some(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: "No 3- or 4-finger gesture works now".into(),
                lines: vec![P::T("Windows' own gestures are off too. Turn reading on, or turn them back on in Windows Settings.".into())],
                buttons: vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on"), (Ctl::Pad(Pad::Windows), "Open Windows Settings")],
                group: 0,
            });
        } else {
            v.note = note(Tone::Info, I_INFO, "Windows runs its own 3- and 4-finger gestures", "des-k runs none of these until reading is on.".into(), vec![(Ctl::Pad(Pad::ReadOn), "Turn reading on")]);
        }
        v.done_line = "Touchpad reading is off.".into();
        return v;
    }
    if let Some(i) = st.rows.iter().position(|r| *r == PadRow::CantRead) {
        let row = setup::ROWS[i].0;
        let one = row.trim_end_matches('s');
        v.head = "des-k will run";
        v.sub = "des-k runs no gestures for now.".into();
        v.note = note(Tone::Warn, I_WARN, &format!("Can't read Windows' {one} setting"), format!("Until des-k can read it, it runs no gestures, and the ones you turned off don't work. Set {row} again in Windows Settings. {LOG}"), windows("Open Windows Settings"));
        v.done_line = format!("Not ready: des-k can't read Windows' {one} setting.");
        v.done_button = true;
        return v;
    }
    let on: Vec<usize> = (0..4).filter(|&i| st.rows[i] == PadRow::StillOn).collect();
    let off = 4 - on.len();
    match on.len() {
        0 => {
            v.head = "des-k runs";
            v.ready = true;
            v.quit = st.saved;
            v.sub = if st.saved {
                "All four are off in Windows. des-k runs these, also in admin windows.".into()
            } else {
                "All four are off in Windows. des-k runs these, also in admin windows. You turned them off yourself, so Quit has nothing to give back.".into()
            };
            v.done_line = "des-k runs your gestures.".into();
        }
        4 => {
            v.head = "des-k will run";
            v.sub = "des-k runs your 3- and 4-finger gestures once Windows' own are off.".into();
            v.note = note(Tone::Info, I_SETUP, "Turn off four gestures in Windows", "Settings › Touchpad: under Three-finger and Four-finger gestures, set Swipes and Taps to “Nothing”.".into(), windows("Set up in Windows Settings"));
            v.done_line = "Not ready: none of the four are off in Windows yet.".into();
            v.done_button = true;
        }
        _ => {
            v.head = "des-k will run";
            v.sub = format!("{off} of 4 done. des-k starts once all four are off in Windows. Until then, the {off} you turned off don't work.");
            let names: Vec<String> = on.iter().map(|&i| spelled_row(setup::ROWS[i].0)).collect();
            let mut line = names.join(", ");
            if let Some(c) = line.get(..1) {
                line = c.to_uppercase() + &line[1..];
            }
            v.note = note(Tone::Info, I_SETUP, "Still on in Windows", line + ".", windows("Open Windows Settings"));
            v.done_line = format!("Not ready: {off} of 4 are off in Windows. Until all four are, those {off} don't work.");
            v.done_button = true;
        }
    }
    v
}

/// "3-finger swipes" → "three-finger swipes", as a sentence has it.
pub(super) fn spelled_row(row: &str) -> String {
    row.replacen("3-", "three-", 1).replacen("4-", "four-", 1)
}

impl Ctx<'_, '_> {
    // ---- Step 4: touchpad ----

    pub(super) fn touchpad(&mut self, y: f32) -> f32 {
        let pv = self.pad();
        let mut h = 0.0;
        for (g, fingers) in [(0usize, 3u8), (1, 4)] {
            if g > 0 {
                h += 12.0;
            }
            let cap = format!("{} · click an action to change it", pv.head);
            let chips = pv.chips;
            h += self.card(y + h, |c, y| {
                let mut h = c.pad_header(y, fingers, &cap);
                for (k, tap) in [(0usize, false), (1, true)] {
                    if k > 0 {
                        c.sep(y + h);
                    }
                    h += c.gesture_row(y + h, fingers, tap, chips[g * 2 + k]);
                }
                h
            });
        }
        h += 12.0;
        let (give, quit) = (pv.give, pv.quit);
        h += self.card(y + h, |c, y| {
            let mut h = 0.0;
            if give {
                let cap = format!("Undo: Windows uses its own gestures again from your next sign-in.{}", if quit { " Quit does the same." } else { "" });
                let bw = c.cx.btn_w("Give back", None);
                h += c.row(y, "Give back to Windows", &[P::T(cap)], (bw, 32.0), |c, x, y| {
                    c.button(Ctl::Pad(Pad::GiveBack), x, y, "Give back");
                }, None);
                c.sep(y + h);
            }
            h += c.more(y + h, "Reading, swipe distance, wait for a second gesture");
            if c.w.more[Step::Pad.index()] {
                for (key, title, cap) in [
                    ("touchpad.enabled", "Read touchpad gestures", "Also in admin windows."),
                    ("touchpad.swipe_mm", "Swipe distance", "How far fingers move before a swipe counts."),
                    ("touchpad.twice_ms", "Wait for a second gesture", "Only for gestures with a twice action."),
                ] {
                    c.sep(y + h);
                    h += c.setting(y + h, key, title, cap);
                }
            }
            h
        });
        h
    }

    /// A gesture card's header: the fingers' circle, its title, a caption on the right.
    fn pad_header(&mut self, y: f32, n: u8, cap: &str) -> f32 {
        let title = format!("{n} fingers");
        let h = self.header(y, &title, Some(n), cap);
        self.text_node(UIA_HeaderControlTypeId, &format!("{title}: {cap}"), None);
        h
    }

    fn gesture_row(&mut self, y: f32, fingers: u8, tap: bool, chip: Chip) -> f32 {
        let b = self.cx.c.t.b;
        let motions: &[Motion] = if tap { &[Motion::Tap] } else { &[Motion::Left, Motion::Right, Motion::Up, Motion::Down] };
        let (x, px) = (X0 + 16.0, X0 + 16.0 + 116.0 + 16.0);
        let pw_all = X0 + self.cw - 16.0 - px;
        let cols = if tap { 1 } else { 2 };
        let pw = (pw_all - 8.0 * (cols as f32 - 1.0)) / cols as f32;
        let rows: Vec<usize> = motions.iter().filter_map(|m| settings::GESTURE_ROWS.iter().position(|r| *r == (fingers, *m))).collect();
        let mut ly = 0.0;
        for line in rows.chunks(cols) {
            let lh = line.iter().map(|&row| self.measured(|c| c.picker(row, 0.0, 0.0, pw))).fold(0.0, f32::max);
            for (k, &row) in line.iter().enumerate() {
                self.picker(row, px + k as f32 * (pw + 8.0), y + 8.0 + ly, pw);
            }
            ly += lh + 8.0;
        }
        let ph: f32 = ly - 8.0;
        self.cx.c.text(x, y + 14.0, 20.0, if tap { "Tap" } else { "Swipe" }, F::Bold, b.tx);
        let (icon, text, col) = match chip {
            Chip::Ok => (Some(I_OK), "Off in Windows", b.good),
            Chip::Off => (None, "Off in Windows", b.tx2),
            Chip::On => (None, "Windows uses it", b.tx2),
            Chip::Err => (Some(I_WARN), "Can't read it", b.warn),
            Chip::Back => (None, "Back at next sign-in", b.tx2),
            Chip::Part => (Some(I_WARN), "Partly written back", b.warn),
            Chip::Not => (Some(I_WARN), "Not written back", b.warn),
        };
        let mut cx = x;
        if let Some(i) = icon {
            self.cx.icon(cx, y + 36.0, 16.0, i, F::IconSmall, col);
            cx += 16.0;
        }
        self.cx.c.text(cx, y + 36.0, 16.0, text, F::Small, b.tx2);
        let gesture = if tap { "tap" } else { "swipes" };
        self.text_node(UIA_TextControlTypeId, &format!("{fingers}-finger {gesture}: {text}"), None);
        // Its name and Windows' state for it, on the left, need 44 dip.
        8.0 + ph.max(44.0) + 8.0
    }

    /// A gesture picker: its direction mark, what it does, its twice action, ▾.
    fn picker(&mut self, row: usize, x: f32, y: f32, w: f32) -> f32 {
        let t = self.cx.c.t;
        let b = t.b;
        let (fingers, motion) = settings::GESTURE_ROWS[row];
        let s = self.s;
        let once = s.rule(fingers, motion, 1);
        let twice = s.rule(fingers, motion, 2);
        let ctl = Ctl::Picker(row);
        let open = self.w.menu.is_some_and(|m| m.row == row);
        let waiting = self.w.rec == Some(Rec::Gesture(row, 1)) || self.w.rec == Some(Rec::Gesture(row, 2));
        let text = sp::menu::once_label(once.unwrap_or(Action::Nothing));
        let sub = if twice.is_some() { format!("Twice: {}", sp::menu::twice_label(twice)) } else { String::new() };
        let h = if sub.is_empty() { 32.0 } else { 40.0 };
        if waiting {
            // Key combo…: the field records, held keys and a ghost.
            self.cx.c.fill_rr((x, y, w, h), 6.0, op(b.acc));
            self.cx.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 4.0, op(b.card));
        } else if open {
            self.cx.c.fill_rr((x, y, w, h), 6.0, op(b.acc));
            self.cx.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 4.0, op(b.card));
        } else {
            self.cx.c.boxed((x, y, w, h), 6.0, if self.cx.hovered(ctl) { mix(b.track, b.tx, 0.05) } else { b.track }, Some(b.line));
        }
        let glyph = match motion {
            Motion::Left => "\u{E72B}",
            Motion::Right => "\u{E72A}",
            Motion::Up => "\u{E74A}",
            Motion::Down => "\u{E74B}",
            Motion::Tap => "",
        };
        let my = y + (h - 22.0) / 2.0;
        if t.hc {
            self.cx.c.stroke_rr((x + 6.5, my + 0.5, 21.0, 21.0), 4.0, 1.0, op(b.acc));
        } else {
            self.cx.c.fill_rr((x + 6.0, my, 22.0, 22.0), 4.0, op(b.held));
        }
        let mark = if t.dark && !t.hc { mix(b.acc_b, 0xFFFFFF, 0.28) } else { b.acc_b };
        if glyph.is_empty() {
            self.cx.c.circle(x + 17.0, my + 11.0, 3.0, Some(op(mark)), None);
        } else {
            let gw = self.cx.c.width(glyph, F::IconTiny);
            self.cx.icon(x + 6.0 + (22.0 - gw) / 2.0, my, 22.0, glyph, F::IconTiny, mark);
        }
        let lx = x + 36.0;
        let lw = w - 36.0 - 26.0;
        if waiting {
            let ghost = "key";
            let gw = self.cx.c.width(ghost, F::Small) + 12.0;
            self.cx.c.stroke_dash((lx, my, gw, 22.0), 4.0, 1.0, op(b.tx3), DashStyleDash);
            self.cx.c.text(lx + 6.0, my, 22.0, ghost, F::Small, b.tx3);
        } else if sub.is_empty() {
            let shown = self.cx.c.fit(&text, F::Body, lw);
            self.cx.c.text(lx, y, 32.0, &shown, F::Body, b.tx);
        } else {
            let shown = self.cx.c.fit(&text, F::Body, lw);
            self.cx.c.text(lx, y + 2.0, 20.0, &shown, F::Body, b.tx);
            let s2 = self.cx.c.fit(&sub, F::Small, lw);
            self.cx.c.text(lx, y + 21.0, 16.0, &s2, F::Small, b.tx2);
        }
        if !waiting {
            let cw = self.cx.c.width(I_DOWN, F::IconTiny);
            self.cx.icon(x + w - 10.0 - cw, y, h, I_DOWN, F::IconTiny, b.tx2);
        }
        self.cx.ring((x, y, w, h), 6.0, ctl);
        self.hit(ctl, (x, y, w, h));
        let dir = match motion {
            Motion::Left => "swipe left",
            Motion::Right => "swipe right",
            Motion::Up => "swipe up",
            Motion::Down => "swipe down",
            Motion::Tap => "tap",
        };
        self.cx.say(ctl, UIA_ComboBoxControlTypeId, &format!("{fingers} fingers, {dir}"), &sub, false, (None, Some(text)));
        h
    }

    /// The open menu, over everything: its lines in two columns and a footer, the chosen one
    /// checked, the default marked, kept inside the window.
    pub(super) fn menu(&mut self) {
        let Some(m) = self.w.menu else { return };
        let t = self.cx.c.t;
        let b = t.b;
        let lines = sp::menu::menu_lines(&m, self.s);
        let (fingers, motion) = settings::GESTURE_ROWS[m.row];
        let dir = if motion == Motion::Tap { "tap".to_string() } else { format!("swipe {}", motion.word()) };
        let head = match m.family {
            Some(f) => sp::menu::FAMILIES[f].0.to_string(),
            None if m.times == 2 => format!("{fingers} fingers, {dir}, done twice"),
            None => format!("{fingers} fingers, {dir}"),
        };
        let small = m.family.is_some() || m.times == 2;
        let mw = if small { 272.0 } else { 480.0 };
        // des-k's column is the wider one, as its names are longer.
        let colw = if small { mw - 8.0 } else { 252.0 };
        let colw1 = mw - 8.0 - 4.0 - colw;
        let n0 = lines.iter().filter(|l| l.col == 0).count();
        let n1 = lines.iter().filter(|l| l.col == 1).count();
        let rows = n0.max(n1) as f32;
        let feet = lines.iter().any(|l| l.col == 2) as u8 as f32 + lines.iter().any(|l| l.col == 3) as u8 as f32;
        let cols_head = if m.family.is_none() && m.times == 1 { 24.0 } else { 0.0 };
        let mh = 4.0 + 24.0 + cols_head + rows * 28.0 + feet * (9.0 + 28.0) + 4.0;
        let chosen = lines.iter().position(|l| l.on && l.col < 2);
        let off = chosen.map_or(0.0, |i| {
            let l = &lines[i];
            let k = lines.iter().take(i).filter(|x| x.col == l.col).count();
            4.0 + 24.0 + cols_head + k as f32 * 28.0
        });
        let my = (m.at.1 - off).clamp(8.0, (self.w.vh - mh - 8.0).max(8.0));
        let mx = m.at.0.min(self.w.vw - mw - 16.0).max(8.0);
        self.cx.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.cx.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::color::alpha(0, 0.08));
        }
        self.cx.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.hit(Ctl::Item(usize::MAX), (mx, my, mw, mh));
        self.cx.c.text(mx + 14.0, my + 4.0, 24.0, &head, F::SmallBold, b.tx2);
        let mut ys = [my + 28.0; 2];
        if cols_head > 0.0 {
            self.cx.c.text(mx + 14.0, my + 28.0, 24.0, "des-k", F::SmallBold, b.tx2);
            self.cx.c.text(mx + 4.0 + colw + 4.0 + 10.0, my + 28.0, 24.0, "Windows and media", F::SmallBold, b.tx2);
            ys = [my + 52.0; 2];
        }
        let foot_y = my + 4.0 + 24.0 + cols_head + rows * 28.0;
        let mut fx = 0.0;
        let mut drew_rule = [false; 2];
        for (i, l) in lines.iter().enumerate() {
            let (x, y, w) = match l.col {
                0 | 1 => {
                    let c = if small { 0 } else { l.col as usize };
                    let y = ys[c];
                    ys[c] += 28.0;
                    (mx + 4.0 + c as f32 * (colw + 4.0), y, if c == 0 { colw } else { colw1 })
                }
                c => {
                    let row = (c - 2) as usize;
                    let y = foot_y + row as f32 * 37.0 + 9.0;
                    if !drew_rule[row] {
                        self.cx.c.rule(mx + 4.0, y - 5.0, mw - 8.0, op(b.line));
                        drew_rule[row] = true;
                        fx = 0.0;
                    }
                    let per = lines.iter().filter(|x| x.col == c).count() as f32;
                    let w = if small { mw - 8.0 } else { (mw - 8.0 - 4.0 * (per - 1.0)) / per };
                    let x = mx + 4.0 + if small { 0.0 } else { fx };
                    fx += w + 4.0;
                    (x, y + if small { (lines.iter().take(i).filter(|x| x.col == c).count()) as f32 * 28.0 } else { 0.0 }, w)
                }
            };
            let ctl = Ctl::Item(i);
            let hot = self.cx.hovered(ctl) || (self.w.kb && m.focus == i);
            let (fill, ink, check) = match () {
                _ if l.on && t.hc => (Some(b.hi), b.on_acc, b.on_acc),
                _ if l.on => (Some(b.track), b.tx, b.acc),
                _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx, b.acc),
                _ => (None, b.tx, b.acc),
            };
            if let Some(f) = fill {
                self.cx.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
            }
            if l.on {
                self.cx.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, check);
            }
            let more = matches!(l.does, Does::Family(_) | Does::Twice);
            let room = w - 30.0 - 6.0 - if l.default { 52.0 } else { 0.0 } - if more { 16.0 } else { 0.0 };
            let shown = self.cx.c.fit(&l.text, F::Body, room);
            self.cx.c.text(x + 30.0, y, 28.0, &shown, F::Body, ink);
            let mut rx = x + w - 10.0;
            if more {
                let cw = self.cx.c.width(I_MORE, F::IconTiny);
                rx -= cw;
                self.cx.icon(rx, y, 28.0, I_MORE, F::IconTiny, b.tx2);
                rx -= 6.0;
            }
            if l.default {
                let dw = self.cx.c.width("Default", F::Small);
                self.cx.c.text(rx - dw, y, 28.0, "Default", F::Small, b.tx2);
            }
            if self.w.kb && m.focus == i {
                self.cx.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
            }
            self.hit(ctl, (x, y, w, 28.0));
        }
    }
}

#[cfg(test)]
mod tests;
