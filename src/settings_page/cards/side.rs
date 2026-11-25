// The side list: each section, and the links under it (spec 0017).
use super::*;
use crate::settings_page::rows::{SECTIONS, Section, home};

impl Ctx<'_, '_> {
    // ---- The side list, banners and menu ----

    pub(in crate::settings_page) fn side(&mut self) {
        let t = self.c.t;
        let b = t.b;
        let vh = self.w.vh;
        let problems = settings::store::problems();
        let count = |sec: Option<Section>| problems.iter().filter(|p| home(&p.key) == sec).count();
        let mut items: Vec<(Option<Section>, &str, &str)> = vec![];
        if count(None) > 0 {
            items.push((None, "Notes", I_NOTES));
        }
        items.extend(SECTIONS.iter().map(|s| (Some(*s), s.name(), s.icon())));
        let current = self.current();
        let mut y = 8.0;
        for (sec, name, icon) in items {
            let ctl = Ctl::Side(sec);
            let bx = (12.0, y, NAV - 24.0, 36.0);
            let here = current == Some(sec);
            let (ink, ic) = if here && t.hc { (b.on_acc, b.on_acc) } else if here { (b.tx, b.acc) } else { (b.tx, b.tx2) };
            if here {
                self.c.fill_rr(bx, 6.0, op(if t.hc { b.hi } else { mix(b.bg, b.acc, 0.10) }));
                self.c.fill_rr((12.0, y + 10.0, 3.0, 16.0), 1.5, op(if t.hc { b.on_acc } else { b.acc }));
            } else if self.hovered(ctl) {
                self.c.fill_rr(bx, 6.0, op(mix(b.bg, b.tx, 0.05)));
            }
            self.icon(24.0, y, 36.0, icon, F::Icon, ic);
            self.c.text(52.0, y, 36.0, name, if here { F::Bold } else { F::Body }, ink);
            let n = count(sec);
            if n > 0 {
                let label = n.to_string();
                let cw = (self.c.width(&label, F::SmallBold) + 12.0).max(20.0);
                let (fill, fg, edge) = match () {
                    _ if t.hc && here => (b.on_acc, b.hi, b.on_acc),
                    _ if t.hc => (b.card, b.tx, b.tx),
                    _ => (b.card, b.warn, b.warn),
                };
                self.c.boxed((NAV - 24.0 - cw, y + 8.0, cw, 20.0), 10.0, fill, Some(edge));
                self.c.text(NAV - 24.0 - cw + (cw - self.c.width(&label, F::SmallBold)) / 2.0, y + 8.0, 20.0, &label, F::SmallBold, fg);
            }
            self.ring(bx, 6.0, ctl);
            self.hit(ctl, bx, 0);
            let spoken = if n > 0 { format!("{name}, {n} {}", if n == 1 { "problem" } else { "problems" }) } else { name.to_string() };
            self.say(ctl, UIA_ListItemControlTypeId, &spoken, "", false, (None, None));
            y += 38.0;
        }
        // Welcome and the file, at the foot.
        // What's new opens inside des-k, so it has no open elsewhere mark (spec 0029 AC-8).
        let foot = [(Ctl::Welcome, "Welcome", "\u{EA80}", true), (Ctl::WhatsNew, "What's new", "\u{E734}", false), (Ctl::Open, "Open settings.toml", "\u{E8A5}", true)];
        let mut fy = vh - 14.0 - 38.0 * foot.len() as f32;
        self.c.rule(24.0, fy - 7.0, NAV - 48.0, op(b.line));
        for (ctl, name, icon, out) in foot {
            let bx = (12.0, fy, NAV - 24.0, 36.0);
            if self.hovered(ctl) {
                self.c.fill_rr(bx, 6.0, op(mix(b.bg, b.tx, 0.05)));
            }
            self.icon(24.0, fy, 36.0, icon, F::Icon, b.tx2);
            self.c.text(52.0, fy, 36.0, name, F::Body, b.tx);
            if out {
                let ow = self.c.width(I_OUT, F::IconSmall);
                self.icon(NAV - 24.0 - ow, fy, 36.0, I_OUT, F::IconSmall, b.tx2);
            }
            self.ring(bx, 6.0, ctl);
            self.hit(ctl, bx, 0);
            self.say(ctl, UIA_HyperlinkControlTypeId, name, "", false, (None, None));
            fy += 38.0;
        }
    }

    /// The side list item the view is in: the last section whose top has scrolled past.
    fn current(&self) -> Option<Option<Section>> {
        current(self.w.scroll, self.out.height, self.w.vh - self.out.fixed, &self.out.tops)
    }
}
