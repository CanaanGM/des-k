// The Motion card's chart and rows (spec 0017).
use super::*;

impl Ctx<'_, '_> {
    /// Every des-k motion on one time axis, in the grey well, or "At once" (spec 0017 AC-11).
    pub(super) fn motion(&mut self, y: f32) -> f32 {
        let th = self.cx.c.t;
        let b = th.b;
        let s = self.s;
        let max = if (s.fade_ms.max(s.overview_fade_ms)) > 1000 { 2000.0 } else { 1000.0 };
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + 16.0;
        let bar = if th.hc { b.acc } else if th.dark { mix(b.acc, 0xFFFFFF, 0.28) } else { b.acc };
        let fade = s.overview_animation == settings::Animation::Fade;
        let lanes: [(&str, u32, Option<&str>, &str); 2] = [
            ("Indicator fades out", s.fade_ms, (!s.indicator_enabled).then_some("Indicator off"), ""),
            ("Overview fades in and out", s.overview_fade_ms, (!fade).then_some("No fade"), " each"),
        ];
        let body = |c: &mut Self, top: f32| -> f32 {
            let (nx, tx, vx) = (wx + 16.0, wx + 16.0 + 196.0 + 16.0, wx + ww - 16.0);
            let trw = vx - 100.0 - 16.0 - tx;
            let mut h = 14.0;
            for (name, ms, none, each) in lanes {
                let ly = top + h;
                c.cx.c.text(nx, ly, 24.0, name, F::Body, b.tx);
                for q in 0..4 {
                    c.cx.c.fill_rr((tx + q as f32 * trw / 4.0, ly, 1.0, 24.0), 0.0, op(b.grid));
                }
                c.cx.c.fill_rr((tx + trw - 1.0, ly, 1.0, 24.0), 0.0, op(b.grid));
                if s.fastest {
                    c.cx.c.fill_rr((tx, ly + 6.0, 3.0, 12.0), 1.5, op(bar));
                } else if none.is_none() && ms > 0 {
                    let bw = trw * (ms as f32 / max).min(1.0);
                    c.cx.c.fill_rr((tx, ly + 8.0, bw, 8.0), 0.0, op(bar));
                    c.cx.c.fill_rr((tx + (bw - 8.0).max(0.0), ly + 8.0, bw.min(8.0), 8.0), 4.0, op(bar));
                }
                let v: Vec<(String, F, u32)> = if s.fastest {
                    vec![("At once".into(), F::SmallBold, b.tx)]
                } else if let Some(word) = none {
                    vec![(word.into(), F::Small, b.tx2)]
                } else {
                    vec![(format!("{ms} ms"), F::SmallBold, b.tx), (each.into(), F::Small, b.tx2)]
                };
                let vw: f32 = v.iter().map(|(t, f, _)| c.cx.c.width(t, *f)).sum();
                let runs: Vec<(&str, F, u32)> = v.iter().map(|(t, f, col)| (t.as_str(), *f, *col)).collect();
                c.cx.c.para(vx - vw, ly + 4.0, vw + 1.0, 16.0, &runs);
                h += 24.0 + 6.0;
            }
            let ay = top + h;
            c.cx.c.text(tx, ay, 16.0, "0", F::Small, b.tx2);
            let half = format!("{} ms", max as u32 / 2);
            let hw = c.cx.c.width(&half, F::Small);
            c.cx.c.text(tx + trw / 2.0 - hw / 2.0, ay, 16.0, &half, F::Small, b.tx2);
            let full = format!("{} ms", max as u32);
            c.cx.c.text(tx + trw - c.cx.c.width(&full, F::Small), ay, 16.0, &full, F::Small, b.tx2);
            h += 16.0 + 10.0;
            c.cx.c.rule(nx, top + h, ww - 32.0, op(b.line));
            h += 8.0;
            let foot = [t("You set these times in "), P::Link("Indicator".into(), Ctl::Link(Link::Indicator)), t(" and "), P::Link("Overview".into(), Ctl::Link(Link::Overview)), t(".")];
            h += c.cx.caption(nx, top + h, ww - 32.0, &foot);
            h + 10.0
        };
        let h = self.measured(|c| body(c, 0.0));
        self.cx.c.boxed((wx, wy, ww, h), 6.0, b.track, Some(b.line));
        // One image for Narrator, read as a sentence built from the lanes drawn.
        let said = if s.fastest {
            "Every des-k motion happens at once".to_string()
        } else {
            let ind = if s.indicator_enabled { format!("{} ms", s.fade_ms) } else { "nothing, it is off".into() };
            let ov = if fade { format!("fades in and out over {} ms each", s.overview_fade_ms) } else { "has no fade".into() };
            format!("Indicator fades out over {ind}; the overview {ov}")
        };
        if !self.cx.said.iter().any(|(c, _)| *c == Ctl::Chart) {
            let s = self.cx.c.s;
            let rect = RECT { left: (wx * s) as i32, top: (wy * s) as i32, right: ((wx + ww) * s) as i32, bottom: ((wy + h) * s) as i32 };
            self.cx.said.push((Ctl::Chart, Node { role: UIA_ImageControlTypeId, name: said, rect, ..Node::default() }));
        }
        body(self, wy);
        16.0 + h + 14.0
    }
}
