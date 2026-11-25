// The first run's own drawing: one pass over the window that draws or only measures, the
// steps but the touchpad's, what sits above the step, and the footer.
use super::*;
use super::pad::{PadView, pad_view};
use super::steps::{Field, Held, MORE_KEYS, Rec, Step, TAKES, TRIES, field_of, field_of_key, held, rec_note, row_caps, steps, take_caption, title_of, words};

// ---- Drawing ----

pub(super) type P = controls::P<Ctl>;
pub(super) type NoteSpec = controls::NoteSpec<Ctl, u8>;

/// What one pass of compose found.
pub(super) struct Out {
    pub(super) hits: Vec<Hit>,
    pub(super) content: f32,
    pub(super) mid: (f32, f32),
    pub(super) said: Vec<(Ctl, Node)>, // what Narrator reads for each stop, in order
    pub(super) texts: Vec<Node>,       // and the words around them: progress, heading, the tries
}

/// One pass over the window, drawing or only measuring.
pub(super) struct Ctx<'a, 'f> {
    pub(super) cx: controls::Ctx<'a, 'f, Ctl, u8>,
    pub(super) w: &'a Win,
    pub(super) s: &'a Settings, // from the paint's own handle, one version for the whole pass
    pub(super) cw: f32,
    out: Out,
}

impl<'a, 'f> Screen<'a, 'f> for Ctx<'a, 'f> {
    type C = Ctl;
    type E = u8;

    fn cx(&mut self) -> &mut controls::Ctx<'a, 'f, Ctl, u8> {
        &mut self.cx
    }

    fn column(&self) -> (f32, f32) {
        (X0, self.cw)
    }

    /// Lays `f` out without drawing it or keeping its controls.
    fn measured(&mut self, f: impl FnOnce(&mut Self) -> f32) -> f32 {
        let texts = self.out.texts.len();
        let m = self.cx.mark();
        let h = f(self);
        self.cx.rewind(m);
        self.out.texts.truncate(texts);
        h
    }
}

impl Ctx<'_, '_> {
    pub(super) fn hit(&mut self, ctl: Ctl, at: Bx) {
        let group = self.cx.group;
        self.cx.hit(ctl, at, group);
    }

    pub(super) fn text_node(&mut self, role: UIA_CONTROLTYPE_ID, name: &str, value: Option<String>) {
        if self.cx.c.draw || self.out.texts.iter().any(|n| n.name == name) {
            return;
        }
        self.out.texts.push(Node { role, name: name.to_string(), value, ..Node::default() });
    }

    /// A plain button in the step or the footer.
    pub(super) fn button(&mut self, ctl: Ctl, x: f32, y: f32, label: &str) -> f32 {
        let group = self.cx.group;
        self.cx.button(ctl, x, y, label, None, false, group)
    }

    /// A stepper's width: its value, its unit, − and +.
    fn stepper_w(&self, v: i64, unit: &str) -> f32 {
        let u = if unit.is_empty() { String::new() } else { format!(" {unit}") };
        64.0 + (self.cx.c.width(&v.to_string(), F::Body) + self.cx.c.width(&u, F::Small) + 8.0).max(56.0)
    }

    /// The tint of a row that is recording or warns.
    fn tint(&self, y: f32, h: f32) {
        let b = self.cx.c.t.b;
        if !self.cx.c.t.hc {
            self.cx.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, h), 0.0, op(mix(b.card, b.acc_a, 0.06)));
        }
    }

    /// A row, tinted under it while `tinted` (recording, or a warning).
    #[allow(clippy::too_many_arguments)]
    fn tinted_row(&mut self, y: f32, title: &str, cap: &[P], size: (f32, f32), ctl: impl Fn(&mut Self, f32, f32), note: Option<&NoteSpec>, tinted: bool) -> f32 {
        if tinted {
            let h = self.measured(|c| c.row(y, title, cap, size, &ctl, note));
            self.tint(y, h);
        }
        self.row(y, title, cap, size, ctl, note)
    }

    /// The More row: a chevron, and what it opens.
    pub(super) fn more(&mut self, y: f32, what: &str) -> f32 {
        let b = self.cx.c.t.b;
        let open = self.w.more[self.w.step.index()];
        let icon = if open { I_UP } else { I_DOWN };
        let iw = self.cx.c.width(icon, F::IconSmall);
        let h = self.row(y, "More", &[P::T(what.into())], (iw, 20.0), |c, x, y| c.cx.icon(x, y, 20.0, icon, F::IconSmall, b.tx2), None);
        if self.cx.hovered(Ctl::More) {
            self.cx.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, h - 1.0), 0.0, ui::alpha(b.tx, 0.04));
        }
        let at = (X0 + 4.0, y + 2.0, self.cw - 8.0, h - 4.0);
        self.cx.ring(at, 6.0, Ctl::More);
        self.hit(Ctl::More, (X0, y, self.cw, h));
        self.cx.say(Ctl::More, UIA_ButtonControlTypeId, &format!("More: {what}"), "", false, (None, Some(if open { "open" } else { "closed" }.into())));
        h
    }

    /// Key caps for one key, 22 dip, from `x`. Returns their width.
    fn caps(&self, x: f32, y: f32, caps: &[String], held: bool) -> f32 {
        let mut cx = x;
        for (i, k) in caps.iter().enumerate() {
            if i > 0 {
                cx += 4.0;
            }
            cx += self.cx.c.key_cap(cx, y, 22.0, k, held);
        }
        cx - x
    }

    fn caps_w(&self, caps: &[String]) -> f32 {
        caps.iter()
            .enumerate()
            .map(|(i, k)| {
                let (label, f) = match k.as_str() {
                    "Left" | "Right" | "Up" | "Down" => ("\u{E72B}", F::IconTiny),
                    k => (k, F::Small),
                };
                (self.cx.c.width(label, f) + 2.0 * (22.0 / 2.0 - 5.0) + 2.0).max(22.0) + if i > 0 { 4.0 } else { 0.0 }
            })
            .sum()
    }

    /// A key field, 32 dip: its key and the pen, "Set key", or recording. `fill`: its width
    /// when it fills a column. Returns its width.
    #[allow(clippy::too_many_arguments)]
    fn key_field(&mut self, f: Field, x: f32, y: f32, fill: Option<f32>, caps: Option<&[String]>, rec: Option<(&[String], &str)>, help: &str) -> f32 {
        let b = self.cx.c.t.b;
        let ctl = Ctl::Field(f);
        let pen_w = self.cx.c.width(I_PEN, F::IconSmall);
        let set_w = self.cx.c.width("Set key", F::Body);
        let w = fill.unwrap_or_else(|| match (rec, caps) {
            (Some((held, ghost)), _) => 10.0 + if held.is_empty() { self.cx.c.width(ghost, F::Small) + 12.0 } else { self.caps_w(held) },
            (None, Some(k)) => 5.0 + self.caps_w(k) + 6.0 + pen_w + 4.0,
            (None, None) => 10.0 + 10.0 + 6.0 + set_w + 12.0,
        });
        let hot = self.cx.hovered(ctl);
        match rec {
            Some((held, ghost)) => {
                self.cx.c.fill_rr((x, y, w, 32.0), 6.0, op(b.acc));
                self.cx.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, 28.0), 4.0, op(b.card));
                if held.is_empty() {
                    let gw = self.cx.c.width(ghost, F::Small) + 12.0;
                    self.cx.c.stroke_dash((x + 5.0, y + 5.0, gw, 22.0), 4.0, 1.0, op(b.tx3), DashStyleDash);
                    self.cx.c.text(x + 11.0, y + 5.0, 22.0, ghost, F::Small, b.tx3);
                } else {
                    self.caps(x + 5.0, y + 5.0, held, true);
                }
            }
            None => {
                self.cx.c.boxed((x, y, w, 32.0), 6.0, if hot { mix(b.track, b.tx, 0.05) } else { b.track }, Some(b.line));
                match caps {
                    Some(k) => {
                        self.caps(x + 5.0, y + 5.0, k, false);
                        self.cx.icon(x + w - 4.0 - pen_w, y, 32.0, I_PEN, F::IconSmall, b.tx3);
                    }
                    None => {
                        self.cx.icon(x + 10.0, y, 32.0, I_ADD, F::IconTiny, b.tx2);
                        self.cx.c.text(x + 26.0, y, 32.0, "Set key", F::Body, b.tx);
                    }
                }
            }
        }
        self.cx.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0));
        let value = if rec.is_some() { "recording".to_string() } else { caps.map_or("no key".into(), words) };
        self.cx.say(ctl, UIA_ButtonControlTypeId, &format!("{}: key", title_of(f)), help, false, (None, Some(value)));
        w
    }

    // ---- Above the step: progress, heading, the fixed note ----

    /// Returns the bottom of what sits above the step's view.
    fn head(&mut self) -> f32 {
        let t = self.cx.c.t;
        let b = t.b;
        let list = steps(self.w);
        let n = list.len();
        let j = list.iter().position(|s| *s == self.w.step).unwrap_or(0);
        let frac = (j + 1) as f32 / n as f32;
        let bw = self.w.vw - 2.0 * X0;
        if t.hc {
            self.cx.c.boxed((X0, 0.0, bw, 6.0), 3.0, b.bg, Some(b.tx));
            self.cx.c.fill_rr((X0 + 1.0, 1.0, (bw - 2.0) * frac, 4.0), 2.0, op(b.acc_a));
        } else {
            self.cx.c.fill_rr((X0, 0.0, bw, 3.0), 1.5, op(b.line));
            self.cx.c.fill_rr((X0, 0.0, bw * frac, 3.0), 1.5, op(b.acc_a));
        }
        let eye = format!("Step {} of {n}", j + 1);
        self.text_node(UIA_ProgressBarControlTypeId, &eye, Some(format!("{} of {n}", j + 1)));
        self.cx.c.text(X0, 19.0, 16.0, &eye, F::Small, b.tx2);
        let title = self.w.step.title();
        self.text_node(UIA_HeaderControlTypeId, title, None);
        self.cx.c.text(X0, 37.0, 32.0, title, F::Heading, b.tx);
        let mut y = 69.0;
        let sub = self.sub();
        if !sub.is_empty() {
            self.text_node(UIA_TextControlTypeId, &sub, None);
            y += 4.0 + self.cx.c.para(X0, 73.0, self.cw, 20.0, &[(&sub, F::Body, b.tx2)]);
        }
        if let Some(n) = self.fixed() {
            y += 12.0;
            y += self.cx.note(X0, y, self.cw, &n);
        }
        y
    }

    fn sub(&self) -> String {
        let w = self.w;
        match w.step {
            Step::Try if w.pics.screens.len() > 1 => "Switch the screen under your mouse. Your other screens stay as they are.".into(),
            Step::Try => "Switch workspaces on your screen. Plug in another, and it gets its own.".into(),
            Step::Keys => "des-k's default shortcuts. Each one overrides a Windows shortcut. Turn any off, or click a key to change it.".into(),
            Step::Behavior => "des-k's defaults. Change any now or later.".into(),
            Step::Pad => self.pad().sub,
            Step::Done => "Everything is saved. Open this again any time: des-k settings › Welcome.".into(),
        }
    }

    pub(super) fn pad(&self) -> PadView {
        let st = self.w.pics.pad.unwrap_or(PadState { pad: true, enabled: self.s.touchpad_enabled, rows: [PadRow::StillOn; 4], off: false, saved: false });
        pad_view(&st, LAST.get())
    }

    /// The note that never scrolls away: a failed save, then the touchpad's, or a gesture
    /// recording's.
    fn fixed(&self) -> Option<NoteSpec> {
        if let Some((what, why)) = &self.w.error {
            return Some(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: format!("Couldn't save “{what}”"),
                lines: vec![P::T(format!("Your change isn't saved or in use. Close settings.toml in any program that has it open, then try again. Windows said: {why}"))],
                buttons: vec![],
                group: 0,
            });
        }
        if self.w.step != Step::Pad {
            return None;
        }
        if let Some(Rec::Gesture(..)) = self.w.rec {
            let (title, line) = rec_note(self.w.rec?);
            let lines = match &self.w.refused {
                Some((t, l)) => return Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: t.clone(), lines: vec![P::T(l.clone())], buttons: vec![(Ctl::Cancel, "Cancel")], group: 0 }),
                None => vec![P::T(line)],
            };
            return Some(NoteSpec { tone: Tone::Rec, icon: I_KEY, title, lines, buttons: vec![(Ctl::Cancel, "Cancel")], group: 0 });
        }
        self.pad().note
    }

    // ---- Step 1: try it ----

    fn try_it(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let mut h = self.screens(y) + 12.0;
        h += self.card(y + h, |c, y| c.tries(y));
        if self.w.tried.iter().all(|t| *t) {
            h += 12.0;
            h += self.cx.c.para(X0, y + h, self.cw, 16.0, &[("That's des-k. Next: your keys.", F::Small, b.tx2)]);
        }
        h
    }

    /// Your screens, each in its workspace's color with its name and number, a window where
    /// that workspace has one, and the pointer on the screen under the mouse.
    fn screens(&mut self, y: f32) -> f32 {
        let list = self.w.pics.screens.clone();
        let (wd, ht) = (576.0, 176.0);
        if list.is_empty() {
            return ht;
        }
        let b = self.cx.c.t.b;
        let bx = list.iter().map(|s| s.rect.left).min().unwrap_or(0) as f32;
        let by = list.iter().map(|s| s.rect.top).min().unwrap_or(0) as f32;
        let bw = list.iter().map(|s| s.rect.right).max().unwrap_or(1) as f32 - bx;
        let bh = list.iter().map(|s| s.rect.bottom).max().unwrap_or(1) as f32 - by;
        let sc = ((wd - 4.0) / bw).min((ht - 18.0) / bh).min(0.09);
        let ox = X0 + (self.cw - wd) / 2.0 + (wd - bw * sc) / 2.0;
        let oy = y + (ht - (bh * sc + 16.0)) / 2.0;
        let mut said = vec![];
        for s in &list {
            let x = ox + (s.rect.left as f32 - bx) * sc + 3.0;
            let yy = oy + (s.rect.top as f32 - by) * sc + 3.0;
            let (w, h) = ((s.rect.right - s.rect.left) as f32 * sc - 6.0, (s.rect.bottom - s.rect.top) as f32 * sc - 6.0);
            let below = list.iter().any(|o| o.device != s.device && (o.rect.top - s.rect.bottom).abs() < 2 && o.rect.left < s.rect.right && o.rect.right > s.rect.left);
            if !below {
                if s.name == "Built-in display" {
                    self.cx.c.polygon(&[(x - 6.0, yy + h), (x + w + 6.0, yy + h), (x + w + 3.0, yy + h + 4.0), (x - 3.0, yy + h + 4.0)], op(b.stand), (op(b.stand), 0.5));
                } else {
                    let cx = x + w / 2.0;
                    self.cx.c.polygon(&[(cx - 8.0, yy + h), (cx + 8.0, yy + h), (cx + 11.0, yy + h + 9.0), (cx - 11.0, yy + h + 9.0)], op(b.stand), (op(b.stand), 0.5));
                    self.cx.c.fill_rr((cx - 22.0, yy + h + 9.0, 44.0, 3.0), 1.5, op(b.stand));
                }
            }
            let a = self.w.pics.view.active.iter().find(|(d, _)| *d == s.device).map_or(0, |(_, a)| *a);
            self.cx.c.fill_rr((x, yy, w, h), 6.0, op(b.bez));
            self.cx.c.fill_rr((x + 2.0, yy + 2.0, w - 4.0, h - 4.0), 4.0, op(b.ws[a.min(8)]));
            let name = self.w.pics.view.names.iter().find(|(d, _)| *d == s.device).map_or_else(|| s.name.clone(), |(_, n)| n.clone());
            said.push(format!("{name} on workspace {}", a + 1));
            // The name in lines that fit, when the face has room for them and the number.
            let mut lines: Vec<String> = vec![];
            for word in name.split(' ') {
                match lines.last_mut() {
                    Some(l) if self.cx.c.width(&format!("{l} {word}"), F::Small) <= w - 16.0 => *l = format!("{l} {word}"),
                    _ => lines.push(word.to_string()),
                }
            }
            let roomy = lines.len() as f32 * 15.0 + 30.0 <= h && lines.iter().all(|l| self.cx.c.width(l, F::Small) <= w - 16.0);
            if roomy {
                for (k, l) in lines.iter().enumerate() {
                    self.cx.c.text(x + 9.0, yy + 7.0 + k as f32 * 15.0, 15.0, l, F::Small, b.on_ws[a.min(8)]);
                }
            }
            let wy = yy + 12.0 + if roomy { lines.len() as f32 * 15.0 } else { 0.0 } + 8.0;
            let wh = h - (wy - yy) - 30.0;
            let busy = self.w.pics.view.windows.iter().any(|(d, ws, _)| *d == s.device && *ws == a);
            if busy && w > 100.0 && wh >= 18.0 {
                self.cx.c.fill_rr((x + w * 0.42, wy, w * 0.48, wh), 3.0, ui::alpha(b.mk_ws[a.min(8)], 0.85));
            }
            let num = (a + 1).to_string();
            if roomy {
                self.cx.c.text(x + 9.0, yy + h - 34.0, 28.0, &num, F::Title, b.on_ws[a.min(8)]);
            } else {
                let nw = self.cx.c.width(&num, F::Title);
                self.cx.c.text(x + (w - nw) / 2.0, yy + (h - 28.0) / 2.0, 28.0, &num, F::Title, b.on_ws[a.min(8)]);
            }
            if s.device == self.w.pics.mouse {
                let (px, py) = (x + w - 28.0, yy + h - 30.0);
                let pts = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.0, 19.8), (9.8, 18.5), (7.0, 12.1), (12.6, 12.1)].map(|(a, z)| (px + a, py + z));
                self.cx.c.polygon(&pts, op(0xFFFFFF), (op(0x111111), 1.1));
            }
        }
        self.text_node(UIA_ImageControlTypeId, &said.join(", "), None);
        ht
    }

    fn tries(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let s = self.s;
        let tried = self.w.tried;
        let now = tried.iter().position(|t| !*t);
        let mut h = 0.0;
        for (i, (title, key, how)) in TRIES.iter().enumerate() {
            if i > 0 {
                self.sep(y + h);
            }
            let done = tried[i];
            let cap = if now == Some(i) {
                how.to_string()
            } else if done && i == 0 {
                self.switched()
            } else {
                String::new()
            };
            let caps = sp::rows::shortcut(key).and_then(|sh| sp::rows::drawn_order(s, sh).first().and_then(|&k| sp::rows::keys_of(s, key).get(k).cloned()));
            let kw = caps.as_ref().map_or_else(|| self.cx.c.width("No key", F::Small), |c| self.caps_w(c));
            let lx = X0 + 16.0 + 20.0 + 12.0;
            let lw = self.cw - 32.0 - 32.0 - kw - 12.0;
            let cap_p: Vec<P> = if cap.is_empty() { vec![] } else { vec![P::T(cap.clone())] };
            let lh = self.measured(|c| c.try_label(lx, 0.0, lw, title, &cap_p, false));
            let top = lh.max(20.0).max(ROW - 16.0);
            let two = !cap.is_empty();
            let row_y = y + h + 8.0;
            let cy = if two { row_y } else { row_y + (top - 20.0) / 2.0 };
            let cx = X0 + 16.0;
            if done {
                self.cx.c.circle(cx + 10.0, cy + 10.0, 10.0, Some(op(b.acc)), None);
                let iw = self.cx.c.width(I_OK, F::IconTiny);
                self.cx.icon(cx + 10.0 - iw / 2.0, cy, 20.0, I_OK, F::IconTiny, b.on_acc);
            } else {
                let now = now == Some(i);
                self.cx.c.circle(cx + 10.0, cy + 10.0, if now { 9.25 } else { 9.5 }, None, Some((op(if now { b.acc } else { b.tx3 }), if now { 1.5 } else { 1.0 })));
                let d = (i + 1).to_string();
                let dw = self.cx.c.width(&d, F::SmallBold);
                self.cx.c.text(cx + 10.0 - dw / 2.0, cy, 20.0, &d, F::SmallBold, if now { b.tx } else { b.tx2 });
            }
            self.try_label(lx, if two { row_y } else { row_y + (top - lh) / 2.0 }, lw, title, &cap_p, done || now == Some(i));
            let ky = if two { row_y - 1.0 } else { row_y + (top - 22.0) / 2.0 };
            let kx = X0 + self.cw - 16.0 - kw;
            match &caps {
                Some(c) => {
                    self.caps(kx, ky, c, done && i == 0 && tried == [true, false, false]);
                }
                None => self.cx.c.text(kx, ky, 22.0, "No key", F::Small, b.tx2),
            }
            let state = if done { "done" } else if now == Some(i) { "now" } else { "to do" };
            let spoken = format!("{}. {title}, {}, {state}. {cap}", i + 1, caps.as_ref().map_or("no key".into(), |c| words(c)));
            self.text_node(UIA_ListItemControlTypeId, spoken.trim_end(), None);
            h += 8.0 + top + 8.0;
        }
        h
    }

    fn try_label(&mut self, x: f32, y: f32, w: f32, title: &str, cap: &[P], strong: bool) -> f32 {
        let b = self.cx.c.t.b;
        let mut h = self.cx.c.para(x, y, w, 20.0, &[(title, F::Bold, if strong { b.tx } else { b.tx2 })]);
        if !cap.is_empty() {
            h += self.cx.caption(x, y + h, w, cap);
        }
        h
    }

    /// The switch row's result.
    fn switched(&self) -> String {
        let back = row_caps(self.s, &["shortcuts.previous"]).map(|c| words(&c));
        let undo = |who: &str| match &back {
            Some(k) => format!("{who} hidden, not closed: {k} brings them back."),
            None => format!("{who} hidden, not closed."),
        };
        match &self.w.switched {
            Some((_, to, true)) => format!("Switched to workspace {}. {}", to + 1, undo("Your windows are")),
            Some((name, to, false)) => format!("Only the {name} switched, to workspace {}. {}", to + 1, undo("Its windows are")),
            None => "Switched.".into(),
        }
    }

    // ---- Step 2: shortcuts ----

    fn keys(&mut self, y: f32) -> f32 {
        self.card(y, |c, y| {
            let b = c.cx.c.t.b;
            let head = "Override Windows?";
            let col3 = c.cx.c.width(head, F::SmallBold).max(c.cx.seg_w(&["Yes", "No"]));
            c.cx.c.text(X0 + 16.0, y + 8.0, 16.0, "Key · click to change", F::SmallBold, b.tx2);
            c.cx.c.text(X0 + 16.0 + KEY_W + 16.0, y + 8.0, 16.0, "Action", F::SmallBold, b.tx2);
            c.cx.c.text(X0 + c.cw - 16.0 - col3, y + 8.0, 16.0, head, F::SmallBold, b.tx2);
            let mut h = 32.0;
            for i in 0..TAKES.len() {
                c.sep(y + h);
                h += c.take_row(y + h, i, col3);
            }
            c.sep(y + h);
            h += c.more(y + h, "Send and pin keys");
            if c.w.more[Step::Keys.index()] {
                for i in 0..MORE_KEYS.len() {
                    c.sep(y + h);
                    h += c.more_key(y + h, i);
                }
            }
            h
        })
    }

    /// The note under a key field: recording, a refused key, or a clash.
    fn key_note(&self, f: Field) -> Option<(NoteSpec, Vec<String>)> {
        if let Some(p) = &self.w.prompt
            && field_of_key(p.key) == Some(f)
        {
            let n = NoteSpec { tone: Tone::Warn, icon: I_WARN, title: p.title.clone(), lines: vec![P::T(p.sub.clone())], buttons: vec![(Ctl::Move, "Move it here"), (Ctl::Cancel, "Cancel")], group: 1 };
            return Some((n, p.held.clone()));
        }
        let rec = self.w.rec.filter(|r| field_of(*r) == Some(f))?;
        let n = match &self.w.refused {
            Some((t, l)) => NoteSpec { tone: Tone::Warn, icon: I_WARN, title: t.clone(), lines: vec![P::T(l.clone())], buttons: vec![(Ctl::Cancel, "Cancel")], group: 1 },
            None => {
                let (t, l) = rec_note(rec);
                NoteSpec { tone: Tone::Rec, icon: I_KEY, title: t, lines: vec![P::T(l)], buttons: vec![(Ctl::Cancel, "Cancel")], group: 1 }
            }
        };
        Some((n, vec![]))
    }

    fn ghost(f: Field) -> &'static str {
        match f {
            Field::Take(3..=5) => "1–9",
            Field::Take(0) | Field::More(0 | 1) => "← →",
            _ => "key",
        }
    }

    fn take_row(&mut self, y: f32, i: usize, col3: f32) -> f32 {
        let b = self.cx.c.t.b;
        let s = self.s;
        let t = &TAKES[i];
        let f = Field::Take(i);
        let cap = take_caption(s, i);
        let x = X0 + 16.0;
        let lx = x + KEY_W + 16.0;
        let lw = X0 + self.cw - 16.0 - col3 - 16.0 - lx;
        let lh = self.cx.label_h(lw, t.title, &[P::T(cap.clone())]);
        let top = lh.max(32.0).max(ROW - 16.0);
        let note = self.key_note(f);
        let mut h = 8.0 + top;
        if let Some((n, _)) = &note {
            let nw = self.cw - 32.0;
            h += 6.0 + self.measured(|c| c.cx.in_note(x, 0.0, nw, n));
        }
        h += 8.0;
        if note.is_some() {
            self.tint(y, h);
        }
        let fy = y + 8.0 + (top - 32.0) / 2.0;
        if t.keys.is_empty() {
            // The Copilot key: no key to record.
            let caps = vec!["Copilot".to_string()];
            self.cx.c.boxed((x, fy, KEY_W, 32.0), 6.0, b.track, Some(b.line));
            self.caps(x + 5.0, fy + 5.0, &caps, false);
        } else {
            let caps = row_caps(s, t.keys);
            let rec = note.as_ref().map(|(_, held)| (held.as_slice(), Self::ghost(f)));
            self.key_field(f, x, fy, Some(KEY_W), caps.as_deref(), rec, &cap);
        }
        // Yes or No, greyed while another action holds the key.
        let (sel, na) = if t.keys.is_empty() {
            (if s.copilot_as_right_ctrl { 0 } else { 1 }, false)
        } else {
            match held(s, t.keys) {
                Held::Desk => (0, false),
                Held::Took(_) => (0, true),
                _ => (1, false),
            }
        };
        let sw = self.cx.seg_w(&["Yes", "No"]);
        let name = format!("{}: override Windows?", t.title);
        self.cx.seg(X0 + self.cw - 16.0 - sw, y + 8.0 + (top - 28.0) / 2.0, &["Yes", "No"], Some(sel), na, |k| Ctl::SideAt(i, k == 0), Ctl::Side(i), Some((&name, &cap)));
        self.cx.label(lx, y + 8.0 + (top - lh) / 2.0, lw, t.title, &[P::T(cap.clone())]);
        if let Some((n, _)) = &note {
            self.cx.in_note(x, y + 8.0 + top + 6.0, self.cw - 32.0, n);
        }
        h
    }

    fn more_key(&mut self, y: f32, i: usize) -> f32 {
        let (title, keys) = MORE_KEYS[i];
        let f = Field::More(i);
        let caps = row_caps(self.s, keys);
        let note = self.key_note(f);
        let rec = note.as_ref().map(|(_, held)| (held.clone(), Self::ghost(f)));
        let fw = self.measured(|c| c.key_field(f, 0.0, 0.0, None, caps.as_deref(), rec.as_ref().map(|(h, g)| (h.as_slice(), *g)), ""));
        let n = note.map(|(n, _)| n);
        self.tinted_row(y, title, &[], (fw, 32.0), |c, x, y| {
            c.key_field(f, x, y, None, caps.as_deref(), rec.as_ref().map(|(h, g)| (h.as_slice(), *g)), "");
        }, n.as_ref(), n.is_some())
    }

    // ---- Step 3: behavior ----

    fn behavior(&mut self, y: f32) -> f32 {
        self.card(y, |c, y| {
            let s = c.s;
            let n = s.workspaces as i64;
            let warn = n > 2 && (c.cx.focused(Ctl::Minus("workspaces")) || c.cx.hovered(Ctl::Minus("workspaces")));
            let note = warn.then(|| c.lowering());
            let cap = format!("Every screen gets workspaces 1 to {n}.");
            let sw = c.stepper_w(n, "");
            let mut h = c.tinted_row(y, "Workspaces per screen", &[P::T(cap.clone())], (sw, 32.0), |c, x, y| {
                c.cx.stepper((None, Ctl::Minus("workspaces"), Ctl::Plus("workspaces")), x, y, (n.to_string(), String::new()), (n > 2, n < 9), false, false, Some(("Workspaces per screen", &cap)));
            }, note.as_ref(), warn);
            for (key, title, cap) in [
                ("follow_activation", "Follow a window", "When a hidden window comes to the front (an app reuses it, a dialog pops up), go to its workspace."),
                ("indicator.enabled", "Workspace indicator", "Shows the workspace for a moment after a switch, send or pin."),
                ("restore.enabled", "Restore after restart", "Reopened windows go back to their workspaces. Applies from des-k's next start."),
            ] {
                c.sep(y + h);
                h += c.toggle_row(y + h, key, title, cap);
            }
            c.sep(y + h);
            h += c.more(y + h, "Animations, indicator style and timing, restore time, overview");
            if c.w.more[Step::Behavior.index()] {
                let rows: [(&'static str, &str, &str); 9] = [
                    ("fastest", "Skip animations", "Every des-k fade and move happens at once, and the overview leaves out its window shadows. Windows stops animating the settings, welcome and hints windows as they open and close. Windows' own Animation effects setting doesn't change des-k."),
                    ("indicator.style", "Indicator style", "The number and a bar per workspace, with the screen's name when you have two or more."),
                    ("indicator.hold_ms", "Indicator stays", "How long it shows. 0 to 5000 ms."),
                    ("indicator.fade_ms", "Indicator fades out", "How long it takes to go. 0 to 2000 ms."),
                    ("restore.minutes", "Restore runs for", "Apps you open later stay on the workspace you're on."),
                    ("overview.labels", "Window names in overview", ""),
                    ("overview.animation", "Overview animation", ""),
                    ("overview.fade_ms", "Overview fades for", "How long the fade in and the fade out each take. 0 to 1000 ms."),
                    ("overview.live", "Live overview", "Off: shows windows as they were when the overview opened."),
                ];
                for (key, title, cap) in rows {
                    c.sep(y + h);
                    h += c.setting(y + h, key, title, cap);
                }
            }
            h
        })
    }

    /// What lowering the count moves, before you press − (as the settings page warns).
    fn lowering(&self) -> NoteSpec {
        let n = self.s.workspaces;
        let list = self.w.pics.view.removals.first().cloned().unwrap_or_default();
        let win = |k: usize| match k {
            0 => "none".to_string(),
            1 => "1 window".to_string(),
            k => format!("{k} windows"),
        };
        let each: Vec<String> = list.iter().filter(|m| !m.unplugged || m.windows > 0).map(|m| format!("{}{}: {}", m.name, if m.unplugged { " (not plugged in)" } else { "" }, win(m.windows))).collect();
        let what = if each.is_empty() { String::new() } else { format!(" ({})", each.join("; ")) };
        NoteSpec {
            tone: Tone::Warn,
            icon: I_WARN,
            title: format!("Lowering to {} removes workspace {n} from every screen", n - 1),
            lines: vec![P::T(format!("Its windows move to workspace {}{what}. A screen showing workspace {n} switches to {}. Raising it again won't move them back.", n - 1, n - 1))],
            buttons: vec![],
            group: 1,
        }
    }

    fn toggle_row(&mut self, y: f32, key: &'static str, title: &str, cap: &str) -> f32 {
        let on = sp::rows::value(self.s, key) != 0;
        let sw = self.cx.switch_w(on);
        let caps: Vec<P> = if cap.is_empty() { vec![] } else { vec![P::T(cap.into())] };
        self.row(y, title, &caps, (sw, 20.0), |c, x, y| c.cx.switch(Ctl::Toggle(key), x, y, on, false, Some((title, cap))), None)
    }

    /// A More row: a switch, a number or a choice, greyed with every reason when not in use.
    pub(super) fn setting(&mut self, y: f32, key: &'static str, title: &str, cap: &str) -> f32 {
        let Some(r) = sp::rows::find_row(key) else { return 0.0 };
        let v = sp::rows::value(self.s, key);
        let why = sp::rows::unused(self.s, key);
        let na = !why.is_empty() && matches!(r.kind, Kind::Step { .. });
        let mut caps: Vec<P> = if cap.is_empty() { vec![] } else { vec![P::T(cap.into())] };
        if na {
            let reasons: Vec<&str> = why.iter().map(|w| match w {
                sp::rows::Why::AnimationNone => "Overview animation is None",
                sp::rows::Why::Skip => "Skip animations is on",
                sp::rows::Why::IndicatorOff => "the indicator is off",
                sp::rows::Why::Dots => "the dots show no names",
            }).collect();
            caps.push(P::Why(format!(" Not in use: {}.", reasons.join(", and "))));
        }
        let help: String = caps.iter().map(|p| match p {
            P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str(),
            P::Icon(..) => "",
        }).collect();
        match r.kind {
            Kind::Toggle => self.toggle_row(y, key, title, cap),
            Kind::Step { lo, hi, unit, .. } => {
                let sw = self.stepper_w(v, unit);
                self.row(y, title, &caps, (sw, 32.0), |c, x, y| c.cx.stepper((None, Ctl::Minus(key), Ctl::Plus(key)), x, y, (v.to_string(), if unit.is_empty() { String::new() } else { format!(" {unit}") }), (v > lo, v < hi), na, false, Some((title, &help))), None)
            }
            Kind::Choice(_) | Kind::Pair(..) => {
                let list = words_of(r);
                let sel = if matches!(r.kind, Kind::Pair(..)) { (v == 0) as usize } else { v as usize };
                let sw = self.cx.seg_w(&list);
                self.row(y, title, &caps, (sw, 28.0), |c, x, y| c.cx.seg(x, y, &list, Some(sel), false, |i| Ctl::Choice(key, i), Ctl::Pick(key), Some((title, &help))), None)
            }
            Kind::Ladder(_) => 0.0, // the first run asks nothing about slideshows
        }
    }

    // ---- Step 5: done ----

    fn done(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let s = self.s;
        let pad = self.w.pics.pad.is_none_or(|p| p.pad);
        let pv = self.pad();
        let mut h = self.card(y, |c, y| {
            let mut h = 0.0;
            let tp = if pad {
                let (icon, col) = if pv.ready { (I_OK, b.good) } else { (I_WARN, b.warn) };
                vec![P::Icon(icon, col), P::T(format!(" {}", pv.done_line))]
            } else {
                vec![P::T("No precision touchpad found. Plug one in any time and set it up in des-k settings › Touchpad.".into())]
            };
            let button = pad && pv.done_button;
            let bw = if button { c.cx.btn_w("Open Windows Settings", None) } else { 0.0 };
            h += c.row(y, "Touchpad", &tp, (bw, 32.0), |c, x, y| {
                if button {
                    c.button(Ctl::Pad(Pad::Windows), x, y, "Open Windows Settings");
                }
            }, None);
            c.sep(y + h);
            let cap = "Every key and gesture. Right-click the des-k icon by the clock, then Shortcut hints. Don't see it? Click ^ first.";
            let bw = c.cx.btn_w("Show", None);
            h += c.row(y + h, "Shortcut hints", &[P::T(cap.into())], (bw, 32.0), |c, x, y| {
                c.button(Ctl::Show, x, y, "Show");
            }, None);
            c.sep(y + h);
            h += c.hints_row(y + h);
            h
        });
        h += 16.0;
        h += self.cx.c.para(X0, y + h, self.cw, 20.0, &[("Your main shortcuts", F::Bold, b.tx)]) + 8.0;
        h += self.card(y + h, |c, y| {
            let main: [(&str, &[&'static str]); 4] = [
                ("Switch workspace", &["shortcuts.previous", "shortcuts.next"]),
                ("Send window to next or previous", &["shortcuts.send_previous", "shortcuts.send_next"]),
                ("Open overview", &["shortcuts.overview"]),
                ("Find a window", &["shortcuts.finder"]),
            ];
            let mut h = 0.0;
            for (i, (title, keys)) in main.iter().enumerate() {
                if i > 0 {
                    c.sep(y + h);
                }
                let caps = row_caps(s, keys);
                let kw = caps.as_ref().map_or_else(|| c.cx.c.width("No key", F::Small), |k| c.caps_w(k));
                let top = 28.0;
                let ry = y + h + 6.0;
                c.cx.c.text(X0 + 16.0, ry, top, title, F::Body, b.tx);
                match &caps {
                    Some(k) => {
                        c.caps(X0 + c.cw - 16.0 - kw, ry + 3.0, k, false);
                    }
                    None => c.cx.c.text(X0 + c.cw - 16.0 - kw, ry, top, "No key", F::Small, b.tx2),
                }
                c.text_node(UIA_ListItemControlTypeId, &format!("{title}: {}", caps.as_ref().map_or("no key".into(), |k| words(k))), None);
                h += 40.0;
            }
            c.sep(y + h);
            let line = taken_line(s);
            let lh = c.cx.c.para(X0 + 16.0, y + h + 6.0, c.cw - 32.0, 16.0, &[(&line, F::Small, b.tx2)]);
            c.text_node(UIA_TextControlTypeId, &line, None);
            h + 6.0 + lh.max(28.0) + 6.0
        });
        h
    }

    /// The hints key: Set key, the key and Change, or recording it.
    fn hints_row(&mut self, y: f32) -> f32 {
        let s = self.s;
        let f = Field::Hints;
        let caps = row_caps(s, &["shortcuts.hints"]);
        let note = self.key_note(f);
        let cap = match (&note, &caps) {
            (Some(_), _) => vec![],
            (None, Some(_)) => vec![P::T("Opens the hints on the screen under your mouse.".into())],
            (None, None) => vec![P::T("Not set.".into())],
        };
        let rec = note.as_ref().map(|(_, held)| held.clone());
        let n = note.map(|(n, _)| n);
        let (cw, ch) = match (&rec, &caps) {
            (Some(held), _) => (self.measured(|c| c.key_field(f, 0.0, 0.0, None, None, Some((held.as_slice(), "key")), "")), 32.0),
            (None, Some(k)) => (self.caps_w(k) + 8.0 + self.cx.btn_w("Change", None), 32.0),
            (None, None) => (self.cx.btn_w("Set key", None), 32.0),
        };
        self.tinted_row(y, "Shortcut hints key", &cap, (cw, ch), |c, x, y| match (&rec, &caps) {
            (Some(held), _) => {
                c.key_field(f, x, y, None, None, Some((held.as_slice(), "key")), "");
            }
            (None, Some(k)) => {
                let kw = c.caps(x, y + 5.0, k, false);
                c.button(Ctl::Field(f), x + kw + 8.0, y, "Change");
            }
            (None, None) => {
                c.button(Ctl::Field(f), x, y, "Set key");
            }
        }, n.as_ref(), n.is_some())
    }

    // ---- The footer ----

    fn footer(&mut self) {
        let b = self.cx.c.t.b;
        let (vw, vh) = (self.w.vw, self.w.vh);
        let y = vh - FOOT;
        self.cx.c.fill_rr((0.0, y, vw, FOOT), 0.0, op(b.band));
        self.cx.c.rule(0.0, y, vw, op(b.line));
        let by = y + 16.0;
        let mut x = X0;
        let step = self.w.step;
        if step == Step::Try && !self.w.tried.iter().all(|t| *t) {
            x += self.button(Ctl::Skip, x, by, "Skip") + 8.0;
        }
        if step != Step::Try {
            self.button(Ctl::Back, x, by, "Back");
        }
        let next = if step == Step::Done { "Done" } else { "Next" };
        let nw = self.cx.btn_w(next, None);
        self.cx.primary(Ctl::Next, vw - X0 - nw, by, next, self.cx.group);
    }
}

/// "Taken over from Windows: …", from live settings.
pub(super) fn taken_line(s: &Settings) -> String {
    let mut keys: Vec<String> = vec![];
    let has = |k: &str| sp::rows::shortcut(k).is_some_and(|sh| sp::rows::has_win(s, sh));
    for (k, w) in [("shortcuts.overview", "Win Tab"), ("shortcuts.finder", "Win F")] {
        if has(k) {
            keys.push(w.into());
        }
    }
    if has("shortcuts.previous") && has("shortcuts.next") {
        keys.push("Win Ctrl ← →".into());
    } else if has("shortcuts.previous") {
        keys.push("Win Ctrl ←".into());
    } else if has("shortcuts.next") {
        keys.push("Win Ctrl →".into());
    }
    match (has("shortcuts.jump"), has("shortcuts.send_to"), has("shortcuts.send_to_stay")) {
        (true, true, true) => keys.push("Win 1–9 (also with Shift or Ctrl)".into()),
        (j, sh, c) => {
            for (on, w) in [(j, "Win 1–9"), (sh, "Win Shift 1–9"), (c, "Win Ctrl 1–9")] {
                if on {
                    keys.push(w.into());
                }
            }
        }
    }
    if s.copilot_as_right_ctrl {
        keys.push("the Copilot key".into());
    }
    match keys.len() {
        0 => "des-k takes no keys from Windows. Change any in des-k settings › Shortcuts.".into(),
        1 => format!("Taken over from Windows: {}. Change any in des-k settings › Shortcuts.", keys[0]),
        n => format!("Taken over from Windows: {} and {}. Change any in des-k settings › Shortcuts.", keys[..n - 1].join(", "), keys[n - 1]),
    }
}

/// A choice row's words, as the first run shows them.
pub(super) fn words_of(r: &sp::rows::Row) -> Vec<&'static str> {
    match r.kind {
        Kind::Choice(list) => list.iter().map(|(_, l)| *l).collect(),
        Kind::Pair(on, off) => vec![on, off],
        _ => vec![],
    }
}

/// The whole window, back to front: the step, then what sits above and below it, the scroll
/// thumb and the open menu.
pub(super) fn compose(c: &mut Canvas, w: &Win) -> Out {
    let s = settings::get();
    let cw = w.vw - 2.0 * X0;
    let cx = controls::Ctx { c, hits: vec![], said: vec![], ro: false, focus: w.focus, hover: w.hover, kb: w.kb, group: 0, style: Style::FirstRun };
    let mut x = Ctx { cx, w, s: &s, cw, out: Out { hits: vec![], content: 0.0, mid: (0.0, 0.0), said: vec![], texts: vec![] } };
    let b = x.cx.c.t.b;
    let head = x.measured(|c| c.head());
    let (top, bottom) = (head + 16.0, w.vh - FOOT - 8.0);
    x.out.mid = (top, bottom);
    x.cx.group = 1;
    let y0 = top - w.scroll;
    let content = match w.step {
        Step::Try => x.try_it(y0),
        Step::Keys => x.keys(y0),
        Step::Behavior => x.behavior(y0),
        Step::Pad => x.touchpad(y0),
        Step::Done => x.done(y0),
    };
    x.out.content = content;
    // The head and the footer cover the step scrolled under them; a line marks a cut edge.
    x.cx.c.fill_rr((0.0, 0.0, w.vw, top), 0.0, op(b.bg));
    x.cx.c.fill_rr((0.0, bottom, w.vw, w.vh - bottom), 0.0, op(b.bg));
    // Drawn after the step, it still comes first for Narrator and the Tab order.
    x.cx.group = 0;
    let (t0, s0) = (x.out.texts.len(), x.cx.said.len());
    x.head();
    let k = x.out.texts.len() - t0;
    x.out.texts.rotate_right(k);
    let k = x.cx.said.len() - s0;
    x.cx.said.rotate_right(k);
    let view = bottom - top;
    if w.scroll > 0.5 {
        x.cx.c.rule(X0 - 1.0, top, cw + 2.0, op(b.line));
    }
    if w.scroll < content - view - 1.0 {
        x.cx.c.rule(X0 - 1.0, bottom - 1.0, cw + 2.0, op(b.line));
    }
    x.cx.group = 2;
    x.footer();
    if let Some((ty, th)) = thumb(w.scroll, content, view, top) {
        x.cx.c.fill_rr((w.vw - 5.0, ty, 2.0, th), 1.0, op(b.tx3));
    }
    x.cx.group = 3;
    x.menu();
    // The menu's hits first.
    x.cx.hits.sort_by_key(|h| std::cmp::Reverse(h.group == 3));
    Out { hits: x.cx.hits, said: x.cx.said, ..x.out }
}

/// The thumb's top and height, when the step holds more than its view.
pub(super) fn thumb(scroll: f32, content: f32, view: f32, top: f32) -> Option<(f32, f32)> {
    if content - view <= 1.0 {
        return None;
    }
    let th = ((view - 8.0) * view / content).max(24.0);
    let ty = top + 4.0 + (view - 8.0 - th) * (scroll / (content - view).max(1.0));
    Some((ty, th))
}

#[cfg(test)]
mod tests;
