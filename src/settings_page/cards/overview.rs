// The Overview card's picture with its live tiles (spec 0012, spec 0021).
use super::*;
use crate::settings_page::rows::word;

impl Ctx<'_, '_> {
    // ---- Overview and Motion ----

    /// The overview, drawn: up to two screens' first four workspaces with their windows; names
    /// follow Window names in overview.
    pub(super) fn overview_art(&mut self, y: f32) -> f32 {
        let t = self.c.t;
        let b = t.b;
        let (tw, th, gap) = (80.0, 46.0, 10.0);
        let screens: Vec<crate::ws::ScreenInfo> = self.w.pics.screens.iter().take(2).cloned().collect();
        let rows = screens.len().max(1);
        let (w2, h2) = (4.0 * (tw + gap) + 30.0, rows as f32 * (th + 22.0) + 14.0 - if rows == 1 { 0.0 } else { 2.0 });
        let x0 = X0 + (self.cw - w2) / 2.0;
        let y0 = y + 14.0;
        self.c.fill_rr((x0, y0, w2, h2), 6.0, op(b.bez));
        let labels = word(settings::LABELS, self.s.overview_labels);
        let white = ui::LIGHT.b.card;
        let (chip, chip_fg) = (ui::DARK.b.bg, ui::DARK.b.tx);
        let samples = ["Mail", "Code", "Notes"];
        self.out.geo.band = Some((x0, y0, w2, h2));
        for (r, sc) in screens.iter().enumerate() {
            let ry = y0 + 8.0 + r as f32 * (th + 22.0);
            let name = self.name_of(sc);
            self.c.text(x0 + 16.0, ry, 14.0, &name, F::Small, if t.hc { b.card } else { white });
            let a = self.active_on(&sc.device);
            self.out.geo.groups.push((sc.device.clone(), (x0 + 16.0, ry + 16.0, 4.0 * tw + 3.0 * gap, th)));
            for i in 0..4 {
                let (x, yy) = (x0 + 16.0 + i as f32 * (tw + gap), ry + 16.0);
                // Past your count: an empty slot, as a screen not plugged in is drawn (spec 0021 AC-2).
                if i >= self.s.workspaces {
                    if t.hc {
                        self.c.boxed((x, yy, tw, th), 4.0, b.bez, Some(b.card));
                    } else {
                        self.c.fill_rr((x, yy, tw, th), 4.0, ui::alpha(white, 0.06));
                        self.c.stroke_rr((x + 0.5, yy + 0.5, tw - 1.0, th - 1.0), 3.5, 1.0, ui::alpha(white, 0.22));
                    }
                    continue;
                }
                let ctl = Ctl::ShowTile(r, i);
                self.hit(ctl, (x, yy, tw, th), 1);
                self.out.geo.shows.push((sc.device.clone(), i, (x, yy, tw, th)));
                let fill = if t.hc { if i == a { b.hi } else { b.card } } else { b.ws[i] };
                self.c.fill_rr((x, yy, tw, th), 4.0, op(fill));
                if i == a {
                    self.c.stroke_rr((x, yy, tw, th), 4.0, 2.0, op(if t.hc { b.card } else { white }));
                }
                let k = self.w.pics.view.windows.iter().filter(|(d, ws, _)| *d == sc.device && *ws == i).count().min(3);
                for (j, sample) in samples.iter().enumerate().take(k) {
                    let (wx, wy) = (x + 6.0 + j as f32 * 20.0, yy + 6.0 + j as f32 * 4.0);
                    self.c.fill_rr((wx, wy, 34.0, 22.0), 2.0, ui::alpha(if t.hc { b.tx } else { white }, if t.hc { 1.0 } else { 0.92 }));
                    let show = labels == "always" || (labels == "hover" && r == 0 && i == a && j + 1 == k);
                    if show {
                        self.c.fill_rr((wx - 4.0, wy + 23.0, 42.0, 16.0), 3.0, op(chip));
                        let sw = self.c.width(sample, F::Small);
                        self.c.text(wx + 17.0 - sw / 2.0, wy + 23.0, 16.0, sample, F::Small, chip_fg);
                    }
                }
                if self.hovered(ctl) && self.picks() && i != a {
                    self.cue((x, yy, tw, th), 0x14141A);
                }
            }
        }
        14.0 + h2 + 6.0
    }
}

#[cfg(test)]
mod tests;
