// The shared widgets: focus cues, fields, buttons, switches, steppers, segments, captions,
// labels, chips and notes, drawn through `Ctx`; and rows, cards and their headers, drawn through
// a screen's own context (spec 0017, 0035).
use super::{Ctx, Id, NoteSpec, P, Screen, Style, Tone};
use crate::ui::{self, canvas::Bx, color::{mix, op}, fonts::F};
use windows::Win32::Graphics::GdiPlus::DashStyleDash;
use windows::Win32::UI::Accessibility::{UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId, UIA_ComboBoxControlTypeId, UIA_HyperlinkControlTypeId};

const I_WARN: &str = "\u{E7BA}";
const I_ADD: &str = "\u{E710}";
const I_MINUS: &str = "\u{E738}";
const ROW: f32 = 52.0; // a row's least height

impl<C: Id, E: Copy> Ctx<'_, '_, C, E> {
    /// The hover cue on a tile or swatch that opens the picker: an inset 1.5 dip edge in `ink`.
    pub(crate) fn cue(&self, (x, y, w, h): Bx, ink: u32) {
        self.c.stroke_rr((x + 0.75, y + 0.75, w - 1.5, h - 1.5), 3.25, 1.5, ui::color::alpha(ink, 0.6));
    }

    /// The focus ring: 2 dip in the text color, 2 dip out (AC-SET-9).
    pub(crate) fn ring(&self, (x, y, w, h): Bx, r: f32, ctl: C) {
        if self.focused(ctl) {
            self.c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), r + 3.0, 2.0, op(self.c.t.b.tx));
        }
    }

    pub(crate) fn icon(&self, x: f32, y: f32, lh: f32, icon: &str, f: F, color: u32) {
        self.c.text(x, y, lh, icon, f, color);
    }

    /// A field's fill with its edge: 1 dip all round and the darker bottom edge you chose
    /// (AC-SET-9); high contrast has the edge alone.
    pub(crate) fn field(&self, (x, y, w, h): Bx, fill: u32, ring: Option<u32>) {
        let b = self.c.t.b;
        match ring {
            Some(col) => {
                self.c.fill_rr((x, y, w, h), 6.0, op(col));
                self.c.fill_rr((x + 2.0, y + 2.0, w - 4.0, h - 4.0), 4.0, op(fill));
            }
            None if self.c.t.hc => self.c.boxed((x, y, w, h), 6.0, fill, Some(b.line)),
            None => {
                self.c.fill_rr((x, y, w, h), 6.0, op(b.tx3));
                self.c.fill_rr((x, y, w, h - 1.0), 6.0, op(b.line));
                self.c.fill_rr((x + 1.0, y + 1.0, w - 2.0, h - 3.0), 5.0, op(fill));
            }
        }
    }

    /// A button label's inset on each side.
    fn pad(&self) -> f32 {
        if self.style == Style::FirstRun { 15.0 } else { 14.0 }
    }

    pub(crate) fn btn_w(&self, label: &str, icon: Option<&str>) -> f32 {
        self.c.width(label, F::Body) + 2.0 * self.pad() + icon.map_or(0.0, |_| 18.0)
    }

    /// A button: 32 dip, its label, an icon before it if any. `na`: greyed. Returns its width.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn button(&mut self, ctl: C, x: f32, y: f32, label: &str, icon: Option<&str>, na: bool, group: E) -> f32 {
        let b = self.c.t.b;
        let w = self.btn_w(label, icon);
        let off = na || (self.ro && ctl.writes());
        let fill = if self.hovered(ctl) && !off { mix(b.card, b.tx, 0.05) } else { b.card };
        match self.style {
            Style::Settings => self.field((x, y, w, 32.0), fill, None),
            Style::FirstRun => self.c.boxed((x, y, w, 32.0), 6.0, fill, Some(b.line)),
        }
        let ink = if off { b.tx3 } else { b.tx };
        let mut tx = x + self.pad();
        if let Some(i) = icon {
            self.icon(tx, y, 32.0, i, F::IconSmall, ink);
            tx += 18.0;
        }
        self.c.text(tx, y, 32.0, label, F::Body, ink);
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), group);
        self.say(ctl, UIA_ButtonControlTypeId, label, "", off, (None, None));
        w
    }

    /// The purple main button, in the first run's look (its Next, What's new's Got it).
    /// Returns its width.
    pub(crate) fn primary(&mut self, ctl: C, x: f32, y: f32, label: &str, group: E) -> f32 {
        let b = self.c.t.b;
        let w = self.btn_w(label, None);
        let hot = self.hovered(ctl);
        self.c.fill_rr((x, y, w, 32.0), 6.0, op(if hot && !self.c.t.hc { mix(b.acc, b.tx, 0.12) } else { b.acc }));
        self.c.text(x + self.pad(), y, 32.0, label, F::Body, b.on_acc);
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), group);
        self.say(ctl, UIA_ButtonControlTypeId, label, "", false, (None, None));
        w
    }

    /// The word and the switch: 40 × 20, with On or Off before it. `x`: the word's left edge.
    pub(crate) fn switch_w(&self, on: bool) -> f32 {
        self.c.width(if on { "On" } else { "Off" }, F::Body) + 10.0 + 40.0
    }

    /// `na`: greyed, kept but not in use, drawn as read only. `say`: its name and help for
    /// Narrator, when the row doesn't say it.
    pub(crate) fn switch(&mut self, ctl: C, x: f32, y: f32, on: bool, na: bool, say: Option<(&str, &str)>) {
        let b = self.c.t.b;
        let label = if on { "On" } else { "Off" };
        let lw = self.c.width(label, F::Body);
        self.c.text(x, y, 20.0, label, F::Body, b.tx2);
        let (sx, ro) = (x + lw + 10.0, self.ro || na);
        let track = (sx, y, 40.0, 20.0);
        if on {
            self.c.fill_rr(track, 10.0, op(if ro { b.tx3 } else { b.acc }));
            self.c.circle(sx + 30.0, y + 10.0, 6.0, Some(op(b.on_acc)), None);
        } else {
            let fill = if self.hovered(ctl) && !ro { mix(b.card, b.tx, 0.06) } else { b.card };
            self.c.boxed(track, 10.0, fill, Some(if ro { b.tx3 } else { b.tx2 }));
            self.c.circle(sx + 10.0, y + 10.0, 6.0, Some(op(if ro { b.tx3 } else { b.tx2 })), None);
        }
        self.ring(track, 10.0, ctl);
        self.hit(ctl, (x, y - 6.0, lw + 50.0, 32.0), self.group);
        if let Some((name, help)) = say {
            self.say(ctl, UIA_CheckBoxControlTypeId, name, help, ro, (Some(on), None));
        }
    }

    pub(crate) fn stepper_w(&self, text: &str) -> f32 {
        64.0 + (self.c.width(text, F::Body) + 16.0).max(56.0)
    }

    /// − value +: with a `stop`, one keyboard stop (AC-SET-3: steppers only) and `minus` and
    /// `plus` its click areas; without one, − and + are each a stop, ringed alone. The value
    /// comes as its number and its unit words. `na`: greyed, kept but not in use; its − and +
    /// do nothing but stay stops, so you hear why. `grey`: a value that follows a default, still
    /// live (spec 0028). `say`: the title and help Narrator reads with − and +.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn stepper(&mut self, (stop, minus, plus): (Option<C>, C, C), x: f32, y: f32, (num, unit): (String, String), (down, up): (bool, bool), na: bool, grey: bool, say: Option<(&str, &str)>) {
        let b = self.c.t.b;
        let (nw, uw) = (self.c.width(&num, F::Body), self.c.width(&unit, F::Small));
        let first = self.style == Style::FirstRun;
        let w = 64.0 + (nw + uw + if first { 8.0 } else { 16.0 }).max(56.0);
        if first {
            self.c.boxed((x, y, w, 32.0), 6.0, b.card, Some(if na && self.c.t.hc { b.tx3 } else { b.line }));
        } else {
            self.field((x, y, w, 32.0), b.card, None);
        }
        let mid = w - 64.0;
        let tx = x + 32.0 + (mid - nw - uw) / 2.0;
        let off = na || self.ro;
        self.c.text(tx, y, 32.0, &num, F::Body, if off { b.tx3 } else if grey { b.tx2 } else { b.tx });
        self.c.text(tx + nw, y, 32.0, &unit, F::Small, if off { b.tx3 } else { b.tx2 });
        for (ctl, ix, icon, live, what) in [(minus, x, I_MINUS, down, "Less"), (plus, x + w - 32.0, I_ADD, up, "More")] {
            let live = live && !off;
            if self.hovered(ctl) && live {
                self.c.fill_rr((ix + 4.0, y + 4.0, 24.0, 24.0), 4.0, op(mix(b.card, b.tx, 0.08)));
            }
            let iw = self.c.width(icon, F::IconTiny);
            self.icon(ix + (32.0 - iw) / 2.0, y, 32.0, icon, F::IconTiny, if live { b.tx } else { b.tx3 });
            if stop.is_none() && self.focused(ctl) {
                self.c.stroke_rr((ix + 1.0, y + 1.0, 30.0, 30.0), 5.0, 2.0, op(b.tx));
            }
            self.hit(ctl, (ix, y, 32.0, 32.0), self.group);
            if let Some((title, help)) = say {
                self.say(ctl, UIA_ButtonControlTypeId, &format!("{what}: {title}"), help, off, (None, Some(format!("{num}{unit}"))));
            }
        }
        if let Some(stop) = stop {
            self.ring((x, y, w, 32.0), 6.0, stop);
        }
    }

    pub(crate) fn seg_w(&self, words: &[&str]) -> f32 {
        4.0 + words.iter().map(|l| self.c.width(l, F::Small) + 20.0).sum::<f32>() + 2.0 * (words.len() as f32 - 1.0)
    }

    /// Choice buttons in one track, 28 dip; the chosen one raised with the ring (AC-SET-9).
    /// `na`: greyed. Word `i` is the control `ctl(i)`; `stop` is the one keyboard stop.
    /// `say`: its name and help for Narrator, when the row doesn't say it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn seg(&mut self, x: f32, y: f32, words: &[&str], sel: Option<usize>, na: bool, ctl: impl Fn(usize) -> C, stop: C, say: Option<(&str, &str)>) {
        let b = self.c.t.b;
        let w = self.seg_w(words);
        self.c.boxed((x, y, w, 28.0), 6.0, b.track, Some(b.line));
        let off = na || self.ro;
        let mut ox = x + 2.0;
        for (i, label) in words.iter().enumerate() {
            let iw = self.c.width(label, F::Small) + 20.0;
            let bx = (ox, y + 2.0, iw, 24.0);
            let chosen = sel == Some(i) && !na;
            let fg = if chosen {
                if self.c.t.hc {
                    self.c.fill_rr(bx, 4.0, op(if self.ro { b.tx3 } else { b.hi }));
                    b.on_acc
                } else {
                    let edge = if self.style == Style::FirstRun { b.line } else { b.tx3 };
                    self.c.fill_rr((bx.0 - 1.0, bx.1 - 1.0, bx.2 + 2.0, bx.3 + 2.0), 5.0, op(edge));
                    self.c.fill_rr(bx, 4.0, op(b.hi));
                    if self.ro { b.tx3 } else { b.tx }
                }
            } else {
                if self.hovered(ctl(i)) && !off {
                    self.c.fill_rr(bx, 4.0, op(mix(b.track, b.tx, 0.06)));
                }
                if off { b.tx3 } else { b.tx2 }
            };
            self.c.text(ox + 10.0, y + 2.0, 24.0, label, if chosen { F::SmallBold } else { F::Small }, fg);
            self.hit(ctl(i), bx, self.group);
            ox += iw + 2.0;
        }
        self.ring((x, y, w, 28.0), 6.0, stop);
        if let Some((name, help)) = say {
            let shown = sel.and_then(|i| words.get(i)).copied().unwrap_or("");
            self.say(stop, UIA_ComboBoxControlTypeId, name, help, off, (None, Some(shown.to_string())));
        }
    }

    /// A caption's pieces as one wrapped text, 12/16; links get their hits, hover and ring.
    /// Returns its height.
    pub(crate) fn caption(&mut self, x: f32, y: f32, w: f32, pieces: &[P<C>]) -> f32 {
        let b = self.c.t.b;
        // A warning is two runs, its mark and its words; `at` is each piece's first run.
        let (mut runs, mut at): (Vec<(&str, F, u32)>, Vec<usize>) = (vec![], vec![]);
        for p in pieces {
            at.push(runs.len());
            match p {
                P::T(s) => runs.push((s.as_str(), F::Small, b.tx2)),
                P::Why(s) => runs.push((s.as_str(), F::Small, b.tx)),
                P::Link(s, _) => runs.push((s.as_str(), F::SmallBold, b.link)),
                P::Mono(s) => runs.push((s.as_str(), F::Mono, b.tx2)),
                P::Warn(s) => {
                    runs.push((I_WARN, F::IconTiny, b.warn));
                    runs.push((s.as_str(), F::Small, b.warn));
                }
                P::Icon(s, col) => runs.push((*s, F::IconSmall, *col)),
            }
        }
        let (h, boxes) = self.c.para_boxes(x, y, w, 16.0, &runs);
        for (p, &i) in pieces.iter().zip(&at) {
            let P::Link(text, ctl) = p else { continue };
            let bxs = &boxes[i];
            for &(bx, by, bw, bh) in bxs {
                if self.hovered(*ctl) || self.c.t.hc {
                    self.c.rule(bx, by + bh - 2.0, bw, op(b.link));
                }
                self.hit(*ctl, (bx - 2.0, by, bw + 4.0, bh), self.group);
            }
            if let Some(&first) = bxs.first() {
                self.ring((first.0 - 1.0, first.1, first.2 + 2.0, first.3), 2.0, *ctl);
            }
            self.say(*ctl, UIA_HyperlinkControlTypeId, text.trim(), "", false, (None, None));
        }
        h
    }

    pub(crate) fn label_h(&mut self, w: f32, title: &str, cap: &[P<C>]) -> f32 {
        self.measured(|c| c.label(0.0, 0.0, w, title, cap))
    }

    /// A row's name, 600 14/20, and its caption under it.
    pub(crate) fn label(&mut self, x: f32, y: f32, w: f32, title: &str, cap: &[P<C>]) -> f32 {
        let b = self.c.t.b;
        let mut h = 0.0;
        if !title.is_empty() {
            h += self.c.para(x, y, w, 20.0, &[(title, F::Bold, b.tx)]);
        }
        if !cap.is_empty() {
            h += self.caption(x, y + h, w, cap);
        }
        h
    }

    /// The purple chip after a heading.
    pub(crate) fn chip(&mut self, x: f32, y: f32, icon: &str, text: &str) {
        let b = self.c.t.b;
        let w = 10.0 + 12.0 + 6.0 + self.c.width(text, F::Small) + 10.0;
        let (fill, edge, ic) = if self.c.t.hc { (b.card, b.tx, b.tx) } else { (mix(b.card, b.acc_b, 0.10), mix(b.card, b.acc_b, 0.30), b.acc_b) };
        self.c.boxed((x, y, w, 24.0), 12.0, fill, Some(edge));
        self.icon(x + 10.0, y, 24.0, icon, F::IconSmall, ic);
        self.c.text(x + 28.0, y, 24.0, text, F::Small, b.tx);
    }

    fn tone_color(&self, tone: Tone) -> u32 {
        let b = self.c.t.b;
        match tone {
            Tone::Info => b.tx2,
            Tone::Warn => b.warn,
            Tone::Good => b.good,
            Tone::Rec => b.acc,
        }
    }

    /// A note's text and buttons, from `x`; returns its height. Buttons sit on the right when
    /// they leave the text room, else under it.
    pub(crate) fn note_body(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec<C, E>) -> f32 {
        let col = self.tone_color(n.tone);
        let b = self.c.t.b;
        self.icon(x, y + 2.0, 16.0, n.icon, F::Icon, col);
        let bw: f32 = n.buttons.iter().map(|(_, l)| self.btn_w(l, None) + 8.0).sum();
        let beside = bw > 0.0 && bw < w * if self.style == Style::FirstRun { 0.42 } else { 0.4 };
        let tx = x + 28.0;
        let tw = if beside { w - 28.0 - bw - 4.0 } else { w - 28.0 };
        // No title: the caption alone, on the icon's line (a hex box's typing line).
        let mut h = if n.title.is_empty() { 2.0 } else { self.c.para(tx, y, tw, 20.0, &[(&n.title, F::Bold, b.tx)]) };
        if !n.lines.is_empty() {
            h += self.caption(tx, y + h, tw, &n.lines);
        }
        if bw > 0.0 {
            let by = if beside { y + ((h - 32.0) / 2.0).max(0.0) } else { y + h + 8.0 };
            let mut bx = if beside { x + w - bw + 8.0 } else { tx };
            for (ctl, label) in &n.buttons {
                bx += self.button(*ctl, bx, by, label, None, false, n.group) + 8.0;
            }
            h = if beside { h.max(32.0) } else { h + 8.0 + 32.0 };
        }
        h
    }

    /// A note on its own: card colored, with its edge (above a section, a banner, Notes).
    pub(crate) fn note(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec<C, E>) -> f32 {
        let h = self.measured(|c| c.note_body(x + 12.0, y + 10.0, w - 24.0, n)) + 20.0;
        let b = self.c.t.b;
        self.c.boxed((x, y, w, h), 6.0, b.card, Some(b.line));
        self.note_body(x + 12.0, y + 10.0, w - 24.0, n);
        h
    }

    /// A note inside a row: a dashed line over it, on the row's own background.
    pub(crate) fn in_note(&mut self, x: f32, y: f32, w: f32, n: &NoteSpec<C, E>) -> f32 {
        let b = self.c.t.b;
        if self.style == Style::FirstRun {
            let mut dx = 0.0;
            while dx < w {
                self.c.fill_rr((x + dx, y, 3.0f32.min(w - dx), 1.0), 0.0, op(b.line));
                dx += 6.0;
            }
        } else if self.c.draw {
            self.c.stroke_dash((x, y, w, 0.0), 0.0, 1.0, op(b.line), DashStyleDash);
        }
        10.0 + self.note_body(x, y + 10.0, w, n) + 2.0
    }
}

/// Rows and cards in a screen's column. Every screen context has them.
pub(crate) trait Cards<'a, 'f: 'a>: Screen<'a, 'f> {
    /// One row in a card: name and caption on the left, the control on the right, both
    /// centred; under them, a note if one is given. The control is drawn first, so a link in
    /// the caption is its own stop after it (Reset under its row). Returns the row's height.
    #[allow(clippy::too_many_arguments)]
    fn row(&mut self, y: f32, title: &str, cap: &[P<Self::C>], (cw, ch): (f32, f32), ctl: impl FnOnce(&mut Self, f32, f32), note: Option<&NoteSpec<Self::C, Self::E>>) -> f32 {
        let (x0, col) = self.column();
        let (x, w) = (x0 + 16.0, col - 32.0);
        let lw = if cw > 0.0 { w - cw - 16.0 } else { w };
        let lh = self.cx().label_h(lw, title, cap);
        let top = lh.max(ch).max(ROW - 16.0);
        ctl(self, x + w - cw, y + 8.0 + (top - ch) / 2.0);
        self.cx().label(x, y + 8.0 + (top - lh) / 2.0, lw, title, cap);
        let mut h = 8.0 + top;
        if let Some(n) = note {
            h += 6.0 + self.cx().in_note(x, y + h + 6.0, w, n);
        }
        h + 8.0
    }

    /// A card's line between two rows.
    fn sep(&mut self, y: f32) {
        let (x0, col) = self.column();
        let c = &self.cx().c;
        c.rule(x0 + 1.0, y, col - 2.0, op(c.t.b.line));
    }

    /// A card around `body`, which draws from its top and returns its height.
    fn card(&mut self, y: f32, body: impl Fn(&mut Self, f32) -> f32) -> f32 {
        let h = self.measured(|c| body(c, y));
        let (x0, col) = self.column();
        let c = &self.cx().c;
        let b = c.t.b;
        c.boxed((x0, y, col, h), 8.0, b.card, Some(b.line));
        body(self, y);
        if self.cx().style == Style::FirstRun {
            // The edge again, over rows that tint up to it.
            self.cx().c.stroke_rr((x0 + 0.5, y + 0.5, col - 1.0, h - 1.0), 8.0, 1.0, op(b.line));
        }
        h
    }

    /// A card's tinted header: its title, and a caption on the right. `n`: the fingers' circle.
    fn header(&mut self, y: f32, title: &str, n: Option<u8>, cap: &str) -> f32 {
        let (x0, col) = self.column();
        let c = &self.cx().c;
        let b = c.t.b;
        let tint = if c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.09) };
        c.fill_rr((x0 + 1.0, y + 1.0, col - 2.0, 36.0), 7.0, op(tint));
        c.fill_rr((x0 + 1.0, y + 20.0, col - 2.0, 17.0), 0.0, op(tint));
        c.rule(x0 + 1.0, y + 37.0, col - 2.0, op(b.line));
        let mut x = x0 + 16.0;
        if let Some(n) = n {
            c.circle(x + 11.0, y + 19.0, 11.0, Some(op(b.acc_b)), None);
            let d = n.to_string();
            let dw = c.width(&d, F::SmallBold);
            c.text(x + 11.0 - dw / 2.0, y + 8.0, 22.0, &d, F::SmallBold, b.on_acc_b);
            x += 32.0;
        }
        c.text(x, y + 8.0, 22.0, title, F::Bold, b.tx);
        if !cap.is_empty() {
            let cw = c.width(cap, F::Small);
            c.text(x0 + col - 16.0 - cw, y + 8.0, 22.0, cap, F::Small, b.tx2);
        }
        38.0
    }
}

impl<'a, 'f: 'a, S: Screen<'a, 'f>> Cards<'a, 'f> for S {}
