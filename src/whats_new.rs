// What's new (spec 0029): after an update, one window with a card per new feature, each with a
// button that opens the settings page at that feature. It opens once per release that has
// cards, never with the first run, and waits while you're busy. `whats_new_seen` in
// settings.toml remembers the last release shown; it is written as the window opens, so a crash
// can never make it open again. Drawn to the approved design (pdesk-design
// whats-new/whats-new-r1.html, direction C): the first run's frame, a small drawing per row, and
// the footer band. One drawn window with its own focus list and no child controls; lives on the
// main thread, its keys reach wndproc through main's loop. Sizes are in dip.
use std::cell::RefCell;
use std::path::Path;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::WM_MOUSELEAVE,
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::controls::{self, Style};
use crate::settings::{self, Settings, edit::Edit};
use crate::settings_page::{self as sp, rows::Section};
use crate::ui::{self, Theme, canvas::{Bx, Canvas, buffered}, color::{mix, op}, dark_title, fonts::{F, Fonts}, theme};
use crate::update::{self, Version};

const CLASS: PCWSTR = w!("des-k-whats-new");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0);

// Layout, in dip (design C: 700 wide; the first run's head and footer band).
const W: f32 = 700.0;
const X0: f32 = 32.0;
const FOOT: f32 = 68.0;
const PIC: (f32, f32) = (112.0, 68.0);
const MOST: f32 = 680.0; // the tallest client area before the cards scroll

const I_OUT: &str = "\u{E8A7}";
const I_ARROW: &str = "\u{E72A}";
const I_UNDO: &str = "\u{E7A7}";
const INK: u32 = 0x14141A; // a drawn screen's number, as the settings page's (ui::ON_WS)
// Stand ins for your pictures in the small drawing, about as light as the workspace colors.
const DUNES: (u32, u32) = (0xFBE0BE, 0xE59C66);
const FOREST: (u32, u32) = (0xCDE6D8, 0x6FAE90);

/// A card's small drawing, left of its words.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pic {
    Screens,
    Lands,
    Names,
    Back,
    Note,
}

/// A card's words under its title; Back and Attention fill in your own key and words.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Text {
    Plain(&'static str),
    Back,
    Attention,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Card {
    pic: Pic,
    title: &'static str,
    text: Text,
    button: &'static str,
    to: (Section, Option<sp::rows::Card>),
}

/// 0.9.0's cards, biggest first (AC-3).
const V0_9_0: [Card; 5] = [
    Card {
        pic: Pic::Screens,
        title: "A picture for each workspace",
        text: Text::Plain("Give each workspace its own picture, or a slideshow from a folder. Your Windows wallpaper is never changed."),
        button: "Pick pictures",
        to: (Section::Workspaces, Some(sp::rows::Card::Wallpaper)),
    },
    Card {
        pic: Pic::Lands,
        title: "Open apps in their place",
        text: Text::Plain("Give an app a workspace, and its new windows open there without switching your screens."),
        button: "Add an app",
        to: (Section::Apps, None),
    },
    Card {
        pic: Pic::Names,
        title: "Name your workspaces",
        text: Text::Plain("Call a workspace Mail or Code, and rename a screen. The names show on the indicator, the overview and the finder."),
        button: "Name them",
        to: (Section::Workspaces, Some(sp::rows::Card::Names)),
    },
    Card { pic: Pic::Back, title: "Go back", text: Text::Back, button: "Change the key", to: (Section::Shortcuts, Some(sp::rows::Card::Back)) },
    Card { pic: Pic::Note, title: "A hidden window that wants you", text: Text::Attention, button: "Change the words", to: (Section::Attention, None) },
];

/// 0.10.0's cards (spec 0038 AC-18, spec 0039 AC-16, spec 0041 AC-13). Nothing about the Copilot
/// key: an upgrader keeps theirs.
const V0_10_0: [Card; 4] = [
    Card {
        pic: Pic::Screens,
        title: "Keys can follow your focus",
        text: Text::Plain("Shortcuts can act on the focused window's screen instead of the one under the mouse. And when a window comes forward on another workspace, the indicator names its app."),
        button: "Choose",
        to: (Section::Workspaces, None),
    },
    Card {
        pic: Pic::Screens,
        title: "Pause des-k",
        text: Text::Plain("Pick Pause des-k in the tray for a game or to try Windows on its own: every window comes back and Windows gets its keys back. Resume puts each window back on its workspace."),
        button: "Set a key",
        to: (Section::Shortcuts, None),
    },
    Card {
        pic: Pic::Screens,
        title: "Workspaces per screen",
        text: Text::Plain("Each screen can have its own number of workspaces, say two on the laptop and six on the big screen. Each screen's row has its own + and ×."),
        button: "Choose",
        to: (Section::Workspaces, None),
    },
    Card {
        pic: Pic::Lands,
        title: "Apps des-k leaves alone",
        text: Text::Plain("Pick Leave alone for an app that breaks when hidden or keeps grabbing focus. Its windows then stay in view on every workspace, and des-k never moves them."),
        button: "Pick an app",
        to: (Section::Apps, None),
    },
];

/// Every release with news, oldest first. A release without an entry opens nothing; add one
/// before each release that has news (spec 0029 Follow-up).
const NEWS: &[(Version, &[Card])] = &[(Version(0, 9, 0), &V0_9_0), (Version(0, 10, 0), &V0_10_0)];

/// The releases to show, newest first: those newer than `seen` and not newer than the running
/// des-k. A missing `seen` is older than every release; one newer than ours (a downgrade) shows
/// nothing (AC-9).
pub(crate) fn due(seen: Option<Version>, ours: Version) -> Vec<(Version, &'static [Card])> {
    let seen = seen.unwrap_or(Version(0, 0, 0));
    NEWS.iter().rev().filter(|(v, _)| *v > seen && *v <= ours).copied().collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ctl {
    Card(usize), // the n-th card shown, counting through every release
    Link,
    Got,
}

// What's new is never read only: its buttons open the settings page and write nothing.
impl controls::Id for Ctl {
    fn writes(self) -> bool {
        false
    }
}

/// A click area; its group says it is inside the scrolled card area, so it counts only within
/// its view.
type Hit = controls::Hit<Ctl, bool>;

struct Win {
    hwnd: HWND,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    news: Vec<(Version, &'static [Card])>,
    ours: Version,
    updated: bool, // opened by an update, not the sidebar: the Updated pill
    scroll: f32,
    content: f32,
    mid: (f32, f32), // the card area's top and bottom
    hits: Vec<Hit>,
    focus: Ctl,
    kb: bool, // the focus ring shows only after a key, as in Windows
    hover: Option<Ctl>,
    press: Option<Ctl>,
    vh: f32,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
    // Releases due while you were busy, shown by `tick` once you aren't (AC-7).
    static WAITING: RefCell<Vec<(Version, &'static [Card])>> = const { RefCell::new(Vec::new()) };
}

fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&WIN, "what's new get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&WIN, "what's new set", f)
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

/// At start, right after the first run's check (AC-1, AC-2): the first run wins and only marks
/// this release seen; otherwise the releases due open now, or once you're not busy.
pub fn at_start(first_run: bool) {
    let Some(ours) = update::ours() else { return };
    let s = settings::get();
    if first_run {
        if s.whats_new_seen != Some(ours) {
            remember(ours);
        }
        return;
    }
    let news = due(s.whats_new_seen, ours);
    if news.is_empty() {
        return;
    }
    let (mon, _) = crate::ws::switch::under_mouse();
    if crate::indicator::busy(mon) {
        info!("what's new: waiting until you're not busy (a full screen app or a presentation)");
        WAITING.set(news);
        return;
    }
    open_due(news, ours);
}

/// Main's 1 s timer: opens what waited once the screen under the mouse isn't busy (AC-7).
pub fn tick() {
    if WAITING.with_borrow(Vec::is_empty) {
        return;
    }
    let (mon, _) = crate::ws::switch::under_mouse();
    if crate::indicator::busy(mon) {
        return;
    }
    let news = WAITING.take();
    if let Some(ours) = update::ours() {
        open_due(news, ours);
    }
}

/// Written before the window can be closed, so it never opens twice (AC-1, AC-10).
fn open_due(news: Vec<(Version, &'static [Card])>, ours: Version) {
    remember(ours);
    let n: usize = news.iter().map(|(_, c)| c.len()).sum();
    info!("what's new: shown for {ours}, {n} cards");
    show(news, ours, true);
}

fn remember(ours: Version) {
    if let Err(why) = settings::edit::write(Edit::Value("whats_new_seen", ours.to_string().into())) {
        error!("what's new: could not write whats_new_seen = \"{ours}\", so What's new opens again at the next start ({why})");
    }
}

/// The settings page's sidebar link (AC-8): the newest release with cards, at any time; writes
/// nothing.
pub fn show_current() {
    let Some(ours) = update::ours() else { return };
    let news: Vec<_> = due(None, ours).into_iter().take(1).collect();
    if !news.is_empty() {
        show(news, ours, false);
    }
}

/// After a settings reload: the Back card's key and the Attention card's words are live.
pub fn refresh() {
    if hwnd().is_some() {
        layout();
    }
}

// ---- Drawing ----

/// What one pass of compose found.
struct Out {
    hits: Vec<Hit>,
    content: f32,
    mid: (f32, f32),
}

struct Ctx<'a, 'f> {
    cx: controls::Ctx<'a, 'f, Ctl, bool>, // its group: drawing the card area
    w: &'a Win,
    s: &'a Settings,
    cw: f32,
    out: Out,
}

/// A pass's shared context over `c`, in the first run's look.
fn shared<'a, 'f>(c: &'a mut Canvas<'f>, w: &Win, scrolled: bool) -> controls::Ctx<'a, 'f, Ctl, bool> {
    controls::Ctx { c, hits: vec![], said: vec![], ro: false, focus: Some(w.focus), hover: w.hover, kb: w.kb, group: scrolled, style: Style::FirstRun }
}

impl Ctx<'_, '_> {
    fn hit(&mut self, ctl: Ctl, at: Bx) {
        let group = self.cx.group;
        self.cx.hit(ctl, at, group);
    }

    /// A workspace chip, 18 dip, as the settings page's.
    fn chip(&self, x: f32, y: f32, i: usize) {
        let b = self.cx.c.t.b;
        let (fill, ink) = if self.cx.c.t.hc { (b.acc, b.on_acc) } else { (b.ws[i], b.on_ws[i]) };
        self.cx.c.fill_rr((x, y, 18.0, 18.0), 3.0, op(fill));
        let d = (i + 1).to_string();
        self.cx.c.text(x + (18.0 - self.cx.c.width(&d, F::SmallBold)) / 2.0, y, 18.0, &d, F::SmallBold, ink);
    }

    /// A chip, and a name after it, on a small card: the drawings' tag.
    fn tag(&self, x: f32, y: f32, i: usize, name: &str) {
        let b = self.cx.c.t.b;
        let tw = if name.is_empty() { 0.0 } else { 4.0 + self.cx.c.width(name, F::SmallBold) };
        self.cx.c.boxed((x, y, 3.0 + 18.0 + tw + 5.0, 24.0), 4.0, b.card, Some(b.line));
        self.chip(x + 3.0, y + 3.0, i);
        if !name.is_empty() {
            self.cx.c.text(x + 25.0, y, 24.0, name, F::SmallBold, b.tx);
        }
    }

    /// A drawn screen as the settings page draws one (screens_art), shrunk: its bezel, the
    /// picture inside, and its number.
    fn screen(&self, (x, y, w, h): Bx, pic: (u32, u32), n: usize) {
        let b = self.cx.c.t.b;
        let hc = self.cx.c.t.hc;
        self.cx.c.fill_rr((x, y, w, h), 4.0, op(if hc { b.tx } else { b.bez }));
        if hc {
            self.cx.c.fill_rr((x + 1.5, y + 1.5, w - 3.0, h - 3.0), 2.5, op(b.card));
        } else {
            self.cx.c.gradient(x + 1.5, y + 1.5, w - 3.0, h - 3.0, pic.0, pic.1);
        }
        self.cx.c.text(x + 5.0, y + h - 17.0, 16.0, &n.to_string(), F::SmallBold, if hc { b.tx } else { INK });
    }

    /// A card's small drawing, 112 × 68 on the soft tint (design C).
    fn pic(&self, x: f32, y: f32, pic: Pic) {
        let b = self.cx.c.t.b;
        let hc = self.cx.c.t.hc;
        if hc {
            self.cx.c.boxed((x, y, PIC.0, PIC.1), 6.0, b.card, Some(b.tx));
        } else {
            self.cx.c.fill_rr((x, y, PIC.0, PIC.1), 6.0, op(mix(b.card, b.acc_a, 0.09)));
        }
        let stand = op(if hc { b.tx } else { b.stand });
        match pic {
            Pic::Screens => {
                // A laptop beside a monitor, bottoms lined up; the stand at half size.
                let (ox, oy) = (x + 8.0, y + 12.0);
                self.cx.c.fill_rr((ox, oy + 34.0, 46.0, 2.0), 1.0, stand);
                self.screen((ox + 6.0, oy + 10.0, 34.0, 24.0), DUNES, 1);
                let mx = ox + 52.0;
                self.cx.c.fill_rr((mx + 25.0 - 4.0, oy + 34.0, 8.0, 4.5), 0.0, stand);
                self.cx.c.fill_rr((mx + 25.0 - 11.0, oy + 38.5, 22.0, 1.5), 0.75, stand);
                self.screen((mx, oy, 50.0, 34.0), FOREST, 3);
            }
            Pic::Lands => {
                let (wx, wy) = (x + 12.0, y + 19.0);
                self.cx.c.boxed((wx, wy, 38.0, 30.0), 3.0, b.card, Some(b.line));
                self.cx.c.fill_rr((wx + 1.0, wy + 1.0, 36.0, 5.0), 2.0, op(b.line));
                self.cx.c.text(x + 56.0, y + 26.0, 16.0, I_ARROW, F::IconSmall, b.link);
                self.tag(x + 76.0, y + 22.0, 2, "");
            }
            Pic::Names => {
                self.tag(x + 10.0, y + 9.0, 0, "Mail");
                self.tag(x + 30.0, y + 36.0, 1, "Code");
            }
            Pic::Back => {
                self.tag(x + 10.0, y + 22.0, 1, "");
                self.cx.c.text(x + 46.0, y + 26.0, 16.0, I_UNDO, F::IconSmall, b.link);
                self.tag(x + 76.0, y + 22.0, 0, "");
            }
            Pic::Note => {
                let (nx, ny, nw, nh) = (x + 10.0, y + 14.0, PIC.0 - 20.0, 40.0);
                self.cx.c.boxed((nx, ny, nw, nh), 6.0, b.card, Some(b.line));
                self.chip(nx + 7.0, ny + 11.0, 3);
                let line = settings::fill(&self.s.attention_text, "Slack");
                let tw = nw - 7.0 - 18.0 - 6.0 - 6.0;
                let runs = [(line.as_str(), F::SmallBold, b.tx)];
                let th = self.cx.c.measuring().para(0.0, 0.0, tw, 14.0, &runs);
                if th <= 28.0 {
                    self.cx.c.para(nx + 31.0, ny + (nh - th) / 2.0, tw, 14.0, &runs);
                } else {
                    self.cx.c.text(nx + 31.0, ny, nh, &self.cx.c.fit(&line, F::SmallBold, tw), F::SmallBold, b.tx);
                }
                self.cx.c.circle(x + PIC.0 - 10.0, y + 10.0, 3.0, Some(op(b.acc)), None);
            }
        }
    }

    /// The back key's caps, or None when no key is set.
    fn back_caps(&self) -> Option<Vec<String>> {
        self.s.back.first().map(|c| c.to_string().split('+').map(str::to_string).collect())
    }

    /// One card's row: the drawing, its title and words, its button. Returns its height.
    fn row(&mut self, y: f32, i: usize, card: &Card) -> f32 {
        let b = self.cx.c.t.b;
        let caps = if card.text == Text::Back { self.back_caps() } else { None };
        let button = if card.text == Text::Back && caps.is_none() { "Set a key" } else { card.button };
        let bw = self.cx.btn_w(button, None);
        let lx = X0 + 16.0 + PIC.0 + 16.0;
        let lw = X0 + self.cw - 16.0 - bw - 16.0 - lx;
        let attention = settings::fill(&self.s.attention_text, "Slack");
        let back = if caps.is_some() {
            let again = if self.s.jump_back && self.s.jump.is_some() { " Pressing the number of the workspace you're on does too." } else { "" };
            format!("Takes the screen back to the workspace it showed before. Again returns you.{again}")
        } else {
            "Take a screen back to the workspace it showed before. No key is set for it yet.".to_string()
        };
        let runs: Vec<(&str, F, u32)> = match card.text {
            Text::Plain(t) => vec![(t, F::Small, b.tx2)],
            Text::Back => vec![(back.as_str(), F::Small, b.tx2)],
            Text::Attention => vec![
                ("When an app on another workspace asks for you, a note says ", F::Small, b.tx2),
                (attention.as_str(), F::SmallBold, b.tx),
                (" and its workspace keeps a dot.", F::Small, b.tx2),
            ],
        };
        let th = 20.0 + 2.0 + self.cx.c.measuring().para(0.0, 0.0, lw, 16.0, &runs);
        let top = th.max(PIC.1);
        let h = 14.0 + top + 14.0;
        self.pic(X0 + 16.0, y + 14.0 + (top - PIC.1) / 2.0, card.pic);
        let ty = y + 14.0 + (top - th) / 2.0;
        self.cx.c.text(lx, ty, 20.0, card.title, F::Bold, b.tx);
        if let Some(caps) = &caps {
            let mut cx = lx + self.cx.c.width(card.title, F::Bold) + 10.0;
            for k in caps {
                cx += self.cx.c.key_cap(cx, ty - 1.0, 22.0, k, false) + 4.0;
            }
        }
        self.cx.c.para(lx, ty + 22.0, lw, 16.0, &runs);
        let _ = self.cx.button(Ctl::Card(i), X0 + self.cw - 16.0 - bw, y + (h - 32.0) / 2.0, button, None, false, self.cx.group);
        h
    }

    fn head(&mut self) -> f32 {
        let b = self.cx.c.t.b;
        let mut x = X0;
        if self.w.updated {
            let pw = self.cx.c.width("Updated", F::SmallBold) + 16.0;
            self.cx.c.fill_rr((x, 18.0, pw, 18.0), 9.0, op(b.acc));
            self.cx.c.text(x + 8.0, 18.0, 18.0, "Updated", F::SmallBold, b.on_acc);
            x += pw + 8.0;
        }
        self.cx.c.text(x, 18.0, 18.0, &format!("des-k {}", self.w.ours), F::Small, b.tx2);
        self.cx.c.text(X0, 37.0, 32.0, "What's new", F::Heading, b.tx);
        let sub = "A few things you can do now. Each button opens its setting.";
        73.0 + self.cx.c.para(X0, 73.0, self.cw, 20.0, &[(sub, F::Body, b.tx2)])
    }

    /// Every release's cards, each release under its own heading when there are two or more.
    fn cards(&mut self, y0: f32) -> f32 {
        let b = self.cx.c.t.b;
        let many = self.w.news.len() > 1;
        let mut y = y0;
        let mut i = 0;
        for (k, (v, cards)) in self.w.news.iter().enumerate() {
            if many {
                if k > 0 {
                    y += 20.0;
                }
                let name = v.to_string();
                self.cx.c.text(X0, y, 20.0, &name, F::Bold, b.tx);
                if k == 0 {
                    self.cx.c.text(X0 + self.cx.c.width(&name, F::Bold) + 8.0, y, 20.0, "newest", F::Small, b.tx2);
                }
                y += 28.0;
            }
            let h: f32 = {
                let mut canvas = self.cx.c.measuring();
                let mut m = Ctx { cx: shared(&mut canvas, self.w, true), w: self.w, s: self.s, cw: self.cw, out: Out { hits: vec![], content: 0.0, mid: (0.0, 0.0) } };
                cards.iter().enumerate().map(|(j, c)| m.row(0.0, j, c)).sum()
            };
            self.cx.c.boxed((X0, y, self.cw, h), 8.0, b.card, Some(b.line));
            let mut ry = y;
            for (j, card) in cards.iter().enumerate() {
                if j > 0 {
                    self.cx.c.rule(X0 + 1.0, ry, self.cw - 2.0, op(b.line));
                }
                ry += self.row(ry, i, card);
                i += 1;
            }
            y += h;
        }
        y - y0
    }

    fn footer(&mut self) {
        let b = self.cx.c.t.b;
        let vh = self.w.vh;
        let y = vh - FOOT;
        self.cx.c.fill_rr((0.0, y, W, FOOT), 0.0, op(b.band));
        self.cx.c.rule(0.0, y, W, op(b.line));
        let link = format!("Everything in {}", self.w.ours);
        let lw = self.cx.c.width(&link, F::Body);
        let at = (X0, y + 22.0, lw + 6.0 + self.cx.c.width(I_OUT, F::IconSmall), 24.0);
        let ink = if self.cx.hovered(Ctl::Link) && !self.cx.c.t.hc { mix(b.link, b.tx, 0.25) } else { b.link };
        self.cx.c.text(X0, at.1, 24.0, &link, F::Body, ink);
        self.cx.c.text(X0 + lw + 6.0, at.1, 24.0, I_OUT, F::IconSmall, ink);
        self.cx.ring(at, 3.0, Ctl::Link);
        self.hit(Ctl::Link, at);
        let gw = self.cx.btn_w("Got it", None);
        let _ = self.cx.primary(Ctl::Got, W - X0 - gw, y + 18.0, "Got it", self.cx.group);
    }
}

/// The whole window, back to front: the cards, then the head and footer over them.
fn compose(c: &mut Canvas, w: &Win, s: &Settings) -> Out {
    let cw = W - 2.0 * X0;
    let mut x = Ctx { cx: shared(c, w, false), w, s, cw, out: Out { hits: vec![], content: 0.0, mid: (0.0, 0.0) } };
    let b = x.cx.c.t.b;
    let head = {
        let draw = std::mem::replace(&mut x.cx.c.draw, false);
        let h = x.head();
        x.cx.c.draw = draw;
        h
    };
    let (top, bottom) = (head + 20.0, w.vh - FOOT - 8.0);
    x.out.mid = (top, bottom);
    x.cx.group = true;
    x.out.content = x.cards(top - w.scroll);
    x.cx.group = false;
    // The head and the footer cover the cards scrolled under them; a line marks a cut edge.
    x.cx.c.fill_rr((0.0, 0.0, W, top), 0.0, op(b.bg));
    x.cx.c.fill_rr((0.0, bottom, W, w.vh - bottom), 0.0, op(b.bg));
    x.head();
    let view = bottom - top;
    if w.scroll > 0.5 {
        x.cx.c.rule(X0 - 1.0, top, cw + 2.0, op(b.line));
    }
    if w.scroll < x.out.content - view - 1.0 {
        x.cx.c.rule(X0 - 1.0, bottom - 1.0, cw + 2.0, op(b.line));
    }
    if x.out.content - view > 1.0 {
        let th = ((view - 8.0) * view / x.out.content).max(24.0);
        let ty = top + 4.0 + (view - 8.0 - th) * (w.scroll / (x.out.content - view).max(1.0));
        x.cx.c.fill_rr((W - 5.0, ty, 2.0, th), 1.0, op(b.tx3));
    }
    x.footer();
    Out { hits: x.cx.hits, ..x.out }
}

fn repaint() {
    if let Some(h) = hwnd() {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// Lays the window out again for its scroll and repaints.
fn layout() {
    let Some(out) = get(|w| unsafe {
        let dc = GetDC(Some(w.hwnd));
        let out = compose(&mut Canvas::new(dc, false, w.scale, w.theme, &w.fonts), w, &settings::get());
        ReleaseDC(Some(w.hwnd), dc);
        out
    }) else {
        return;
    };
    let _ = set(|w| {
        let max = (out.content - (out.mid.1 - out.mid.0)).max(0.0);
        w.scroll = w.scroll.clamp(0.0, max);
        w.content = out.content;
        w.mid = out.mid;
        w.hits = out.hits;
    });
    repaint();
}

/// The height the window wants: every card, unless that's taller than MOST or the screen.
fn wanted(w: &Win, cap: f32) -> f32 {
    let probe = Win { vh: 10_000.0, hits: vec![], news: w.news.clone(), fonts: Fonts::new(w.scale), ..*w };
    let out = unsafe {
        let dc = GetDC(None);
        let out = compose(&mut Canvas::new(dc, false, w.scale, w.theme, &probe.fonts), &probe, &settings::get());
        ReleaseDC(None, dc);
        out
    };
    (out.mid.0 + out.content + 8.0 + FOOT).min(MOST).min(cap)
}

fn scroll_by(dy: f32) {
    let _ = set(|w| {
        let max = (w.content - (w.mid.1 - w.mid.0)).max(0.0);
        w.scroll = (w.scroll + dy).clamp(0.0, max);
    });
    layout();
}

/// Scrolls the focused card's button into view after Tab.
fn reveal() {
    let dy = get(|w| {
        let h = w.hits.iter().find(|h| h.ctl == w.focus && h.group)?;
        let (top, bottom) = (h.at.1 - 12.0, h.at.1 + h.at.3 + 12.0);
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
        None => repaint(),
    }
}

fn stops(w: &Win) -> Vec<Ctl> {
    let n: usize = w.news.iter().map(|(_, c)| c.len()).sum();
    (0..n).map(Ctl::Card).chain([Ctl::Link, Ctl::Got]).collect()
}

fn act(ctl: Ctl) {
    match ctl {
        // What's new stays open behind the page (AC-4).
        Ctl::Card(i) => {
            if let Some(to) = get(|w| w.news.iter().flat_map(|(_, c)| c.iter()).nth(i).map(|c| c.to)).flatten() {
                sp::open_at(Some(to.0), to.1);
            }
        }
        // Built from des-k's own version only, opened through explorer so it runs as you (AC-5).
        Ctl::Link => {
            if let Some(v) = get(|w| w.ours) {
                crate::tray::open(Path::new(&update::page(v)));
            }
        }
        Ctl::Got => {
            if let Some(h) = hwnd() {
                unsafe { let _ = DestroyWindow(h); }
            }
        }
    }
}

fn on_key(h: HWND, vk: VIRTUAL_KEY) {
    let _ = set(|w| w.kb = true);
    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
    match vk {
        VK_ESCAPE => unsafe { let _ = DestroyWindow(h); },
        VK_TAB => {
            let _ = set(|w| {
                let list = stops(w);
                let n = list.len();
                let i = list.iter().position(|c| *c == w.focus).unwrap_or(n - 1);
                w.focus = list[if shift { (i + n - 1) % n } else { (i + 1) % n }];
            });
            reveal();
        }
        VK_RETURN | VK_SPACE => {
            if let Some(f) = get(|w| w.focus) {
                act(f);
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

/// The control under a point; a card's button counts only inside the card area's view.
fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    w.hits.iter().find(|h| (!h.group || (y >= w.mid.0 && y < w.mid.1)) && inside(h.at)).map(|h| h.ctl)
}

/// Opens the window on the screen under the mouse with `news`, or brings the open one to the
/// front with them (AC-1, AC-8).
fn show(news: Vec<(Version, &'static [Card])>, ours: Version, updated: bool) {
    if let Some(h) = hwnd() {
        let _ = set(|w| {
            w.updated |= updated;
            if news.len() > w.news.len() {
                w.news = news;
            }
        });
        layout();
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
            Err(e) => return warn!("what's new: no window: {e}"),
        };
        if let Some(icon) = crate::tray::draw_icon() {
            SendMessageW(h, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(icon.0 as isize)));
        }
        let scale = GetDpiForWindow(h) as f32 / 96.0;
        let t = theme();
        dark_title(h, &t);
        ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        let (_, frame) = size(h, W, 0.0);
        let cap = ((work.bottom - work.top) - frame) as f32 / scale;
        let mut win = Win {
            hwnd: h,
            scale,
            theme: t,
            fonts: Fonts::new(scale),
            news,
            ours,
            updated,
            scroll: 0.0,
            content: 0.0,
            mid: (0.0, 0.0),
            hits: vec![],
            focus: Ctl::Got, // Enter closes it at once (AC-6)
            kb: false,
            hover: None,
            press: None,
            vh: 0.0,
        };
        win.vh = wanted(&win, cap);
        let vh = win.vh;
        WIN.set(Some(win));
        let (ww, wh) = size(h, W, vh);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
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
                        compose(&mut c, w, &settings::get());
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
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetCapture(h);
                let _ = set(|w| {
                    w.kb = false;
                    w.press = hit_at(w, point(lp, w.scale));
                    if let Some(p) = w.press {
                        w.focus = p;
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
                    repaint();
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let _ = set(|w| w.hover = None);
                repaint();
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
                if wp.0 as u32 != SIZE_MINIMIZED && set(|w| w.vh = ((lp.0 >> 16) & 0xFFFF) as f32 / w.scale).is_some() {
                    layout();
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                drop(WIN.with_borrow_mut(Option::take));
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
                .unwrap_or(MOST);
                let (ww, wh) = size(h, W, vh);
                let _ = SetWindowPos(h, None, to.left, to.top, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
                layout();
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::refresh();
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
mod tests {
    use super::*;

    #[allow(non_snake_case)]
    fn V(a: u16, b: u16, c: u16) -> Version {
        Version(a, b, c)
    }

    // Who sees What's new at a start (AC-9): an upgrader from 0.8.0 (no key) sees 0.9.0; once
    // seen, never again; a downgrade, or a release with no cards, shows nothing.
    #[test]
    fn due_shows_each_release_once_and_never_after_a_downgrade() {
        let ours = V(0, 9, 0);
        assert_eq!(due(None, ours).len(), 1);
        assert_eq!(due(Some(V(0, 8, 0)), ours)[0].0, ours);
        assert!(due(Some(ours), ours).is_empty(), "seen: never again");
        assert!(due(Some(V(9, 9, 9)), ours).is_empty(), "a downgrade opens nothing");
        assert!(due(None, V(0, 8, 5)).is_empty(), "a des-k older than every release with cards");
        // A patch release without its own cards opens nothing for someone who saw 0.9.0.
        assert!(due(Some(ours), V(0, 9, 1)).is_empty());
        // ...but still shows 0.9.0's to someone who skipped it.
        assert_eq!(due(Some(V(0, 8, 0)), V(0, 9, 1))[0].0, ours);
    }

    // Newest first, so someone two releases behind reads the latest news at the top (AC-1).
    #[test]
    fn due_lists_newest_first_and_news_stays_in_order() {
        assert!(NEWS.windows(2).all(|p| p[0].0 < p[1].0), "NEWS must stay oldest first");
        let all = due(None, V(99, 0, 0));
        assert!(all.windows(2).all(|p| p[0].0 > p[1].0));
        assert_eq!(V0_9_0.len(), 5);
    }

    /// Each render state, set up as the render test draws it: the releases shown, the focus,
    /// the settings.
    fn each_state(theme: Theme, scale: f32, f: &mut dyn FnMut(&str, &Win, &Settings)) {
        const NEXT: [Card; 1] = [Card { pic: Pic::Lands, title: "A card for 0.10.0", text: Text::Plain("Made up for this picture only, to show two releases at once."), button: "Open it", to: (Section::Apps, None) }];
        let plain = Settings::default();
        let no_key = Settings { back: vec![], ..Settings::default() };
        type Setup = fn(&mut Win);
        let states: Vec<(&str, &Settings, Setup)> = vec![
            ("updated", &plain, |_| {}),
            ("no-key-tab", &no_key, |w| {
                w.kb = true;
                w.focus = Ctl::Card(3);
            }),
            ("two-releases", &plain, |w| w.news.insert(0, (Version(0, 10, 0), &NEXT as &[Card]))),
            ("sidebar", &plain, |w| w.updated = false),
            ("0.10.0", &plain, |w| {
                (w.news, w.ours) = (due(Some(V(0, 9, 0)), V(0, 10, 0)), V(0, 10, 0));
            }),
        ];
        for (sname, s, setup) in &states {
            let mut win = Win {
                hwnd: HWND::default(), scale, theme, fonts: Fonts::new(scale), news: due(None, V(0, 9, 0)), ours: V(0, 9, 0), updated: true,
                scroll: 0.0, content: 0.0, mid: (0.0, 0.0), hits: vec![], focus: Ctl::Got, kb: false, hover: None, press: None, vh: 10_000.0,
            };
            setup(&mut win);
            let out = unsafe {
                let dc = GetDC(None);
                let out = compose(&mut Canvas::new(dc, false, scale, theme, &win.fonts), &win, s);
                ReleaseDC(None, dc);
                out
            };
            win.vh = (out.mid.0 + out.content + 8.0 + FOOT).min(MOST);
            f(sname, &win, s);
        }
    }

    // Not a check that can fail: draws the window offscreen into target/whatsnew/*.png, so the
    // drawing can be compared with the approved design (pdesk-design whats-new/whats-new-r1.html,
    // direction C) without updating des-k. Run with `cargo test render -- --ignored`.
    #[test]
    #[ignore]
    fn render_whats_new() {
        assert!(ui::canvas::gdiplus());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("whatsnew");
        std::fs::create_dir_all(&dir).unwrap();
        let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let runs: Vec<(&str, Theme, f32)> = vec![("light", ui::LIGHT, 1.0), ("dark", ui::DARK, 1.0), ("hc", night_sky, 1.0), ("light-150", ui::LIGHT, 1.5)];
        for (tname, theme, scale) in &runs {
            each_state(*theme, *scale, &mut |sname, win, s| {
                let (pw, ph) = ((W * scale).round() as i32, (win.vh * scale).round() as i32);
                ui::tests::save_png(pw, ph, &dir.join(format!("{sname}-{tname}.png")), |dc| {
                    let mut c = Canvas::new(dc, true, *scale, *theme, &win.fonts);
                    c.boxed((0.0, 0.0, W + 1.0, win.vh + 1.0), 0.0, theme.b.bg, None);
                    compose(&mut c, win, s);
                });
            });
        }
    }

    // The click areas in every render state, kept from before the controls merge so the merge
    // can't change them (spec 0035 AC-7). Run by hand, as the PNGs are:
    // `cargo test snapshot -- --ignored`.
    #[test]
    #[ignore]
    fn snapshot_whats_new() {
        use std::fmt::Write;
        assert!(ui::canvas::gdiplus());
        let night_sky = ui::high_contrast([0x000000, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let mut got = String::new();
        for (tname, theme) in [("light", ui::LIGHT), ("hc", night_sky)] {
            each_state(theme, 1.0, &mut |sname, win, s| {
                let out = unsafe {
                    let dc = GetDC(None);
                    let out = compose(&mut Canvas::new(dc, false, win.scale, win.theme, &win.fonts), win, s);
                    ReleaseDC(None, dc);
                    out
                };
                let _ = writeln!(got, "== {sname} {tname}");
                for h in &out.hits {
                    let _ = writeln!(got, "hit {:?} {} {}", h.ctl, h.group, crate::snapshot::bx(h.at));
                }
            });
        }
        crate::snapshot::check("whats_new", &got);
    }
}
