// The settings page (spec 0012, in look B by spec 0017's 0017-settings-page.md): every setting in
// settings.toml, changed by clicking or from the keyboard, saved at once and in effect at once.
// Drawn to the approved design (pdesk-design restart/settings-r3.html, with the Style row and
// preview of the indicator approval and the motion rows of settings-r7.html): Windows' title
// bar, a side list, and one scrolling column of cards. One drawn window with its own focus list
// and no child controls, so everything scrolls together; the gesture picker's menu is drawn in
// it too. Lives on the main thread: its keys reach wndproc through main's loop. Sizes are in
// dip, scaled to the screen's dpi when drawn. The page never keeps its own copy of a value: it
// draws what settings::get() holds, and every change goes through settings::edit::write.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{Gdi::*, GdiPlus::DashStyleDash},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::*,
        Controls::{EM_SETLIMITTEXT, EM_SETMARGINS, EM_SETSEL, WM_MOUSELEAVE},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::controls::{self, Screen, Style, Tone, widgets::Cards};
use crate::settings::{self, Action, Combo, Motion, Owner, Pick, Settings, edit::Edit, parse::Problem};
use crate::setup::{self, GaveBack, Row as PadRow, State as PadState, Wrote};
use crate::ui::{self, Theme, canvas::{Bx, Canvas, buffered}, color::{mix, op}, dark_title, fonts::{F, Fonts}, theme};
use crate::uia::{self, Node, Tree};

mod cards;
mod choosers;
mod edit_box;
mod input;
pub(crate) mod menu;
mod picker;
pub(crate) mod recording;
pub(crate) mod rows;

use cards::attention::WORDS;
use cards::colors::Hex;
use cards::names::Nm;
use cards::place::SPOT;
use cards::wallpaper::{wall_count, wall_previews};
use cards::{NoteSpec, changed};
use edit_box::{make_box, place_box};
use input::wndproc;
use menu::Menu;
use picker::draw::{Fit, pick_fit};
use picker::{ColorPick, Ease, Geo, How, must_close, pick_close, windows_words};
use recording::{Prompt, Rec};
use rows::{Card, SECTIONS, Section, file_value, find_row, home, title};

const CLASS: PCWSTR = w!("des-k-settings");
// Clips its one child, the text box the popup words and the name boxes share (specs 0022, 0023),
// so a paint never covers it.
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_THICKFRAME.0 | WS_CLIPCHILDREN.0);
const WHO: &str = "des-k settings";

// The client area and its parts, in dip (AC-SET-1): 1040 × 680 with Windows' title bar, never
// narrower, as tall as a 1366 × 768 screen at 125% allows.
const W: f32 = 1040.0;
const H: f32 = 648.0;
const MIN_H: f32 = 528.0;
const NAV: f32 = 240.0; // the side list
const X0: f32 = 248.0; // the column's left edge
const PAD_R: f32 = 32.0;
const COL_MAX: f32 = 1000.0; // the column stops growing here
const ROW: f32 = 52.0; // a row's least height

// Segoe Fluent Icons.
const I_NOTES: &str = "\u{E7BA}";
const I_WARN: &str = "\u{E7BA}";
const I_INFO: &str = "\u{E946}";
const I_OK: &str = "\u{E73E}";
const I_KEY: &str = "\u{E765}";
const I_ADD: &str = "\u{E710}";
const I_X: &str = "\u{E711}";
const I_OUT: &str = "\u{E8A7}";
const I_CHEV: &str = "\u{E70D}";
const I_MORE: &str = "\u{E76C}";
const I_SETUP: &str = "\u{E713}";
const I_APPS: &str = "\u{E71D}";
const I_APP: &str = "\u{E737}";
const I_SCREEN: &str = "\u{E7F4}";
const I_PIC: &str = "\u{EB9F}";
const I_FOLDER: &str = "\u{E8B7}";

pub(crate) static DEFAULTS: LazyLock<Settings> = LazyLock::new(Settings::default);

use settings::{CTRL, SHIFT, WIN};

/// Where a caption link goes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Link {
    Skip, // the Skip animations switch
    Indicator,
    Overview,
    Workspaces,
}

/// What a touchpad note's button does.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pad {
    Windows,  // Set up in, or Open, Windows Settings: setup::use_gestures (AC-SET-6)
    ReadOn,   // Turn reading on
    GiveBack, // Give back, or Try again
}

/// Something you can click, focus from the keyboard, or both.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Side(Option<Section>), // None: Notes
    Welcome,
    WhatsNew, // spec 0029 AC-8
    Open,     // Open settings.toml
    Toggle(&'static str),
    Minus(&'static str),
    Plus(&'static str),
    Stepper(&'static str), // focus only: a number's − and + are one stop, Left and Right step
    Choice(&'static str, usize),
    Pick(&'static str), // focus only: a choice is one stop, Left and Right move
    Reset(&'static str),
    Link(Link),
    Tiles, // focus only: the workspace tiles, one stop for every screen
    // A plugged in screen's tile: its row in Pics::screens, its workspace. A click opens the color
    // picker (spec 0021); pointing at the last one shows its ×.
    WsTile(usize, usize),
    TileAdd,
    TileRemove,
    Forget(usize), // a screen not plugged in, by its place in View::waiting
    KeyRow(&'static str), // focus only: a shortcut row is one stop
    Key(&'static str, usize), // a key field, by its index in the file: records one in its place
    KeyX(&'static str, usize),
    AddKey(&'static str), // + or Set key
    WinSide(&'static str, bool), // des-k (true) or Windows
    StopRec(&'static str),
    Move(&'static str),
    Cancel(&'static str),
    Picker(usize), // a gesture row's picker, by GESTURE_ROWS index
    Item(usize),   // a line of the open menu
    Show,          // the indicator preview's Show: the real indicator on this screen
    Chart,         // the Motion chart: never clicked or focused, one image for Narrator
    Pad(Pad),
    Dismiss,     // the save error banner
    DismissEdit, // the hand edit banner
    Hex(&'static str), // a color's hex box, by its key: ACCENT or one of WS_KEYS (spec 0020)
    Words,             // the attention text box (spec 0022)
    Screens,           // Where it shows' screen chooser: opens its menu
    ScreenItem(usize), // a line of the open screen menu
    SameAll,           // Same as all screens: takes out the chosen screen's table
    NameScreens,       // the Names card's screen chooser (spec 0023)
    Name(Nm),          // a name box
    NameSame(usize),   // a workspace row's Same as all screens: takes out the screen's key
    WallScreens,       // the Wallpaper card's screen chooser (spec 0026)
    WallPick(usize),   // a workspace row's picture menu
    WallSame(usize),   // a workspace row's Same as all screens: takes out the screen's key
    WallTick(usize),   // a picture's tick box in an open Wallpaper menu, by its line (spec 0028)
    WallUse(usize),    // an All screens row's Use this there too: takes out the plugged in screens' keys
    // The Apps section (spec 0027): Add, and a rule row's two menus and remove, by its place in
    // Settings::apps.
    AppAdd,
    AppWs(usize),
    AppScreen(usize),
    AppX(usize),
    Picture,           // focus only: the place picture is one stop; Left and Right move
    Spot(usize),       // a third of the picture across and down, 0 to 8 in reading order
    ShowThere,         // Show it there: the real indicator on the chosen screen
    // The color picker (spec 0021): a live tile of the Overview picture (its screen row, its
    // workspace), a Colors swatch by key, and the open picker's own parts.
    ShowTile(usize, usize),
    Swatch(&'static str),
    Preset(usize),
    Ring,
    Square,
    PickDone,
    PickCancel,
    PickCard, // inside the picker, between its parts
}

impl Ctl {
    fn key(self) -> Option<&'static str> {
        match self {
            Ctl::Toggle(k) | Ctl::Minus(k) | Ctl::Plus(k) | Ctl::Stepper(k) | Ctl::Choice(k, _) | Ctl::Pick(k) | Ctl::Reset(k) => Some(k),
            Ctl::KeyRow(k) | Ctl::Key(k, _) | Ctl::KeyX(k, _) | Ctl::AddKey(k) | Ctl::WinSide(k, _) | Ctl::StopRec(k) | Ctl::Move(k) | Ctl::Cancel(k) => Some(k),
            Ctl::Hex(k) => Some(k),
            Ctl::Words => Some(WORDS),
            Ctl::Picture | Ctl::Spot(_) => Some(SPOT),
            _ => None,
        }
    }

    /// Changes the file (or starts recording a change); a read only page still lets you focus
    /// it and follow links.
    fn writes(self) -> bool {
        matches!(
            self,
            Ctl::Toggle(_) | Ctl::Minus(_) | Ctl::Plus(_) | Ctl::Choice(..) | Ctl::Reset(_) | Ctl::WsTile(..) | Ctl::TileAdd | Ctl::TileRemove | Ctl::Forget(_)
                | Ctl::KeyRow(_) | Ctl::Key(..) | Ctl::KeyX(..) | Ctl::AddKey(_) | Ctl::WinSide(..) | Ctl::Move(_) | Ctl::Picker(_) | Ctl::Item(_) | Ctl::Pad(Pad::ReadOn | Pad::GiveBack)
                | Ctl::Hex(_) | Ctl::SameAll | Ctl::Picture | Ctl::Spot(_) | Ctl::ShowTile(..) | Ctl::Swatch(_) | Ctl::Words | Ctl::Name(_) | Ctl::NameSame(_)
                | Ctl::AppAdd | Ctl::AppWs(_) | Ctl::AppScreen(_) | Ctl::AppX(_) | Ctl::WallPick(_) | Ctl::WallSame(_) | Ctl::WallTick(_) | Ctl::WallUse(_)
        )
    }

    /// The keyboard stop this control belongs to: one per row (spec 0012 AC-16).
    fn stop(self) -> Ctl {
        match self {
            Ctl::Minus(k) | Ctl::Plus(k) => Ctl::Stepper(k),
            Ctl::Choice(k, _) => Ctl::Pick(k),
            Ctl::WsTile(..) | Ctl::TileAdd | Ctl::TileRemove => Ctl::Tiles,
            Ctl::Spot(_) => Ctl::Picture,
            Ctl::Key(k, _) | Ctl::KeyX(k, _) | Ctl::AddKey(k) | Ctl::WinSide(k, _) => Ctl::KeyRow(k),
            c => c,
        }
    }

    /// A stepper's keyboard stop, then its − and +.
    fn steps(key: &'static str) -> (Option<Ctl>, Ctl, Ctl) {
        (Some(Ctl::Stepper(key)), Ctl::Minus(key), Ctl::Plus(key))
    }

    fn in_side(self) -> bool {
        matches!(self, Ctl::Side(_) | Ctl::Welcome | Ctl::WhatsNew | Ctl::Open)
    }
}

impl controls::Id for Ctl {
    fn writes(self) -> bool {
        Ctl::writes(self)
    }
}

/// Where a control sits, in window dip. Group: 0 the side list, 1 the column (clipped under the
/// banners), 2 the banners, 3 the open menu.
type Hit = controls::Hit<Ctl, u8>;

/// What the tiles, the preview and the touchpad draw, copied out of ws and Windows.
#[derive(Clone, PartialEq, Default)]
struct Pics {
    screens: Vec<crate::ws::views::ScreenInfo>,
    view: crate::ws::views::View,
    pad: Option<PadState>,
    back: Option<GaveBack>,
    here: Option<String>, // the device the page is on, for the indicator preview
    apps: Vec<crate::apps::Seen>, // apps with a window open now (spec 0027 AC-13)
    wall_failed: bool, // des-k can't draw behind the desktop icons (spec 0026 AC-12)
    wall_problems: Vec<(String, crate::wallpaper::Problem)>, // values in settings.toml that can't show, by key (AC-8)
    wall_now: BTreeMap<(String, usize), String>, // each slideshow's picture now, by screen id and workspace (spec 0028)
    wall_counts: Vec<(String, usize)>, // the folders settings.toml names, with their picture counts
}

/// The pictures' data now, or None while ws is busy (the next tick tries again).
fn pictures(h: Option<HWND>) -> Option<Pics> {
    let n = settings::get().workspaces;
    let view = crate::ws::views::view(&[n.saturating_sub(1).max(2), DEFAULTS.workspaces])?;
    let screens = crate::ws::views::screen_info();
    let here = h.and_then(|h| {
        let mon = unsafe { MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST) };
        let mut mi = MONITORINFOEXW { monitorInfo: MONITORINFO { cbSize: size_of::<MONITORINFOEXW>() as u32, ..Default::default() }, ..Default::default() };
        unsafe { GetMonitorInfoW(mon, &mut mi.monitorInfo).as_bool() }.then(|| String::from_utf16_lossy(&mi.szDevice).trim_end_matches('\0').trim_start_matches(r"\\.\").to_string())
    });
    let apps = crate::apps::seen(&view.open);
    let s = settings::get();
    let mut files: Vec<String> = s.wallpaper.iter().chain(s.wallpaper_screens.values().flat_map(|a| a.iter().flatten())).map(Pick::key).filter(|f| !f.is_empty()).collect();
    files.sort();
    files.dedup();
    let wall_problems = files.into_iter().filter_map(|f| Some((f.clone(), crate::wallpaper::problem(&f)?))).collect();
    // Only folders a value names are read, for their rows' captions (spec 0028 AC-13).
    let mut wall_counts: Vec<(String, usize)> = s
        .wallpaper
        .iter()
        .chain(s.wallpaper_screens.values().flat_map(|a| a.iter().flatten()))
        .filter_map(|p| match p {
            Pick::Folder(f) => Some((f.clone(), wall_count(f))),
            _ => None,
        })
        .collect();
    wall_counts.sort();
    wall_counts.dedup();
    let wall_now = crate::backdrop::current_all();
    Some(Pics { screens, view, pad: Some(setup::state()), back: crate::welcome::gave_back(), here, apps, wall_failed: crate::backdrop::failed(), wall_problems, wall_now, wall_counts })
}

/// Takes a new copy of the pictures' data; lays the page out again only if it changed.
fn update_pictures(always: bool) {
    let changed = pictures(hwnd()).and_then(|new| {
        set(|w| {
            let changed = w.pics != new;
            w.pics = new;
            changed
        })
    });
    if changed == Some(true) || always {
        layout();
    }
}

/// Main loop, every second and on a display change: the tiles follow your windows while the
/// page is open and not minimized.
pub fn tick() {
    if let Some(h) = hwnd()
        && !unsafe { IsIconic(h) }.as_bool()
    {
        update_pictures(false);
        wall_previews();
    }
}

/// A hand edit was put in effect: an open page shows what changed until Dismiss or the next
/// one. Changes made on the page show none.
pub fn hand_edit(changes: &[settings::store::Change], _moves: &[String]) {
    if hwnd().is_none() {
        return;
    }
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    let lines: Vec<String> = changes.iter().map(|c| changed(c.key, &c.was, &c.now)).collect();
    let note = Some((format!("{:02}:{:02}", t.wHour, t.wMinute), lines));
    // While the color picker is open the page's notes hold still; it shows this one on close
    // (spec 0021 AC-13). The reload calls this before refresh, so a hand edit that also closes
    // the picker is held already.
    let _ = set(|w| match w.pick.as_mut() {
        Some(p) => p.held_edit = note,
        None => w.edited = note,
    });
}

struct Win {
    hwnd: HWND,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    scroll: f32,
    height: f32,  // the column's full height
    fixed: f32,   // the banners' height above it
    tops: Vec<(Option<Section>, f32)>, // where each section starts in the column; None: Notes
    cards: Vec<(Card, f32)>,           // and each card What's new opens at (spec 0029 AC-4)
    hits: Vec<Hit>,
    focus: Option<Ctl>,
    kb: bool, // the focus ring shows only after a key, as in Windows
    hover: Option<Ctl>,
    press: Option<Ctl>,
    drag: Option<f32>, // dragging the scroll thumb: where in the thumb it was grabbed
    vw: f32,           // the client area, in dip
    vh: f32,
    error: Option<(String, String)>, // the last save that failed: what, and Windows' words
    rec: Option<Rec>,
    refused: Option<(String, String)>, // the last key the recorder refused: title and line
    prompt: Option<Prompt>,
    menu: Option<Menu>,
    pics: Pics,
    edited: Option<(String, Vec<String>)>, // the last hand edit's banner: its time and lines
    tree: Tree,
    hexes: Vec<Hex>,              // hex boxes holding digits not saved yet (spec 0020)
    own: Option<u32>,             // the last own accent while the page is open (AC-12)
    screen: Option<String>,       // the screen Where it shows changes, by id; None: All screens
    chooser: Option<(Ctl, Bx, usize)>, // the open screen menu: the chooser it opened from, where, its focused line
    dragging: Option<(f64, f64)>, // Custom: where a drag on the picture has the card, in percent
    pick: Option<ColorPick>,      // the open color picker (spec 0021)
    geo: Geo,                     // where its anchors were last drawn
    ease: Option<Ease>,           // the page scrolling on its own, for the picker
    down_at: (i32, i32),          // the last press, in screen pixels, for the picker's guard
    words: Option<String>,        // the popup words as typed, before they save (spec 0022)
    names_screen: Option<String>, // the screen the Names card shows, by id; None: All screens (spec 0023)
    typed: Option<String>,        // the name box with focus, as typed
    unsaved: Vec<(Option<String>, Nm, String)>, // name boxes whose save failed: screen, row, your text (AC-13)
    wall_screen: Option<String>, // the screen the Wallpaper card shows, by id; None: All screens (spec 0026)
    wall_files: Vec<String>,     // the wallpapers folder's pictures, read as a row's menu opened
    wall_dirs: Vec<(String, usize)>, // its slideshow folders and their picture counts (spec 0028)
    wall_top: usize,             // the first picture that menu shows when they don't all fit
    wall_waiting: bool,          // a preview was still being decoded at the last tick
}

thread_local! {
    static PAGE: RefCell<Option<Win>> = const { RefCell::new(None) };
}

// Never hold the borrow across a call that comes back here: settings::edit::write reloads, and the
// reload calls refresh().
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&PAGE, "settings page get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&PAGE, "settings page set", f)
}

/// What one pass of compose found.
struct Out {
    hits: Vec<Hit>,
    height: f32,
    fixed: f32,
    tops: Vec<(Option<Section>, f32)>,
    cards: Vec<(Card, f32)>, // where each card What's new opens at starts, like tops
    said: Vec<(Ctl, Node)>, // what Narrator reads for each stop, in page order
    geo: Geo,
}

/// A piece of a caption (`controls::P`) over the page's controls.
type P = controls::P<Ctl>;

fn t(s: impl Into<String>) -> P {
    P::T(s.into())
}

/// A stepper's value as its number and its unit words: minutes in words, 0 % as Off.
fn step_words(v: i64, unit: &str) -> (String, String) {
    match unit {
        rows::MINUTES => (rows::minutes(v), String::new()),
        "%" if v == 0 => ("Off".to_string(), String::new()),
        "" | "%" => (v.to_string(), unit.to_string()),
        u => (v.to_string(), format!(" {u}")),
    }
}

/// One pass over the page, drawing or only measuring: the shared context, and the page's own.
struct Ctx<'a, 'f> {
    cx: controls::Ctx<'a, 'f, Ctl, u8>,
    w: &'a Win,
    s: &'a Settings, // from the paint's own handle, one version for the whole pass
    cw: f32,  // the column's width
    top0: f32, // where the column starts in the window, after scrolling
    out: Out,
}

impl<'a, 'f> Screen<'a, 'f> for Ctx<'a, 'f> {
    type C = Ctl;
    type E = u8;

    fn cx(&mut self) -> &mut controls::Ctx<'a, 'f, Ctl, u8> {
        &mut self.cx
    }

    fn column(&self) -> (f32, f32) {
        (X0, self.cw)
    }

    /// Lays `f` out without drawing it or keeping its controls, for a box that needs its
    /// height before its contents.
    fn measured(&mut self, f: impl FnOnce(&mut Self) -> f32) -> f32 {
        let (tops, cards, geo) = (self.out.tops.len(), self.out.cards.len(), self.out.geo.clone());
        let m = self.cx.mark();
        let h = f(self);
        self.cx.rewind(m);
        self.out.tops.truncate(tops);
        self.out.cards.truncate(cards);
        self.out.geo = geo;
        h
    }
}

impl Ctx<'_, '_> {
    /// A click on a tile or swatch opens the color picker: not in high contrast or read only.
    fn picks(&self) -> bool {
        !self.cx.c.t.hc && !self.cx.ro
    }
}

/// The side list item the view is in, for a scroll, column height, the visible height (`view`)
/// and section tops.
fn current(scroll: f32, height: f32, view: f32, tops: &[(Option<Section>, f32)]) -> Option<Option<Section>> {
    if scroll >= height - view - 1.0 {
        return tops.last().map(|(s, _)| *s);
    }
    let pos = scroll + 60.0;
    tops.iter().take_while(|(_, top)| *top <= pos).last().or(tops.first()).map(|(s, _)| *s)
}

/// The scroll thumb's top and height, when the column is taller than the view.
fn thumb(scroll: f32, height: f32, view: f32, top: f32) -> Option<(f32, f32)> {
    (height > view + 12.0).then(|| {
        let h = ((view - 8.0) * view / height).max(24.0);
        (top + 4.0 + (view - 8.0 - h) * scroll / (height - view).max(1.0), h)
    })
}

/// The whole page, back to front: the column, then the banners over its top, the side list,
/// the scroll thumb and the open menu.
fn compose(c: &mut Canvas, w: &Win) -> Out {
    let s = settings::get();
    let cw = (w.vw - X0 - PAD_R).min(COL_MAX);
    let cx = controls::Ctx { c, hits: vec![], said: vec![], ro: settings::store::parse_error().is_some(), focus: w.focus, hover: w.hover, kb: w.kb, group: 1, style: Style::Settings };
    let mut x = Ctx { cx, w, s: &s, cw, top0: 0.0, out: Out { hits: vec![], height: 0.0, fixed: 0.0, tops: vec![], cards: vec![], said: vec![], geo: Geo::default() } };
    let fixed = x.measured(|c| c.banners());
    x.out.fixed = fixed;
    let top0 = fixed - w.scroll;
    x.top0 = top0;
    let b = x.cx.c.t.b;
    let mut y = top0 + 9.0;
    x.cx.c.text(X0, y, 32.0, "Settings", F::Heading, b.tx);
    y += 36.0;
    y += x.cx.c.para(X0, y, cw, 20.0, &[("Every change saves to settings.toml and works at once. Restore waits for des-k's next start.", F::Body, b.tx2)]);
    let problems = settings::store::problems();
    let notes: Vec<&Problem> = problems.iter().filter(|p| home(&p.key).is_none()).collect();
    if !notes.is_empty() {
        y += 32.0;
        y += x.heading(None, y);
        for p in notes {
            let n = NoteSpec { tone: Tone::Warn, icon: I_WARN, title: title(p, &s), lines: vec![P::Mono(p.message.clone())], buttons: vec![(Ctl::Open, "Open settings.toml")], group: 1 };
            y += x.cx.note(X0, y, cw, &n) + 8.0;
        }
    }
    for sec in SECTIONS {
        y += 32.0;
        y += x.section(sec, y);
    }
    x.out.height = y + 32.0 - top0;
    // The banners cover the column scrolled under them; a line marks a cut edge.
    x.banners();
    let view = w.vh - fixed;
    if w.scroll > 0.5 {
        x.cx.c.rule(NAV - 1.0, fixed, w.vw - NAV + 1.0, op(b.line));
    }
    if w.scroll < x.out.height - view - 1.0 {
        x.cx.c.rule(NAV - 1.0, w.vh - 1.0, w.vw - NAV + 1.0, op(b.line));
    }
    x.side();
    if let Some((ty, th)) = thumb(w.scroll, x.out.height, view, fixed) {
        x.cx.c.fill_rr((w.vw - 6.0, ty, 2.0, th), 1.0, op(if w.drag.is_some() { b.tx2 } else { b.tx3 }));
    }
    x.menu();
    x.screens_menu();
    x.pick_card();
    // The menu's hits first, then the banners', the side list's, the column's.
    x.cx.hits.sort_by_key(|h| std::cmp::Reverse(h.group));
    Out { hits: x.cx.hits, said: x.cx.said, ..x.out }
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

/// Whether the page is the window recording: main hands it WM_RECORDED.
pub fn owns(h: HWND) -> bool {
    hwnd() == Some(h)
}

/// The theme the page paints with: the color picker's preview while it shows a color the
/// file doesn't hold (spec 0021 AC-5).
fn painted(w: &Win) -> Theme {
    w.pick.as_ref().and_then(|p| p.preview).unwrap_or(w.theme)
}

fn repaint() {
    if let Some(h) = hwnd() {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// Lays the page out again for its scroll, keeps the focus on a stop that is still there, and
/// gives Narrator the stops as painted. An open color picker keeps its place while it still
/// fits, is placed again (never scrolling) when it doesn't, and closes when what you clicked is
/// gone (spec 0021 AC-14).
fn layout() {
    lay();
    match pick_fit() {
        Some(Fit::Placed) => lay(),
        Some(Fit::Gone) => return pick_close(How::Quiet),
        _ => {}
    }
    place_box();
    repaint();
}

fn lay() {
    for _ in 0..2 {
        let Some(out) = get(|w| unsafe {
            let dc = GetDC(Some(w.hwnd));
            let out = {
                let mut c = Canvas::new(dc, false, w.scale, painted(w), &w.fonts);
                compose(&mut c, w)
            };
            ReleaseDC(Some(w.hwnd), dc);
            out
        }) else {
            return;
        };
        let moved = set(|w| {
            let max = (out.height - (w.vh - out.fixed)).max(0.0);
            let scroll = w.scroll.clamp(0.0, max);
            let moved = scroll != w.scroll;
            w.scroll = scroll;
            w.height = out.height;
            w.fixed = out.fixed;
            w.tops = out.tops;
            w.cards = out.cards;
            w.hits = out.hits;
            w.geo = out.geo;
            if let Some(f) = w.focus
                && !w.hits.iter().any(|h| h.ctl.stop() == f)
            {
                // A Reset or Same as all screens that just took itself away: the focus moves to
                // its row's control.
                w.focus = match f {
                    Ctl::NameSame(k) => Some(Ctl::Name(Nm::Ws(k))).filter(|c| w.hits.iter().any(|h| h.ctl == *c)),
                    Ctl::WallUse(k) => Some(Ctl::WallPick(k)).filter(|c| w.hits.iter().any(|h| h.ctl == *c)),
                    _ => f.key().and_then(|k| w.hits.iter().find(|h| h.ctl.key() == Some(k) && h.ctl.stop() != Ctl::Reset(k)).map(|h| h.ctl.stop())),
                };
            }
            let s = w.scale;
            let px = |b: Bx| RECT { left: (b.0 * s) as i32, top: (b.1 * s) as i32, right: ((b.0 + b.2) * s) as i32, bottom: ((b.1 + b.3) * s) as i32 };
            let kids: Vec<Node> = out
                .said
                .into_iter()
                .map(|(ctl, mut n)| {
                    let bx = w.hits.iter().filter(|h| h.ctl.stop() == ctl).map(|h| h.at).reduce(|a, b| {
                        let (l, t) = (a.0.min(b.0), a.1.min(b.1));
                        (l, t, (a.0 + a.2).max(b.0 + b.2) - l, (a.1 + a.3).max(b.1 + b.3) - t)
                    });
                    n.rect = bx.map(px).unwrap_or(n.rect);
                    n.focused = w.focus == Some(ctl) && n.focusable;
                    n
                })
                .collect();
            let root = Node { role: UIA_PaneControlTypeId, name: WHO.into(), rect: px((0.0, 0.0, w.vw, w.vh)), kids, ..Node::default() };
            if let Ok(mut t) = w.tree.lock() {
                *t = root;
            }
            moved
        });
        if moved != Some(true) {
            break;
        }
    }
}

/// Shows what des-k read back from the file. Called after every reload, so the color picker's
/// close rules that follow a reload live here (spec 0021 AC-10, AC-12).
pub fn refresh() {
    if hwnd().is_some() {
        if get(must_close) == Some(true) {
            pick_close(How::Quiet);
        }
        update_pictures(true);
    }
}

fn scroll_to(y: f32) {
    let _ = set(|w| w.scroll = y);
    layout();
}

fn scroll_by(dy: f32) {
    if let Some(y) = get(|w| w.scroll + dy) {
        scroll_to(y);
    }
}

/// Scrolls the focused stop into view.
fn reveal() {
    let dy = get(|w| {
        let h = w.hits.iter().filter(|h| Some(h.ctl.stop()) == w.focus && h.group == 1).map(|h| h.at).next()?;
        let (top, bottom) = (h.1 - 12.0, h.1 + h.3 + 12.0);
        if top < w.fixed {
            Some(top - w.fixed)
        } else if bottom > w.vh {
            Some(bottom - w.vh)
        } else {
            None
        }
    })
    .flatten();
    match dy {
        Some(dy) => scroll_by(dy),
        None => layout(), // the focus can show the removal note
    }
    if let Some((h, tree)) = get(|w| (w.hwnd, w.tree.clone())) {
        uia::focus_moved(h, &tree, WHO);
    }
}

fn jump(sec: Option<Section>) {
    if let Some(top) = get(|w| w.tops.iter().find(|(s, _)| *s == sec).map(|(_, t)| *t)).flatten() {
        scroll_to(top - 8.0);
    }
}

/// Scrolls so `card` sits at the top, as `jump` does for its section.
fn jump_card(card: Card) {
    if let Some(top) = get(|w| w.cards.iter().find(|(c, _)| *c == card).map(|(_, t)| *t)).flatten() {
        scroll_to(top - 8.0);
    }
}

/// The page's own writers change its notes: never while the color picker is open, which takes
/// every click and key and holds the notes still (spec 0021 AC-13).
fn assert_no_pick() {
    debug_assert!(get(|w| w.pick.is_none()) != Some(false), "the page wrote while the color picker was open");
}

/// Writes one value; a failure shows what stays and why until the next good save.
fn save(key: &'static str, v: i64) {
    assert_no_pick();
    let Some(row) = find_row(key) else { return };
    match settings::edit::write(Edit::Value(key, file_value(row, v))) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
        }
        Err(why) => failed(row.title, why),
    }
}

/// A failed write's note, with Windows' words.
fn failed(what: &str, why: String) {
    assert_no_pick();
    let said = windows_words(&why);
    let _ = set(|w| w.error = Some((what.to_string(), said)));
    refresh();
}

/// Writes an edit that is not one row's value; a failure shows like a failed save.
fn commit(edit: Edit, title: &str) {
    let _ = wrote(edit, title);
}

/// `commit`, saying whether it saved.
fn wrote(edit: Edit, title: &str) -> bool {
    assert_no_pick();
    match settings::edit::write(edit) {
        Ok(()) => {
            let _ = set(|w| w.error = None);
            refresh();
            true
        }
        Err(why) => {
            failed(title, why);
            false
        }
    }
}

/// Opens the page on the screen under the mouse, or brings the open one to the front (AC-SET-1).
/// `section`: scroll to it.
pub fn open(section: Option<Section>) {
    open_at(section, None);
}

/// `open`, scrolled to `card` inside `section` when given (What's new's buttons, spec 0029 AC-4).
pub fn open_at(section: Option<Section>, card: Option<Card>) {
    let go = || match card {
        Some(c) => jump_card(c),
        None if section.is_some() => jump(section),
        None => {}
    };
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        go();
        return;
    }
    ui::canvas::gdiplus();
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when the page opens a second time
        let (mon, _) = crate::ws::switch::under_mouse();
        let (work, _) = work_area(mon);
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("des-k settings"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return warn!("settings page: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let scale = GetDpiForWindow(h) as f32 / 96.0;
        let t = theme();
        dark_title(h, &t);
        crate::ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        PAGE.set(Some(Win {
            hwnd: h,
            scale,
            theme: t,
            fonts: Fonts::new(scale),
            scroll: 0.0,
            height: 0.0,
            fixed: 0.0,
            tops: vec![],
            cards: vec![],
            hits: vec![],
            focus: None,
            kb: false,
            hover: None,
            press: None,
            drag: None,
            vw: W,
            vh: H,
            error: None,
            rec: None,
            refused: None,
            prompt: None,
            menu: None,
            pics: Pics::default(),
            edited: None,
            tree: Arc::new(Mutex::new(Node::default())),
            hexes: vec![],
            own: None,
            screen: None,
            chooser: None,
            dragging: None,
            pick: None,
            geo: Geo::default(),
            ease: None,
            down_at: (0, 0),
            words: None,
            names_screen: None,
            typed: None,
            unsaved: vec![],
            wall_screen: None,
            wall_files: vec![],
            wall_dirs: vec![],
            wall_top: 0,
            wall_waiting: false,
        }));
        make_box(h, inst.into());
        // Shorter than H when the screen is (1366 × 768 at 125%); WM_SIZE sets vw and vh.
        let (ww, wh) = size(h, W, H);
        let wh = wh.min(work.bottom - work.top);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        update_pictures(true);
        go();
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
    }
}

/// The window's outer size for a client area `cw` × `ch` dip, at its dpi.
fn size(h: HWND, cw: f32, ch: f32) -> (i32, i32) {
    unsafe {
        let dpi = GetDpiForWindow(h);
        let s = dpi as f32 / 96.0;
        let mut r = RECT { left: 0, top: 0, right: (cw * s).round() as i32, bottom: (ch * s).round() as i32 };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        (r.right - r.left, r.bottom - r.top)
    }
}

/// The work area of the screen a window or rect is on, and that screen's whole rect.
fn work_area(mon: HMONITOR) -> (RECT, RECT) {
    let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = unsafe { GetMonitorInfoW(mon, &mut mi) };
    (mi.rcWork, mi.rcMonitor)
}

#[cfg(test)]
pub(crate) mod tests;
