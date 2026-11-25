// The Attention card: the popup words and its preview (spec 0022).
use super::*;

pub(in crate::settings_page) const WORDS: &str = "attention.text";
const WORDS_W: f32 = 300.0;

// The popup preview's sample (spec 0022): an app everyone knows, on workspace 3.
const SAMPLE_APP: &str = "Slack";
const SAMPLE_WS: usize = 2;

impl Ctx<'_, '_> {
    // ---- Attention (spec 0022) ----

    /// The switch, your words, and the popup they make, drawn by the indicator itself for a
    /// sample app on another workspace of this screen.
    pub(super) fn attention(&mut self, y: f32) -> f32 {
        use crate::indicator::{Card, Line};
        let b = self.cx.c.t.b;
        let mut h = self.setting("attention.enabled", y);
        self.sep(y + h);
        h += self.words_row(y + h);
        let n = self.s.workspaces;
        let here = self.w.pics.here.clone().unwrap_or_default();
        let a = self.active_on(&here);
        let named = self.w.pics.screens.len() > 1 && self.s.indicator_screen_name;
        let label = self.w.pics.view.names.iter().find(|(d, _)| *d == here).map(|(_, l)| l.clone());
        let ws = if SAMPLE_WS.min(n - 1) == a { (a + 1) % n } else { SAMPLE_WS.min(n - 1) };
        let text = settings::fill(self.w.words.as_deref().unwrap_or(&self.s.attention_text), SAMPLE_APP);
        let card = Card { wants: vec![ws], names: self.names_here(&here), ..Card::new(a, n, if named { label } else { None }) }.with(Line::Wants { text, ws });
        let style = self.s.indicator_style;
        let (pw, ph) = crate::indicator::draw_preview(&self.cx.c.measuring(), 0.0, 0.0, style, &card);
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + h + 4.0;
        let well_h = 16.0 + ph + 10.0 + 20.0 + 10.0;
        self.cx.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        crate::indicator::draw_preview(self.cx.c, wx + (ww - pw) / 2.0, wy + 16.0, style, &card);
        let on = self.s.attention_enabled;
        if !on && !self.cx.c.t.hc {
            self.cx.c.fill_rr((wx + 1.0, wy + 1.0, ww - 2.0, well_h - 2.0), 5.0, ui::alpha(b.track, 0.6));
        }
        let line = if on {
            format!("When {SAMPLE_APP} flashes on workspace {}, its screen shows this for 3 s.", ws + 1)
        } else {
            "Off: a window that flashes on a workspace you can't see shows nothing.".into()
        };
        let lw = self.cx.c.width(&line, F::Small);
        self.cx.c.text(wx + (ww - lw) / 2.0, wy + 16.0 + ph + 10.0, 20.0, &line, F::Small, b.tx2);
        h + 4.0 + well_h + 16.0
    }

    /// Your words for the popup: a text box, with the tag explained and the default to go back to.
    fn words_row(&mut self, y: f32) -> f32 {
        let text = self.w.words.clone().unwrap_or_else(|| self.s.attention_text.clone());
        let off = !self.s.attention_enabled;
        let focused = self.w.focus == Some(Ctl::Words) && !self.cx.ro && !off;
        let note = self.bad(WORDS).or_else(|| {
            let line = "Enter saves them; Esc puts back the saved words. Leaving the box saves them too.";
            focused.then(|| NoteSpec { tone: Tone::Info, icon: I_INFO, title: String::new(), lines: vec![t(line)], buttons: vec![], group: 1 })
        });
        let mut cap = vec![t(format!("Up to {} characters. ", settings::ATTENTION_MAX)), P::Mono("{{appname}}".into()), t(" becomes the app's name.")];
        if self.s.attention_text != settings::ATTENTION_TEXT {
            cap.push(t(" Default: "));
            cap.push(P::Mono(settings::ATTENTION_TEXT.into()));
            cap.push(t("."));
            cap.push(P::Link(" Reset".into(), Ctl::Reset(WORDS)));
        }
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str(), P::Icon(..) => "" }).collect();
        self.cx.say(Ctl::Words, UIA_EditControlTypeId, "Popup words", help.trim(), self.cx.ro || off, (None, Some(text.clone())));
        self.row(y, "Popup words", &cap, (WORDS_W, 32.0), |c, x, y| c.words_box(x, y, &text, focused, off), note.as_ref())
    }

    /// The text box, drawn as the hex box: focus shows a 2 dip accent bottom edge and a caret.
    fn words_box(&mut self, x: f32, y: f32, text: &str, focused: bool, na: bool) {
        let b = self.cx.c.t.b;
        let bx = (x, y, WORDS_W, 32.0);
        self.cx.hit(Ctl::Words, bx, 1);
        self.cx.field(bx, b.card, None);
        if focused {
            self.cx.c.fill_rr((x + 1.0, y + 29.0, WORDS_W - 2.0, 2.0), 0.0, op(b.acc));
        }
        let room = WORDS_W - 24.0;
        let shown = self.cx.c.fit(text, F::Body, room);
        let ink = if na || self.cx.ro { b.tx3 } else { b.tx };
        self.cx.c.text(x + 12.0, y, 32.0, &shown, F::Body, ink);
        if focused {
            let cx = x + 12.0 + self.cx.c.width(&shown, F::Body) + 1.0;
            self.cx.c.fill_rr((cx, y + 7.0, 1.0, 18.0), 0.0, op(b.tx));
        }
        if self.cx.focused(Ctl::Words) && !focused {
            self.cx.ring(bx, 6.0, Ctl::Words);
        }
    }

    /// The indicator's time line: how long it shows, then its fade (none with Skip animations).
    pub(super) fn fade_line(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let (hold, fade) = (self.s.hold_ms as f32, if self.s.fastest { 0.0 } else { self.s.fade_ms as f32 });
        let total = (hold + fade).max(1.0);
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let by = y + 2.0;
        self.cx.c.boxed((x, by, w, 8.0), 4.0, b.track, Some(b.line));
        let hw = w * hold / total;
        self.cx.c.fill_rr((x, by, hw, 8.0), 4.0, op(b.acc));
        if fade > 0.0 {
            if self.cx.c.t.hc {
                self.cx.c.stroke_rr((x + hw, by + 0.5, w - hw - 0.5, 7.0), 3.5, 1.0, op(b.acc));
            } else {
                self.cx.c.gradient(x + hw, by, w - hw, 8.0, b.acc, mix(b.card, b.acc, 0.15));
            }
        }
        let ly = by + 8.0 + 6.0;
        self.cx.c.text(x, ly, 16.0, "Shows", F::Small, b.tx2);
        let right = |c: &mut Self, runs: &[(&str, F, u32)]| {
            let rw: f32 = runs.iter().map(|(t, f, _)| c.cx.c.width(t, *f)).sum();
            c.cx.c.para(x + w - rw, ly, rw + 1.0, 16.0, runs);
        };
        if self.s.fastest {
            let ms = format!("{} ms", self.s.hold_ms);
            right(self, &[("gone at ", F::Small, b.tx2), (&ms, F::SmallBold, b.tx), (", no fade", F::Small, b.tx2)]);
        } else {
            let (from, gone) = (format!("{} ms", self.s.hold_ms), format!("{} ms", self.s.hold_ms + self.s.fade_ms));
            let fw = self.cx.c.width("fades from ", F::Small) + self.cx.c.width(&from, F::SmallBold);
            let fx = (x + hw - fw / 2.0).clamp(x + 60.0, x + w - fw - 120.0);
            self.cx.c.para(fx, ly, fw + 1.0, 16.0, &[("fades from ", F::Small, b.tx2), (&from, F::SmallBold, b.tx)]);
            right(self, &[("gone at ", F::Small, b.tx2), (&gone, F::SmallBold, b.tx)]);
        }
        2.0 + 8.0 + 6.0 + 16.0 + 14.0
    }
}
