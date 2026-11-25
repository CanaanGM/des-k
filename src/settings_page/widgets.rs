// The page's widgets: focus cues, fields, buttons, switches, steppers, segments, captions,
// rows, cards and chips (spec 0017). They move to `controls` in spec 0035 step 6.
use super::*;
use super::cards::NoteSpec;
use super::rows::{MINUTES, minutes};

impl Ctx<'_, '_> {
    /// The pointer is on the last workspace's tile, on any screen: its × shows.
    /// The hover cue on a tile or swatch that opens the picker: an inset 1.5 dip edge in `ink`.
    pub(super) fn cue(&self, (x, y, w, h): Bx, ink: u32) {
        self.c.stroke_rr((x + 0.75, y + 0.75, w - 1.5, h - 1.5), 3.25, 1.5, ui::alpha(ink, 0.6));
    }

    /// The focus ring: 2 dip in the text color, 2 dip out (AC-SET-9).
    pub(super) fn ring(&self, (x, y, w, h): Bx, r: f32, ctl: Ctl) {
        if self.focused(ctl) {
            self.c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), r + 3.0, 2.0, op(self.c.t.b.tx));
        }
    }

    pub(super) fn icon(&self, x: f32, y: f32, lh: f32, icon: &str, f: F, color: u32) {
        self.c.text(x, y, lh, icon, f, color);
    }

    /// A field's fill with its edge: 1 dip all round and the darker bottom edge you chose
    /// (AC-SET-9); high contrast has the edge alone.
    pub(super) fn field(&self, (x, y, w, h): Bx, fill: u32, ring: Option<u32>) {
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

    pub(super) fn btn_w(&self, label: &str, icon: Option<&str>) -> f32 {
        self.c.width(label, F::Body) + 28.0 + icon.map_or(0.0, |_| 18.0)
    }

    /// A button: 32 dip, its label, an icon before it if any. `na`: greyed. Returns its width.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn button(&mut self, ctl: Ctl, x: f32, y: f32, label: &str, icon: Option<&str>, na: bool, group: u8) -> f32 {
        let b = self.c.t.b;
        let w = self.btn_w(label, icon);
        let off = na || (self.ro && ctl.writes());
        let fill = if self.hovered(ctl) && !off { mix(b.card, b.tx, 0.05) } else { b.card };
        self.field((x, y, w, 32.0), fill, None);
        let ink = if off { b.tx3 } else { b.tx };
        let mut tx = x + 14.0;
        if let Some(i) = icon {
            self.icon(tx, y, 32.0, i, F::IconSmall, ink);
            tx += 18.0;
        }
        self.c.text(tx, y, 32.0, label, F::Body, ink);
        self.ring((x, y, w, 32.0), 6.0, ctl);
        self.hit(ctl, (x, y, w, 32.0), group);
        if group != 0 {
            self.say(ctl, UIA_ButtonControlTypeId, label, "", off, (None, None));
        }
        w
    }

    /// The word and the switch: 40 × 20, with On or Off before it. `x`: the word's left edge.
    pub(super) fn switch_w(&self, on: bool) -> f32 {
        self.c.width(if on { "On" } else { "Off" }, F::Body) + 10.0 + 40.0
    }

    /// `na`: greyed, kept but not in use, drawn as read only.
    pub(super) fn switch(&mut self, key: &'static str, x: f32, y: f32, on: bool, na: bool) {
        let b = self.c.t.b;
        let ctl = Ctl::Toggle(key);
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
        self.hit(ctl, (x, y - 6.0, lw + 50.0, 32.0), 1);
    }

    pub(super) fn stepper_w(&self, text: &str) -> f32 {
        64.0 + (self.c.width(text, F::Body) + 16.0).max(56.0)
    }

    /// − value +, one keyboard stop (AC-SET-3: steppers only). `na`: greyed, kept but not in use;
    /// its − and + do nothing. `grey`: a value that follows a default, still live (spec 0028).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn stepper(&mut self, key: &'static str, x: f32, y: f32, (v, unit): (i64, &str), (down, up): (bool, bool), na: bool, grey: bool) {
        let b = self.c.t.b;
        let (num, unit) = match unit {
            MINUTES => (minutes(v), String::new()),
            "%" if v == 0 => ("Off".to_string(), String::new()),
            "" | "%" => (v.to_string(), unit.to_string()),
            u => (v.to_string(), format!(" {u}")),
        };
        let (nw, uw) = (self.c.width(&num, F::Body), self.c.width(&unit, F::Small));
        let w = 64.0 + (nw + uw + 16.0).max(56.0);
        self.field((x, y, w, 32.0), b.card, None);
        let mid = w - 64.0;
        let tx = x + 32.0 + (mid - nw - uw) / 2.0;
        let off = na || self.ro;
        self.c.text(tx, y, 32.0, &num, F::Body, if off { b.tx3 } else if grey { b.tx2 } else { b.tx });
        self.c.text(tx + nw, y, 32.0, &unit, F::Small, if off { b.tx3 } else { b.tx2 });
        for (ctl, ix, icon, live) in [(Ctl::Minus(key), x, I_MINUS, down), (Ctl::Plus(key), x + w - 32.0, I_ADD, up)] {
            let live = live && !off;
            if self.hovered(ctl) && live {
                self.c.fill_rr((ix + 4.0, y + 4.0, 24.0, 24.0), 4.0, op(mix(b.card, b.tx, 0.08)));
            }
            let iw = self.c.width(icon, F::IconTiny);
            self.icon(ix + (32.0 - iw) / 2.0, y, 32.0, icon, F::IconTiny, if live { b.tx } else { b.tx3 });
            self.hit(ctl, (ix, y, 32.0, 32.0), 1);
        }
        self.ring((x, y, w, 32.0), 6.0, Ctl::Stepper(key));
    }

    pub(super) fn seg_w(&self, words: &[&str]) -> f32 {
        4.0 + words.iter().map(|l| self.c.width(l, F::Small) + 20.0).sum::<f32>() + 2.0 * (words.len() as f32 - 1.0)
    }

    /// Choice buttons in one track, 28 dip; the chosen one raised with the ring (AC-SET-9).
    /// `na`: greyed. Each word is a Choice(key, i), or for a shortcut row a WinSide.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn seg(&mut self, x: f32, y: f32, words: &[&str], sel: Option<usize>, na: bool, ctl: impl Fn(usize) -> Ctl, stop: Ctl) {
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
                    self.c.fill_rr((bx.0 - 1.0, bx.1 - 1.0, bx.2 + 2.0, bx.3 + 2.0), 5.0, op(b.tx3));
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
            self.hit(ctl(i), bx, 1);
            ox += iw + 2.0;
        }
        self.ring((x, y, w, 28.0), 6.0, stop);
    }

    /// A caption's pieces as one wrapped text, 12/16; links get their hits, hover and ring.
    /// Returns its height.
    pub(super) fn caption(&mut self, x: f32, y: f32, w: f32, pieces: &[P]) -> f32 {
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
                self.hit(*ctl, (bx - 2.0, by, bw + 4.0, bh), 1);
            }
            if let Some(&first) = bxs.first() {
                self.ring((first.0 - 1.0, first.1, first.2 + 2.0, first.3), 2.0, *ctl);
            }
            self.say(*ctl, UIA_HyperlinkControlTypeId, text.trim(), "", false, (None, None));
        }
        h
    }

    pub(super) fn label_h(&mut self, w: f32, title: &str, cap: &[P]) -> f32 {
        self.measured(|c| c.label(0.0, 0.0, w, title, cap))
    }

    /// A row's name, 600 14/20, and its caption under it.
    pub(super) fn label(&mut self, x: f32, y: f32, w: f32, title: &str, cap: &[P]) -> f32 {
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

    /// One row in a card: name and caption on the left, the control on the right, both
    /// centred; under them, a note if one is given. The control is drawn first, so a link in
    /// the caption is its own stop after it (Reset under its row). Returns the row's height.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn row(&mut self, y: f32, title: &str, cap: &[P], (cw, ch): (f32, f32), ctl: impl FnOnce(&mut Self, f32, f32), note: Option<&NoteSpec>) -> f32 {
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let lw = if cw > 0.0 { w - cw - 16.0 } else { w };
        let lh = self.label_h(lw, title, cap);
        let top = lh.max(ch).max(ROW - 16.0);
        ctl(self, x + w - cw, y + 8.0 + (top - ch) / 2.0);
        self.label(x, y + 8.0 + (top - lh) / 2.0, lw, title, cap);
        let mut h = 8.0 + top;
        if let Some(n) = note {
            h += 6.0 + self.in_note(x, y + h + 6.0, w, n);
        }
        h + 8.0
    }

    /// A card's line between two rows.
    pub(super) fn sep(&self, y: f32) {
        self.c.rule(X0 + 1.0, y, self.cw - 2.0, op(self.c.t.b.line));
    }

    /// A card around `body`, which draws from its top and returns its height.
    pub(super) fn card(&mut self, y: f32, body: impl Fn(&mut Self, f32) -> f32) -> f32 {
        let h = self.measured(|c| body(c, y));
        let b = self.c.t.b;
        self.c.boxed((X0, y, self.cw, h), 8.0, b.card, Some(b.line));
        body(self, y);
        h
    }

    /// A card's tinted header: its title, and a caption on the right. `n`: the fingers' circle.
    pub(super) fn header(&mut self, y: f32, title: &str, n: Option<u8>, cap: &str) -> f32 {
        let b = self.c.t.b;
        let tint = if self.c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.09) };
        self.c.fill_rr((X0 + 1.0, y + 1.0, self.cw - 2.0, 36.0), 7.0, op(tint));
        self.c.fill_rr((X0 + 1.0, y + 20.0, self.cw - 2.0, 17.0), 0.0, op(tint));
        self.c.rule(X0 + 1.0, y + 37.0, self.cw - 2.0, op(b.line));
        let mut x = X0 + 16.0;
        if let Some(n) = n {
            self.c.circle(x + 11.0, y + 19.0, 11.0, Some(op(b.acc_b)), None);
            let d = n.to_string();
            let dw = self.c.width(&d, F::SmallBold);
            self.c.text(x + 11.0 - dw / 2.0, y + 8.0, 22.0, &d, F::SmallBold, b.on_acc_b);
            x += 32.0;
        }
        self.c.text(x, y + 8.0, 22.0, title, F::Bold, b.tx);
        if !cap.is_empty() {
            let cw = self.c.width(cap, F::Small);
            self.c.text(X0 + self.cw - 16.0 - cw, y + 8.0, 22.0, cap, F::Small, b.tx2);
        }
        38.0
    }

    /// The purple chip after a heading.
    pub(super) fn chip(&mut self, x: f32, y: f32, icon: &str, text: &str) {
        let b = self.c.t.b;
        let w = 10.0 + 12.0 + 6.0 + self.c.width(text, F::Small) + 10.0;
        let (fill, edge, ic) = if self.c.t.hc { (b.card, b.tx, b.tx) } else { (mix(b.card, b.acc_b, 0.10), mix(b.card, b.acc_b, 0.30), b.acc_b) };
        self.c.boxed((x, y, w, 24.0), 12.0, fill, Some(edge));
        self.icon(x + 10.0, y, 24.0, icon, F::IconSmall, ic);
        self.c.text(x + 28.0, y, 24.0, text, F::Small, b.tx);
    }
}
