// The Wallpaper card: each workspace's picture or slideshow, per screen or for all, its menu and
// previews, and what each choice writes (spec 0026, spec 0028).
use super::*;
use crate::settings_page::choosers::{Lead, Mark, chooser_lines};
use crate::settings_page::rows::{Card, LADDER, MINUTES, ladder_step, minutes};
use super::names::{NameChoice, name_choices};

const WALL_PICK_W: f32 = 264.0; // a Wallpaper row's menu button and its screen chooser (spec 0026)
const WALL_TIME_W: f32 = 148.0; // a Wallpaper row's room for its time per picture (spec 0028)

/// A workspace's own time per picture, `[wallpaper.intervals] wsN`: its stepper's key.
pub(in crate::settings_page) const IV_KEYS: [&str; 9] = ["wallpaper.intervals.ws1", "wallpaper.intervals.ws2", "wallpaper.intervals.ws3", "wallpaper.intervals.ws4", "wallpaper.intervals.ws5", "wallpaper.intervals.ws6", "wallpaper.intervals.ws7", "wallpaper.intervals.ws8", "wallpaper.intervals.ws9"];

// ---- Wallpaper (spec 0026 AC-13, AC-15) ----

/// How a Wallpaper row reads, set by the file's keys alone, as a Names row does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum WallState {
    All,     // an All screens row: [wallpaper] wsN
    Follow,  // a screen's row with no key of its own: it shows All screens' picture
    Own,     // a screen's own picture, even one equal to All screens'
    Windows, // "" on this screen: your Windows wallpaper there
}

/// Row `k` for `screen` (None: All screens): its state and the value it shows, `Pick::None`
/// for your Windows wallpaper.
pub(crate) fn wall_held(s: &Settings, screen: Option<&str>, k: usize) -> (WallState, Pick) {
    let all = s.wallpaper[k].clone();
    let Some(id) = screen else { return (WallState::All, all) };
    match s.wallpaper_screens.get(id).and_then(|a| a[k].clone()) {
        None => (WallState::Follow, all),
        Some(Pick::None) => (WallState::Windows, Pick::None),
        Some(p) => (WallState::Own, p),
    }
}

/// A value's name on its menu button and in captions: the picture or folder, or "3 pictures"
/// for a set (spec 0028 AC-11); "" for none.
fn wall_label(p: &Pick) -> String {
    match p {
        Pick::None => String::new(),
        Pick::Picture(f) | Pick::Folder(f) => f.clone(),
        Pick::Set(v) => format!("{} pictures", v.len()),
    }
}

/// Why a value can't show, in its row's words: a set says it of its pictures (spec 0028 AC-8).
fn wall_why(p: &Pick, problem: crate::wallpaper::Problem) -> &'static str {
    use crate::wallpaper::Problem;
    match (p, problem) {
        (Pick::Set(_), Problem::Missing) => "none are in the folder",
        (Pick::Set(_), Problem::Unreadable) => "none can be read",
        _ => problem.why(),
    }
}

/// A value as the file holds it: a name, or a list for a set (spec 0028 AC-14).
fn wall_value(p: &Pick) -> toml_edit::Value {
    match p {
        Pick::None => "".into(),
        Pick::Picture(f) | Pick::Folder(f) => f.as_str().into(),
        Pick::Set(v) => {
            let mut list: toml_edit::Array = v.iter().map(String::as_str).collect();
            list.fmt();
            list.into()
        }
    }
}

pub(in crate::settings_page) fn wall_path(screen: Option<&str>, k: usize) -> Vec<String> {
    let key = format!("ws{}", k + 1);
    match screen {
        Some(id) => vec!["wallpaper".into(), "screens".into(), id.into(), key],
        None => vec!["wallpaper".into(), key],
    }
}

/// What Use this there too writes for row `k`: each plugged in screen's own value taken out,
/// in one write, so those screens follow All screens again. A screen not plugged in keeps its
/// own, since you can't see what it would lose.
pub(in crate::settings_page) fn wall_use_edit(s: &Settings, present: &[String], k: usize) -> Option<Edit> {
    present
        .iter()
        .filter(|id| s.wallpaper_screens.get(*id).is_some_and(|o| o[k].is_some()))
        .map(|id| Edit::Remove(wall_path(Some(id), k)))
        .reduce(|a, b| Edit::Pair(Box::new(a), Box::new(b)))
}

/// What choosing `pick` (`Pick::None` for Windows wallpaper) in row `k` writes, if anything
/// (spec 0026 AC-15, spec 0028 AC-11). What the row already follows takes its key out; on All
/// screens Windows wallpaper is no key. Values compare by their key, so a set by its names.
/// Windows wallpaper on All screens is a reset (amended 2026-11-22): every screen's own value
/// for the row goes too, plugged in or not, so no screen keeps a picture there.
pub(crate) fn wall_edit(s: &Settings, screen: Option<&str>, k: usize, pick: &Pick) -> Option<Edit> {
    let path = wall_path(screen, k);
    let (all, key) = (s.wallpaper[k].key(), pick.key());
    let Some(id) = screen else {
        return match pick {
            Pick::None => {
                let own = s.wallpaper_screens.iter().filter(|(_, o)| o[k].is_some()).map(|(id, _)| Edit::Remove(wall_path(Some(id), k)));
                (!all.is_empty()).then_some(Edit::Remove(path)).into_iter().chain(own).reduce(|a, b| Edit::Pair(Box::new(a), Box::new(b)))
            }
            _ => (key != all).then(|| Edit::At(path, wall_value(pick))),
        };
    };
    let own = s.wallpaper_screens.get(id).and_then(|a| a[k].as_ref().map(Pick::key));
    match own {
        _ if key == all => own.is_some().then_some(Edit::Remove(path)),
        Some(o) if o == key => None,
        _ => Some(Edit::At(path, wall_value(pick))),
    }
}

/// What ticking or unticking `file` in row `k`'s menu writes (spec 0028 AC-11): the row's
/// pictures with `file` added or taken out, sorted by name; one picture is a plain name, none is
/// Windows wallpaper. A row that follows all screens starts from all screens' value.
pub(crate) fn wall_tick(s: &Settings, screen: Option<&str>, k: usize, file: &str) -> Option<Edit> {
    let mut names = match wall_held(s, screen, k).1 {
        Pick::Picture(f) => vec![f],
        Pick::Set(v) => v,
        Pick::None | Pick::Folder(_) => vec![],
    };
    match names.iter().position(|n| n == file) {
        Some(i) => {
            names.remove(i);
        }
        None => names.push(file.to_string()),
    }
    names.sort_by(|a, b| settings::parse::name_order(a, b));
    let pick = match names.len() {
        0 => Pick::None,
        1 => Pick::Picture(names.remove(0)),
        _ => Pick::Set(names),
    };
    wall_edit(s, screen, k, &pick)
}

/// What a step of row `k`'s time per picture writes (spec 0028 AC-11): the next ladder value as
/// `[wallpaper.intervals] wsN`, or the key taken out when that is the default, so the file never
/// keeps one that changes nothing.
pub(crate) fn wall_interval_edit(s: &Settings, k: usize, up: bool) -> Option<Edit> {
    let v = s.interval_for(k) as i64;
    let n = ladder_step(LADDER, v, up);
    let path = vec!["wallpaper".to_string(), "intervals".to_string(), format!("ws{}", k + 1)];
    match n {
        n if n == v => None,
        n if n == s.wallpaper_interval as i64 => s.wallpaper_intervals[k].is_some().then_some(Edit::Remove(path)),
        n => Some(Edit::At(path, n.into())),
    }
}

/// The screen a preview takes its shape and Windows wallpaper from: the chosen one while it is
/// plugged in, else the main display (the one at 0, 0), else the first.
fn wall_rect(v: &crate::ws::View, screen: Option<&str>) -> Option<RECT> {
    let main = |p: &&crate::ws::Present| p.full.left <= 0 && p.full.top <= 0 && p.full.right > 0 && p.full.bottom > 0;
    screen.and_then(|id| v.present.iter().find(|p| p.id == id)).or_else(|| v.present.iter().find(main)).or_else(|| v.present.first()).map(|p| p.full)
}

/// The screen whose slideshow a row's preview shows (spec 0028 AC-11): the chosen one while it
/// is plugged in, the main display for All screens, and None for a screen not plugged in.
fn wall_shown_id(v: &crate::ws::View, screen: Option<&str>) -> Option<String> {
    let main = |p: &&crate::ws::Present| p.full.left <= 0 && p.full.top <= 0 && p.full.right > 0 && p.full.bottom > 0;
    match screen {
        Some(id) => v.present.iter().find(|p| p.id == id).map(|p| p.id.clone()),
        None => v.present.iter().find(main).or_else(|| v.present.first()).map(|p| p.id.clone()),
    }
}

/// A preview's size in dip: the screen's shape inside 64 × 40.
fn wall_box(rect: Option<RECT>) -> (f32, f32) {
    let aspect = rect.map_or(1.6, |r| (r.right - r.left) as f32 / (r.bottom - r.top).max(1) as f32);
    if aspect >= 1.6 { (64.0, (64.0 / aspect).round()) } else { ((40.0 * aspect).round(), 40.0) }
}

/// A preview's size in pixels at `scale`, the size its picture is decoded at.
fn wall_px(rect: Option<RECT>, scale: f32) -> (i32, i32) {
    let (w, h) = wall_box(rect);
    ((w * scale).round() as i32, (h * scale).round() as i32)
}

/// The places a Wallpaper menu's list scrolls: its `n` pictures, then with `m` folders a
/// Slideshows heading and the folders (spec 0028 AC-11).
fn wall_total(n: usize, m: usize) -> usize {
    n + m + usize::from(m > 0)
}

/// Menu line `line` (0 Windows wallpaper, 1 to n pictures, then folders, then Open folder) as
/// a place in the list, counting the heading: the list's places are 1 to `wall_total`.
fn wall_slot(line: usize, n: usize, m: usize) -> usize {
    if line > n && m > 0 { line + 1 } else { line }
}

/// How many pictures a Wallpaper menu shows at once in a `vh` dip window: up to 10.
fn wall_rows(vh: f32, n: usize) -> usize {
    (((vh - 200.0) / 28.0).floor().max(3.0) as usize).min(10).min(n)
}

/// The first picture shown so menu line `line` (1 to n are the pictures) is in view.
fn wall_reveal(top: usize, line: usize, n: usize, rows: usize) -> usize {
    let top = match line {
        l if l == 0 || l > n => top,
        l if l - 1 < top => l - 1,
        l if l > top + rows => l - rows,
        _ => top,
    };
    top.min(n.saturating_sub(rows))
}

/// The wallpapers folder's pictures, by name with case ignored (AC-13, AC-14), read as a
/// row's menu opens. A missing folder lists none.
pub(in crate::settings_page) fn wall_folder() -> Vec<String> {
    wall_names(&crate::ws::data_dir().join("wallpapers"), true, settings::parse::is_picture_name)
}

/// The folder's slideshow folders and their picture counts (spec 0028 AC-11, AC-13), read only
/// as a row's menu opens: subfolders whose names the file's reader takes as a folder.
pub(in crate::settings_page) fn wall_dirs() -> Vec<(String, usize)> {
    let dir = crate::ws::data_dir().join("wallpapers");
    wall_names(&dir, false, settings::parse::is_folder_name)
        .into_iter()
        .map(|f| {
            let n = wall_count(&f);
            (f, n)
        })
        .collect()
}

/// How many pictures folder `name` holds at its top level; 0 when it isn't there.
pub(in crate::settings_page) fn wall_count(name: &str) -> usize {
    wall_names(&crate::ws::data_dir().join("wallpapers").join(name), true, settings::parse::is_picture_name).len()
}

/// The names of `dir`'s files (or subfolders) that pass `keep`, in slideshow order.
fn wall_names(dir: &std::path::Path, files: bool, keep: fn(&str) -> bool) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| if files { t.is_file() } else { t.is_dir() }))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| keep(n))
        .collect();
    names.sort_by(|a, b| settings::parse::name_order(a, b));
    names
}

/// What the Wallpaper card's previews need: the screen, the preview's size in pixels, and the
/// pictures its rows show, a slideshow's the one showing now (spec 0028 AC-11).
pub(in crate::settings_page) fn wall_wanted(w: &Win, s: &Settings) -> Option<(RECT, (i32, i32), Vec<String>)> {
    let screen = wall_screen(w, s);
    let rect = wall_rect(&w.pics.view, screen.as_deref())?;
    let mut files: Vec<String> = (0..s.workspaces).filter_map(|k| wall_shows(w, s, screen.as_deref(), k)).filter(|f| !f.is_empty()).collect();
    files.sort();
    files.dedup();
    Some((rect, wall_px(Some(rect), w.scale), files))
}

/// The picture row `k`'s preview shows: its picture, "" for your Windows wallpaper, and for a
/// slideshow the picture showing now on the row's screen. When that screen runs no such
/// slideshow (it has its own value, isn't plugged in, or hasn't chosen yet) a set shows its
/// first picture and a folder None, a folder tile, since naming one would mean reading it.
fn wall_shows(w: &Win, s: &Settings, screen: Option<&str>, k: usize) -> Option<String> {
    let pick = wall_held(s, screen, k).1;
    let now = || {
        let id = wall_shown_id(&w.pics.view, screen)?;
        // The main display may run its own slideshow for this workspace, not All screens'.
        (wall_held(s, Some(&id), k).1 == pick).then(|| w.pics.wall_now.get(&(id, k)).cloned())?
    };
    match pick {
        Pick::None => Some(String::new()),
        Pick::Picture(f) => Some(f),
        Pick::Folder(_) => now(),
        Pick::Set(ref v) => now().or_else(|| v.first().cloned()),
    }
}

impl Ctx<'_, '_> {
    // ---- Wallpaper (spec 0026 AC-8, AC-12 to AC-15, AC-18), as approved in
    // pdesk-design/wallpaper/wallpaper-design.html r3; its slideshow parts are spec 0028's ----

    /// The Wallpaper card, built like the Names card: with two or more known screens the
    /// chooser, then a row per workspace in use, then Fit, Dim and Transition.
    pub(super) fn wallpaper(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let (list, i) = wall_chosen(self.w);
        let two = wall_two(self.w, self.s);
        self.out.cards.push((Card::Wallpaper, y - self.top0));
        self.header(y, "Wallpaper", None, "");
        if two {
            let who = if i == 0 { "all screens".to_string() } else { list[i].name.clone() };
            let rw = self.cx.c.width("For ", F::Small) + self.cx.c.width(&who, F::SmallBold);
            self.cx.c.para(X0 + self.cw - 16.0 - rw, y + 11.0, rw + 1.0, 16.0, &[("For ", F::Small, b.tx2), (&who, F::SmallBold, b.tx)]);
        }
        let mut h = 38.0;
        if self.w.pics.wall_failed {
            let n = NoteSpec {
                tone: Tone::Warn,
                icon: I_WARN,
                title: "des-k can't draw behind your desktop icons right now.".into(),
                lines: vec![t("Every screen shows your Windows wallpaper. des-k keeps trying every 5 seconds and right after explorer restarts; your choices below stay as they are.")],
                buttons: vec![],
                group: 1,
            };
            h += 12.0 + self.cx.note_body(X0 + 16.0, y + h + 12.0, self.cw - 32.0, &n) + 12.0;
            self.sep(y + h);
        }
        if two {
            h += self.wall_chooser_row(y + h, &list, i);
        }
        // While any row is a slideshow every row keeps its time's space, so the menus line up.
        let screen = wall_screen(self.w, self.s);
        let slides = (0..self.s.workspaces).any(|k| matches!(wall_held(self.s, screen.as_deref(), k).1, Pick::Folder(_) | Pick::Set(_)));
        for k in 0..self.s.workspaces {
            if k > 0 || two || self.w.pics.wall_failed {
                self.sep(y + h);
            }
            h += self.wall_row(y + h, k, slides);
        }
        for key in ["wallpaper.fit", "wallpaper.dim", "wallpaper.transition", "wallpaper.interval", "wallpaper.shuffle"] {
            self.sep(y + h);
            h += self.setting(key, y + h);
        }
        h
    }

    fn wall_chooser_row(&mut self, y: f32, list: &[NameChoice], i: usize) -> f32 {
        let cap = match &list[i].id {
            None => "Pictures every screen shows, unless it has its own. They come from des-k's wallpapers folder; each row's menu opens it.".to_string(),
            Some(id) => {
                let own = self.s.wallpaper_screens.get(id);
                match (0..self.s.workspaces).filter(|&k| own.is_some_and(|o| o[k].is_some())).count() {
                    0 => "Every row follows all screens.".to_string(),
                    1 => "1 workspace differs on this screen. Other rows follow all screens.".to_string(),
                    n => format!("{n} workspaces differ on this screen. Other rows follow all screens."),
                }
            }
        };
        let name = list[i].name.clone();
        self.cx.say(Ctl::WallScreens, UIA_ComboBoxControlTypeId, "Screen", &cap, false, (None, Some(name.clone())));
        self.row(y, "Screen", &[t(cap)], (WALL_PICK_W, 32.0), |c, x, y| c.screen_picker(Ctl::WallScreens, x, y, WALL_PICK_W, &name), None)
    }

    /// One workspace's row: its chip, a preview in the screen's shape fitted and dimmed as the
    /// screen shows it, its menu button, and for a slideshow its time per picture (spec 0028
    /// AC-11); the caption says how it follows, or why it can't show. `slides`: some row on the
    /// card is a slideshow, so this one keeps the time's space too.
    fn wall_row(&mut self, y: f32, k: usize, slides: bool) -> f32 {
        let screen = wall_screen(self.w, self.s);
        let (state, pick) = wall_held(self.s, screen.as_deref(), k);
        let key = pick.key();
        let name = wall_label(&pick);
        let problem = self.w.pics.wall_problems.iter().find(|(f, _)| !key.is_empty() && *f == key).map(|p| p.1);
        let all_none = self.s.wallpaper[k] == Pick::None;
        let same = |ro: bool| (!ro).then(|| P::Link(" Same as all screens".into(), Ctl::WallSame(k)));
        let show = matches!(pick, Pick::Folder(_) | Pick::Set(_));
        let count = match &pick {
            Pick::Set(v) => v.len(),
            Pick::Folder(f) => self.w.pics.wall_counts.iter().find(|(n, _)| n == f).map_or(0, |c| c.1),
            _ => 0,
        };
        let pictures = if count == 1 { "1 picture".to_string() } else { format!("{count} pictures") };
        let cap: Vec<P> = match (problem, state) {
            (Some(p), _) => vec![P::Warn(format!(" Can't show {name} · {}", wall_why(&pick, p)))],
            (None, WallState::All) => {
                let mut cap = vec![];
                if show {
                    let ticked = if matches!(pick, Pick::Set(_)) { " you ticked" } else { "" };
                    let time = if self.s.wallpaper_intervals[k].is_some() { "its own time" } else { "the default time below" };
                    cap.push(t(format!("Slideshow of {pictures}{ticked}, each for {time}.")));
                }
                // Every screen you have overrides this row, so it shows nowhere: said like
                // Transition's "Not in use", ahead of which screens differ.
                let present = &self.w.pics.view.present;
                let unused = !present.is_empty() && present.iter().all(|p| self.s.wallpaper_screens.get(&p.id).is_some_and(|o| o[k].is_some()));
                if unused {
                    cap.push(P::Why(format!("{}Not in use: ", if cap.is_empty() { "" } else { " " })));
                    cap.push(t(if present.len() == 1 { "your only screen has its own." } else { "every screen you have has its own." }));
                }
                let differ = self.wall_differ(k);
                if !differ.is_empty() {
                    cap.push(t(format!("{}{differ}", if cap.is_empty() { "" } else { " " })));
                }
                // One click from the reason to the fix, for the screens plugged in only (spec 0026
                // AC-13 grown on 2026-11-21: you asked for it, VS Code's "also modified in" style).
                if unused && !self.cx.ro {
                    cap.push(P::Link(" Use this there too".into(), Ctl::WallUse(k)));
                }
                cap
            }
            (None, WallState::Follow) if all_none => vec![t("Follows all screens, which has no picture for it.")],
            (None, WallState::Follow) if show => vec![t(format!("Follows all screens: a slideshow of {pictures}."))],
            (None, WallState::Follow) => vec![t("Follows all screens.")],
            (None, WallState::Own) if show => [Some(t("Its own slideshow on this screen.")), same(self.cx.ro)].into_iter().flatten().collect(),
            (None, WallState::Own) => [Some(t("Its own picture on this screen.")), same(self.cx.ro)].into_iter().flatten().collect(),
            (None, WallState::Windows) => [Some(t("Your Windows wallpaper on this screen.")), same(self.cx.ro)].into_iter().flatten().collect(),
        };
        let (label, grey) = match state {
            WallState::Follow => (format!("{} · all screens", if key.is_empty() { "Windows wallpaper" } else { name.as_str() }), true),
            _ if key.is_empty() => ("Windows wallpaper".to_string(), true),
            _ => (name.clone(), false),
        };
        let mark = match pick {
            Pick::Folder(_) => Mark::Glyph(I_FOLDER),
            Pick::Set(_) => Mark::Stack,
            _ => Mark::Glyph(I_PIC),
        };
        let help: String = cap.iter().map(|p| match p { P::T(s) | P::Why(s) | P::Link(s, _) | P::Mono(s) | P::Warn(s) => s.as_str(), P::Icon(..) => "" }).collect();
        self.cx.say(Ctl::WallPick(k), UIA_ComboBoxControlTypeId, &format!("Workspace {} wallpaper", k + 1), help.trim(), self.cx.ro, (None, Some(label.clone())));
        // A value that can't show leaves your Windows wallpaper on the screen (AC-8).
        let shown = if problem.is_some() { Some(String::new()) } else { wall_shows(self.w, self.s, screen.as_deref(), k) };
        let rect = wall_rect(&self.w.pics.view, screen.as_deref());
        let chip = self.ws_chip_w(k) + 10.0;
        let cw = chip + 64.0 + 10.0 + WALL_PICK_W + if slides { 10.0 + WALL_TIME_W } else { 0.0 };
        // The time is the same on every screen, so it is never greyed by a screen; grey means
        // it follows the Interval row.
        let iv = self.s.interval_for(k) as i64;
        let iv_grey = self.s.wallpaper_intervals[k].is_none();
        if show {
            self.cx.say(Ctl::Stepper(IV_KEYS[k]), UIA_SpinnerControlTypeId, &format!("Workspace {} time per picture", k + 1), "", self.cx.ro, (None, Some(minutes(iv))));
        }
        let ends = (LADDER[0], LADDER[LADDER.len() - 1]);
        self.row(y, &format!("Workspace {}", k + 1), &cap, (cw, 40.0), |c, x, y| {
            c.ws_chip(x, y + 11.0, k);
            c.wall_preview(x + chip, y, rect, shown.as_deref());
            let px = x + chip + 74.0;
            c.app_picker(Ctl::WallPick(k), (px, y + 4.0, WALL_PICK_W), mark, &label, grey);
            if show {
                c.cx.stepper(Ctl::steps(IV_KEYS[k]), px + WALL_PICK_W + 10.0, y + 4.0, step_words(iv, MINUTES), (iv > ends.0, iv < ends.1), false, iv_grey, None);
            }
        }, None)
    }

    /// An All screens row's caption words: each screen whose own value differs; "" for none.
    fn wall_differ(&self, k: usize) -> String {
        let line: Vec<String> = self
            .s
            .wallpaper_screens
            .iter()
            .filter_map(|(id, o)| match o[k].as_ref()? {
                Pick::None => Some(format!("{}: Windows wallpaper there.", self.screen_called(id))),
                own => Some(format!("{} has its own: {}.", self.screen_called(id), wall_label(own))),
            })
            .collect();
        line.join(" ")
    }

    /// A row's preview in a 64 × 40 slot: `file` fitted for `rect`'s screen and dimmed as it shows
    /// there (AC-18), or with "" that screen's Windows wallpaper, never dimmed. A plain tile
    /// until its decode is ready; None, a slideshow with no picture yet, is a folder tile.
    fn wall_preview(&mut self, x: f32, y: f32, rect: Option<RECT>, file: Option<&str>) {
        let b = self.cx.c.t.b;
        let (w, h) = wall_box(rect);
        let bx = (x + (64.0 - w) / 2.0, y + (40.0 - h) / 2.0, w, h);
        let size = wall_px(rect, self.w.scale);
        let Some(file) = file else {
            self.cx.c.fill_rr(bx, 3.0, op(b.track));
            let gw = self.cx.c.width(I_FOLDER, F::IconSmall);
            self.cx.icon(bx.0 + (w - gw) / 2.0, bx.1, h, I_FOLDER, F::IconSmall, b.tx2);
            let edge = if self.cx.c.t.hc { op(b.line) } else { ui::color::alpha(b.tx, 0.18) };
            self.cx.c.stroke_rr((bx.0 + 0.5, bx.1 + 0.5, bx.2 - 1.0, bx.3 - 1.0), 3.0, 1.0, edge);
            return;
        };
        let mut drawn = false;
        match rect {
            Some(r) if !file.is_empty() => {
                if let Some(p) = crate::wallpaper::cached_ws(r, file, size) {
                    self.cx.c.picture_rr(bx, 3.0, &p);
                    let a = crate::backdrop::paint::dim_alpha(self.s.wallpaper_dim);
                    if a > 0 {
                        self.cx.c.fill_rr(bx, 3.0, ui::color::alpha(0, a as f32 / 255.0));
                    }
                    drawn = true;
                }
            }
            Some(r) => match crate::wallpaper::cached(r, size) {
                Some(crate::wallpaper::Face::Picture(p)) => {
                    self.cx.c.picture_rr(bx, 3.0, &p);
                    drawn = true;
                }
                Some(crate::wallpaper::Face::Color(c)) => {
                    self.cx.c.fill_rr(bx, 3.0, op(c));
                    drawn = true;
                }
                _ => {}
            },
            None => {}
        }
        if !drawn {
            self.cx.c.fill_rr(bx, 3.0, op(b.track));
        }
        let edge = if self.cx.c.t.hc { op(b.line) } else { ui::color::alpha(b.tx, 0.18) };
        self.cx.c.stroke_rr((bx.0 + 0.5, bx.1 + 0.5, bx.2 - 1.0, bx.3 - 1.0), 3.0, 1.0, edge);
    }

    /// A Wallpaper row's open menu (spec 0026 AC-13, AC-14; spec 0028 AC-11): Windows wallpaper,
    /// then one list that scrolls when it doesn't fit, of the folder's pictures with their tick
    /// boxes, a Slideshows heading and its folders with their picture counts, then Open folder and
    /// a line on how ticking works. On one screen that line adds that choosing All screens' value
    /// follows again; an empty folder says which files to put there.
    pub(in crate::settings_page) fn wall_menu(&mut self) {
        let Some((Ctl::WallPick(k), at, focus)) = self.w.chooser else { return };
        let t = self.cx.c.t;
        let b = t.b;
        let screen = wall_screen(self.w, self.s);
        let files = self.w.wall_files.clone();
        let dirs = self.w.wall_dirs.clone();
        let (n, m) = (files.len(), dirs.len());
        let total = wall_total(n, m);
        let (_, cur) = chooser_lines(self.w, self.s, Ctl::WallPick(k));
        let held = wall_held(self.s, screen.as_deref(), k).1;
        let ticked = |f: &str| match &held {
            Pick::Picture(p) => p == f,
            Pick::Set(v) => v.iter().any(|x| x == f),
            _ => false,
        };
        let all = self.s.wallpaper[k].key();
        let rows = wall_rows(self.w.vh, total);
        let top = self.w.wall_top.min(total.saturating_sub(rows));
        let mw = at.2.max(if screen.is_some() { 320.0 } else { 300.0 });
        let empty = "No pictures yet. Put .jpg, .png, .bmp or .gif files in the folder, or a folder of them for a slideshow.";
        let hint = "Tick pictures for a slideshow of them. Click a name to pick just that one.";
        let follow = "Choosing All screens' picture makes this row follow all screens again.";
        let foot_text = match (n > 0, screen.is_some() && total > 0) {
            (true, true) => format!("{hint} {follow}"),
            (true, false) => hint.to_string(),
            (false, true) => follow.to_string(),
            (false, false) => String::new(),
        };
        let para_h = |c: &mut Self, text: &str| c.measured(|c| c.cx.c.para(0.0, 0.0, mw - 28.0, 16.0, &[(text, F::Small, b.tx2)]));
        let slot = if total == 0 { para_h(self, empty) + 12.0 } else { rows as f32 * 28.0 };
        let foot = if foot_text.is_empty() { 0.0 } else { para_h(self, &foot_text) + 12.0 };
        let mh = 4.0 + 24.0 + 28.0 + 9.0 + slot + 9.0 + 28.0 + foot + 4.0;
        let below = at.1 + at.3 + 4.0;
        let my = if below + mh <= self.w.vh - 8.0 { below } else { (at.1 - mh - 4.0).max(8.0) };
        let mx = at.0.min(self.w.vw - mw - 16.0).max(NAV);
        self.cx.c.fill_rr((mx - 1.0, my - 1.0, mw + 2.0, mh + 2.0), 9.0, op(if t.hc { b.tx } else { mix(b.bg, b.tx, 0.10) }));
        if !t.hc {
            self.cx.c.fill_rr((mx, my + 4.0, mw, mh), 8.0, ui::color::alpha(0, 0.08));
        }
        self.cx.c.fill_rr((mx, my, mw, mh), 8.0, op(b.card));
        self.cx.c.text(mx + 14.0, my + 4.0, 24.0, "Picture", F::SmallBold, b.tx2);
        let (x, w) = (mx + 4.0, mw - 8.0);
        // On one screen, the line All screens uses says so.
        let tag = |name: &str| -> &'static str { if screen.is_some() && name == all { "All screens" } else { "" } };
        let mut ly = my + 28.0;
        self.wall_line(0, (x, ly, w), "Windows wallpaper", tag(""), Lead::Check(cur == Some(0)), focus == 0);
        ly += 28.0;
        self.cx.c.rule(mx + 10.0, ly + 4.0, mw - 20.0, op(b.line));
        ly += 9.0;
        if total == 0 {
            self.cx.c.para(mx + 14.0, ly + 6.0, mw - 28.0, 16.0, &[(empty, F::Small, b.tx2)]);
        } else {
            for (row, place) in (top..total).take(rows).enumerate() {
                let ry = ly + row as f32 * 28.0;
                if place < n {
                    let name = &files[place];
                    self.wall_line(place + 1, (x, ry, w), name, tag(name), Lead::Tick(ticked(name)), focus == place + 1);
                } else if place == n {
                    if n > 0 {
                        self.cx.c.rule(mx + 10.0, ry + 4.0, mw - 20.0, op(b.line));
                    }
                    self.cx.c.text(mx + 14.0, ry + 8.0, 20.0, "Slideshows", F::Small, b.tx2);
                } else {
                    let (name, count) = &dirs[place - n - 1];
                    let i = place;
                    let side = match tag(name) {
                        "" if *count == 1 => "1 picture".to_string(),
                        "" => format!("{count} pictures"),
                        tagged => tagged.to_string(),
                    };
                    self.wall_line(i, (x, ry, w), name, &side, Lead::Folder(cur == Some(i)), focus == i);
                }
            }
            if total > rows {
                let th = (slot * rows as f32 / total as f32).max(16.0);
                let ty = ly + (slot - th) * top as f32 / (total - rows) as f32;
                self.cx.c.fill_rr((mx + mw - 6.0, ty + 2.0, 3.0, th - 4.0), 1.5, op(b.tx3));
            }
        }
        ly += slot;
        self.cx.c.rule(mx + 10.0, ly + 4.0, mw - 20.0, op(b.line));
        ly += 9.0;
        self.wall_line(n + m + 1, (x, ly, w), "Open folder", "", Lead::Open, focus == n + m + 1);
        ly += 28.0;
        if foot > 0.0 {
            self.cx.c.para(mx + 14.0, ly + 6.0, mw - 28.0, 16.0, &[(&foot_text, F::Small, b.tx2)]);
        }
        // Inside the menu, between lines; after them, since a click takes the first hit it is in.
        self.cx.hit(Ctl::ScreenItem(usize::MAX), (mx, my, mw, mh), 3);
    }

    /// One line of a Wallpaper menu: what leads it (a check, a picture's tick box, a folder mark,
    /// Open folder's mark with its open mark on the right), the name, and on the right "All
    /// screens" or a folder's picture count.
    fn wall_line(&mut self, i: usize, (x, y, w): (f32, f32, f32), name: &str, side: &str, lead: Lead, focus: bool) {
        let t = self.cx.c.t;
        let b = t.b;
        let ctl = Ctl::ScreenItem(i);
        let on = matches!(lead, Lead::Check(true) | Lead::Folder(true));
        let hot = self.cx.hovered(ctl) || self.cx.hovered(Ctl::WallTick(i)) || (self.w.kb && focus);
        let (fill, ink) = match () {
            _ if on && t.hc => (Some(b.hi), b.on_acc),
            _ if on => (Some(b.track), b.tx),
            _ if hot => (Some(mix(b.card, b.tx, 0.05)), b.tx),
            _ => (None, b.tx),
        };
        if let Some(f) = fill {
            self.cx.c.fill_rr((x, y, w, 28.0), 4.0, op(f));
        }
        if on {
            self.cx.icon(x + 10.0, y, 28.0, I_OK, F::IconSmall, if t.hc { b.on_acc } else { b.acc });
        }
        let mut nx = x + 30.0;
        let side_ink = if on && t.hc { b.on_acc } else { b.tx2 };
        match lead {
            Lead::Tick(ticked) => {
                // Filled with the accent and checked when ticked (design r3).
                let bx = (x + 8.0, y + 6.0, 16.0, 16.0);
                if ticked {
                    // High contrast pairs its check ink with the highlight, as a checked line does.
                    self.cx.c.fill_rr(bx, 3.0, op(if t.hc { b.hi } else { b.acc }));
                    let iw = self.cx.c.width(I_OK, F::IconTiny);
                    self.cx.icon(bx.0 + (16.0 - iw) / 2.0, y, 28.0, I_OK, F::IconTiny, b.on_acc);
                } else {
                    self.cx.c.fill_rr(bx, 3.0, op(b.card));
                    let edge = if self.cx.hovered(Ctl::WallTick(i)) { b.tx } else { b.tx2 };
                    self.cx.c.stroke_rr((bx.0 + 0.5, bx.1 + 0.5, 15.0, 15.0), 3.0, 1.0, op(edge));
                }
                nx = x + 34.0;
                // Before the line's own hit, since a click takes the first hit it is in.
                self.cx.hit(Ctl::WallTick(i), (x, y, 30.0, 28.0), 3);
            }
            Lead::Folder(_) => {
                self.cx.icon(nx, y, 28.0, I_FOLDER, F::IconSmall, side_ink);
                nx += 24.0;
            }
            Lead::Open => {
                self.cx.icon(nx, y, 28.0, I_FOLDER, F::IconSmall, side_ink);
                nx += 24.0;
                let ow = self.cx.c.width(I_OUT, F::IconSmall);
                self.cx.icon(x + w - 10.0 - ow, y, 28.0, I_OUT, F::IconSmall, side_ink);
            }
            Lead::Check(_) => {}
        }
        let sw = if side.is_empty() { 0.0 } else { self.cx.c.width(side, F::Small) + 10.0 };
        let shown = self.cx.c.fit(name, F::Body, x + w - nx - 6.0 - sw);
        self.cx.c.text(nx, y, 28.0, &shown, F::Body, ink);
        if !side.is_empty() {
            self.cx.c.text(x + w - sw, y, 28.0, side, F::Small, side_ink);
        }
        if self.w.kb && focus {
            self.cx.c.stroke_rr((x + 1.0, y + 1.0, w - 2.0, 26.0), 4.0, 2.0, op(b.tx));
        }
        self.cx.hit(ctl, (x, y, w, 28.0), 3);
    }
}

/// The Wallpaper card's chooser lines, the Names card's (spec 0026 AC-13), and the chosen one.
pub(in crate::settings_page) fn wall_chosen(w: &Win) -> (Vec<NameChoice>, usize) {
    let list = name_choices(&w.pics.view);
    let i = w.wall_screen.as_ref().and_then(|id| list.iter().position(|c| c.id.as_ref() == Some(id))).unwrap_or(0);
    (list, i)
}

/// Whether the Wallpaper card shows its screen chooser: with two or more known screens, and
/// with one that has its own pictures, which otherwise no row could reach: every change would
/// go to All screens, which that screen ignores (tried live 2026-11-21).
fn wall_two(w: &Win, s: &Settings) -> bool {
    let (list, _) = wall_chosen(w);
    list.len() > 2 || list.get(1).and_then(|c| c.id.as_ref()).is_some_and(|id| s.wallpaper_screens.get(id).is_some_and(|o| o.iter().any(Option::is_some)))
}

/// The screen the Wallpaper card's rows write for: None for All screens, and while the card
/// has no chooser.
pub(in crate::settings_page) fn wall_screen(w: &Win, s: &Settings) -> Option<String> {
    let (list, i) = wall_chosen(w);
    if wall_two(w, s) { list[i].id.clone() } else { None }
}

/// The row whose Wallpaper menu is open, if one is.
pub(in crate::settings_page) fn chooser_wall(w: &Win) -> Option<usize> {
    match w.chooser {
        Some((Ctl::WallPick(k), ..)) => Some(k),
        _ => None,
    }
}

/// Scrolls an open Wallpaper menu's list so line `line` is in view; `fresh`: from the top.
pub(in crate::settings_page) fn wall_show_line(w: &mut Win, line: usize, fresh: bool) {
    let (n, m) = (w.wall_files.len(), w.wall_dirs.len());
    let total = wall_total(n, m);
    let top = if fresh { 0 } else { w.wall_top };
    w.wall_top = wall_reveal(top, wall_slot(line, n, m), total, wall_rows(w.vh, total));
}

/// A Wallpaper menu's line chosen: Windows wallpaper, a picture or a folder writes the row's
/// key; Open folder opens it. The menu closes first, since a write lays the page out again.
pub(in crate::settings_page) fn choose_wall(k: usize, i: usize) {
    // None: Open folder.
    let Some((pick, screen)) = get(|w| {
        let (n, m) = (w.wall_files.len(), w.wall_dirs.len());
        let pick = match i {
            0 => Some(Pick::None),
            i if i <= n => Some(Pick::Picture(w.wall_files[i - 1].clone())),
            i if i <= n + m => Some(Pick::Folder(w.wall_dirs[i - n - 1].0.clone())),
            _ => None,
        };
        (pick, wall_screen(w, &settings::get()))
    }) else { return };
    let _ = set(|w| {
        w.chooser = None;
        w.focus = Some(Ctl::WallPick(k));
    });
    layout();
    match pick {
        None => open_wallpapers(),
        Some(p) => {
            if let Some(edit) = wall_edit(&settings::get(), screen.as_deref(), k, &p) {
                commit(edit, &format!("Workspace {} wallpaper", k + 1));
            }
        }
    }
}

/// A picture's tick box in an open Wallpaper menu: adds it to the row's set or takes it out,
/// and the menu stays open for more (spec 0028 AC-11).
pub(in crate::settings_page) fn tick_wall(i: usize) {
    let Some(Some((k, file, screen))) = get(|w| Some((chooser_wall(w)?, w.wall_files.get(i.checked_sub(1)?)?.clone(), wall_screen(w, &settings::get())))) else { return };
    let _ = set(|w| {
        if let Some((from, at, _)) = w.chooser {
            w.chooser = Some((from, at, i));
        }
    });
    if let Some(edit) = wall_tick(&settings::get(), screen.as_deref(), k, &file) {
        commit(edit, &format!("Workspace {} wallpaper", k + 1));
    } else {
        layout();
    }
}

/// Open folder (AC-14): the wallpapers folder in explorer, running as you, made first if it is
/// missing. `create_dir` fails rather than follows anything already at that name.
fn open_wallpapers() {
    let dir = crate::ws::data_dir().join("wallpapers");
    if !dir.is_dir()
        && let Err(e) = std::fs::create_dir(&dir)
    {
        warn!("settings page: the wallpapers folder was not made ({e})");
    }
    crate::tray::open(&dir);
}

/// The wheel over an open Wallpaper menu moves its pictures, three a notch.
pub(in crate::settings_page) fn wall_wheel(delta: f32) {
    let moved = set(|w| {
        let n = wall_total(w.wall_files.len(), w.wall_dirs.len());
        let rows = wall_rows(w.vh, n);
        let notches = (-delta / 120.0 * 3.0).round() as isize;
        let top = (w.wall_top as isize + notches).clamp(0, n.saturating_sub(rows) as isize) as usize;
        std::mem::replace(&mut w.wall_top, top) != top
    });
    if moved == Some(true) {
        layout();
    }
}

/// Main loop's tick: asks for the Wallpaper card's previews not decoded yet, and paints once
/// they are (AC-13). Nothing is asked again once each was tried.
pub(in crate::settings_page) fn wall_previews() {
    let s = settings::get();
    let Some((want, waiting)) = get(|w| (wall_wanted(w, &s), w.wall_waiting)) else { return };
    let missing = want.as_ref().is_some_and(|(r, size, files)| crate::wallpaper::cached(*r, *size).is_none() || files.iter().any(|f| !crate::wallpaper::tried_ws(*r, f, *size, s.wallpaper_fit)));
    if missing && let Some(want) = want {
        crate::wallpaper::check(vec![want], s.wallpaper_fit);
    }
    let _ = set(|w| w.wall_waiting = missing);
    if waiting {
        layout();
    }
}

#[cfg(test)]
mod tests;
