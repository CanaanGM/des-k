// The Colors section: your accent and the workspace colors, their hex boxes and what each
// writes (spec 0020).
use super::*;

// ---- Colors and Where it shows (spec 0020) ----

pub(crate) const ACCENT: &str = "colors.accent";
pub(crate) const WS_KEYS: [&str; 9] = ["colors.ws1", "colors.ws2", "colors.ws3", "colors.ws4", "colors.ws5", "colors.ws6", "colors.ws7", "colors.ws8", "colors.ws9"];
/// des-k's own workspace colors by name, as a default row says them.
const WS_NAMES: [&str; 9] = ["blue", "gold", "green", "violet", "orange", "cyan", "pink", "taupe", "red"];

pub(in crate::settings_page) fn ws_index(key: &str) -> Option<usize> {
    WS_KEYS.iter().position(|k| *k == key)
}

pub(in crate::settings_page) fn path_of(key: &str) -> Vec<String> {
    key.split('.').map(String::from).collect()
}

/// What a hex box saves (AC-12): a workspace's own des-k color takes its key out, so the file
/// only holds what differs.
pub(crate) fn color_edit(key: &'static str, c: u32) -> Edit {
    match ws_index(key) {
        Some(i) if c == ui::WS[i] => Edit::Remove(path_of(key)),
        _ => Edit::At(path_of(key), format!("#{c:06X}").into()),
    }
}

/// Six hex digits as a color.
fn hex_color(digits: &str) -> Option<u32> {
    (digits.len() == 6).then(|| u32::from_str_radix(digits, 16).ok()).flatten()
}

/// What a paste puts in a hex box: exactly six hex digits once spaces and a leading # are
/// trimmed, in upper case; anything else is refused.
pub(crate) fn pasted(text: &str) -> Option<String> {
    let t = text.trim();
    let t = t.strip_prefix('#').unwrap_or(t).trim();
    (t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit())).then(|| t.to_ascii_uppercase())
}

/// A hex box being typed in: its digits, the caret and the other end of the selection, and
/// the line under its row.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct Hex {
    pub(crate) key: &'static str,
    pub(crate) digits: String,
    pub(crate) caret: usize,
    pub(crate) anchor: usize,
    pub(crate) line: Option<&'static str>,
}

impl Ctx<'_, '_> {
    // ---- Colors (spec 0020 AC-12) ----

    pub(super) fn colors(&mut self, y: f32) -> f32 {
        let mut h = 0.0;
        if self.cx.c.t.hc {
            let n = NoteSpec { tone: Tone::Info, icon: I_INFO, title: "High contrast is on, so des-k uses Windows' contrast colors".into(), lines: vec![t("Your colors stay in the file and come back when it's off.")], buttons: vec![], group: 1 };
            h += self.cx.note(X0, y, self.cw, &n) + 12.0;
        }
        h += self.card(y + h, |c, y| {
            let mut h = 0.0;
            if !c.cx.c.t.hc {
                h += c.sample(y);
                c.sep(y + h);
            }
            h += c.accent_row(y + h);
            if matches!(c.s.accent, settings::Accent::Rgb(_)) {
                c.sep(y + h);
                h += c.hex_row(ACCENT, y + h);
            }
            h
        });
        h += 12.0;
        h + self.card(y + h, |c, y| {
            let mut h = c.header(y, "Workspace colors", None, "In every des-k window");
            let n = c.s.most();
            for (i, key) in WS_KEYS.iter().take(n).enumerate() {
                if i > 0 {
                    c.sep(y + h);
                }
                h += c.hex_row(key, y + h);
            }
            let cap = [t(format!("One row per workspace you have ({n} now, set under ")), P::Link("Workspaces".into(), Ctl::Link(Link::Workspaces)), t("). Colors for workspaces you don't have are kept.")];
            h + 4.0 + c.cx.caption(X0 + 16.0, y + h + 4.0, c.cw - 32.0, &cap) + 14.0
        })
    }

    /// des-k's own parts in the colors in use, and what each of the three colors paints.
    fn sample(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let (wx, ww) = (X0 + 16.0, self.cw - 32.0);
        let wy = y + 16.0;
        let foot = "des-k makes both neighbors from the accent, and picks black or white text for each. The des-k logo keeps its own colors.";
        let fh = self.measured(|c| c.cx.c.para(0.0, 0.0, ww - 32.0, 16.0, &[(foot, F::Small, b.tx2)]));
        let body = 3.0 * 44.0;
        let well_h = 16.0 + body + 12.0 + 1.0 + 10.0 + fh + 12.0;
        self.cx.c.boxed((wx, wy, ww, well_h), 6.0, b.track, Some(b.line));
        // The little card: a header with its badge, a row with a switch and a button.
        let (cx, cy, cw) = (wx + 16.0, wy + 16.0 + (body - 80.0) / 2.0, 300.0);
        self.cx.c.boxed((cx, cy, cw, 80.0), 6.0, b.card, Some(b.line));
        self.cx.c.fill_rr((cx + 1.0, cy + 1.0, cw - 2.0, 34.0), 5.0, op(mix(b.card, b.acc_a, 0.09)));
        self.cx.c.fill_rr((cx + 1.0, cy + 20.0, cw - 2.0, 15.0), 0.0, op(mix(b.card, b.acc_a, 0.09)));
        self.cx.c.rule(cx + 1.0, cy + 35.0, cw - 2.0, op(b.line));
        self.cx.c.circle(cx + 23.0, cy + 18.0, 11.0, Some(op(b.acc_b)), None);
        let dw = self.cx.c.width("3", F::SmallBold);
        self.cx.c.text(cx + 23.0 - dw / 2.0, cy + 7.0, 22.0, "3", F::SmallBold, b.on_acc_b);
        self.cx.c.text(cx + 44.0, cy + 7.0, 22.0, "A section header", F::Bold, b.tx);
        self.cx.c.text(cx + 12.0, cy + 36.0, 44.0, "A row", F::Bold, b.tx);
        let bw = self.cx.c.width("Button", F::Body) + 28.0;
        let bx = cx + cw - 12.0 - bw;
        self.cx.c.fill_rr((bx, cy + 42.0, bw, 32.0), 4.0, op(b.acc));
        self.cx.c.text(bx + 14.0, cy + 42.0, 32.0, "Button", F::Body, b.on_acc);
        let sx = bx - 16.0 - 40.0;
        self.cx.c.fill_rr((sx, cy + 48.0, 40.0, 20.0), 10.0, op(b.acc));
        self.cx.c.circle(sx + 30.0, cy + 58.0, 6.0, Some(op(b.on_acc)), None);
        // What each color paints.
        let lx = cx + cw + 20.0;
        let legend = [
            (b.acc, "Buttons and switches", "the accent: also the chosen row and links"),
            (b.acc_a, "Headers and bands", "the accent turned 30° one way"),
            (b.acc_b, "Badges and marks", "the accent turned 30° the other way"),
        ];
        let mut lw: f32 = 0.0;
        for (i, (col, name, line)) in legend.iter().enumerate() {
            let ly = wy + 16.0 + i as f32 * 44.0;
            self.cx.c.fill_rr((lx, ly + 6.0, 28.0, 20.0), 4.0, op(*col));
            self.cx.c.text(lx + 38.0, ly, 22.0, name, F::Bold, b.tx);
            self.cx.c.text(lx + 38.0, ly + 21.0, 16.0, line, F::Small, b.tx2);
            lw = lw.max(38.0 + self.cx.c.width(name, F::Bold).max(self.cx.c.width(line, F::Small)));
        }
        self.out.geo.legend = Some((lx, wy + 16.0, lw, body));
        let ry = wy + 16.0 + body + 12.0;
        self.cx.c.rule(wx + 16.0, ry, ww - 32.0, op(b.line));
        self.cx.c.para(wx + 16.0, ry + 11.0, ww - 32.0, 16.0, &[(foot, F::Small, b.tx2)]);
        16.0 + well_h + 16.0
    }

    /// A bad color in the file, as its row's note.
    fn color_bad(&self, key: &str) -> Option<NoteSpec> {
        let problems = settings::store::problems();
        let p = problems.iter().find(|p| p.key == key)?;
        let what = ws_index(key).map_or("Windows' accent".to_string(), |i| format!("des-k's {}", WS_NAMES[i]));
        Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: format!("des-k can't use this value, so it uses {what}"), lines: vec![P::Mono(p.message.clone()), t(" Type a color here, or Reset takes the line out.")], buttons: vec![], group: 1 })
    }

    /// Accent color: Windows or My own, one stop. Greyed and blank in high contrast.
    fn accent_row(&mut self, y: f32) -> f32 {
        let own = matches!(self.s.accent, settings::Accent::Rgb(_));
        let hc = self.cx.c.t.hc;
        let note = self.color_bad(ACCENT);
        let mut cap = vec![];
        if own {
            cap.push(t("Your own color, in light and dark. Default: Windows."));
        } else if ui::look().accent.is_none() {
            cap.push(t("des-k can't read Windows' accent color, so it uses des-k purple."));
        } else {
            cap.push(t("Follows Windows' accent color, in the shade Windows uses for light or dark: Settings › Personalization › Colors."));
        }
        if !own && let Some(c) = self.w.own {
            cap.push(t(format!(" My own goes back to #{c:06X}.")));
        }
        if own || note.is_some() {
            cap.push(P::Link(" Reset".into(), Ctl::Reset(ACCENT)));
        }
        let words = ["Windows", "My own"];
        let size = (self.cx.seg_w(&words), 28.0);
        self.cx.say(Ctl::Pick(ACCENT), UIA_ComboBoxControlTypeId, "Accent color", "", self.cx.ro || hc, (None, Some(words[own as usize].into())));
        self.row(y, "Accent color", &cap, size, |c, x, y| c.cx.seg(x, y, &words, Some(own as usize), hc, |i| Ctl::Choice(ACCENT, i), Ctl::Pick(ACCENT), None), note.as_ref())
    }

    /// Your accent or Workspace N: the swatch and the hex box, with the default and Reset; a
    /// tinted band and a line under it while you type.
    fn hex_row(&mut self, key: &'static str, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let hc = self.cx.c.t.hc;
        let wi = ws_index(key);
        let saved = match wi {
            Some(i) => self.s.ws_colors[i],
            None => match self.s.accent {
                settings::Accent::Rgb(c) => Some(c),
                settings::Accent::Windows => None,
            },
        };
        let hex = self.w.hexes.iter().find(|x| x.key == key).cloned();
        let focused = self.w.focus == Some(Ctl::Hex(key));
        let title = wi.map_or("Your accent".to_string(), |i| format!("Workspace {}", i + 1));
        let mut cap = vec![];
        match (wi, saved) {
            (None, _) => cap.push(t("Six digits 0–9 or A–F after the #, or paste a color.")),
            (Some(i), None) => cap.push(t(format!("des-k's {}.", WS_NAMES[i]))),
            (Some(i), Some(_)) => {
                cap.push(t(format!("Your color. Default: #{:06X}.", ui::WS[i])));
                cap.push(P::Link(" Reset".into(), Ctl::Reset(key)));
            }
        }
        let bad = self.color_bad(key);
        if bad.is_some() && wi.is_some() && saved.is_none() {
            cap.push(P::Link(" Reset".into(), Ctl::Reset(key)));
        }
        let typing = !hc && hex.as_ref().is_some_and(|x| focused && Some(x.digits.as_str()) != shown_digits(self.cx.c.t, self.s, key).as_deref());
        let note = bad.or_else(|| match &hex {
            Some(Hex { line: Some(l), .. }) => Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: (*l).into(), lines: vec![], buttons: vec![], group: 1 }),
            _ if typing => Some(NoteSpec { tone: Tone::Info, icon: I_INFO, title: String::new(), lines: vec![t("Enter saves it; Esc puts back the saved color. Leaving the box saves it too.")], buttons: vec![], group: 1 }),
            _ => None,
        });
        let digits = hex.as_ref().map_or_else(|| shown_digits(self.cx.c.t, self.s, key).unwrap_or_default(), |x| x.digits.clone());
        // The color picker's preview shows here too, live (spec 0021 AC-5).
        let digits = match &self.w.pick {
            Some(p) if p.key == key && p.preview.is_some() => format!("{:06X}", p.color),
            _ => digits,
        };
        let swatch = (!hc).then(|| hex_color(&digits).unwrap_or(wi.map_or(b.acc, |i| b.ws[i])));
        let warn = hex.as_ref().is_some_and(|x| x.line.is_some());
        let sel = hex.as_ref().map(|x| (x.caret, x.anchor));
        let spoken = if hc { String::new() } else { format!("#{digits}") };
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str(), P::Icon(..) => "" }).collect();
        self.cx.say(Ctl::Swatch(key), UIA_ButtonControlTypeId, &format!("{title} color"), "Opens the color picker.", self.cx.ro || hc, (None, None));
        self.cx.say(Ctl::Hex(key), UIA_EditControlTypeId, &format!("{title} color"), help.trim(), self.cx.ro || hc, (None, Some(spoken)));
        let draw = |c: &mut Self, y: f32| {
            c.row(y, &title, &cap, (164.0, 32.0), |c, x, y| {
                c.swatch(key, x, y, wi, swatch);
                c.hex_box(key, x + 56.0, y, if hc { "" } else { &digits }, sel, warn, hc);
            }, note.as_ref())
        };
        if typing && !hc {
            let rh = self.measured(|c| draw(c, y));
            let band = op(mix(b.card, b.acc_a, 0.06));
            // Your accent is its card's last row: the band keeps the card's round bottom corners.
            self.cx.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, rh - 1.0), 7.0, band);
            self.cx.c.fill_rr((X0 + 1.0, y, self.cw - 2.0, rh - 9.0), 0.0, band);
        }
        draw(self, y)
    }

    /// A color's swatch: a workspace's is its tile with its number in its ink; None (high
    /// contrast) draws it greyed and blank. A button: it opens the color picker (spec 0021 AC-3).
    fn swatch(&mut self, key: &'static str, x: f32, y: f32, wi: Option<usize>, color: Option<u32>) {
        let b = self.cx.c.t.b;
        let bx = (x, y, 48.0, 32.0);
        let ctl = Ctl::Swatch(key);
        self.cx.hit(ctl, bx, 1);
        self.out.geo.swatches.push((key, bx));
        match color {
            None => self.cx.c.boxed(bx, 4.0, b.card, Some(b.tx3)),
            Some(c) => {
                self.cx.c.fill_rr(bx, 4.0, op(c));
                if b.ws_edge > 0.0 {
                    self.cx.c.stroke_rr((x + 0.5, y + 0.5, 47.0, 31.0), 3.5, 1.0, op(mix(c, 0, b.ws_edge)));
                }
            }
        }
        if let Some(i) = wi {
            let ink = color.map_or(b.tx3, ui::ink);
            self.cx.c.text(x + 6.0, y + 32.0 - 17.0, 16.0, &(i + 1).to_string(), F::SmallBold, ink);
        }
        if let Some(c) = color
            && self.cx.hovered(ctl)
            && self.picks()
            && !self.cx.focused(ctl)
        {
            self.cx.cue(bx, ui::ink(c));
        }
        self.cx.ring(bx, 4.0, ctl);
    }

    /// The hex box (spec 0020): a fixed grey #, then up to six digits in Cascadia Mono. Focus
    /// shows a 2 dip accent bottom edge and a caret, as WinUI's text box, no ring; fewer than
    /// six shows a warn edge. `na`: greyed and blank (high contrast).
    #[allow(clippy::too_many_arguments)]
    fn hex_box(&mut self, key: &'static str, x: f32, y: f32, digits: &str, sel: Option<(usize, usize)>, warn: bool, na: bool) {
        let b = self.cx.c.t.b;
        let ctl = Ctl::Hex(key);
        let focused = self.w.focus == Some(ctl) && !na && !self.cx.ro;
        let bx = (x, y, 108.0, 32.0);
        if warn {
            self.cx.c.fill_rr(bx, 6.0, op(b.warn));
            self.cx.c.fill_rr((x + 1.0, y + 1.0, 106.0, 30.0), 5.0, op(b.card));
            self.cx.c.fill_rr((x + 1.0, y + 29.0, 106.0, 2.0), 0.0, op(b.warn));
        } else {
            self.cx.field(bx, b.card, None);
            if focused {
                self.cx.c.fill_rr((x + 1.0, y + 29.0, 106.0, 2.0), 0.0, op(b.acc));
            }
        }
        let cw = self.cx.c.width("0", F::Code);
        let hx = x + 10.0;
        self.cx.c.text(hx, y, 32.0, "#", F::Code, b.tx3);
        let dx = hx + cw;
        let (lo, hi) = sel.filter(|_| focused).map_or((0, 0), |(c, a)| (c.min(a), c.max(a)));
        if hi > lo {
            self.cx.c.fill_rr((dx + lo as f32 * cw, y + 7.0, (hi - lo) as f32 * cw, 18.0), 2.0, op(b.acc));
        }
        for (i, ch) in digits.chars().enumerate() {
            let ink = if na || self.cx.ro { b.tx3 } else if i >= lo && i < hi { b.on_acc } else { b.tx };
            self.cx.c.text(dx + i as f32 * cw, y, 32.0, &ch.to_string(), F::Code, ink);
        }
        if focused && hi == lo && let Some((c, _)) = sel {
            self.cx.c.fill_rr((dx + c as f32 * cw, y + 7.0, 1.0, 18.0), 0.0, op(b.tx));
        }
        if self.cx.focused(ctl) && !focused {
            self.cx.ring(bx, 6.0, ctl);
        }
        self.cx.hit(ctl, bx, 1);
    }
}

/// The digits a hex box shows when no one is typing: the color in use, None in high contrast.
fn shown_digits(t: Theme, s: &Settings, key: &str) -> Option<String> {
    if t.hc {
        return None;
    }
    let c = match ws_index(key) {
        Some(i) => s.ws_colors[i].unwrap_or(ui::WS[i]),
        None => match s.accent {
            settings::Accent::Rgb(c) => c,
            settings::Accent::Windows => t.b.acc,
        },
    };
    Some(format!("{c:06X}"))
}

// ---- Colors and Where it shows: what the controls do (spec 0020) ----

const INCOMPLETE: &str = "Six digits are needed. Nothing is saved until there are six; Esc puts back the saved color.";
const REFUSED: &str = "That isn't a color. Paste six digits 0–9 or A–F; nothing changed.";
const COULDNT: &str = "Couldn't save your color. Enter tries again.";

/// Windows or My own (AC-12). My own writes the accent des-k uses now, so nothing changes
/// until you type, or the last own color while the page is open. Off in high contrast.
pub(in crate::settings_page) fn set_accent(own: bool) {
    let Some((hc, acc, last)) = get(|w| (w.theme.hc, w.theme.b.acc, w.own)) else { return };
    if hc {
        return;
    }
    match (own, settings::get().accent) {
        (false, settings::Accent::Rgb(c)) => {
            let _ = set(|w| w.own = Some(c));
            commit(Edit::At(path_of(ACCENT), "windows".into()), "Accent color");
        }
        (true, settings::Accent::Windows) => commit(Edit::At(path_of(ACCENT), format!("#{:06X}", last.unwrap_or(acc)).into()), "Accent color"),
        _ => {}
    }
}

pub(in crate::settings_page) fn color_title(key: &str) -> String {
    ws_index(key).map_or("Your accent".into(), |i| format!("Workspace {} color", i + 1))
}

/// Reset takes a color's key out of the file; the page keeps your last own accent.
pub(in crate::settings_page) fn reset_color(key: &'static str) {
    if key == ACCENT
        && let settings::Accent::Rgb(c) = settings::get().accent
    {
        let _ = set(|w| w.own = Some(c));
    }
    let _ = set(|w| w.hexes.retain(|x| x.key != key));
    commit(Edit::Remove(path_of(key)), &color_title(key));
}

/// Focus came to a hex box: Tab selects its digits, a click puts the caret there. Nothing to
/// type in high contrast or read only.
pub(in crate::settings_page) fn enter_hex(key: &'static str, caret: Option<usize>) {
    let _ = set(|w| {
        if w.theme.hc || settings::store::parse_error().is_some() {
            return;
        }
        if !w.hexes.iter().any(|x| x.key == key) {
            let digits = shown_digits(w.theme, &settings::get(), key).unwrap_or_default();
            w.hexes.push(Hex { key, caret: digits.len(), anchor: 0, digits, line: None });
        }
        if let Some(x) = w.hexes.iter_mut().find(|x| x.key == key) {
            x.caret = caret.unwrap_or(x.digits.len()).min(x.digits.len());
            x.anchor = if caret.is_some() { x.caret } else { 0 };
        }
    });
}

pub(in crate::settings_page) fn enter_if_new(key: &'static str) {
    if get(|w| w.hexes.iter().any(|x| x.key == key)) == Some(false) {
        enter_hex(key, None);
    }
}

/// Focus left a hex box: six digits save; fewer keep the digits and the line until you fix
/// them or press Esc.
pub(in crate::settings_page) fn leave_hex(key: &'static str) {
    let Some((x, shown)) = get(|w| Some((w.hexes.iter().find(|x| x.key == key)?.clone(), shown_digits(w.theme, &settings::get(), key)))).flatten() else { return };
    match hex_color(&x.digits) {
        Some(_) if Some(&x.digits) == shown.as_ref() => {
            let _ = set(|w| w.hexes.retain(|x| x.key != key));
        }
        Some(_) => save_hex(key),
        None => {
            let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(|x| x.line = Some(INCOMPLETE)));
        }
    }
}

/// Enter, or leaving with six digits: saves in upper case. A workspace's own default takes its
/// key out. A failed save keeps your digits and says Enter tries again.
fn save_hex(key: &'static str) {
    let Some(x) = get(|w| w.hexes.iter().find(|x| x.key == key).cloned()).flatten() else { return };
    let Some(c) = hex_color(&x.digits) else {
        let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(|x| x.line = Some(INCOMPLETE)));
        return layout();
    };
    let edit = if key == ACCENT { Edit::At(path_of(ACCENT), format!("#{c:06X}").into()) } else { color_edit(key, c) };
    match settings::edit::write(edit) {
        Ok(()) => {
            let still = set(|w| {
                w.error = None;
                w.hexes.retain(|x| x.key != key);
                w.focus == Some(Ctl::Hex(key))
            });
            if still == Some(true) {
                enter_hex(key, Some(6));
            }
            refresh();
        }
        Err(why) => {
            let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(|x| x.line = Some(COULDNT)));
            failed(&color_title(key), why);
        }
    }
}

/// Typed or pasted digits go in at the caret, over the selection, never more than six.
pub(in crate::settings_page) fn hex_insert(key: &'static str, text: &str) {
    let _ = set(|w| {
        let Some(x) = w.hexes.iter_mut().find(|x| x.key == key) else { return };
        let (lo, hi) = (x.caret.min(x.anchor), x.caret.max(x.anchor));
        let mut d = x.digits.clone();
        d.replace_range(lo..hi, "");
        let put: String = text.chars().filter(char::is_ascii_hexdigit).take(6 - d.len()).map(|c| c.to_ascii_uppercase()).collect();
        d.insert_str(lo, &put);
        x.digits = d;
        x.caret = lo + put.len();
        x.anchor = x.caret;
        if x.line == Some(REFUSED) || x.digits.len() == 6 {
            x.line = None;
        }
    });
    layout();
}

/// A hex box's own keys; false lets the page have the key (Up, Down, Tab move on).
pub(in crate::settings_page) fn hex_key(key: &'static str, vk: VIRTUAL_KEY) -> bool {
    let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
    let (ctrl, shift) = (down(VK_CONTROL), down(VK_SHIFT));
    // Focus can land here without focus_on (a Reset that went): the first key starts it.
    enter_if_new(key);
    let has = get(|w| w.hexes.iter().any(|x| x.key == key)) == Some(true);
    if !has {
        // Read only or high contrast: Esc never closes the page from inside a box.
        return vk == VK_ESCAPE;
    }
    let edit = |f: &mut dyn FnMut(&mut Hex)| {
        let _ = set(|w| w.hexes.iter_mut().filter(|x| x.key == key).for_each(&mut *f));
        layout();
    };
    match vk {
        VK_LEFT | VK_RIGHT | VK_HOME | VK_END => edit(&mut |x| {
            let (lo, hi) = (x.caret.min(x.anchor), x.caret.max(x.anchor));
            x.caret = match vk {
                VK_HOME => 0,
                VK_END => x.digits.len(),
                VK_LEFT if !shift && hi > lo => lo,
                VK_RIGHT if !shift && hi > lo => hi,
                VK_LEFT => x.caret.saturating_sub(1),
                _ => (x.caret + 1).min(x.digits.len()),
            };
            if !shift {
                x.anchor = x.caret;
            }
        }),
        VK_BACK | VK_DELETE => edit(&mut |x| {
            let (mut lo, mut hi) = (x.caret.min(x.anchor), x.caret.max(x.anchor));
            if lo == hi {
                if vk == VK_BACK && lo > 0 {
                    lo -= 1;
                } else if vk == VK_DELETE && hi < x.digits.len() {
                    hi += 1;
                }
            }
            x.digits.replace_range(lo..hi, "");
            (x.caret, x.anchor) = (lo, lo);
        }),
        VK_RETURN => save_hex(key),
        VK_ESCAPE => {
            let shown = get(|w| shown_digits(w.theme, &settings::get(), key)).flatten().unwrap_or_default();
            edit(&mut |x| {
                if x.digits != shown || x.line.is_some() {
                    (x.digits, x.line, x.caret, x.anchor) = (shown.clone(), None, shown.len(), 0);
                }
            });
        }
        VK_SPACE => {}
        _ if ctrl && vk.0 == b'A' as u16 => edit(&mut |x| (x.caret, x.anchor) = (x.digits.len(), 0)),
        _ if ctrl && (vk.0 == b'C' as u16 || vk.0 == b'X' as u16) => {
            let cut = vk.0 == b'X' as u16;
            let text = get(|w| w.hexes.iter().find(|x| x.key == key).map(|x| x.digits[x.caret.min(x.anchor)..x.caret.max(x.anchor)].to_string())).flatten().unwrap_or_default();
            if !text.is_empty() {
                clip_set(&text);
                if cut {
                    hex_insert(key, "");
                }
            }
        }
        _ if ctrl && vk.0 == b'V' as u16 => match clip_get().as_deref().and_then(pasted) {
            Some(d) => edit(&mut |x| (x.digits, x.caret, x.anchor, x.line) = (d.clone(), 6, 6, None)),
            None => edit(&mut |x| x.line = Some(REFUSED)),
        },
        _ => return false,
    }
    true
}

/// The clipboard's text, if it has any.
fn clip_get() -> Option<String> {
    use windows::Win32::{Foundation::HGLOBAL, System::DataExchange::*, System::Memory::*};
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        OpenClipboard(hwnd()).ok()?;
        let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
            let g = HGLOBAL(h.0);
            let p = GlobalLock(g) as *const u16;
            if p.is_null() {
                return None;
            }
            let n = (0..GlobalSize(g) / 2).take_while(|&i| *p.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            let _ = GlobalUnlock(g);
            Some(s)
        });
        let _ = CloseClipboard();
        text
    }
}

fn clip_set(text: &str) {
    use windows::Win32::{Foundation::{GlobalFree, HANDLE}, System::DataExchange::*, System::Memory::*};
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe {
        let Ok(g) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) else { return };
        let p = GlobalLock(g) as *mut u16;
        if p.is_null() {
            let _ = GlobalFree(Some(g));
            return;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
        let _ = GlobalUnlock(g);
        if OpenClipboard(hwnd()).is_err() {
            let _ = GlobalFree(Some(g));
            return;
        }
        let _ = EmptyClipboard();
        // The clipboard owns the memory once it takes it.
        if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).is_err() {
            let _ = GlobalFree(Some(g));
        }
        let _ = CloseClipboard();
    }
}

/// Where a click in a hex box puts the caret: the digit edge nearest it.
pub(in crate::settings_page) fn caret_at(at: Bx, x: f32) -> usize {
    let cw = get(|w| unsafe {
        let dc = GetDC(Some(w.hwnd));
        let c = Canvas::new(dc, false, w.scale, w.theme, &w.fonts);
        let cw = c.width("0", F::Code);
        drop(c);
        ReleaseDC(Some(w.hwnd), dc);
        cw
    })
    .unwrap_or(8.0);
    ((x - at.0 - 10.0 - cw) / cw).round().clamp(0.0, 6.0) as usize
}

#[cfg(test)]
mod tests;
