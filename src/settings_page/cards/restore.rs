// The Restore card (spec 0012).
use super::*;

impl Ctx<'_, '_> {
    pub(super) fn restore(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let mut h = self.setting("restore.enabled", y);
        let m = self.s.restore_minutes as f32;
        let (x, w) = (X0 + 16.0, self.cw - 32.0);
        let ty = y + h + 4.0;
        let lead = format!("For {} min:", self.s.restore_minutes);
        self.c.para(x, ty, w, 16.0, &[(&lead, F::SmallBold, b.tx), (" reopened windows go back", F::Small, b.tx2)]);
        let tail = "After that, windows open where you are";
        self.c.text(x + w - self.c.width(tail, F::Small), ty, 16.0, tail, F::Small, b.tx2);
        let by = ty + 22.0;
        self.c.boxed((x, by, w, 8.0), 4.0, b.track, Some(b.line));
        let mw = w * (m / 12.0).min(1.0);
        self.c.fill_rr((x, by, mw, 8.0), 4.0, op(b.acc));
        let ly = by + 14.0;
        self.c.text(x, ly, 16.0, "des-k starts", F::Small, b.tx2);
        let at = format!("{} min", self.s.restore_minutes);
        let aw = self.c.width(&at, F::Small);
        self.c.text((x + mw - aw / 2.0).clamp(x + 90.0, x + w - aw), ly, 16.0, &at, F::Small, b.tx2);
        h += 4.0 + 22.0 + 8.0 + 6.0 + 16.0 + 14.0;
        self.sep(y + h);
        h + self.setting("restore.minutes", y + h)
    }
}
