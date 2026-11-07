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
use crate::ui::{self, Canvas, F, Theme, mix, op};

/// Also how `--quit` and the single-instance check find the running des-k.
pub const CLASS: windows::core::PCWSTR = w!("des-k-indicator");
/// Posted to the main loop on WM_DISPLAYCHANGE (handled there, never inside a sent message).
pub const WM_DISPLAY_CHANGED: u32 = WM_APP + 1;

const FADE_TIMER: usize = 1;
const FADE_STEP_MS: u32 = 16;
const LINE_MS: u32 = 1500; // a card with a line stays at least this long, so it can be read
const NOTE_MS: u32 = 3000; // a screen unplugged or back

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
}

impl Card {
    pub fn new(active: usize, count: usize, screen: Option<String>) -> Card {
        Card { active, count, screen, screen_id: String::new(), lines: vec![] }
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
}

impl Line {
    fn note(&self) -> bool {
        matches!(self, Line::ScreenGone { .. } | Line::ScreenBack { .. })
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
}

fn windows(n: usize) -> String {
    if n == 1 { "1 window".into() } else { format!("{n} windows") }
}

/// A line's icon (a Segoe Fluent Icons code), whether it warns, its title with the workspace
/// chip that ends it, if any, and its grey second line.
struct Words {
    icon: char,
    warn: bool,
    title: String,
    chip: Option<usize>,
    sub: String,
}

fn words(card: &Card, line: &Line, named: bool) -> Words {
    let on = card.active + 1;
    let w = |icon: char, title: String, sub: String| Words { icon, warn: false, title, chip: None, sub };
    match line {
        Line::Sent { to, unpinned } => Words {
            icon: '\u{E724}',
            warn: false,
            title: "Window sent to ".into(),
            chip: Some(*to),
            sub: if *unpinned { format!("Unpinned, you stay on {on}") } else { format!("You stay on {on}") },
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
            Words { icon: if *left > 0 { '\u{E7BA}' } else { '\u{E7F4}' }, warn: *left > 0, title, chip: None, sub }
        }
    }
}

/// What Narrator reads: the painted words, in order.
fn spoken(card: &Card, style: Style) -> String {
    let named = style == Style::Card && card.screen.is_some();
    let mut s = format!("Workspace {} of {}", card.active + 1, card.count);
    if let (true, Some(name)) = (named, &card.screen) {
        s += &format!(", {name}");
    }
    for line in &card.lines {
        let w = words(card, line, named);
        s += &format!(", {}{}", w.title, w.chip.map_or(String::new(), |n| (n + 1).to_string()));
        if !w.sub.is_empty() {
            s += &format!(", {}", w.sub);
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

/// The chip a line's title ends with: its width.
fn chip_w(c: &Canvas, n: usize) -> f32 {
    (c.width(&(n + 1).to_string(), F::SmallBold) + 10.0).max(20.0)
}

/// A line's widest unwrapped run.
fn natural(c: &Canvas, w: &Words) -> f32 {
    let title = c.width(&w.title, F::Bold) + w.chip.map_or(0.0, |n| 2.0 + chip_w(c, n));
    title.max(c.width(&w.sub, F::Small))
}

/// The lines under the rule from `y`: icons centered in a column `icon_w` wide at `x`, the text
/// `gap` to its right and `text_w` wide. Returns their height.
fn lines(c: &Canvas, x: f32, y: f32, icon_w: f32, gap: f32, text_w: f32, all: &[Words]) -> f32 {
    let t = &c.t.b;
    let mut h = 0.0;
    for (i, w) in all.iter().enumerate() {
        if i > 0 {
            h += 8.0;
        }
        let (top, xt) = (y + h, x + icon_w + gap);
        let glyph = w.icon.to_string();
        c.text(x + (icon_w - c.width(&glyph, F::Icon)) / 2.0, top, 20.0, &glyph, F::Icon, if w.warn { t.warn } else { t.tx2 });
        let title_h = match w.chip {
            Some(n) => {
                c.text(xt, top, 20.0, &w.title, F::Bold, t.tx);
                let (cx, cw) = (xt + c.width(&w.title, F::Bold) + 2.0, chip_w(c, n));
                let color = t.ws[n % 9];
                let edge = (t.ws_edge > 0.0).then(|| mix(color, 0, t.ws_edge));
                c.boxed((cx, top + 1.0, cw, 18.0), 4.0, color, edge);
                let num = (n + 1).to_string();
                c.text(cx + (cw - c.width(&num, F::SmallBold)) / 2.0, top + 1.0, 18.0, &num, F::SmallBold, t.on_ws[n % 9]);
                20.0
            }
            None => c.para(xt, top, text_w, 20.0, &[(&w.title, F::Bold, t.tx)]),
        };
        let sub_h = if w.sub.is_empty() { 0.0 } else { c.para(xt, top + title_h, text_w, 16.0, &[(&w.sub, F::Small, t.tx2)]) };
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
    let target = card.lines.iter().find_map(|l| if let Line::Sent { to, .. } = l { Some(*to) } else { None });
    let lit = |i: usize| i == card.active || pinned || target == Some(i);
    let text_nat = all.iter().map(|w| natural(c, w)).fold(0.0, f32::max);
    let rule_px = c.s.floor().max(1.0) / c.s;

    // Card: block, name and bars; lines in the block's and the name's columns.
    // Dots: a pill; lines in a 16 wide icon column.
    let (w, pad_x, icon_w, gap, rows_top) = match style {
        Style::Card => {
            let bars: f32 = (0..card.count).map(|i| if i == card.active { 28.0 } else { 14.0 }).sum::<f32>() + 6.0 * card.count.saturating_sub(1) as f32;
            let name = card.screen.as_deref().filter(|_| named).map_or(0.0, |n| c.width(n, F::Bold));
            (((P + BLOCK + COL + bars.max(name).max(text_nat) + PR + P).ceil()).min(MAX_W), P, BLOCK, COL, P + BLOCK)
        }
        Style::Dots => {
            let dots: f32 = (0..card.count).map(|i| if i == card.active { 24.0 } else { 8.0 }).sum::<f32>() + 8.0 * card.count.saturating_sub(1) as f32;
            let pad = if all.is_empty() { 16.0 } else { 12.0 };
            ((pad + dots.max(if all.is_empty() { 0.0 } else { 26.0 + text_nat }) + pad).ceil().min(MAX_W), pad, 16.0, 10.0, 12.0 + 8.0)
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
    let lines_h = if all.is_empty() { 0.0 } else { rule_gap + rule_px + after + lines(&c.measuring(), 0.0, 0.0, icon_w, gap, text_w, &all) };
    let bottom = match (style, all.is_empty()) {
        (Style::Card, _) => P,
        (Style::Dots, true) => 12.0,
        (Style::Dots, false) => 10.0,
    };
    let h = rows_top + lines_h + bottom;
    if !c.draw {
        return (w, h);
    }

    let radius = if style == Style::Dots && all.is_empty() { 16.0 } else { 8.0 };
    c.boxed((x, y, w, h), radius, t.card, edge.then_some(t.edge));
    match style {
        Style::Card => {
            let color = t.ws[card.active % 9];
            let ring = (t.ws_edge > 0.0).then(|| mix(color, 0, t.ws_edge));
            c.boxed((x + P, y + P, BLOCK, BLOCK), 4.0, color, ring);
            let num = (card.active + 1).to_string();
            c.text(x + P + (BLOCK - c.width(&num, F::Number)) / 2.0, y + P, BLOCK, &num, F::Number, t.on_ws[card.active % 9]);
            let side = x + P + BLOCK + COL;
            let side_h = if named { 20.0 + 10.0 + 12.0 } else { 12.0 };
            let mut row = y + P + (BLOCK - side_h) / 2.0;
            if let (true, Some(name)) = (named, &card.screen) {
                c.text(side, row, 20.0, &c.fit(name, F::Bold, text_w), F::Bold, t.tx);
                row += 30.0;
            }
            let mut bx = side;
            for i in 0..card.count {
                let (bw, bh) = if i == card.active { (28.0, 8.0) } else if lit(i) { (14.0, 6.0) } else { (14.0, 4.0) };
                mark(c, (bx, row + (12.0 - bh) / 2.0, bw, bh), bh / 2.0, i, lit(i), target == Some(i));
                bx += bw + 6.0;
            }
        }
        Style::Dots => {
            let (mut dx, dy) = (x + pad_x, y + 12.0);
            for i in 0..card.count {
                let dw = if i == card.active { 24.0 } else { 8.0 };
                mark(c, (dx, dy, dw, 8.0), 4.0, i, lit(i), target == Some(i));
                dx += dw + 8.0;
            }
        }
    }
    if !all.is_empty() {
        let ry = y + rows_top + rule_gap;
        c.fill_rr((x + pad_x, ry, w - 2.0 * pad_x, rule_px), 0.0, op(t.line));
        lines(c, x + pad_x, ry + rule_px + after, icon_w, gap, text_w, &all);
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
                    ui::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
                        let bg = CreateSolidBrush(ui::cref(s.theme.b.card));
                        FillRect(mem, &rc, bg);
                        let _ = DeleteObject(bg.into());
                        let fonts = ui::fonts(s.scale);
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
                crate::ws::woke();
                LRESULT(1)
            }
            WM_DISPLAYCHANGE => {
                let _ = PostMessageW(Some(hwnd), WM_DISPLAY_CHANGED, WPARAM(0), LPARAM(0));
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
    ui::gdiplus();
    // No-activate and see-through to the mouse: never steals focus, never takes a click.
    let h = ui::popup(CLASS, Some(wndproc), WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT, true)?;
    // An admin des-k would drop `des-k --quit` from a normal terminal otherwise (spec 0008).
    if let Err(e) = unsafe { ChangeWindowMessageFilterEx(h, WM_CLOSE, MSGFLT_ALLOW, None) } {
        warn!("indicator: WM_CLOSE not let in from normal programs: {e}");
    }
    WND.set(Some(h));
    Some(h)
}

/// Show `card` centered on `mon`'s work area, replacing whatever the indicator shows.
pub fn show(mon: HMONITOR, card: Card) {
    let st = settings::get();
    if !st.indicator_enabled {
        return;
    }
    let Some(h) = window() else { return };
    let style = st.indicator_style;
    let theme = ui::system_theme();
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy); }
    let scale = dx as f32 / 96.0;
    let fonts = ui::fonts(scale);
    let (w, ht) = unsafe {
        let dc = GetDC(None);
        let size = draw(&Canvas::new(dc, false, scale, theme, &fonts), 0.0, 0.0, style, &card, false);
        ReleaseDC(None, dc);
        size
    };
    let (w, ht) = ((w * scale).ceil() as i32, (ht * scale).ceil() as i32);

    let hold = if card.lines.iter().any(Line::note) {
        NOTE_MS
    } else if !card.lines.is_empty() {
        st.hold_ms.max(LINE_MS)
    } else {
        st.hold_ms
    };
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
        let _ = InvalidateRect(Some(h), None, false);
        SetTimer(Some(h), FADE_TIMER, hold, None);
    }
    crate::uia::notify(h, &text, "des-k indicator", false);
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
    use crate::ui::{DARK, Fonts, LIGHT, Theme};

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
        assert!(words(&c, &back, true).warn);
        let none = Line::ScreenBack { name: "X".into(), moved: 0, left: 2, here: true };
        assert_eq!(words(&c, &none, true).sub, "2 windows could not be moved");
        let gone = Line::ScreenGone { name: "LG HDR 4K".into(), moved: 1, to: Some("Built-in display".into()) };
        assert_eq!(words(&c, &gone, true).sub, "1 window moved to Built-in display");
    }

    #[test]
    fn narrator_reads_the_painted_words() {
        let c = card(2, true).with(Line::Sent { to: 1, unpinned: false });
        assert_eq!(spoken(&c, Style::Card), "Workspace 3 of 4, DELL U2720Q, Window sent to 2, You stay on 3");
        assert_eq!(spoken(&card(0, true), Style::Dots), "Workspace 1 of 4");
    }

    /// Draws every state the design shows to target/indicator/ PNGs, to compare by eye with
    /// pdesk-design/restart/captures/ir5-* (spec 0017 AC-1).
    #[test]
    #[ignore]
    fn render() {
        ui::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("indicator");
        std::fs::create_dir_all(&dir).unwrap();
        let dell = || Some("DELL U2720Q".to_string());
        let sent = |stay| Line::Sent { to: 2, unpinned: stay };
        let gone = |to: Option<&str>| Line::ScreenGone { name: "DELL U2720Q".into(), moved: 5, to: to.map(Into::into) };
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
                    let (w, h) = unsafe {
                        let dc = GetDC(None);
                        let size = draw(&Canvas::new(dc, false, s, t, &fonts), 0.0, 0.0, style, card, true);
                        ReleaseDC(None, dc);
                        size
                    };
                    let (pw, ph) = (((w + 48.0) * s).ceil() as i32, ((h + 48.0) * s).ceil() as i32);
                    let path = dir.join(format!("{sname}-{name}-{tname}.png"));
                    ui::tests::save_png(pw, ph, &path, |dc| unsafe {
                        let bg = CreateSolidBrush(ui::cref(backdrop(&t)));
                        FillRect(dc, &RECT { left: 0, top: 0, right: pw, bottom: ph }, bg);
                        let _ = DeleteObject(bg.into());
                        draw_preview(&Canvas::new(dc, true, s, t, &fonts), 24.0, 24.0, style, card);
                    });
                }
            }
        }
    }
}
