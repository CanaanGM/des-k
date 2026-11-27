// The first run (spec 0017, 0017-first-run.md; replaces spec 0011's welcome screen where they
// differ): five steps in one window, Try it, Shortcuts, Behavior, Touchpad and Done, where every
// setting it shows changes on the spot through settings::edit::write and is in effect at once. Drawn
// to the approved design (pdesk-design restart/first-run-r11.html, with the More rows of
// first-run-r15.html): Windows' title bar, a thin progress line, the step, and a footer band.
// One drawn window with its own focus list and no child controls; the gesture picker's menu is
// drawn in it too. Tab visits every control, so a stepper's − and + are two stops. Lives on the
// main thread: its keys reach wndproc through main's loop. Sizes are in dip, scaled to the
// screen's dpi when drawn. It keeps no copy of a value: it draws what settings::get() holds.
use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{Gdi::*, GdiPlus::DashStyleDash},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Accessibility::*,
        Controls::WM_MOUSELEAVE,
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::controls::{self, Screen, Style, Tone, record, widgets::Cards};
use crate::settings::{self, Action, CTRL, Combo, Motion, Owner, Settings, WIN, edit::Edit};
use crate::settings_page::{self as sp, menu::{Does, Menu}, recording::{Plan, Prompt}, rows::{Kind, WinKey}};
use crate::setup::{self, GaveBack, Row as PadRow, State as PadState, Wrote};
use crate::ui::{self, Theme, canvas::{Bx, Canvas, buffered}, color::{mix, op}, dark_title, fonts::{F, Fonts}, theme};
use crate::uia::{self, Node, Tree};

mod draw;
mod pad;
mod steps;

use draw::{compose, words_of};
use steps::{Field, Rec, Step, TAKES, commit, field_of, field_of_key, go, grow_tick, next_step, plan_pair, plan_side, rec_for, run, save, title_of};

const CLASS: PCWSTR = w!("des-k-welcome");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);
const WHO: &str = "des-k first run";

// Layout, in dip. Two client heights only (600 and 680 with Windows' title bar), so Next moves
// the window's bottom edge at most once.
const W: f32 = 640.0;
const SHORT: f32 = 568.0;
const TALL: f32 = 648.0;
const GROW_MS: u32 = 150;
const GROW_TIMER: usize = 1;
const X0: f32 = 32.0; // the step's left edge
const FOOT: f32 = 68.0; // the footer band
const ROW: f32 = 52.0; // a row's least height
const KEY_W: f32 = 160.0; // a shortcut row's key column

const I_OK: &str = "\u{E73E}";
const I_WARN: &str = "\u{E7BA}";
const I_INFO: &str = "\u{E946}";
const I_KEY: &str = "\u{E765}";
const I_ADD: &str = "\u{E710}";
const I_PEN: &str = "\u{E70F}";
const I_DOWN: &str = "\u{E70D}";
const I_UP: &str = "\u{E70E}";
const I_MORE: &str = "\u{E76C}";
const I_SETUP: &str = "\u{E713}";

thread_local! {
    static FIRST: RefCell<Option<Win>> = const { RefCell::new(None) };
    /// The last give back's result: kept until des-k exits or a later give back replaces it; a
    /// give back that worked also goes once Windows' gestures read off again (AC-FR-5).
    static LAST: Cell<Option<GaveBack>> = const { Cell::new(None) };
}

/// What step 1 asks you to try; main tells the first run when you did.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Try {
    Switch,
    Overview,
    Find,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Pad {
    Windows, // Set up in, or Open, Windows Settings
    ReadOn,
    GiveBack, // Give back, or Try again
}

/// Something you can click, focus from the keyboard, or both.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Next,
    Back,
    Skip,
    Field(Field),
    Side(usize),         // focus only: a step 2 row's des-k or Windows, arrows move it
    SideAt(usize, bool), // des-k (true) or Windows, clicked
    More,
    Toggle(&'static str),
    Minus(&'static str),
    Plus(&'static str),
    Pick(&'static str), // focus only: a choice, arrows move it
    Choice(&'static str, usize),
    Picker(usize),
    Item(usize),
    Pad(Pad),
    Show, // Shortcut hints
    Cancel,
    Move,
}

impl Ctl {
    /// The keyboard stop this control belongs to.
    fn stop(self) -> Ctl {
        match self {
            Ctl::Choice(k, _) => Ctl::Pick(k),
            Ctl::SideAt(i, _) => Ctl::Side(i),
            c => c,
        }
    }

    /// Enter presses it; on anything else Enter presses Next (AC-FR-6).
    fn button(self) -> bool {
        !matches!(self, Ctl::Toggle(_) | Ctl::Pick(_) | Ctl::Side(_))
    }
}

// The first run is never read only: it opens before settings.toml could have gone bad.
impl controls::Id for Ctl {
    fn writes(self) -> bool {
        false
    }
}

/// Where a control sits, in window dip. Group: 0 above the step, 1 the step (clipped to its
/// view), 2 the footer, 3 the open menu.
type Hit = controls::Hit<Ctl, u8>;

/// What the screens picture and the touchpad step draw, copied out of ws and Windows.
#[derive(Clone, PartialEq, Default)]
struct Pics {
    screens: Vec<crate::ws::views::ScreenInfo>,
    view: crate::ws::views::View,
    pad: Option<PadState>,
    mouse: String, // the device under the mouse
}

struct Win {
    hwnd: HWND,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    step: Step,
    more: [bool; 5], // per step: More open
    tried: [bool; 3],
    switched: Option<(String, usize, bool)>, // the screen that switched, to which workspace, the only screen
    scroll: f32,
    to_end: bool, // the next layout scrolls to the end (More just opened)
    content: f32,
    mid: (f32, f32), // the step's view: its top and bottom
    hits: Vec<Hit>,
    focus: Option<Ctl>,
    kb: bool, // the focus ring shows only after a key, as in Windows
    hover: Option<Ctl>,
    press: Option<Ctl>,
    vw: f32,
    vh: f32,
    cap: f32,                              // the most client height the screen has room for
    grow: Option<(Instant, f32, f32, u32)>, // started, from, to, ms
    rec: Option<Rec>,
    refused: Option<(String, String)>,
    prompt: Option<Prompt>,
    menu: Option<Menu>,
    error: Option<(String, String)>, // the last save that failed: what, and Windows' words
    pics: Pics,
    tree: Tree,
}

// Never hold the borrow across a call that comes back here: settings::edit::write reloads, and the
// reload calls refresh().
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&FIRST, "first run get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&FIRST, "first run set", f)
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

fn repaint() {
    if let Some(h) = hwnd() {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// Lays the window out again for its scroll, keeps the focus on a stop that is still there,
/// and gives Narrator the step as painted.
fn layout() {
    for _ in 0..2 {
        let Some(out) = get(|w| unsafe {
            let dc = GetDC(Some(w.hwnd));
            let out = {
                let mut c = Canvas::new(dc, false, w.scale, w.theme, &w.fonts);
                compose(&mut c, w)
            };
            ReleaseDC(Some(w.hwnd), dc);
            out
        }) else {
            return;
        };
        let moved = set(|w| {
            let view = out.mid.1 - out.mid.0;
            let max = (out.content - view).max(0.0);
            let scroll = if std::mem::take(&mut w.to_end) { max } else { w.scroll.clamp(0.0, max) };
            let moved = scroll != w.scroll;
            w.scroll = scroll;
            w.content = out.content;
            w.mid = out.mid;
            w.hits = out.hits;
            if let Some(f) = w.focus
                && !w.hits.iter().any(|h| h.ctl.stop() == f)
            {
                w.focus = Some(Ctl::Next);
            }
            let s = w.scale;
            let px = |b: Bx| RECT { left: (b.0 * s) as i32, top: (b.1 * s) as i32, right: ((b.0 + b.2) * s) as i32, bottom: ((b.1 + b.3) * s) as i32 };
            let mut kids: Vec<Node> = out.texts;
            kids.extend(out.said.into_iter().map(|(ctl, mut n)| {
                if let Some(h) = w.hits.iter().find(|h| h.ctl.stop() == ctl) {
                    n.rect = px(h.at);
                }
                n.focused = w.focus == Some(ctl);
                n
            }));
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
    repaint();
}

/// Reads des-k and Windows again and redraws, if the first run is open: after every settings
/// reload and on WM_TOUCHPADS_CHANGED.
pub fn refresh() {
    if hwnd().is_some() {
        update_pictures();
        layout();
    }
}

/// Main loop, every second and on a display change: the screens and the touchpad follow you.
pub fn tick() {
    if let Some(h) = hwnd()
        && !unsafe { IsIconic(h) }.as_bool()
        && update_pictures()
    {
        layout();
    }
}

/// Takes a new copy of the pictures' data; true when it changed.
fn update_pictures() -> bool {
    // The first run changes the shared count: what lowering it moves on the screens that follow it.
    let st = settings::get();
    let lower = |id: &str| st.follows_shared(id).then(|| st.shared_count.saturating_sub(1).max(2));
    let Some(view) = crate::ws::views::view(&[&lower]) else { return false };
    let st = setup::state();
    // A give back that worked is over once Windows' gestures read off again; a failed one
    // stays until a later give back replaces it (AC-FR-5).
    if st.off && LAST.get().is_some_and(|b| !b.failed()) {
        LAST.set(None);
    }
    let new = Pics { screens: crate::ws::views::screen_info(), view, pad: Some(st), mouse: crate::ws::switch::under_mouse().1 };
    set(|w| {
        let changed = w.pics != new;
        w.pics = new;
        changed
    }) == Some(true)
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

/// Scrolls the focused stop into view, and tells Narrator.
fn reveal() {
    let dy = get(|w| {
        let h = w.hits.iter().filter(|h| Some(h.ctl.stop()) == w.focus && h.group == 1).map(|h| h.at).next()?;
        let (top, bottom) = (h.1 - 12.0, h.1 + h.3 + 12.0);
        if top < w.mid.0 {
            Some(top - w.mid.0)
        } else if bottom > w.mid.1 {
            Some(bottom - w.mid.1)
        } else {
            None
        }
    })
    .flatten();
    match dy {
        Some(dy) => scroll_by(dy),
        None => layout(),
    }
    if let Some((h, tree)) = get(|w| (w.hwnd, w.tree.clone())) {
        uia::focus_moved(h, &tree, WHO);
    }
}

/// Starts recording; the hook swallows every key until it stops.
fn start_rec(rec: Rec) {
    let _ = set(|w| {
        w.rec = Some(rec);
        w.refused = None;
        w.prompt = None;
        w.menu = None;
        w.focus = Some(Ctl::Cancel);
    });
    // One recording at a time.
    crate::hints::cancel_recording();
    crate::settings_page::recording::cancel_recording();
    record::start(hwnd());
    layout();
    reveal();
}

fn stop_rec() {
    record::stop(hwnd());
    let _ = set(|w| {
        let f = w.rec.and_then(field_of);
        w.rec = None;
        w.refused = None;
        if w.focus == Some(Ctl::Cancel) {
            w.focus = Some(f.map_or(Ctl::Next, Ctl::Field));
        }
    });
    layout();
}

/// Another window starts recording, so a recording here stops.
pub fn cancel_recording() {
    if get(|w| w.rec.is_some()) == Some(true) {
        stop_rec();
    }
}

/// The window the hook is recording for is this one.
pub fn owns(h: HWND) -> bool {
    hwnd() == Some(h)
}

/// The hook's WM_RECORDED: the first key down that is not a modifier, and the modifiers held
/// with it. Every key but Esc alone is recorded (AC-FR-6).
pub fn recorded(vk: u16, mods: u8) {
    let Some(rec) = get(|w| w.rec).flatten() else { return };
    if vk == VK_ESCAPE.0 && mods == 0 {
        return stop_rec();
    }
    let kind = match rec {
        Rec::Digits(_) => record::Kind::Digit,
        Rec::Pair(..) => record::Kind::Arrow,
        Rec::List(..) | Rec::Gesture(..) => record::Kind::Combo,
    };
    if let Err((title, sub)) = record::check(vk, mods, kind, &record::WORDS) {
        let _ = set(|w| w.refused = Some((title.into(), sub.into())));
        return layout();
    }
    stop_rec();
    let title = |r: Rec| field_of(r).map_or("Gestures", title_of);
    match rec {
        Rec::Digits(key) => run(sp::recording::plan_digits(key, mods)),
        Rec::Pair(p, n) => run(plan_pair(p, n, mods, true, title(rec))),
        Rec::List(..) | Rec::Gesture(..) => {
            let combo = Combo { mods, vk };
            match rec {
                Rec::List(key, replace) => run(sp::recording::plan_combo(key, replace, combo)),
                Rec::Gesture(row, times) => {
                    if let Some(e) = sp::menu::gesture_edit(row, times, Some(Action::Combo(combo))) {
                        commit(e, "Gestures");
                    }
                }
                _ => {}
            }
        }
    }
}

/// Step 1: you did something it asks you to try (main and the touchpad call this).
pub(crate) fn tried(t: Try) {
    if hwnd().is_none() {
        return;
    }
    let switched = (t == Try::Switch).then(|| {
        let (_, dev) = crate::ws::switch::under_mouse();
        let screens = crate::ws::views::screen_info();
        let view = crate::ws::views::view(&[]);
        let to = view.as_ref().and_then(|v| v.active.iter().find(|(d, _)| *d == dev).map(|(_, a)| *a)).unwrap_or(0);
        let name = view.as_ref().and_then(|v| v.names.iter().find(|(d, _)| *d == dev).map(|(_, n)| n.clone())).or_else(|| screens.iter().find(|s| s.device == dev).map(|s| s.name.clone())).unwrap_or_default();
        (name, to, screens.len() < 2)
    });
    let _ = set(|w| {
        w.tried[t as usize] = true;
        if let Some(sw) = switched {
            w.switched = Some(sw);
        }
    });
    update_pictures();
    layout();
}

fn step_key(key: &'static str, up: bool) {
    let s = settings::get();
    let Some(sp::rows::Row { kind: Kind::Step { lo, hi, by, .. }, .. }) = sp::rows::find_row(key) else { return };
    // Greyed: kept but not in use, so − and + do nothing.
    if !sp::rows::unused(&s, key).is_empty() {
        return;
    }
    let v = sp::rows::value(&s, key);
    let n = sp::rows::stepped(v, *lo, *hi, *by, up);
    if n != v {
        save(key, n);
    }
}

fn open_menu(row: usize, times: u8, family: Option<usize>) {
    let Some(at) = get(|w| w.hits.iter().find(|h| h.ctl == Ctl::Picker(row)).map(|h| h.at)).flatten() else { return };
    let m = Menu { row, times, family, at, focus: 0 };
    let focus = sp::menu::menu_lines(&m, &settings::get()).iter().position(|l| l.on).unwrap_or(0);
    let _ = set(|w| w.menu = Some(Menu { focus, ..m }));
    layout();
}

fn close_menu() {
    let _ = set(|w| w.menu = None);
    layout();
}

fn choose(i: usize) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let lines = sp::menu::menu_lines(&m, &settings::get());
    let Some(l) = lines.get(i) else { return };
    match l.does {
        Does::Set(a) => {
            close_menu();
            if let Some(e) = sp::menu::gesture_edit(m.row, m.times, a) {
                commit(e, "Gestures");
            }
        }
        Does::Combo => start_rec(Rec::Gesture(m.row, m.times)),
        Does::Family(f) => open_menu(m.row, m.times, Some(f)),
        Does::Twice => open_menu(m.row, 2, None),
    }
}

/// Give back, from here or the settings page: one result both screens show.
pub(crate) fn give() {
    if let Some(b) = setup::give_back() {
        LAST.set(Some(b));
    }
    refresh();
}

/// The last give back's result, while it lasts: the hints window and the settings page word
/// their touchpad state from it too.
pub(crate) fn gave_back() -> Option<GaveBack> {
    LAST.get()
}

fn act(ctl: Ctl) {
    let Some(h) = hwnd() else { return };
    let s = settings::get();
    match ctl {
        Ctl::Next => match next_step(false) {
            Some(st) => go(st),
            None => unsafe { let _ = DestroyWindow(h); },
        },
        Ctl::Back => {
            if let Some(st) = next_step(true) {
                go(st);
            }
        }
        Ctl::Skip => go(Step::Done),
        Ctl::Field(f) => {
            if let Some(r) = rec_for(f) {
                start_rec(r);
            }
        }
        Ctl::SideAt(6, desk) => {
            if s.copilot_as_right_ctrl != desk {
                commit(Edit::Value("copilot_as_right_ctrl", desk.into()), TAKES[6].title);
            }
        }
        Ctl::SideAt(i, desk) => run(plan_side(i, desk)),
        Ctl::Side(_) | Ctl::Pick(_) => {}
        Ctl::More => {
            let _ = set(|w| {
                let i = w.step.index();
                w.more[i] = !w.more[i];
                w.to_end = w.more[i]; // opens on its end, as the design scrolls it
            });
            layout();
        }
        Ctl::Toggle(k) => save(k, 1 - sp::rows::value(&s, k)),
        Ctl::Minus(k) => step_key(k, false),
        Ctl::Plus(k) => step_key(k, true),
        Ctl::Choice(k, i) => match sp::rows::find_row(k).map(|r| r.kind) {
            Some(Kind::Pair(..)) => save(k, (i == 0) as i64),
            _ => save(k, i as i64),
        },
        Ctl::Picker(row) => open_menu(row, 1, None),
        Ctl::Item(usize::MAX) => {}
        Ctl::Item(i) => choose(i),
        Ctl::Pad(Pad::Windows) => {
            setup::use_gestures();
            refresh();
        }
        Ctl::Pad(Pad::ReadOn) => save("touchpad.enabled", 1),
        Ctl::Pad(Pad::GiveBack) => give(),
        Ctl::Show => crate::hints::show(),
        Ctl::Cancel => {
            if get(|w| w.rec.is_some()) == Some(true) {
                stop_rec();
            } else {
                let _ = set(|w| {
                    let f = w.prompt.take().and_then(|p| field_of_key(p.key));
                    w.focus = Some(f.map_or(Ctl::Next, Ctl::Field));
                });
                layout();
            }
        }
        Ctl::Move => {
            if let Some(p) = set(|w| w.prompt.take()).flatten() {
                let f = field_of_key(p.key);
                let _ = set(|w| w.focus = Some(f.map_or(Ctl::Next, Ctl::Field)));
                commit(p.edit, f.map_or("Shortcuts", title_of));
            }
        }
    }
}

/// Left and Right on a choice: it moves.
fn change(focus: Ctl, right: bool) {
    let s = settings::get();
    match focus {
        Ctl::Pick(k) => {
            let Some(r) = sp::rows::find_row(k) else { return };
            let n = words_of(r).len();
            let i = sp::rows::value(&s, k) as usize;
            let i = match r.kind {
                Kind::Pair(..) => 1 - sp::rows::value(&s, k) as usize,
                _ => i,
            };
            let to = if right { (i + 1).min(n - 1) } else { i.saturating_sub(1) };
            if to != i {
                act(Ctl::Choice(k, to));
            }
        }
        Ctl::Side(i) => act(Ctl::SideAt(i, !right)),
        _ => return,
    }
    reveal();
}

/// The open menu's keys: Up and Down move, Enter or Space picks, Esc closes or goes back.
fn menu_key(vk: VIRTUAL_KEY) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let n = sp::menu::menu_lines(&m, &settings::get()).len();
    match vk {
        VK_ESCAPE if m.family.is_some() || m.times == 2 => open_menu(m.row, 1, None),
        VK_ESCAPE => close_menu(),
        VK_UP | VK_DOWN | VK_HOME | VK_END | VK_LEFT | VK_RIGHT => {
            let f = match vk {
                VK_UP | VK_LEFT => m.focus.saturating_sub(1),
                VK_DOWN | VK_RIGHT => (m.focus + 1).min(n - 1),
                VK_HOME => 0,
                _ => n - 1,
            };
            let _ = set(|w| w.menu = Some(Menu { focus: f, ..m }));
            layout();
        }
        VK_RETURN | VK_SPACE => choose(m.focus),
        _ => {}
    }
}

/// Every stop in Tab order: above the step, the step, the footer.
fn stops(w: &Win) -> Vec<Ctl> {
    let mut hits: Vec<&Hit> = w.hits.iter().filter(|h| h.group != 3).collect();
    hits.sort_by_key(|h| h.group);
    let mut out: Vec<Ctl> = vec![];
    for h in hits {
        let c = h.ctl.stop();
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn on_key(h: HWND, vk: VIRTUAL_KEY) {
    let _ = set(|w| w.kb = true);
    if get(|w| w.menu.is_some()) == Some(true) {
        return menu_key(vk);
    }
    let focus = get(|w| w.focus).flatten();
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match vk {
        VK_ESCAPE => {
            let layer = get(|w| w.rec.is_some() || w.prompt.is_some()) == Some(true);
            if layer {
                act(Ctl::Cancel);
                reveal();
            } else {
                unsafe { let _ = DestroyWindow(h); }
            }
        }
        VK_TAB => {
            let next = get(|w| {
                let list = stops(w);
                let i = w.focus.and_then(|f| list.iter().position(|c| *c == f));
                let n = list.len();
                if n == 0 {
                    return None;
                }
                Some(match (i, shift) {
                    (Some(i), false) => list[(i + 1) % n],
                    (Some(i), true) => list[(i + n - 1) % n],
                    (None, _) => list[0],
                })
            })
            .flatten();
            if let Some(c) = next {
                let _ = set(|w| w.focus = Some(c));
                reveal();
            }
        }
        VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
            if let Some(f) = focus {
                change(f, matches!(vk, VK_RIGHT | VK_DOWN));
            }
        }
        VK_RETURN => match focus {
            Some(f) if f.button() => {
                act(f);
                reveal();
            }
            _ => act(Ctl::Next),
        },
        VK_SPACE => {
            if let Some(f) = focus.filter(|f| !matches!(f, Ctl::Pick(_) | Ctl::Side(_))) {
                act(f);
                reveal();
            }
        }
        VK_PRIOR | VK_NEXT => {
            let page = get(|w| w.mid.1 - w.mid.0 - 40.0).unwrap_or(0.0);
            scroll_by(if vk == VK_PRIOR { -page } else { page });
        }
        _ => repaint(),
    }
}

fn point(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32 / scale, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / scale)
}

/// The control under a point. The step's controls count only inside its view; while the menu
/// is open, only its lines do.
fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    let menu = w.menu.is_some();
    let shown = |h: &Hit| if menu { h.group == 3 } else { h.group != 1 || (y >= w.mid.0 && y < w.mid.1) };
    w.hits.iter().find(|h| shown(h) && inside(h.at)).map(|h| h.ctl)
}

/// Opens the first run on the screen under the mouse, or brings the open one to the front
/// (AC-FR-1).
pub fn show() {
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
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
        RegisterClassW(&wc); // fails harmlessly when it opens a second time
        let (mon, _) = crate::ws::switch::under_mouse();
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work = mi.rcWork;
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("des-k"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return warn!("first run: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let scale = GetDpiForWindow(h) as f32 / 96.0;
        let t = theme();
        dark_title(h, &t);
        crate::ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        // The tallest step must fit under the title bar; on a short screen both heights cap.
        let (_, frame) = size(h, W, 0.0);
        let cap = (((work.bottom - work.top) - frame) as f32 / scale).min(TALL);
        FIRST.set(Some(Win {
            hwnd: h,
            scale,
            theme: t,
            fonts: Fonts::new(scale),
            step: Step::Try,
            more: [false; 5],
            tried: [false; 3],
            switched: None,
            scroll: 0.0,
            to_end: false,
            content: 0.0,
            mid: (0.0, 0.0),
            hits: vec![],
            focus: Some(Ctl::Next),
            kb: false,
            hover: None,
            press: None,
            vw: W,
            vh: SHORT.min(cap),
            cap,
            grow: None,
            rec: None,
            refused: None,
            prompt: None,
            menu: None,
            error: None,
            pics: Pics::default(),
            tree: Arc::new(Mutex::new(Node::default())),
        }));
        let (ww, wh) = size(h, W, SHORT.min(cap));
        let (_, tall) = size(h, W, cap);
        // Placed so the tallest step fits below the same top edge.
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - tall) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        update_pictures();
        layout();
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

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(h, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(h, &mut r);
                let _ = get(|w| {
                    buffered(dc, 0, 0, r.right, r.bottom, |mem| {
                        let mut c = Canvas::new(mem, true, w.scale, w.theme, &w.fonts);
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, w.theme.b.bg, None);
                        compose(&mut c, w);
                    })
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_KEYDOWN => {
                on_key(h, VIRTUAL_KEY(wp.0 as u16));
                LRESULT(0)
            }
            WM_TIMER if wp.0 == GROW_TIMER => {
                grow_tick(h);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                if get(|w| w.menu.is_some()) == Some(true) {
                    return LRESULT(0);
                }
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // A click while recording only cancels it.
                if get(|w| w.rec.is_some()) == Some(true) {
                    stop_rec();
                    return LRESULT(0);
                }
                if get(|w| w.menu.is_some() && hit_at(w, point(lp, w.scale)).is_none()) == Some(true) {
                    close_menu();
                    return LRESULT(0);
                }
                let _ = SetCapture(h);
                let _ = set(|w| {
                    w.kb = false;
                    w.press = hit_at(w, point(lp, w.scale));
                    if let Some(p) = w.press.filter(|p| !matches!(p, Ctl::Item(_))) {
                        w.focus = Some(p.stop());
                    }
                });
                repaint();
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if set(|w| {
                    let hover = hit_at(w, point(lp, w.scale));
                    std::mem::replace(&mut w.hover, hover) != hover
                }) == Some(true)
                {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    layout(); // pointing at − shows what lowering moves
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let _ = set(|w| w.hover = None);
                layout();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let click = set(|w| {
                    let was = w.press.take();
                    let now = hit_at(w, point(lp, w.scale));
                    (was.is_some() && was == now).then_some(was).flatten()
                })
                .flatten();
                if let Some(c) = click {
                    act(c);
                }
                repaint();
                LRESULT(0)
            }
            WM_SIZE => {
                if wp.0 as u32 != SIZE_MINIMIZED
                    && set(|w| {
                        w.vw = (lp.0 & 0xFFFF) as f32 / w.scale;
                        w.vh = ((lp.0 >> 16) & 0xFFFF) as f32 / w.scale;
                        w.menu = None;
                    })
                    .is_some()
                {
                    layout();
                }
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Recording ends when the window loses focus, and an open menu closes.
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    if get(|w| w.rec.is_some()) == Some(true) {
                        stop_rec();
                    }
                    if get(|w| w.menu.is_some()) == Some(true) {
                        close_menu();
                    }
                }
                refresh();
                LRESULT(0)
            }
            WM_GETOBJECT => match get(|w| w.tree.clone()) {
                Some(tree) => uia::answer_tree(h, wp, lp, &tree),
                None => DefWindowProcW(h, m, wp, lp),
            },
            WM_DESTROY => {
                crate::keys::record(h, false);
                let _ = KillTimer(Some(h), GROW_TIMER);
                let _ = UiaReturnRawElementProvider(h, WPARAM(0), LPARAM(0), None);
                drop(FIRST.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let to = *(lp.0 as *const RECT);
                let scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                let vh = set(|w| {
                    w.scale = scale;
                    w.fonts = Fonts::new(scale);
                    w.vh
                })
                .unwrap_or(SHORT);
                let (ww, wh) = size(h, W, vh);
                let _ = SetWindowPos(h, None, to.left, to.top, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
                layout();
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                crate::ui::refresh();
                let t = theme();
                if set(|w| std::mem::replace(&mut w.theme, t) != t) == Some(true) {
                    dark_title(h, &t);
                    layout();
                }
                DefWindowProcW(h, m, wp, lp)
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}

#[cfg(test)]
mod tests;
