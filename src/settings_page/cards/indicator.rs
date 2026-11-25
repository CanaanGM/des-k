// The Indicator card, and its Show button's real indicator (spec 0012).
use super::*;

impl Ctx<'_, '_> {
    // ---- Indicator and Restore ----

    pub(super) fn indicator(&mut self, y: f32) -> f32 {
        let b = self.c.t.b;
        let mut h = self.setting("indicator.enabled", y);
        self.sep(y + h);
        h += self.setting("indicator.style", y + h);
        // The preview well: the chosen style for this screen, through the indicator's own drawing (AC-SET-7).
        let n = self.s.workspaces;
        let here = self.w.pics.here.clone().unwrap_or_default();
        let a = self.active_on(&here);
        let named = self.w.pics.screens.len() > 1 && self.s.indicator_screen_name;
        let label = self.w.pics.view.names.iter().find(|(d, _)| *d == here).map(|(_, l)| l.clone());
        let card = crate::indicator::Card { names: self.names_here(&here), ..crate::indicator::Card::new(a, n, if named { label } else { None }) };
        let style = self.s.indicator_style;
        let (pw, ph) = crate::indicator::draw_preview(&self.c.measuring(), 0.0, 0.0, style, &card);
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let stage = ph.max(76.0);
        let well_h = 16.0 + stage + 12.0 + 32.0 + 12.0;
        let wy = y + h + 4.0;
        self.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        crate::indicator::draw_preview(self.c, wx + (ww - pw) / 2.0, wy + 16.0 + (stage - ph) / 2.0, style, &card);
        let on = self.s.indicator_enabled;
        let sw = self.btn_w("Show", None);
        let py = wy + 16.0 + stage + 12.0;
        let line = format!("On this screen, which is on workspace {} of {n}.", a + 1);
        self.c.text(wx + 12.0, py, 32.0, &line, F::Small, b.tx2);
        self.button(Ctl::Show, wx + ww - 12.0 - sw, py, "Show", None, !on, 1);
        h += 4.0 + well_h + 14.0;
        h += self.fade_line(y + h);
        self.sep(y + h);
        h += self.setting("indicator.hold_ms", y + h);
        self.sep(y + h);
        h += self.setting("indicator.fade_ms", y + h);
        self.sep(y + h);
        h + self.setting("indicator.screen_name", y + h)
    }
}

/// The real indicator on the screen the page is on, with the current hold and fade.
pub(in crate::settings_page) fn show() {
    let Some(h) = hwnd() else { return };
    crate::ws::show_indicator(unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) });
}
