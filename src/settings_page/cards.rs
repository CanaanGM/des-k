// The settings page's own card helpers: a setting's row with its default and notes, the section
// headings, the problem notes, the banners, and the hand edit lines (spec 0012).
pub(super) mod apps;
pub(super) mod attention;
pub(super) mod colors;
pub(super) mod indicator;
mod motion;
pub(super) mod names;
mod overview;
pub(super) mod place;
mod restore;
pub(super) mod screens;
mod shortcuts;
mod side;
mod touchpad;
pub(super) mod wallpaper;
pub(super) mod workspaces;

use super::*;
use super::rows::{Kind, MINUTES, Section, Why, combo_words, find_row, fix_line, home, is_digits, minutes, shortcut, shown, shown_in_row, title, title_of, unused, value};

/// One hand edit line in the page's words: "Indicator stays: 550 → 800 ms", "Open overview:
/// Win Tab → no key".
pub(super) fn changed(key: &str, was: &str, now: &str) -> String {
    if key == "gestures" {
        return "Gestures: changed".into();
    }
    // The change line lists only which names are set; the page shows the names themselves.
    if key == "names" {
        return "Workspace names: changed".into();
    }
    if key == "wallpaper" {
        return "Wallpapers: changed".into();
    }
    // The Apps section shows the rules themselves; the fallback below would read them as keys.
    if key == "apps" {
        return "App rules: changed".into();
    }
    // The tile rows show each screen's own count (spec 0040).
    if key == "workspaces_per_screen" {
        return "Workspaces per screen: changed".into();
    }
    // A workspace's own time per picture, or "default" (spec 0028 AC-12).
    if let Some(n) = key.strip_prefix("wallpaper.intervals.ws") {
        let words = |v: &str| v.parse().map_or(v.to_string(), minutes);
        return format!("Workspace {n} time per picture: {} → {}", words(was), words(now));
    }
    let name = title_of(key).unwrap_or(key);
    let words = |v: &str| -> String {
        if let Some(r) = find_row(key) {
            return match r.kind {
                Kind::Toggle | Kind::Pair(..) => shown(r, (v == "true") as i64),
                Kind::Choice(list) => list.iter().find(|(w, _)| *w == v).map_or(v.to_string(), |(_, l)| l.to_string()),
                Kind::Step { .. } | Kind::Ladder(_) => v.parse().map_or(v.to_string(), |n| shown(r, n)),
            };
        }
        if is_digits(key) {
            return if v.contains('+') || ["Win", "Ctrl", "Alt", "Shift"].contains(&v) { combo_words(v, true) } else { "no key".into() };
        }
        let list = v.trim_start_matches('[').trim_end_matches(']');
        let keys: Vec<String> = list.split(", ").filter(|k| !k.is_empty()).map(|k| combo_words(k, false)).collect();
        if keys.is_empty() { "no key".into() } else { keys.join(" or ") }
    };
    let (a, b) = (words(was), words(now));
    // "550 → 800 ms": the unit once, after the new value.
    match find_row(key).map(|r| r.kind) {
        Some(Kind::Step { unit, .. }) if !unit.is_empty() => format!("{name}: {} → {b}", a.trim_end_matches(unit).trim_end()),
        _ => format!("{name}: {a} → {b}"),
    }
}

impl Ctx<'_, '_> {
    /// The default and Reset at the end of a caption, when the value differs, or is bad.
    fn default_pieces(&self, key: &'static str, bad: bool) -> Vec<P> {
        let Some(r) = find_row(key) else { return vec![] };
        let (v, d) = (value(self.s, key), value(&DEFAULTS, key));
        if v == d && !bad {
            return vec![];
        }
        vec![t(format!(" Default: {}.", shown(r, d))), P::Link(" Reset".into(), Ctl::Reset(key))]
    }

    /// Problems whose key is this row's, as its in-row note (AC-SET-5).
    fn bad(&self, key: &str) -> Option<NoteSpec> {
        let problems = settings::store::problems();
        let p = problems.iter().find(|p| p.key == key)?;
        let mut lines = vec![P::Mono(p.message.clone())];
        if find_row(key).is_some() {
            lines.push(t(format!(" {}", fix_line(key, self.s))));
        }
        Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: title(p, self.s), lines, buttons: vec![], group: 1 })
    }

    /// A plain setting's row, with its control, default, Reset, greyed reasons and problem.
    fn setting(&mut self, key: &'static str, y: f32) -> f32 {
        let Some(r) = find_row(key) else { return 0.0 };
        let v = value(self.s, key);
        let why = unused(self.s, key);
        let na = !why.is_empty();
        let note = self.bad(key);
        let mut cap: Vec<P> = vec![];
        let base = match key {
            // What the card shows (spec 0023 AC-14).
            "indicator.style" if v == 0 && self.s.indicator_screen_name => "The number and a bar per workspace, with its name and, with two or more screens, the screen's name.",
            "indicator.style" if v == 0 => "The number and a bar per workspace, with its name.",
            "indicator.style" => "A dot per workspace, the current one wide.",
            "copilot_as_right_ctrl" if v != 0 => "So Win Ctrl keys work with it. Taken from Windows, where it opens Copilot, or what you set.",
            "copilot_as_right_ctrl" => "Windows keeps it (Copilot, or what you set). Win Ctrl keys need the left Ctrl.",
            // With Skip animations a change is instant (spec 0028 AC-6), so no fade is promised.
            "wallpaper.interval" if self.s.fastest => "How long each slideshow picture shows, unless its workspace has its own time. A slideshow moves only while you can see it. Same on every screen.",
            _ => r.cap,
        };
        if !base.is_empty() {
            cap.push(t(base));
        }
        let reasons: Vec<&Why> = why.iter().filter(|w| **w != Why::IndicatorOff).collect();
        if !reasons.is_empty() {
            cap.push(P::Why(" Not in use: ".into()));
            for (i, w) in reasons.iter().enumerate() {
                if i > 0 {
                    cap.push(P::Why(", and ".into()));
                }
                match w {
                    Why::AnimationNone => cap.push(P::Why("Overview animation is None".into())),
                    Why::Skip => {
                        cap.push(P::Link("Skip animations".into(), Ctl::Link(Link::Skip)));
                        cap.push(P::Why(" is on".into()));
                    }
                    Why::Dots => cap.push(P::Why("the dots show no names".into())),
                    Why::IndicatorOff => {}
                }
            }
            cap.push(P::Why(".".into()));
        }
        cap.extend(self.default_pieces(key, note.is_some()));
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str(), P::Icon(..) => "" }).collect::<String>().trim().to_string();
        let ro = self.cx.ro;
        match r.kind {
            Kind::Toggle => {
                let size = (self.cx.switch_w(v != 0), 20.0);
                self.cx.say(Ctl::Toggle(key), UIA_ButtonControlTypeId, r.title, &help, ro || na, (Some(v != 0), None));
                self.row(y, r.title, &cap, size, |c, x, y| c.cx.switch(Ctl::Toggle(key), x, y, v != 0, na, None), note.as_ref())
            }
            Kind::Pair(on, off) => {
                let size = (self.cx.seg_w(&[on, off]), 28.0);
                self.cx.say(Ctl::Pick(key), UIA_ComboBoxControlTypeId, r.title, &help, ro, (None, Some(shown(r, v))));
                let sel = Some(if v != 0 { 0 } else { 1 });
                self.row(y, r.title, &cap, size, |c, x, y| c.cx.seg(x, y, &[on, off], sel, false, |i| Ctl::Choice(key, i), Ctl::Pick(key), None), note.as_ref())
            }
            Kind::Choice(words) => {
                let labels: Vec<&str> = words.iter().map(|(_, l)| *l).collect();
                let size = (self.cx.seg_w(&labels), 28.0);
                self.cx.say(Ctl::Pick(key), UIA_ComboBoxControlTypeId, r.title, &help, ro || na, (None, Some(shown(r, v))));
                self.row(y, r.title, &cap, size, |c, x, y| c.cx.seg(x, y, &labels, Some(v as usize), na, |i| Ctl::Choice(key, i), Ctl::Pick(key), None), note.as_ref())
            }
            Kind::Step { lo, hi, unit, .. } => {
                // A bad value shows what des-k uses: the default, already v.
                let text = shown(r, v);
                let size = (self.cx.stepper_w(&text), 32.0);
                let spoken = match unit {
                    "%" if v == 0 => "Off".to_string(),
                    "%" => format!("{v} percent"),
                    _ => format!("{v} {}", if unit == "ms" { "milliseconds" } else if unit == "min" { "minutes" } else if unit == "mm" { "millimetres" } else { "" }).trim().to_string(),
                };
                self.cx.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, r.title, &help, ro || na, (None, Some(spoken)));
                self.row(y, r.title, &cap, size, |c, x, y| c.cx.stepper(Ctl::steps(key), x, y, step_words(v, unit), (v > lo, v < hi), na, false, None), note.as_ref())
            }
            Kind::Ladder(ladder) => {
                let text = shown(r, v);
                let size = (self.cx.stepper_w(&text), 32.0);
                self.cx.say(Ctl::Stepper(key), UIA_SpinnerControlTypeId, r.title, &help, ro || na, (None, Some(text)));
                let (lo, hi) = (ladder[0], ladder[ladder.len() - 1]);
                self.row(y, r.title, &cap, size, |c, x, y| c.cx.stepper(Ctl::steps(key), x, y, step_words(v, MINUTES), (v > lo, v < hi), na, false, None), note.as_ref())
            }
        }
    }

    // ---- Sections ----

    pub(super) fn heading(&mut self, sec: Option<Section>, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        self.out.tops.push((sec, y - self.top0));
        let name = sec.map_or("Notes", Section::name);
        self.cx.c.text(X0, y, 28.0, name, F::Title, b.tx);
        if sec == Some(Section::Restore) {
            self.cx.chip(X0 + self.cx.c.width(name, F::Title) + 12.0, y + 2.0, "\u{E777}", "Applies from des-k's next start");
        }
        let line = sec.map_or("Lines in settings.toml that des-k skips. Fix them in the file; each note goes once its line is fixed.", Section::line);
        // The color picker's way in, said only when a click works (spec 0021 AC-1, AC-2).
        let live = self.picks() && !self.w.pics.screens.is_empty();
        let more = match sec {
            Some(Section::Workspaces) if live => " Click a tile to change its color.",
            Some(Section::Overview) if live => " Click a workspace in the picture to change its color.",
            _ => "",
        };
        let line = format!("{line}{more}");
        let h = 30.0 + self.cx.c.para(X0, y + 30.0, self.cw, 20.0, &[(&line, F::Body, b.tx2)]);
        h + 12.0
    }

    /// This section's problems no row owns (a key des-k turned off), above its cards.
    fn section_notes(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = 0.0;
        let owned = |k: &str| find_row(k).is_some() || shortcut(k).is_some() || k == "workspaces" || shown_in_row(k);
        let problems = settings::store::problems();
        let list: Vec<&Problem> = problems.iter().filter(|p| home(&p.key) == Some(sec) && !owned(&p.key)).collect();
        for p in list {
            let fix = match sec {
                Section::Shortcuts => "Set a key for it below to fix the file. ",
                _ => "",
            };
            let n = NoteSpec { tone: Tone::Warn, icon: I_WARN, title: title(p, self.s), lines: vec![t(fix), P::Mono(p.message.clone())], buttons: vec![], group: 1 };
            h += self.cx.note(X0, y + h, self.cw, &n) + 12.0;
        }
        h
    }

    pub(super) fn section(&mut self, sec: Section, y: f32) -> f32 {
        let mut h = self.heading(Some(sec), y);
        h += self.section_notes(sec, y + h);
        h += match sec {
            Section::Workspaces => {
                let first = self.card(y + h, |c, y| c.workspaces(y)) + 12.0;
                let second = self.card(y + h + first, |c, y| c.names(y)) + 12.0;
                first + second + self.card(y + h + first + second, |c, y| c.wallpaper(y))
            }
            Section::Apps => {
                let first = self.card(y + h, |c, y| c.app_add(y)) + 12.0;
                first + self.card(y + h + first, |c, y| c.app_rules(y))
            }
            Section::Shortcuts => self.shortcuts(y + h),
            Section::Restore => self.card(y + h, |c, y| c.restore(y)),
            Section::Indicator => {
                let first = self.card(y + h, |c, y| c.indicator(y)) + 12.0;
                first + self.card(y + h + first, |c, y| c.place(y))
            }
            Section::Attention => self.card(y + h, |c, y| c.attention(y)),
            Section::Colors => self.colors(y + h),
            Section::Overview => self.card(y + h, |c, y| {
                let mut h = c.overview_art(y);
                for (i, key) in ["overview.labels", "overview.animation", "overview.fade_ms", "overview.live"].into_iter().enumerate() {
                    if i > 0 {
                        c.sep(y + h);
                    }
                    h += c.setting(key, y + h);
                }
                h
            }),
            Section::Motion => self.card(y + h, |c, y| {
                let h = c.motion(y);
                c.sep(y + h);
                h + c.setting("fastest", y + h)
            }),
            Section::Touchpad => self.touchpad(y + h),
        };
        h
    }

    /// The notes above the column, fixed while it scrolls: can't be read, couldn't save,
    /// changed by hand. Returns their height with their margins (0 with none).
    pub(super) fn banners(&mut self) -> f32 {
        let mut list: Vec<NoteSpec> = vec![];
        if let Some(why) = settings::store::parse_error() {
            list.push(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: "settings.toml can't be read, so nothing here can change".into(),
                lines: vec![t("des-k uses the settings it read before (at start, its defaults); they're shown here. Fix the file and this page works again. "), P::Mono(why)],
                buttons: vec![(Ctl::Open, "Open settings.toml")],
                group: 2,
            });
        }
        if let Some((what, why)) = &self.w.error {
            list.push(NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: format!("Couldn't save “{what}”"),
                lines: vec![t("Your change isn't saved or in use. Close settings.toml in any program that has it open, then change this again. Windows said: "), P::Mono(why.clone())],
                buttons: vec![(Ctl::Dismiss, "Dismiss")],
                group: 2,
            });
        }
        if let Some((time, lines)) = &self.w.edited {
            list.push(NoteSpec {
                tone: Tone::Info,
                icon: I_INFO,
                title: format!("settings.toml changed at {time} and is in use"),
                lines: vec![t(if lines.is_empty() { "Nothing des-k shows here changed.".to_string() } else { lines.join(". ") + "." })],
                buttons: vec![(Ctl::DismissEdit, "Dismiss")],
                group: 2,
            });
        }
        if list.is_empty() {
            return 0.0;
        }
        let b = self.cx.c.t.b;
        let total = 12.0 + self.measured(|c| list.iter().map(|n| c.cx.note(X0, 0.0, c.cw, n) + 8.0).sum::<f32>()) - 8.0 + 12.0;
        self.cx.c.fill_rr((NAV, 0.0, self.w.vw - NAV, total), 0.0, op(b.bg));
        let mut y = 12.0;
        for n in &list {
            y += self.cx.note(X0, y, self.cw, n) + 8.0;
        }
        total
    }
}

/// A note as it is drawn: its tone and icon, title, caption pieces and buttons.
pub(super) type NoteSpec = crate::controls::NoteSpec<Ctl, u8>;

#[cfg(test)]
mod tests;
