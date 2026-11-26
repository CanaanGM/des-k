// On-screen workspace indicator (spec 0017): a card with the workspace's number in its color, the
// screen's name when two or more screens are plugged in, and a bar per workspace; or the dots,
// the second style. Lines under a rule say what a send, a pin or a screen change did. It follows
// Windows' own light or dark mode, holds, then fades through ui::motion_ms, and Narrator hears
// each card. A layered popup faded whole, so it never takes focus or a click. Lives on the main
// thread (its messages come through main's loop).
use std::cell::{Cell, RefCell};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::*,
    },
};
use windows::core::w;

use crate::settings::{self, Pos, Spot, Style};
use crate::ui::{self, Theme, canvas::Canvas, color::{mix, op}, fonts::F};

/// Also how `--quit` and the single-instance check find the running des-k.
pub const CLASS: windows::core::PCWSTR = w!("des-k-indicator");
/// Posted to the main loop on WM_DISPLAYCHANGE (handled there, never inside a sent message).
pub const WM_DISPLAY_CHANGED: u32 = WM_APP + 1;
/// Posted to the main loop when a window flashes (spec 0022): wParam is the window.
pub const WM_ATTENTION: u32 = WM_APP + 16;
const HSHELL_FLASH: usize = 0x8006; // HSHELL_REDRAW | HSHELL_HIGHBIT

const FADE_TIMER: usize = 1;
const FADE_STEP_MS: u32 = 16;
const LINE_MS: u32 = 1500; // a card with a line stays at least this long, so it can be read
const NOTE_MS: u32 = 3000; // a screen unplugged or back
const PAUSED_MS: u32 = 6000; // a paused start's note: unasked, right after logon (spec 0039 AC-5)

/// What the indicator says: `active` (0 based) of `count` on a screen, named when there are two
/// or more, with any lines under it.
#[derive(Clone, PartialEq, Debug)]
pub struct Card {
    pub active: usize,
    pub count: usize,
    pub screen: Option<String>,
    /// The screen's id (spec 0016), for its own place (spec 0020 AC-9); empty uses the default.
    pub screen_id: String,
    pub lines: Vec<Line>,
    /// This screen's workspaces with a window that flashed while hidden (spec 0022), 0 based.
    pub wants: Vec<usize>,
    /// This screen's workspace names, 0 based; empty for none (spec 0023).
    pub names: Vec<String>,
}

impl Card {
    pub fn new(active: usize, count: usize, screen: Option<String>) -> Card {
        Card { active, count, screen, screen_id: String::new(), lines: vec![], wants: vec![], names: vec![] }
    }

    pub fn with(mut self, line: Line) -> Card {
        self.lines.push(line);
        self
    }
}

/// One line under the rule, in the design's words (indicator r2.3). Workspaces are 0 based.
#[derive(Clone, PartialEq, Debug)]
pub enum Line {
    /// A window sent to `to`, you stay; `unpinned`: it was pinned.
    Sent { to: usize, unpinned: bool },
    /// A quick second send brought it back onto the shown workspace.
    SentBack,
    /// Sent to the workspace it was already on.
    Already,
    Pinned,
    Unpinned,
    /// Workspace `n` (1 based) asked for, past the count.
    NoWorkspace { n: usize },
    /// A screen unplugged; its windows went to `to`, the main display, or here when None.
    ScreenGone { name: String, moved: usize, to: Option<String> },
    /// A screen back; `here`: the card is on that screen.
    ScreenBack { name: String, moved: usize, left: usize, here: bool },
    /// A window hidden on `ws` flashed (spec 0022): `text` is the filled attention text.
    Wants { text: String, ws: usize },
    /// A new window of `app` went hidden onto `to` by its rule (spec 0027 AC-5); you stay.
    Opened { app: String, to: usize },
    /// state.txt could not be saved, so a switch or send hid nothing (spec 0034 AC-2).
    NotSaved,
    /// Follow switched here because `app`'s window came forward; empty when it has no name
    /// (spec 0038 AC-12). The Card style only.
    Followed { app: String },
    /// A start came up paused, shown at your first input (spec 0039 AC-5).
    Paused,
}

impl Line {
    fn note(&self) -> bool {
        // A popup comes unasked, like a screen note, so it holds as long; so does a warning.
        matches!(self, Line::ScreenGone { .. } | Line::ScreenBack { .. } | Line::Wants { .. } | Line::NotSaved)
    }

    /// Shown with `indicator.enabled = false` too. A window that wants you has its own switch,
    /// `[attention] enabled` (spec 0022 AC-8); a paused start's note reports a state you may have
    /// forgotten, not a key you pressed (spec 0039 AC-5).
    fn shows_when_off(&self) -> bool {
        matches!(self, Line::Wants { .. } | Line::Paused)
    }
}

/// The card as it is on screen.
struct Shown {
    card: Card,
    style: Style,
    scale: f32,
    theme: Theme,
}

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    static SPOKEN: RefCell<String> = const { RefCell::new(String::new()) };
    static ALPHA: Cell<u8> = const { Cell::new(0) };
    static STEP: Cell<u8> = const { Cell::new(255) }; // alpha lost per fade tick
    static SHELLHOOK: Cell<u32> = const { Cell::new(0) }; // 0 until registered
}

/// Follow's line (spec 0038 AC-12): the app's name cut at 40 characters, or "A window" without one.
fn came_forward(app: &str) -> String {
    let app = app.trim();
    if app.is_empty() {
        return "A window came forward".into();
    }
    let mut name: String = app.chars().take(40).collect();
    if app.chars().count() > 40 {
        name.push('…');
    }
    format!("{name} came forward")
}

fn windows(n: usize) -> String {
    if n == 1 { "1 window".into() } else { format!("{n} windows") }
}

/// An icon's color: grey, the warning color, or the accent a mark is drawn in (spec 0022).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ink {
    Plain,
    Warn,
    Accent,
}

/// A line's icon (a Segoe Fluent Icons code) and its color, its title with the workspace chip
/// that ends it, if any, and its grey second line with its own chip.
struct Words {
    icon: char,
    ink: Ink,
    title: String,
    chip: Option<usize>,
    sub: String,
    sub_chip: Option<usize>,
}

fn words(card: &Card, line: &Line, named: bool) -> Words {
    let on = card.active + 1;
    let w = |icon: char, title: String, sub: String| Words { icon, ink: Ink::Plain, title, chip: None, sub, sub_chip: None };
    match line {
        Line::Sent { to, unpinned } => Words {
            icon: '\u{E724}',
            ink: Ink::Plain,
            title: "Window sent to ".into(),
            chip: Some(*to),
            sub: if *unpinned { format!("Unpinned, you stay on {on}") } else { format!("You stay on {on}") },
            sub_chip: None,
        },
        Line::SentBack => w('\u{E7A7}', format!("Window back on {on}"), String::new()),
        Line::Already => w('\u{E73E}', format!("Window already on {on}"), String::new()),
        Line::Pinned => w('\u{E718}', "Pinned".into(), "On every workspace of this screen".into()),
        Line::Unpinned => w('\u{E77A}', "Unpinned".into(), format!("Only on {on} now")),
        Line::NoWorkspace { n } => w('\u{E711}', format!("No workspace {n}"), format!("You have {}", card.count)),
        Line::ScreenGone { name, moved, to } => {
            let sub = match to {
                Some(to) => format!("{} moved to {to}", windows(*moved)),
                None => format!("{} moved here", windows(*moved)),
            };
            w('\u{E7F4}', format!("{name} unplugged"), sub)
        }
        Line::ScreenBack { name, moved, left, here } => {
            let title = if *here && named { "Back".to_string() } else { format!("{name} is back") };
            let sub = match (moved, left) {
                (0, l) if *l > 0 => format!("{} could not be moved", windows(*l)),
                (m, 0) => format!("{} returned", windows(*m)),
                (m, l) => format!("{} returned, {l} could not be moved", windows(*m)),
            };
            let ink = if *left > 0 { Ink::Warn } else { Ink::Plain };
            Words { icon: if *left > 0 { '\u{E7BA}' } else { '\u{E7F4}' }, ink, title, chip: None, sub, sub_chip: None }
        }
        // The bell in the accent, the marks' color: the popup and the dots say the same thing.
        Line::Wants { text, ws } => Words { icon: '\u{EA8F}', ink: Ink::Accent, title: text.clone(), chip: None, sub: "On workspace ".into(), sub_chip: Some(*ws) },
        Line::Opened { app, to } => Words {
            icon: '\u{E8A7}',
            ink: Ink::Plain,
            title: format!("{app} opened on "),
            chip: Some(*to),
            sub: format!("You stay on {on}"),
            sub_chip: None,
        },
        Line::NotSaved => Words { icon: '\u{E7BA}', ink: Ink::Warn, title: "Layout can't be saved".into(), chip: None, sub: "Nothing hidden".into(), sub_chip: None },
        Line::Followed { app } => w('\u{E8AB}', came_forward(app), String::new()),
        Line::Paused => Words { icon: '\u{E769}', ink: Ink::Accent, title: "des-k is paused".into(), chip: None, sub: "Resume from the tray icon".into(), sub_chip: None },
    }
}

/// What Narrator reads: the painted words, in order. Only the Card style paints names: the
/// title, and each one right after its line's chip (spec 0023 AC-4, AC-6, AC-15).
fn spoken(card: &Card, style: Style) -> String {
    let named = style == Style::Card && card.screen.is_some();
    let ws_name = |ws: usize| card.names.get(ws).filter(|n| style == Style::Card && !n.is_empty()).map_or(String::new(), |n| format!(", {n}"));
    let chip = |c: Option<usize>| c.map_or(String::new(), |n| format!("{}{}", n + 1, ws_name(n)));
    let mut s = format!("Workspace {} of {}{}", card.active + 1, card.count, ws_name(card.active));
    if let (true, Some(name)) = (named, &card.screen) {
        s += &format!(", {name}");
    }
    for line in &card.lines {
        let w = words(card, line, named);
        s += &format!(", {}{}", w.title, chip(w.chip));
        if !w.sub.is_empty() {
            s += &format!(", {}{}", w.sub, chip(w.sub_chip));
        }
    }
    s
}

// Sizes in dip, from the design (indicator r2.3, How it works).
const P: f32 = 8.0; // the card's padding
const PR: f32 = 12.0; // and its extra on the right (a media tile, not a symmetric box)
const BLOCK: f32 = 60.0;
const COL: f32 = 14.0; // between the block's column and the name's
const MAX_W: f32 = 380.0;
const WANT: f32 = 5.0; // a mark's dot (spec 0022)
// Marks hang on one line under the bars' 12 dip row or the 8 dip dots, clear of a send
// target's ring (1.5 dip out), so they line up whatever the bar above them is.
const WANT_BAR: f32 = 14.0; // from the bars' row top
const WANT_DOT: f32 = 13.0; // from the dots' top

// A line's chip and the name after it, each this far after what comes before (spec 0023).
const CHIP_GAP: f32 = 6.0;

/// The chip a line's title ends with: its width. Its number is Bold, bigger than the other
/// surfaces' chips, because on a line it was hard to see (spec 0023 AC-6).
fn chip_w(c: &Canvas, n: usize) -> f32 {
    (c.width(&(n + 1).to_string(), F::Bold) + 10.0).max(20.0)
}

/// Workspace `n`'s chip, 18 tall, at x, y.
fn chip(c: &Canvas, x: f32, y: f32, n: usize) {
    let t = &c.t.b;
    let (color, cw) = (t.ws[n % 9], chip_w(c, n));
    let edge = (t.ws_edge > 0.0).then(|| mix(color, 0, t.ws_edge));
    c.boxed((x, y, cw, 18.0), 4.0, color, edge);
    let num = (n + 1).to_string();
    c.text(x + (cw - c.width(&num, F::Bold)) / 2.0, y, 18.0, &num, F::Bold, t.on_ws[n % 9]);
}

/// Where a chip after `words` starts, from the words' left: the words' trailing space is
/// Narrator's, not a gap to draw.
fn chip_x(c: &Canvas, words: &str, f: F) -> f32 {
    c.width(words.trim_end(), f) + CHIP_GAP
}

/// The name workspace `n` shows after a line's chip: none when `names` (the Card style's) has
/// none for it.
fn chip_name(names: &[String], n: usize) -> &str {
    names.get(n).map_or("", String::as_str)
}

/// A chip after `words` in font `f`, and its workspace's name: their width together.
fn chipped(c: &Canvas, words: &str, f: F, n: usize, names: &[String]) -> f32 {
    let name = chip_name(names, n);
    chip_x(c, words, f) + chip_w(c, n) + if name.is_empty() { 0.0 } else { CHIP_GAP + c.width(name, f) }
}

/// A line's widest unwrapped run.
fn natural(c: &Canvas, w: &Words, names: &[String]) -> f32 {
    let title = w.chip.map_or_else(|| c.width(&w.title, F::Bold), |n| chipped(c, &w.title, F::Bold, n, names));
    title.max(w.sub_chip.map_or_else(|| c.width(&w.sub, F::Small), |n| chipped(c, &w.sub, F::Small, n, names)))
}

/// The chip after a line's `words` (in the row `h` tall from `top`, the chip centered on it, so
/// it overhangs the 16 dip sub row by 1), then the name in the words' font, cut alone when the
/// line runs out: the words and the chip stay whole.
#[allow(clippy::too_many_arguments)] // one call per row kind; a struct would only rename them
fn chip_and_name(c: &Canvas, xt: f32, top: f32, h: f32, text_w: f32, words: &str, f: F, n: usize, names: &[String]) {
    let cx = xt + chip_x(c, words, f);
    chip(c, cx, top + (h - 18.0) / 2.0, n);
    let name = line_name(c, text_w, words, f, n, names);
    if !name.is_empty() {
        c.text(cx + chip_w(c, n) + CHIP_GAP, top, h, &name, f, c.t.b.tx);
    }
}

/// The name after a line's chip, cut to what is left of `text_w`.
fn line_name(c: &Canvas, text_w: f32, words: &str, f: F, n: usize, names: &[String]) -> String {
    // The card may be sized to this very line, and the sums round apart by a hair (at 150% the
    // room came out 0.000004 short of the name); far under a pixel, so it never shows.
    let room = text_w - chip_x(c, words, f) - chip_w(c, n) - CHIP_GAP + 0.01;
    c.fit_name(chip_name(names, n), f, room)
}

/// The mark under workspace `i`'s bar or dot, centered on `cx` from `top`, when it has one.
fn want(c: &Canvas, card: &Card, i: usize, cx: f32, top: f32) {
    if card.wants.contains(&i) {
        c.fill_rr((cx - WANT / 2.0, top, WANT, WANT), WANT / 2.0, op(c.t.b.acc));
    }
}

/// The lines under the rule from `y`: icons centered in a column `icon_w` wide at `x`, the text
/// `gap` to its right and `text_w` wide, a chip followed by its workspace's name from `names`.
/// Returns their height.
#[allow(clippy::too_many_arguments)] // the card's columns, measured once and drawn once
fn lines(c: &Canvas, x: f32, y: f32, icon_w: f32, gap: f32, text_w: f32, all: &[Words], names: &[String]) -> f32 {
    let t = &c.t.b;
    let mut h = 0.0;
    for (i, w) in all.iter().enumerate() {
        if i > 0 {
            h += 8.0;
        }
        let (top, xt) = (y + h, x + icon_w + gap);
        let glyph = w.icon.to_string();
        let ink = match w.ink {
            Ink::Plain => t.tx2,
            Ink::Warn => t.warn,
            Ink::Accent => t.acc,
        };
        c.text(x + (icon_w - c.width(&glyph, F::Icon)) / 2.0, top, 20.0, &glyph, F::Icon, ink);
        let title_h = match w.chip {
            Some(n) => {
                c.text(xt, top, 20.0, &w.title, F::Bold, t.tx);
                chip_and_name(c, xt, top, 20.0, text_w, &w.title, F::Bold, n, names);
                20.0
            }
            None => c.para(xt, top, text_w, 20.0, &[(&w.title, F::Bold, t.tx)]),
        };
        let sub_h = match w.sub_chip {
            _ if w.sub.is_empty() => 0.0,
            Some(n) => {
                c.text(xt, top + title_h + 2.0, 16.0, &w.sub, F::Small, t.tx2);
                chip_and_name(c, xt, top + title_h + 2.0, 16.0, text_w, &w.sub, F::Small, n, names);
                20.0
            }
            None => c.para(xt, top + title_h, text_w, 16.0, &[(&w.sub, F::Small, t.tx2)]),
        };
        h += (title_h + sub_h).max(20.0);
    }
    h
}

/// A workspace's bar or dot `w` by `h` with radius `r`: in its color when `lit`, with the ring
/// of a sent target.
fn mark(c: &Canvas, b: (f32, f32, f32, f32), r: f32, i: usize, lit: bool, target: bool) {
    let t = &c.t.b;
    let color = if lit { t.ws[i % 9] } else { t.bar_off };
    let edge = (lit && t.bar_edge > 0.0).then(|| mix(color, 0, t.bar_edge));
    c.boxed(b, r, color, edge);
    if target {
        let (x, y, w, h) = b;
        c.stroke_rr((x - 1.5, y - 1.5, w + 3.0, h + 3.0), r + 1.5, 1.0, op(t.tx2));
    }
}

/// Draws `card` in `style` with its top left at x, y (or only measures, on a measuring Canvas),
/// and returns its size. `edge`: draw the 1 px edge too, for a preview; on screen Windows
/// draws it (ui::border). The settings page and the first run draw their previews with this.
fn draw(c: &Canvas, x: f32, y: f32, style: Style, card: &Card, edge: bool) -> (f32, f32) {
    let t = &c.t.b;
    let named = style == Style::Card && card.screen.is_some();
    let all: Vec<Words> = card.lines.iter().map(|l| words(card, l, named)).collect();
    let pinned = card.lines.contains(&Line::Pinned);
    let target = card.lines.iter().find_map(|l| match l {
        Line::Sent { to, .. } | Line::Opened { to, .. } => Some(*to),
        _ => None,
    });
    let lit = |i: usize| i == card.active || pinned || target == Some(i);
    // Only the Card style shows names (spec 0023 AC-4); the dots' lines end at the chip.
    let names: &[String] = if style == Style::Card { &card.names } else { &[] };
    let text_nat = all.iter().map(|w| natural(c, w, names)).fold(0.0, f32::max);
    // The side column's title: the workspace's name, else the screen's; with both, the screen's
    // moves to a small line under the name.
    let ws_name = chip_name(names, card.active);
    let screen = card.screen.as_deref().filter(|_| named);
    let title = Some(ws_name).filter(|n| !n.is_empty()).or(screen);
    let screen_line = screen.filter(|_| !ws_name.is_empty());
    let rule_px = c.s.floor().max(1.0) / c.s;
    // A row of marks under the bars or dots grows the card by their reach below the row.
    let marked = card.wants.iter().any(|&i| i < card.count);

    // Card: block, name and bars; lines in the block's and the name's columns.
    // Dots: a pill; lines in a 16 wide icon column.
    let (w, pad_x, icon_w, gap, rows_top) = match style {
        Style::Card => {
            let bars: f32 = (0..card.count).map(|i| if i == card.active { 28.0 } else { 14.0 }).sum::<f32>() + 6.0 * card.count.saturating_sub(1) as f32;
            let title_w = title.map_or(0.0, |n| c.width(n, F::Bold)).max(screen_line.map_or(0.0, |n| c.width(n, F::Small)));
            (((P + BLOCK + COL + bars.max(title_w).max(text_nat) + PR + P).ceil()).min(MAX_W), P, BLOCK, COL, P + BLOCK)
        }
        Style::Dots => {
            let dots: f32 = (0..card.count).map(|i| if i == card.active { 24.0 } else { 8.0 }).sum::<f32>() + 8.0 * card.count.saturating_sub(1) as f32;
            let pad = if all.is_empty() { 16.0 } else { 12.0 };
            let reach = if marked { WANT_DOT + WANT - 8.0 } else { 0.0 };
            ((pad + dots.max(if all.is_empty() { 0.0 } else { 26.0 + text_nat }) + pad).ceil().min(MAX_W), pad, 16.0, 10.0, 12.0 + 8.0 + reach)
        }
    };
    let text_w = match style {
        Style::Card => w - P - BLOCK - COL - PR - P,
        Style::Dots => w - 2.0 * pad_x - icon_w - gap,
    };
    // Measured first, so the card is drawn once at its final size.
    let (rule_gap, after) = match style {
        Style::Card => (8.0, 8.0),
        Style::Dots => (12.0, 8.0),
    };
    let lines_h = if all.is_empty() { 0.0 } else { rule_gap + rule_px + after + lines(&c.measuring(), 0.0, 0.0, icon_w, gap, text_w, &all, names) };
    let bottom = match (style, all.is_empty()) {
        (Style::Card, _) => P,
        (Style::Dots, true) => 12.0,
        (Style::Dots, false) => 10.0,
    };
    let h = rows_top + lines_h + bottom;
    if !c.draw {
        return (w, h);
    }

    let radius = if style == Style::Dots && all.is_empty() { h / 2.0 } else { 8.0 };
    c.boxed((x, y, w, h), radius, t.card, edge.then_some(t.edge));
    match style {
        Style::Card => {
            let color = t.ws[card.active % 9];
            let ring = (t.ws_edge > 0.0).then(|| mix(color, 0, t.ws_edge));
            c.boxed((x + P, y + P, BLOCK, BLOCK), 4.0, color, ring);
            let num = (card.active + 1).to_string();
            c.text(x + P + (BLOCK - c.width(&num, F::Number)) / 2.0, y + P, BLOCK, &num, F::Number, t.on_ws[card.active % 9]);
            let side = x + P + BLOCK + COL;
            // Name and screen line, then bars, fit the 60 dip block: 20 + 16 + 6 + 12, or with
            // marks under the bars 20 + 16 + 4 + 12 + 7, so the card never grows (handoff).
            let under = if screen_line.is_none() { 10.0 } else if marked { 4.0 } else { 6.0 };
            let side_h = title.map_or(0.0, |_| 20.0 + screen_line.map_or(0.0, |_| 16.0) + under) + 12.0 + if marked { WANT_BAR + WANT - 12.0 } else { 0.0 };
            let mut row = y + P + (BLOCK - side_h) / 2.0;
            if let Some(title) = title {
                let shown = if ws_name.is_empty() { c.fit(title, F::Bold, text_w) } else { c.fit_name(title, F::Bold, text_w) };
                c.text(side, row, 20.0, &shown, F::Bold, t.tx);
                row += 20.0;
                if let Some(screen) = screen_line {
                    c.text(side, row, 16.0, &c.fit(screen, F::Small, text_w), F::Small, t.tx2);
                    row += 16.0;
                }
                row += under;
            }
            let mut bx = side;
            for i in 0..card.count {
                let (bw, bh) = if i == card.active { (28.0, 8.0) } else if lit(i) { (14.0, 6.0) } else { (14.0, 4.0) };
                mark(c, (bx, row + (12.0 - bh) / 2.0, bw, bh), bh / 2.0, i, lit(i), target == Some(i));
                want(c, card, i, bx + bw / 2.0, row + WANT_BAR);
                bx += bw + 6.0;
            }
        }
        Style::Dots => {
            let (mut dx, dy) = (x + pad_x, y + 12.0);
            for i in 0..card.count {
                let dw = if i == card.active { 24.0 } else { 8.0 };
                mark(c, (dx, dy, dw, 8.0), 4.0, i, lit(i), target == Some(i));
                want(c, card, i, dx + dw / 2.0, dy + WANT_DOT);
                dx += dw + 8.0;
            }
        }
    }
    if !all.is_empty() {
        let ry = y + rows_top + rule_gap;
        c.fill_rr((x + pad_x, ry, w - 2.0 * pad_x, rule_px), 0.0, op(t.line));
        lines(c, x + pad_x, ry + rule_px + after, icon_w, gap, text_w, &all, names);
    }
    (w, h)
}

/// The settings page's and the first run's preview of `style` (spec 0017 AC-IND-10): `card`
/// drawn with its top left at x, y, edge included. Returns its size.
#[cfg_attr(not(test), allow(dead_code))] // called by the settings page and first run slices
pub(crate) fn draw_preview(c: &Canvas, x: f32, y: f32, style: Style, card: &Card) -> (f32, f32) {
    draw(c, x, y, style, card, true)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                ui::get(&SHOWN, "indicator paint", |s| {
                    ui::canvas::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
                        let bg = CreateSolidBrush(ui::color::cref(s.theme.b.card));
                        FillRect(mem, &rc, bg);
                        let _ = DeleteObject(bg.into());
                        let fonts = ui::fonts::fonts(s.scale);
                        draw(&Canvas::new(mem, true, s.scale, s.theme, &fonts), 0.0, 0.0, s.style, &s.card, false);
                    });
                });
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == FADE_TIMER => {
                // First tick ends the hold; then fade a step per tick.
                SetTimer(Some(hwnd), FADE_TIMER, FADE_STEP_MS, None);
                let a = ALPHA.get().saturating_sub(STEP.get());
                ALPHA.set(a);
                if a == 0 {
                    let _ = KillTimer(Some(hwnd), FADE_TIMER);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else {
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), a, LWA_ALPHA);
                }
                LRESULT(0)
            }
            // A card on screen follows a theme change (spec 0017 AC-2).
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::refresh();
                let t = ui::system_theme();
                if ui::set(&SHOWN, "indicator theme", |s| std::mem::replace(&mut s.theme, t) != t) == Some(true) {
                    ui::border(hwnd, t.b.edge);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_GETOBJECT => {
                let text = SPOKEN.with(|s| s.try_borrow().map(|s| s.clone()).unwrap_or_default());
                crate::uia::answer(hwnd, wp, lp, text)
            }
            // Screens can take a while to light up after a wake (spec 0016 AC-18).
            WM_POWERBROADCAST if wp.0 as u32 == PBT_APMRESUMEAUTOMATIC => {
                crate::ws::screens::woke();
                LRESULT(1)
            }
            WM_DISPLAYCHANGE => {
                let _ = PostMessageW(Some(hwnd), WM_DISPLAY_CHANGED, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            // Only posts: the main loop owns `ws` (spec 0022). Each blink sends one, and a stop
            // sends none (tried 2026-11-19).
            m if m != 0 && m == SHELLHOOK.get() => {
                if wp.0 == HSHELL_FLASH {
                    let _ = PostMessageW(Some(hwnd), WM_ATTENTION, WPARAM(lp.0 as usize), LPARAM(0));
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Create the window up front, so it exists for --quit and display-change messages.
pub fn init() {
    let _ = window();
}

fn window() -> Option<HWND> {
    if let Some(h) = WND.get() {
        return Some(h);
    }
    ui::canvas::gdiplus();
    // No-activate and see-through to the mouse: never steals focus, never takes a click.
    let h = ui::popup(CLASS, Some(wndproc), WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, true)?;
    // An admin des-k would drop `des-k --quit` from a normal terminal otherwise (spec 0008).
    if let Err(e) = unsafe { ChangeWindowMessageFilterEx(h, WM_CLOSE, MSGFLT_ALLOW, None) } {
        warn!("indicator: WM_CLOSE not let in from normal programs: {e}");
    }
    // How the taskbar hears a window flash (spec 0022). No message filter on purpose: one would
    // let any program fake a flash.
    let id = unsafe { RegisterWindowMessageW(w!("SHELLHOOK")) };
    if id != 0 && unsafe { RegisterShellHookWindow(h) }.as_bool() {
        SHELLHOOK.set(id);
    } else {
        error!("indicator: no shell hook, so a window flashing on a hidden workspace will show nothing");
    }
    WND.set(Some(h));
    Some(h)
}

/// Whether a popup on `mon` would get in your way (spec 0022 AC-7): presentation mode on any
/// screen, or a full screen app in front on `mon` itself, so a video on another screen does not
/// silence it.
pub fn busy(mon: HMONITOR) -> bool {
    use windows::Win32::UI::Shell::{QUNS_APP, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
    let Ok(state) = (unsafe { SHQueryUserNotificationState() }) else { return false };
    if state == QUNS_PRESENTATION_MODE {
        return true;
    }
    let full = [QUNS_BUSY, QUNS_RUNNING_D3D_FULL_SCREEN, QUNS_APP].contains(&state);
    full && unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST) } == mon
}

/// How long a card with these lines stays before it fades, given the `hold_ms` setting.
fn hold_for(lines: &[Line], hold_ms: u32) -> u32 {
    if lines.contains(&Line::Paused) {
        PAUSED_MS
    } else if lines.iter().any(Line::note) {
        NOTE_MS
    } else if !lines.is_empty() {
        hold_ms.max(LINE_MS)
    } else {
        hold_ms
    }
}

/// Show `card` centered on `mon`'s work area, replacing whatever the indicator shows.
pub fn show(mon: HMONITOR, card: Card) {
    let st = settings::get();
    if !st.indicator_enabled && !card.lines.iter().any(Line::shows_when_off) {
        return;
    }
    let Some(h) = window() else { return };
    let style = st.indicator_style;
    let card = styled(card, style);
    let theme = ui::system_theme();
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy); }
    let scale = dx as f32 / 96.0;
    let fonts = ui::fonts::fonts(scale);
    let (w, ht) = unsafe {
        let dc = GetDC(None);
        let size = draw(&Canvas::new(dc, false, scale, theme, &fonts), 0.0, 0.0, style, &card, false);
        ReleaseDC(None, dc);
        size
    };
    let (w, ht) = ((w * scale).ceil() as i32, (ht * scale).ceil() as i32);

    let hold = hold_for(&card.lines, st.hold_ms);
    // Enough steps to fade from full to 0 in about fade_ms; 0 hides on the first tick.
    let full = (theme.b.alpha * 255.0).round() as u8;
    let ticks = (ui::motion_ms(st.fade_ms) / FADE_STEP_MS).max(1);
    STEP.set((full as u32).div_ceil(ticks) as u8);
    ALPHA.set(full);
    let text = spoken(&card, style);
    let pos = st.indicator_pos_for(&card.screen_id);
    let stored = SHOWN.with(|s| s.try_borrow_mut().map(|mut s| *s = Some(Shown { card, style, scale, theme })).is_ok());
    if !stored {
        return warn!("indicator: skipped, a message came back in while the card was painted");
    }
    let _ = SPOKEN.with(|s| s.try_borrow_mut().map(|mut s| *s = text.clone()));
    unsafe {
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let (x, y) = place(mi.rcWork, w, ht, pos, scale);
        ui::border(h, theme.b.edge);
        let _ = SetLayeredWindowAttributes(h, COLORREF(0), full, LWA_ALPHA);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, ht, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        // Painted now, not when the loop is next idle: a card shown again kept the previous
        // card's picture for a frame (a 60 fps recording, 2026-11-22). The paint reads only SHOWN.
        let _ = RedrawWindow(Some(h), None, None, RDW_INVALIDATE | RDW_UPDATENOW);
        SetTimer(Some(h), FADE_TIMER, hold, None);
    }
    crate::uia::notify(h, &text, "des-k indicator", false);
}

/// `card` as `style` shows it: the dots show a follow as any switch, with no line (spec 0038 AC-13).
fn styled(mut card: Card, style: Style) -> Card {
    if style == Style::Dots {
        card.lines.retain(|l| !matches!(l, Line::Followed { .. }));
    }
    card
}

/// The card's top left on `work` for a card `w` by `h` px (spec 0020 AC-7, AC-8). Pure, so the
/// tests can run it; every `show` runs it again, so a card that grows keeps its anchor (AC-10).
pub(crate) fn place(work: RECT, w: i32, h: i32, pos: Pos, scale: f32) -> (i32, i32) {
    let RECT { left: l, top: t, right: r, bottom: b } = work;
    let gap = (pos.gap as f32 * scale).round() as i32;
    let (x0, xc, x1) = (l + gap, (l + r - w) / 2, r - gap - w);
    let (y0, yc, y1) = (t + gap, (t + b - h) / 2, b - gap - h);
    let (x, y) = match pos.spot {
        Spot::TopLeft => (x0, y0),
        Spot::Top => (xc, y0),
        Spot::TopRight => (x1, y0),
        Spot::Left => (x0, yc),
        Spot::Center => (xc, yc),
        Spot::Right => (x1, yc),
        Spot::BottomLeft => (x0, y1),
        Spot::Bottom => (xc, y1),
        Spot::BottomRight => (x1, y1),
        Spot::Custom => {
            let at = |lo: i32, hi: i32, pct: f64, size: i32| (lo as f64 + pct / 100.0 * (hi - lo) as f64 - size as f64 / 2.0).round() as i32;
            (at(l, r, pos.x, w), at(t, b, pos.y, h))
        }
    };
    // Always inside the work area; a card bigger than it keeps its left or top edge on it.
    (x.min(r - w).max(l), y.min(b - h).max(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{DARK, LIGHT, Theme, fonts::Fonts};

    const WORK: RECT = RECT { left: 0, top: 0, right: 1920, bottom: 1040 };

    fn at(spot: Spot, w: i32, h: i32) -> (i32, i32) {
        place(WORK, w, h, Pos { spot, ..Pos::default() }, 1.5)
    }

    // AC-7: the gap is 24 px at 100% scale, so 36 at 150%, and the other axis stays centered.
    // Center keeps today's place, so nobody's indicator moves on upgrade.
    #[test]
    fn spots_sit_a_scaled_gap_from_their_edges() {
        let (w, h) = (300, 120);
        assert_eq!(at(Spot::TopLeft, w, h), (36, 36));
        assert_eq!(at(Spot::Top, w, h), (810, 36));
        assert_eq!(at(Spot::TopRight, w, h), (1920 - 36 - 300, 36));
        assert_eq!(at(Spot::Left, w, h), (36, 460));
        assert_eq!(at(Spot::Center, w, h), (810, 460));
        assert_eq!(at(Spot::Right, w, h), (1584, 460));
        assert_eq!(at(Spot::BottomLeft, w, h), (36, 1040 - 36 - 120));
        assert_eq!(at(Spot::Bottom, w, h), (810, 884));
        assert_eq!(at(Spot::BottomRight, w, h), (1584, 884));
        // A second screen to the left, above the taskbar: the place follows its work area.
        let left = RECT { left: -2560, top: 0, right: 0, bottom: 1392 };
        assert_eq!(place(left, w, h, Pos { spot: Spot::TopRight, gap: 0, ..Pos::default() }, 1.0), (-300, 0));
    }

    // AC-8, AC-10: a custom place is the card's center in percent, pulled back inside the work
    // area; a card that grows keeps its center, and one bigger than the screen keeps its top left.
    #[test]
    fn custom_is_a_center_kept_inside_the_screen() {
        let custom = |x, y| Pos { spot: Spot::Custom, x, y, ..Pos::default() };
        assert_eq!(place(WORK, 300, 120, custom(100.0, 100.0), 1.0), (1620, 920));
        assert_eq!(place(WORK, 300, 120, custom(0.0, 0.0), 1.0), (0, 0));
        assert_eq!(place(WORK, 300, 120, custom(25.0, 50.0), 1.0), (330, 460));
        assert_eq!(place(WORK, 300, 160, custom(25.0, 50.0), 1.0), (330, 440));
        assert_eq!(place(WORK, 300, 120, custom(33.3, 50.0), 1.0), (489, 460));
        assert_eq!(place(WORK, 2400, 1200, custom(50.0, 50.0), 1.0), (0, 0));
        assert_eq!(place(WORK, 2400, 1200, Pos { spot: Spot::BottomRight, ..Pos::default() }, 1.0), (0, 0));
    }

    // A card that grows stays anchored at its edges: the bottom right corner holds still.
    #[test]
    fn a_growing_card_keeps_its_corner() {
        let (a, b) = (at(Spot::BottomRight, 300, 120), at(Spot::BottomRight, 300, 180));
        assert_eq!((a.0 + 300, a.1 + 120), (b.0 + 300, b.1 + 180));
    }

    fn card(active: usize, screen: bool) -> Card {
        Card::new(active, 4, screen.then(|| "DELL U2720Q".to_string()))
    }

    // The words are the design's; a send names the target in its chip and where you stay, and
    // "Back" is said only when the card itself names that screen.
    #[test]
    fn lines_say_what_happened() {
        let c = card(1, true);
        let w = words(&c, &Line::Sent { to: 2, unpinned: false }, true);
        assert_eq!((w.title.as_str(), w.chip, w.sub.as_str()), ("Window sent to ", Some(2), "You stay on 2"));
        assert_eq!(words(&c, &Line::Sent { to: 0, unpinned: true }, true).sub, "Unpinned, you stay on 2");
        assert_eq!(words(&c, &Line::NoWorkspace { n: 7 }, true).sub, "You have 4");
        let back = Line::ScreenBack { name: "DELL U2720Q".into(), moved: 3, left: 2, here: true };
        assert_eq!(words(&c, &back, true).title, "Back");
        assert_eq!(words(&c, &back, false).title, "DELL U2720Q is back"); // the dots name no screen
        assert_eq!(words(&c, &back, true).sub, "3 windows returned, 2 could not be moved");
        assert_eq!(words(&c, &back, true).ink, Ink::Warn);
        let none = Line::ScreenBack { name: "X".into(), moved: 0, left: 2, here: true };
        assert_eq!(words(&c, &none, true).sub, "2 windows could not be moved");
        let gone = Line::ScreenGone { name: "LG HDR 4K".into(), moved: 1, to: Some("Built-in display".into()) };
        assert_eq!(words(&c, &gone, true).sub, "1 window moved to Built-in display");
        // A rule never switches a screen (spec 0027 AC-5): the note names where it opened, and that you stay.
        let opened = words(&c, &Line::Opened { app: "Notepad".into(), to: 2 }, true);
        assert_eq!((opened.title.as_str(), opened.chip, opened.sub.as_str()), ("Notepad opened on ", Some(2), "You stay on 2"));
    }

    // Spec 0038 AC-12: a switch you didn't press says whose window caused it, never a blank, and
    // a long name can't push the card past its width.
    #[test]
    fn a_follow_names_the_app_that_came_forward() {
        let said = |app: &str| words(&card(1, true), &Line::Followed { app: app.into() }, true).title;
        assert_eq!(said("Slack"), "Slack came forward");
        assert_eq!(said(""), "A window came forward");
        assert_eq!(said("  "), "A window came forward");
        let long = "Microsoft Teams (work or school) Preview Edition";
        assert_eq!(said(long), "Microsoft Teams (work or school) Preview… came forward");
        assert_eq!(said(&long[..40]), format!("{} came forward", &long[..40]), "40 fit whole");
        // AC-13: the dots have no room for words; they show the switch alone.
        let followed = card(1, true).with(Line::Followed { app: "Slack".into() });
        assert_eq!(styled(followed.clone(), Style::Dots).lines, []);
        assert_eq!(styled(followed.clone(), Style::Card), followed);
    }

    #[test]
    fn narrator_reads_the_painted_words() {
        let c = card(2, true).with(Line::Sent { to: 1, unpinned: false });
        assert_eq!(spoken(&c, Style::Card), "Workspace 3 of 4, DELL U2720Q, Window sent to 2, You stay on 3");
        assert_eq!(spoken(&card(0, true), Style::Dots), "Workspace 1 of 4");
    }

    // Spec 0023 AC-15: Narrator hears a name where the card paints one, the title and right
    // after a line's chip, and nowhere the card paints none: Dots, or a workspace with no name.
    #[test]
    fn narrator_reads_the_names_the_card_paints() {
        let names = |c: Card| Card { names: vec!["Mail".into(), String::new(), "Code".into(), String::new()], ..c };
        let sent = names(card(2, true)).with(Line::Sent { to: 0, unpinned: false });
        assert_eq!(spoken(&sent, Style::Card), "Workspace 3 of 4, Code, DELL U2720Q, Window sent to 1, Mail, You stay on 3");
        assert_eq!(spoken(&sent, Style::Dots), "Workspace 3 of 4, Window sent to 1, You stay on 3");
        let wants = names(card(1, false)).with(Line::Wants { text: "Slack misses you".into(), ws: 3 });
        assert_eq!(spoken(&wants, Style::Card), "Workspace 2 of 4, Slack misses you, On workspace 4");
        let wants = names(card(1, false)).with(Line::Wants { text: "Slack misses you".into(), ws: 2 });
        assert_eq!(spoken(&wants, Style::Card), "Workspace 2 of 4, Slack misses you, On workspace 3, Code");
    }

    // Spec 0023 AC-4: a name and the screen line under it fit beside the number block, with or
    // without marks, so naming a workspace never makes the card jump in height; the Dots style
    // shows no names, so its card is the same size named or not.
    #[test]
    fn a_name_never_changes_the_cards_height() {
        let fonts = Fonts::new(1.0);
        let dc = unsafe { GetDC(None) };
        let c = Canvas::new(dc, false, 1.0, LIGHT, &fonts);
        let named = |c: Card| Card { names: vec!["Mail".into(), "Code".into(), "Research".into(), String::new()], ..c };
        let marked = |c: Card| Card { wants: vec![2], ..c };
        let sent = |c: Card| c.with(Line::Sent { to: 2, unpinned: false });
        let cards = [card(1, true), card(1, false), marked(card(1, true)), sent(card(1, true)), marked(sent(card(1, true)))];
        for plain in cards {
            let (_, h) = draw(&c, 0.0, 0.0, Style::Card, &plain, true);
            assert_eq!(draw(&c, 0.0, 0.0, Style::Card, &named(plain.clone()), true).1, h, "{plain:?}");
            let dots = draw(&c, 0.0, 0.0, Style::Dots, &plain, true);
            assert_eq!(draw(&c, 0.0, 0.0, Style::Dots, &named(plain.clone()), true), dots, "{plain:?}");
        }
        unsafe { ReleaseDC(None, dc) };
    }

    // Spec 0023 AC-6: a card grows to fit a line's name, so that name shows whole at every
    // scale; only a line wider than the card's 380 dip cut is cut, and then only the name.
    #[test]
    fn a_line_shows_the_name_it_was_sized_for() {
        let dc = unsafe { GetDC(None) };
        let names: Vec<String> = vec!["Mail".into(), "Code".into(), "Research".into(), "Quarterly planning and budget review for the board".into()];
        for s in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let fonts = Fonts::new(s);
            let c = Canvas::new(dc, false, s, LIGHT, &fonts);
            for (to, whole) in [(2, true), (3, false)] {
                let card = Card { names: names.clone(), ..card(1, true) }.with(Line::Sent { to, unpinned: false });
                let text_w = draw(&c, 0.0, 0.0, Style::Card, &card, true).0 - P - BLOCK - COL - PR - P;
                let shown = line_name(&c, text_w, "Window sent to ", F::Bold, to, &names);
                assert_eq!(shown == names[to], whole, "{s}: {shown}");
                assert!(shown.ends_with('…') != whole && !shown.is_empty(), "{s}: {shown}");
            }
        }
        unsafe { ReleaseDC(None, dc) };
    }

    // Spec 0039 AC-5: a paused start's note is the one reminder that reaches you at logon, so it
    // must show even with the indicator turned off, and stay long enough to be read unasked; the
    // indicator off still silences ordinary cards, and other notes keep their 3 s.
    #[test]
    fn the_paused_note_shows_with_the_indicator_off_and_stays_6_s() {
        assert!(Line::Paused.shows_when_off());
        assert!(!Line::NotSaved.shows_when_off(), "only attention and the paused note pass the switch");
        let paused = card(1, true).with(Line::Paused);
        assert_eq!(hold_for(&paused.lines, 200), 6000);
        assert_eq!(hold_for(&card(1, true).with(Line::NotSaved).lines, 200), 3000);
        assert_eq!(hold_for(&card(1, true).lines, 200), 200);
        assert_eq!(spoken(&paused, Style::Card), "Workspace 2 of 4, DELL U2720Q, des-k is paused, Resume from the tray icon");
    }

    /// Draws every state the design shows to target/indicator/ PNGs, to compare by eye with
    /// pdesk-design/restart/captures/ir5-* (spec 0017 AC-1).
    #[test]
    #[ignore]
    fn render() {
        ui::canvas::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("indicator");
        std::fs::create_dir_all(&dir).unwrap();
        let dell = || Some("DELL U2720Q".to_string());
        let wants = |c: Card, ws: &[usize]| Card { wants: ws.to_vec(), ..c };
        let sent = |stay| Line::Sent { to: 2, unpinned: stay };
        let gone = |to: Option<&str>| Line::ScreenGone { name: "DELL U2720Q".into(), moved: 5, to: to.map(Into::into) };
        let named = |c: Card| Card { names: vec!["Mail".into(), "Code".into(), "Research".into(), String::new()], ..c };
        let states: Vec<(&str, Card)> = vec![
            ("switch", Card::new(1, 4, dell())),
            ("one-screen", Card::new(0, 4, None)),
            ("last", Card::new(3, 4, dell())),
            ("nine", Card::new(6, 9, dell())),
            ("sent", Card::new(1, 4, dell()).with(sent(false))),
            ("sent-wrap", Card::new(3, 4, dell()).with(Line::Sent { to: 0, unpinned: false })),
            ("sent-pinned", Card::new(1, 4, dell()).with(sent(true))),
            ("sent-back", Card::new(1, 4, dell()).with(Line::SentBack)),
            ("already", Card::new(1, 4, dell()).with(Line::Already)),
            ("pinned", Card::new(1, 4, dell()).with(Line::Pinned)),
            ("unpinned", Card::new(1, 4, dell()).with(Line::Unpinned)),
            ("past-count", Card::new(1, 4, dell()).with(Line::NoWorkspace { n: 7 })),
            ("not-saved", Card::new(1, 4, dell()).with(Line::NotSaved)),
            ("paused", Card::new(1, 4, dell()).with(Line::Paused)),
            ("unplugged", Card::new(0, 4, None).with(gone(None))),
            ("back", Card::new(1, 4, dell()).with(Line::ScreenBack { name: "DELL U2720Q".into(), moved: 5, left: 0, here: true })),
            ("back-some-left", Card::new(1, 4, dell()).with(Line::ScreenBack { name: "DELL U2720Q".into(), moved: 3, left: 2, here: true })),
            (
                "two-unplugged",
                Card::new(0, 4, None).with(gone(None)).with(Line::ScreenGone { name: "LG HDR 4K".into(), moved: 1, to: None }),
            ),
            (
                "mixed",
                Card::new(1, 4, dell())
                    .with(Line::ScreenGone { name: "LG HDR 4K".into(), moved: 1, to: Some("Built-in display".into()) })
                    .with(Line::ScreenBack { name: "DELL U2720Q".into(), moved: 5, left: 0, here: true }),
            ),
            ("long-name", Card::new(2, 6, Some("LG ULTRAGEAR 27GR95QE OLED Gaming Monitor".into())).with(Line::ScreenBack { name: "LG".into(), moved: 1, left: 0, here: true })),
            (
                "long-line",
                Card::new(0, 4, Some("Built-in display".into())).with(Line::ScreenGone { name: "LG ULTRAGEAR 27GR95QE OLED Gaming Monitor".into(), moved: 2, to: None }),
            ),
            // Spec 0022: the popup, and the marks every later card carries.
            ("wants", wants(Card::new(0, 4, dell()), &[2]).with(Line::Wants { text: "Slack misses you".into(), ws: 2 })),
            ("wants-one-screen", wants(Card::new(0, 4, None), &[2]).with(Line::Wants { text: "Slack misses you".into(), ws: 2 })),
            ("wants-two", wants(Card::new(0, 4, dell()), &[1, 3]).with(Line::Wants { text: "Outlook misses you".into(), ws: 3 })),
            (
                "wants-long",
                wants(Card::new(1, 6, dell()), &[4]).with(Line::Wants { text: "Microsoft Teams (work or school) wants you back on that call right now".into(), ws: 4 }),
            ),
            ("marked-switch", wants(Card::new(1, 4, dell()), &[2])),
            // Spec 0038 AC-12: a follow's switch says why.
            ("followed", Card::new(2, 4, dell()).with(Line::Followed { app: "Slack".into() })),
            ("marked-one-screen", wants(Card::new(0, 4, None), &[2, 3])),
            ("marked-sent", wants(Card::new(1, 4, dell()), &[2]).with(sent(false))),
            ("marked-nine", wants(Card::new(6, 9, dell()), &[0, 4, 8])),
            // Spec 0023: the handoff's scenes i1 to i13 (names-*); every state above shows the
            // bigger line chip with no names (i5, i6b), and the dots draw these with no names.
            ("names", named(Card::new(1, 4, dell()))),
            ("names-one-screen", named(Card::new(0, 4, None))),
            ("names-unnamed", named(Card::new(3, 4, dell()))),
            ("names-sent", named(Card::new(1, 4, dell())).with(sent(false))),
            ("names-sent-unnamed", named(Card::new(1, 4, dell())).with(Line::Sent { to: 3, unpinned: false })),
            ("names-wants", wants(named(Card::new(0, 4, dell())), &[2]).with(Line::Wants { text: "Slack misses you".into(), ws: 2 })),
            ("names-wants-unnamed", wants(named(Card::new(0, 4, dell())), &[3]).with(Line::Wants { text: "Slack misses you".into(), ws: 3 })),
            (
                "names-wants-long",
                Card { names: vec!["Mail".into(), String::new(), String::new(), "Quarterly planning and budget review for the board".into()], ..wants(Card::new(0, 4, dell()), &[3]) }
                    .with(Line::Wants { text: "Outlook misses you".into(), ws: 3 }),
            ),
            (
                "names-long",
                Card {
                    names: vec![String::new(), "Quarterly planning and budget 26".into(), String::new(), "Reading list and papers to file".into()],
                    ..Card::new(1, 4, Some("Studio monitor above the desk 32".into()))
                }
                .with(Line::Sent { to: 3, unpinned: false }),
            ),
            ("names-renamed", named(Card::new(1, 4, Some("Left".into())))),
            ("names-marked-tight", wants(Card { names: vec!["Mail".into(), "Code".into()], ..Card::new(1, 6, dell()) }, &[0, 4])),
            (
                "names-overflow",
                Card { names: vec![String::new(), String::new(), "第三四半期の計画と予算レビュー会議資料まとめ".into()], ..Card::new(2, 4, Some("左側の壁掛けモニター（会議室用の大型ディスプレイ）".into())) },
            ),
            ("names-note-renamed", named(Card::new(0, 4, Some("Built-in display".into()))).with(Line::ScreenGone { name: "Left".into(), moved: 3, to: None })),
            (
                "names-rtl",
                Card { names: vec!["מסמכים".into(), String::new(), String::new(), "البريد الوارد".into()], ..Card::new(3, 4, dell()) }.with(Line::Sent { to: 0, unpinned: false }),
            ),
            // Spec 0027 AC-5: a rule hid a new window; its chip shows the workspace's name, else its number.
            ("opened", Card::new(1, 4, dell()).with(Line::Opened { app: "Notepad".into(), to: 2 })),
            ("names-opened", named(Card::new(1, 4, dell())).with(Line::Opened { app: "Slack".into(), to: 0 })),
            ("opened-long", named(Card::new(1, 4, dell())).with(Line::Opened { app: "Microsoft Teams (work or school)".into(), to: 2 })),
        ];
        let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
        let (rose, rose_dark) = (ui::tests::rose(true), ui::tests::rose(false));
        let themes: [(&str, Theme, f32); 7] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5), ("rose", rose, 1.0), ("rose-dark", rose_dark, 1.0)];
        let backdrop = |t: &Theme| if t.hc { t.b.bg } else if t.dark { 0x171C28 } else { 0xDDE5F3 };
        for (tname, t, s) in themes {
            let fonts = Fonts::new(s);
            for style in [Style::Card, Style::Dots] {
                let sname = if style == Style::Card { "card" } else { "dots" };
                for (name, card) in &states {
                    let card = &styled(card.clone(), style);
                    let (w, h) = unsafe {
                        let dc = GetDC(None);
                        let size = draw(&Canvas::new(dc, false, s, t, &fonts), 0.0, 0.0, style, card, true);
                        ReleaseDC(None, dc);
                        size
                    };
                    let (pw, ph) = (((w + 48.0) * s).ceil() as i32, ((h + 48.0) * s).ceil() as i32);
                    let path = dir.join(format!("{sname}-{name}-{tname}.png"));
                    ui::tests::save_png(pw, ph, &path, |dc| unsafe {
                        let bg = CreateSolidBrush(ui::color::cref(backdrop(&t)));
                        FillRect(dc, &RECT { left: 0, top: 0, right: pw, bottom: ph }, bg);
                        let _ = DeleteObject(bg.into());
                        draw_preview(&Canvas::new(dc, true, s, t, &fonts), 24.0, 24.0, style, card);
                    });
                }
            }
        }
    }
}
