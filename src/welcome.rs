// The welcome screen (spec 0011, 0011-first-run-setup, Welcome screen): what des-k is, Windows'
// three and four finger gestures row by row with what des-k does for each, and the Use, Choose
// my own and Give back buttons, built to the approved H2 design
// (pdesk-design/welcome/micro-suite-r4.html) plus its 2026-11-01 change. Lives on the main thread: its
// messages come through main's loop, which passes them through IsDialogMessageW so Tab,
// Enter and Esc work. GDI draws the text, GDI+ the smooth shapes, into a back buffer, through
// the shared drawing layer in `ui`. Sizes are in dip (1/96 inch) and scaled to the screen's dpi
// when drawn.
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::{EnableWindow, GetActiveWindow, GetFocus, IsWindowEnabled, SetFocus, VIRTUAL_KEY, VK_NEXT, VK_PRIOR},
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::settings::Motion;
use crate::setup::{self, GaveBack, Row, State, Wrote};
use crate::ui::{self, Bx, Canvas, F, Fonts, Theme, alpha, buffered, dark_title, gdiplus, mix, op, theme};

const CLASS: PCWSTR = w!("des-k-welcome");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_CLIPCHILDREN.0);

// The buttons, created in Tab order. Close is IDCANCEL, so Esc presses it.
const ID_CARD: i32 = 101; // Use / Open Touchpad settings / Try giving back again
const ID_CARD2: i32 = 102; // Open Touchpad settings, beside the disabled Use
const ID_OWN: i32 = 103;
const ID_GIVE: i32 = 104;
const ID_CLOSE: i32 = IDCANCEL.0;
const IDS: [i32; 5] = [ID_CARD, ID_CARD2, ID_OWN, ID_GIVE, ID_CLOSE];

const USE: &str = "Set up in Windows Settings";
const OPEN: &str = "Open Touchpad settings";
const AGAIN: &str = "Try giving back again";
const GIVE: &str = "Give gestures back to Windows";
const OWN: &str = "Choose my own";

// Layout, in dip.
const W: f32 = 480.0;
const SIDE: f32 = 24.0;
const IN: f32 = W - 2.0 * SIDE;
const BAND: f32 = 112.0;
const CARD_IN: f32 = IN - 2.0 * 15.0; // inside the card's border and 14 dip padding
const RING: f32 = 4.0; // a button's window reaches this far past its pill, for the focus ring
const MIN_SHOWN: f32 = 120.0; // the least of the middle a short window shows

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
    /// The last give back's result: kept until des-k exits, a later give back replaces it, or
    /// Windows' gestures read off again, so reopening the screen shows the same rows.
    static LAST: Cell<Option<GaveBack>> = const { Cell::new(None) };
}

/// What the screen shows.
#[derive(Clone, Copy, PartialEq)]
struct View {
    st: State,
    first: bool, // opened at the first start: Skip, not Close
    back: Option<GaveBack>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Primary,
    Quiet,
}

/// A button as the layout placed it: its pill, in dip.
#[derive(Clone, Copy)]
struct Place {
    id: i32,
    kind: Kind,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    back: u32, // the color behind it
    label: &'static str,
    enabled: bool,
}

/// How the screen sits in its window. Short: the whole screen is taller than its screen's work
/// area, so the window is capped and the middle (intro to card) scrolls.
#[derive(Clone, Copy, Default, PartialEq)]
struct Fit {
    height: f32, // the client height
    short: bool,
    content: f32, // the middle's height, when short
    shown: f32,   // how much of the middle shows, when short
}

struct Win {
    hwnd: HWND,
    first: bool,
    dpi: u32,
    scale: f32,
    theme: Theme,
    fonts: Fonts,
    view: View,
    places: Vec<Place>,
    buttons: Vec<(i32, HWND)>,
    focus: Option<HWND>, // the focused button while the window is not active
    fit: Fit,
    scroll: f32,
}

// Never hold the borrow across a call that sends this window a message: a button redraws
// itself at once on focus, enable and text changes, and that comes back as WM_DRAWITEM.
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    ui::get(&WIN, "welcome get", f)
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    ui::set(&WIN, "welcome set", f)
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
}

/// One layout pass: the canvas, and the buttons placed on it so far.
struct Pass<'a> {
    c: Canvas<'a>,
    places: Vec<Place>,
}

impl<'a> Deref for Pass<'a> {
    type Target = Canvas<'a>;
    fn deref(&self) -> &Canvas<'a> {
        &self.c
    }
}

impl DerefMut for Pass<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.c
    }
}

impl Pass<'_> {
    fn place(&mut self, p: Place) {
        match self.places.iter_mut().find(|q| q.id == p.id) {
            Some(q) => *q = p,
            None => self.places.push(p),
        }
    }

    /// A pill button's width for its label.
    fn pill_w(&self, label: &str, kind: Kind) -> f32 {
        34.0 + self.width(label, if kind == Kind::Primary { F::Bold } else { F::Body })
    }
}

/// Lays `f` out without drawing it, for a box that needs its height before its contents.
fn measure(c: &mut Pass, f: impl FnOnce(&mut Pass) -> f32) -> f32 {
    let draw = std::mem::replace(&mut c.draw, false);
    let h = f(c);
    c.draw = draw;
    h
}

/// The whole screen, back to front. With `short` (the client height when the whole screen
/// doesn't fit its screen), the middle scrolls by `scroll` under the band, and the card's
/// buttons move to a strip pinned above the footer, so every button stays in reach.
fn compose(c: &mut Pass, v: &View, short: Option<f32>, scroll: f32) -> Fit {
    let Some(height) = short else {
        let y = BAND + middle(c, v, BAND, true);
        band(c);
        let height = y + footer(c, v, y);
        return Fit { height, ..Fit::default() };
    };
    let pinned = measure(c, |c| strip(c, v, 0.0) + footer(c, v, 0.0));
    let height = height.max(BAND + pinned + MIN_SHOWN);
    let shown = height - BAND - pinned;
    let content = measure(c, |c| middle(c, v, BAND, false));
    let scroll = scroll.clamp(0.0, (content - shown).max(0.0));
    middle(c, v, BAND - scroll, false);
    band(c);
    let y = height - pinned;
    let y = y + strip(c, v, y);
    footer(c, v, y);
    // The scroll thumb, like the settings page's.
    if content > shown + 0.5 {
        let th = (shown * shown / content).max(32.0);
        c.fill_rr((W - 11.0, BAND + scroll / (content - shown) * (shown - th), 5.0, th), 2.5, alpha(c.t.dim, 0.5));
    }
    Fit { height, short: true, content, shown }
}

/// The band at the top: the title, the line under it and the art.
fn band(c: &mut Pass) {
    let t = c.t;
    if t.hc {
        c.boxed((0.0, 0.0, W, BAND), 0.0, t.surface, None);
        c.boxed((0.0, BAND - 1.0, W, 1.0), 0.0, t.stroke, None);
    } else {
        c.gradient(0.0, 0.0, W, BAND, t.band_a, t.band_b);
    }
    c.text(SIDE, 14.0, 36.0, "Welcome to des-k!", F::H1, t.on_band);
    c.text(SIDE, 52.0, 20.0, "Every screen gets its own desktops.", F::Body, t.on_band);
    art(c, W - 18.0 - 172.0, BAND - 8.0 - 62.0, t.on_band);
}

/// The intro, tiles, heading and card from `top`; `inline`: the card holds its buttons.
/// Returns its height.
fn middle(c: &mut Pass, v: &View, top: f32, inline: bool) -> f32 {
    let t = c.t;
    let mut y = top + 16.0;
    let intro = "des-k gives each of your screens its own set of workspaces. You switch the screen under your mouse, and your other screens stay where they are. des-k never moves or resizes your windows. It only hides and shows them.";
    y += c.para(SIDE, y, IN, 20.0, &[(intro, F::Body, t.text)]) + 16.0;
    tiles(c, y);
    y += 56.0 + 8.0;
    // Before any shortcut surprises you: which of Windows' own des-k takes over.
    let takes = [
        ("These keys, and Win+1 … 9 with or without Shift or Ctrl, take over Windows' own: its desktops, Task View, Feedback Hub and the taskbar's apps. Change any of them in des-k's settings, under ", F::Small, t.dim),
        ("Shortcuts", F::SmallBold, t.dim),
        (".", F::Small, t.dim),
    ];
    y += c.para(SIDE, y, IN, 16.0, &takes) + 16.0;
    // The heading, with the count at its right: Windows' own values, in every state.
    pad_icon(c, SIDE, y + 1.0, op(t.text));
    c.text(SIDE + 28.0, y, 22.0, "Your touchpad gestures", F::H2, t.text);
    let n = v.st.rows.iter().filter(|r| **r == Row::Nothing).count();
    let count = format!("Windows: {n} of 4 off");
    let cw = c.width(&count, F::SmallBold);
    c.text(SIDE + IN - cw, y + 3.0, 16.0, &count, F::SmallBold, t.dim);
    y += 22.0 + 8.0;
    // The card: border 1, padding 12 on top and 14 below.
    let h = measure(c, |c| card(c, v, SIDE + 15.0, y + 13.0, inline)) + 28.0;
    c.boxed((SIDE, y, IN, h), 12.0, t.card, Some(t.card_edge));
    card(c, v, SIDE + 15.0, y + 13.0, inline);
    y + h + 24.0 - top
}

/// The card's buttons, pinned above the footer while the middle scrolls. Returns its height.
fn strip(c: &mut Pass, v: &View, y: f32) -> f32 {
    let t = c.t;
    let h = measure(c, |c| buttons(c, v, SIDE, y + 12.0, IN, t.surface)) + 24.0;
    c.boxed((0.0, y, W, h), 0.0, t.surface, None);
    c.boxed((0.0, y, W, 1.0), 0.0, t.card_edge, None);
    buttons(c, v, SIDE, y + 12.0, IN, t.surface);
    h
}

/// The footer from `y`: border 1 on top, padding 14. Returns its height.
fn footer(c: &mut Pass, v: &View, y: f32) -> f32 {
    let t = c.t;
    let fh = measure(c, |c| foot(c, v, y + 15.0)) + 29.0;
    c.boxed((0.0, y, W, fh), 0.0, t.foot, None);
    c.boxed((0.0, y, W, 1.0), 0.0, t.card_edge, None);
    foot(c, v, y + 15.0);
    fh
}

/// Three screens, each on its own workspace: the indicator's dots, the current one bright.
fn art(c: &Canvas, x: f32, y: f32, col: u32) {
    for (i, (dx, dy)) in [(0.0, 6.0), (58.0, 0.0), (116.0, 6.0)].into_iter().enumerate() {
        let (x, y) = (x + dx, y + dy);
        c.fill_rr((x + 1.0, y + 1.0, 54.0, 36.0), 5.0, alpha(col, 0.16));
        c.stroke_rr((x + 1.0, y + 1.0, 54.0, 36.0), 5.0, 1.5, alpha(col, 0.75));
        c.fill_rr((x + 8.0, y + 8.0, 24.0, 14.0), 2.0, alpha(col, 0.55));
        c.fill_rr((x + 20.0, y + 15.0, 28.0, 15.0), 2.0, alpha(col, 0.8));
        c.lines(&[(x + 22.0, y + 37.0), (x + 22.0, y + 43.0)], 1.5, alpha(col, 0.75));
        c.lines(&[(x + 34.0, y + 37.0), (x + 34.0, y + 43.0)], 1.5, alpha(col, 0.75));
        c.lines(&[(x + 16.0, y + 44.0), (x + 40.0, y + 44.0)], 1.5, alpha(col, 0.75));
        for j in 0..3 {
            let (r, a) = if j == i { (3.5, op(col)) } else { (2.5, alpha(col, 0.45)) };
            c.circle(x + 18.0 + j as f32 * 10.0, y + 50.0, r, Some(a), None);
        }
    }
}

fn tiles(c: &Canvas, y: f32) {
    let t = c.t;
    let tw = (IN - 16.0) / 3.0;
    for (i, (what, keys)) in [("Switch", "Win Ctrl ← →"), ("See all", "Win Tab"), ("Find", "Win F")].into_iter().enumerate() {
        let x = SIDE + i as f32 * (tw + 8.0);
        c.boxed((x, y, tw, 56.0), 12.0, t.tile, Some(t.card_edge));
        let (cx, cy) = (x + 25.0, y + 28.0);
        c.circle(cx, cy, 14.0, Some(op(t.tile_ico)), None);
        if t.hc {
            c.circle(cx, cy, 13.5, None, Some((op(t.stroke), 1.0)));
        }
        tile_icon(c, i, cx - 8.0, cy - 8.0, op(t.tile_ico_fg));
        c.text(x + 47.0, y + 9.0, 20.0, what, F::Bold, t.text);
        c.text(x + 47.0, y + 31.0, 16.0, keys, F::SmallBold, t.text);
    }
}

/// The tiles' 16 dip icons: switch, see all, find.
fn tile_icon(c: &Canvas, i: usize, x: f32, y: f32, argb: u32) {
    match i {
        0 => {
            c.lines(&[(x + 2.0, y + 5.0), (x + 12.0, y + 5.0), (x + 9.0, y + 2.0)], 1.5, argb);
            c.lines(&[(x + 14.0, y + 11.0), (x + 4.0, y + 11.0), (x + 7.0, y + 14.0)], 1.5, argb);
        }
        1 => {
            for (dx, dy) in [(1.75, 2.75), (8.75, 2.75), (1.75, 8.75), (8.75, 8.75)] {
                c.stroke_rr((x + dx, y + dy, 5.5, 4.5), 1.0, 1.5, argb);
            }
        }
        _ => {
            c.circle(x + 7.0, y + 7.0, 4.25, None, Some((argb, 1.5)));
            c.lines(&[(x + 10.25, y + 10.25), (x + 14.0, y + 14.0)], 1.5, argb);
        }
    }
}

/// The heading's 20 dip touchpad.
fn pad_icon(c: &Canvas, x: f32, y: f32, argb: u32) {
    c.stroke_rr((x + 2.75, y + 4.75, 14.5, 10.5), 2.5, 1.5, argb);
    for dx in [7.0, 10.0, 13.0] {
        c.circle(x + dx, y + 10.0, 1.0, Some(argb), None);
    }
}

/// The card's primary button (label, enabled), and whether Open Touchpad settings shows
/// beside it.
fn actions(v: &View) -> (Option<(&'static str, bool)>, bool) {
    let st = &v.st;
    if !st.enabled {
        // Reading off: Use can't do anything, so it isn't there; Choose my own turns reading on.
        return (None, st.off);
    }
    let primary = match v.back {
        Some(b) if b.failed() => Some((AGAIN, true)),
        _ if !st.pad => Some((USE, false)), // the No touchpad banner above says why
        _ if st.rows.iter().all(|r| *r == Row::Nothing) => None,
        None if st.saved => Some((OPEN, true)),
        _ => Some((USE, true)),
    };
    (primary, false)
}

/// The card's buttons from x, y on one line, Choose my own wrapping below when it doesn't fit
/// in `w`. `back` is the color behind them. Returns their height.
fn buttons(c: &mut Pass, v: &View, x: f32, y: f32, w: f32, back: u32) -> f32 {
    let (primary, open) = actions(v);
    let (mut bx, mut by) = (x, y);
    if let Some((label, enabled)) = primary {
        let pw = c.pill_w(label, Kind::Primary);
        c.place(Place { id: ID_CARD, kind: Kind::Primary, x: bx, y: by, w: pw, h: 32.0, back, label, enabled });
        bx += pw + 8.0;
    }
    if open {
        let pw = c.pill_w(OPEN, Kind::Quiet);
        c.place(Place { id: ID_CARD2, kind: Kind::Quiet, x: bx, y: by, w: pw, h: 32.0, back, label: OPEN, enabled: true });
        bx += pw + 8.0;
    }
    let pw = c.pill_w(OWN, Kind::Quiet);
    if bx + pw > x + w {
        (bx, by) = (x, by + 32.0 + 8.0);
    }
    c.place(Place { id: ID_OWN, kind: Kind::Quiet, x: bx, y: by, w: pw, h: 32.0, back, label: OWN, enabled: true });
    by + 32.0 - y
}

/// The card's contents at x, top; `inline`: with its buttons at the end. Returns their height.
fn card(c: &mut Pass, v: &View, x: f32, top: f32, inline: bool) -> f32 {
    let t = c.t;
    let st = &v.st;
    let all = st.rows.iter().all(|r| *r == Row::Nothing);
    let mut y = top;
    // Reading off wins over no touchpad. The rows show in every state, captioned with what
    // des-k does with them right now.
    if !st.enabled {
        let sub = "Windows' three and four finger gestures are set to Nothing, so right now they do nothing. Turn reading back on with Choose my own, or set them back in Windows Settings.";
        let sub = st.off.then_some(sub);
        y += banner(c, x, y, CARD_IN, st.off, "Touchpad reading is off in des-k's settings.", sub.as_slice());
    } else {
        match v.back {
            Some(b) => {
                let again = "Your saved values are kept, so you can try again. Settings is open if you want gestures now.";
                let (warn, title, subs) = if !b.failed() {
                    let mut subs = vec!["To have them now, set them in Settings (it's open). des-k has stopped reading gestures.".to_string()];
                    if !b.cleared {
                        subs.push("des-k couldn't clear its copy of your values. The log says why (tray icon › Open log). Press Give gestures back again to clear it.".into());
                    }
                    (false, "Your gestures come back at your next sign in.", subs)
                } else if b.wrote.iter().all(|w| *w == Wrote::NotOne) {
                    (true, "Giving back didn't work.", vec![format!("Nothing was written back. The log says why (tray icon › Open log). {again}")])
                } else {
                    let sub = format!("Some values were written back for your next sign in, some weren't. The log names them (tray icon › Open log). {again}");
                    (true, "Giving back stopped part way.", vec![sub])
                };
                let subs: Vec<&str> = subs.iter().map(String::as_str).collect();
                y += banner(c, x, y, CARD_IN, warn, title, &subs);
            }
            None if !st.pad => {
                let sub = ["Plug one in and this updates by itself. Your keys work as before."];
                y += banner(c, x, y, CARD_IN, false, "No precision touchpad found.", &sub);
            }
            None if all => y += ok_block(c, x, y),
            None => {
                y += match st.rows.iter().position(|r| *r == Row::CantRead) {
                    Some(i) => {
                        let text = format!("des-k can't read the {} setting, so it leaves every gesture to Windows. The log has the details (tray icon › Open log).", setup::ROWS[i].0);
                        c.para(x, y, CARD_IN, 20.0, &[(&text, F::Body, t.text)])
                    }
                    None => c.para(
                        x,
                        y,
                        CARD_IN,
                        20.0,
                        &[
                            ("In ", F::Body, t.text),
                            ("Settings › Touchpad", F::Bold, t.text),
                            (", open ", F::Body, t.text),
                            ("Three-finger gestures", F::Bold, t.text),
                            (" and ", F::Body, t.text),
                            ("Four-finger gestures", F::Bold, t.text),
                            (" and set Swipes and Taps to ", F::Body, t.text),
                            ("Nothing", F::Bold, t.text),
                            (". Then come back here.", F::Body, t.text),
                        ],
                    ),
                };
            }
        }
    }
    // The caption above the rows says which part is des-k's, and whether it runs it now.
    let caption = if !st.enabled {
        "Under each gesture: what des-k would run. Touchpad reading is off, so it runs none of them."
    } else if v.back.is_some() {
        "Under each gesture: what des-k would run. It stopped reading gestures when you gave them back."
    } else {
        "Under each gesture: what des-k runs for it. Choose my own changes them, and changes apply at once."
    };
    y += 14.0;
    y += c.para(x, y, CARD_IN, 20.0, &[(caption, F::Body, t.text)]) + 12.0;
    y += rows(c, v, x, y);
    if st.enabled && v.back.is_none() {
        let after = match all {
            _ if !st.pad => None,
            true => (!st.saved).then_some("You set them to Nothing yourself, so Quit has nothing to give back."),
            false => Some("des-k runs them once all four are off, even over admin windows. After that, quitting des-k gives them back from your next sign in."),
        };
        if let Some(a) = after {
            y += 12.0 + c.para(x, y + 12.0, CARD_IN, 16.0, &[(a, F::Small, t.dim)]);
        }
    }
    if inline {
        y += 12.0 + buttons(c, v, x, y + 12.0, CARD_IN, t.card);
    }
    y - top
}

/// "des-k runs these gestures", with a big check. Returns its height.
fn ok_block(c: &mut Pass, x: f32, y: f32) -> f32 {
    let t = c.t;
    let (tx, tw) = (x + 42.0, CARD_IN - 42.0);
    let line = "Three and four finger swipes and taps now do what you set in des-k's settings, under Touchpad.";
    let th = 20.0 + measure(c, |c| c.para(tx, y, tw, 16.0, &[(line, F::Small, t.dim)]));
    let h = th.max(32.0);
    let cy = y + h / 2.0;
    c.circle(x + 16.0, cy, 16.0, Some(op(t.chip_off)), None);
    if t.hc {
        c.circle(x + 16.0, cy, 15.5, None, Some((op(t.stroke), 1.0)));
    }
    c.check(x + 9.5, cy - 6.0, 13.0, 2.0, op(t.chip_off_fg));
    let ty = y + (h - th) / 2.0;
    c.text(tx, ty, 20.0, "des-k runs these gestures.", F::Bold, t.text);
    c.para(tx, ty + 20.0, tw, 16.0, &[(line, F::Small, t.dim)]);
    h
}

/// An information (or, with `warn`, a warning) banner. Returns its height.
fn banner(c: &mut Pass, x: f32, y: f32, w: f32, warn: bool, title: &str, subs: &[&str]) -> f32 {
    let t = c.t;
    let (tx, tw) = (x + 1.0 + 10.0 + 28.0, w - 2.0 - 20.0 - 28.0);
    let body = |c: &mut Pass| {
        let mut h = c.para(tx, y + 9.0, tw, 20.0, &[(title, F::Bold, t.text)]);
        for s in subs {
            h += 2.0 + c.para(tx, y + 9.0 + h + 2.0, tw, 16.0, &[(s, F::Small, t.dim)]);
        }
        h
    };
    let h = measure(c, body) + 18.0;
    let (fill, edge) = if warn { (t.warn, t.warn_edge) } else { (t.note, t.note_edge) };
    c.boxed((x, y, w, h), 8.0, fill, Some(edge));
    if warn {
        let gw = c.width("!", F::Glyph);
        c.text(x + 11.0 + (20.0 - gw) / 2.0, y + 9.0, 20.0, "!", F::Glyph, t.text);
    } else {
        c.info(x + 21.0, y + 19.0, op(t.text));
    }
    body(c);
    h
}

/// What des-k does for this gesture: its once rule's label, and its twice rule's if any.
pub(crate) fn does(fingers: usize, motion: Motion) -> String {
    let g = &crate::settings::get().gestures;
    let (once, twice) = crate::settings::rules_for(fingers, motion);
    let mut text = once.map_or("nothing".into(), |i| g[i].action.label());
    if let Some(i) = twice {
        text += &format!(" · twice: {}", g[i].action.label());
    }
    text
}

/// The four rows: each one's name and chip, then what des-k does for it (swipes two per
/// line). Returns their height.
fn rows(c: &Canvas, v: &View, x: f32, top: f32) -> f32 {
    let t = c.t;
    let half = (CARD_IN - 12.0) / 2.0;
    let swipes = [("←", Motion::Left), ("→", Motion::Right), ("↑", Motion::Up), ("↓", Motion::Down)];
    let mut y = top;
    for (i, (name, _)) in setup::ROWS.iter().enumerate() {
        if i > 0 {
            y += 14.0;
        }
        c.text(x, y, 20.0, name, F::Body, t.text);
        let (label, check, bg, fg) = match v.back {
            Some(b) => match b.wrote[i] {
                Wrote::All => ("Written back", true, t.chip_off, t.chip_off_fg),
                Wrote::Part => ("Part written", false, t.chip_on, t.chip_on_fg),
                Wrote::NotOne => ("Not written", false, t.chip_err, t.chip_err_fg),
            },
            // Windows' own value, named as Windows', so it never reads as what des-k does.
            None => match v.st.rows[i] {
                Row::Nothing => ("Windows: Nothing", true, t.chip_off, t.chip_off_fg),
                Row::StillOn => ("Windows: still on", false, t.chip_on, t.chip_on_fg),
                Row::CantRead => ("Windows: can't read", false, t.chip_err, t.chip_err_fg),
            },
        };
        let lw = c.width(label, F::SmallBold);
        let mark = if check { 12.0 } else { 0.0 };
        let w = 18.0 + mark + lw;
        let cx = x + CARD_IN - w;
        c.boxed((cx, y, w, 20.0), 10.0, bg, t.hc.then_some(t.stroke));
        if check {
            c.check(cx + 9.0, y + 6.0, 8.5, 1.5, op(fg));
        }
        c.text(cx + 9.0 + mark, y + 2.0, 16.0, label, F::SmallBold, fg);
        y += 20.0 + 4.0;
        // ROWS runs 3 swipes, 3 taps, 4 swipes, 4 taps.
        let fingers = if i < 2 { 3 } else { 4 };
        if i % 2 == 1 {
            y += c.para(x, y, CARD_IN, 20.0, &[(&does(fingers, Motion::Tap), F::Body, t.text)]);
            continue;
        }
        for pair in swipes.chunks(2) {
            let mut h: f32 = 0.0;
            for (j, (arrow, m)) in pair.iter().enumerate() {
                let text = format!("{arrow} {}", does(fingers, *m));
                h = h.max(c.para(x + j as f32 * (half + 12.0), y, half, 20.0, &[(&text, F::Body, t.text)]));
            }
            y += h;
        }
    }
    y - top
}

/// The footer's contents from `top`. Returns their height.
fn foot(c: &mut Pass, v: &View, top: f32) -> f32 {
    let t = c.t;
    // While a failure banner shows, the card holds Try giving back again instead.
    let give = v.st.saved && !v.back.is_some_and(|b| b.failed());
    let left = |c: &mut Pass, y: f32| {
        let mut h = 0.0;
        if v.st.saved {
            if give {
                let w = c.pill_w(GIVE, Kind::Quiet);
                c.place(Place { id: ID_GIVE, kind: Kind::Quiet, x: SIDE, y, w, h: 32.0, back: t.foot, label: GIVE, enabled: true });
                h += 36.0;
            }
            let line = "Gives back what Windows had before des-k saved your values, from your next sign in.";
            h += c.para(SIDE, y + h, 300.0, 16.0, &[(line, F::Small, t.dim)]);
        } else if v.first {
            let runs = [("You can open this again in des-k's settings, under Touchpad: ", F::Small, t.dim), ("Touchpad setup", F::SmallBold, t.dim), (".", F::Small, t.dim)];
            h += c.para(SIDE, y, 300.0, 16.0, &runs);
        }
        h
    };
    let lh = measure(c, |c| left(c, top));
    let inner = lh.max(32.0);
    left(c, top + (inner - lh) / 2.0);
    let label = if v.first { "Skip" } else { "Close" };
    let w = c.pill_w(label, Kind::Quiet);
    c.place(Place { id: ID_CLOSE, kind: Kind::Quiet, x: W - SIDE - w, y: top + (inner - 32.0) / 2.0, w, h: 32.0, back: t.foot, label, enabled: true });
    inner
}

/// One button, filling its window: the backdrop, the pill, the focus ring.
/// `x`, `y`, `w`, `h` are the pill's, in dip.
fn paint_button(c: &Canvas, p: &Place, (x, y, w, h): Bx, pressed: bool, disabled: bool, focus: bool) {
    let t = c.t;
    let (fill, edge, fg) = match (disabled, p.kind) {
        (true, _) => (p.back, if t.hc { t.disabled } else { t.card_edge }, t.disabled),
        (false, Kind::Primary) => (t.accent, t.accent, t.on_accent),
        (false, _) => (t.btn, t.btn_edge, t.text),
    };
    let fill = if pressed && !disabled { mix(fill, t.text, 0.12) } else { fill };
    c.boxed((x, y, w, h), h / 2.0, fill, Some(edge));
    let f = if p.kind == Kind::Primary { F::Bold } else { F::Body };
    let tw = c.width(p.label, f);
    c.text(x + (w - tw) / 2.0, y + (h - 20.0) / 2.0, 20.0, p.label, f, fg);
    if focus {
        c.stroke_rr((x - 3.0, y - 3.0, w + 6.0, h + 6.0), h / 2.0 + 3.0, 2.0, op(t.text));
    }
}

fn read_view(first: bool) -> View {
    let st = setup::state();
    if st.off {
        LAST.set(None); // Windows' gestures are off again: the give back is over
    }
    View { st, first, back: LAST.get() }
}

/// How the screen fits and its buttons, measured with a screen DC. `cap`: the most client
/// height, in dip, its screen has room for.
fn layout(w: &Win, cap: f32) -> (Fit, Vec<Place>) {
    unsafe {
        let dc = GetDC(None);
        let mut c = Pass { c: Canvas::new(dc, false, w.scale, w.theme, &w.fonts), places: vec![] };
        let mut fit = compose(&mut c, &w.view, None, 0.0);
        if fit.height > cap + 0.5 {
            c.places.clear();
            fit = compose(&mut c, &w.view, Some(cap), w.scroll);
        }
        let places = std::mem::take(&mut c.places);
        drop(c);
        ReleaseDC(None, dc);
        (fit, places)
    }
}

/// The work area of the screen `at` is on, else the window's own screen.
fn work_area(h: HWND, at: Option<RECT>) -> Option<RECT> {
    unsafe {
        let mon = match at {
            Some(r) => MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST),
            None => MonitorFromWindow(h, MONITOR_DEFAULTTONEAREST),
        };
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        GetMonitorInfoW(mon, &mut mi).as_bool().then_some(mi.rcWork)
    }
}

/// The client height, in dip, that fits under the title bar in `work` px of screen height.
fn client_cap(work: i32, dpi: u32, s: f32) -> f32 {
    let mut r = RECT::default();
    unsafe { let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi); }
    (work - (r.bottom - r.top)) as f32 / s
}

/// Scrolls the middle of a short window by `dy` dip. Only the window itself repaints: its
/// buttons are all pinned, so none of them moves.
fn scroll_by(dy: f32) {
    let moved = set(|w| {
        let s = (w.scroll + dy).clamp(0.0, (w.fit.content - w.fit.shown).max(0.0));
        (s != w.scroll).then(|| {
            w.scroll = s;
            w.hwnd
        })
    });
    if let Some(Some(h)) = moved {
        unsafe { let _ = InvalidateRect(Some(h), None, false); }
    }
}

/// The card button when it shows and works, else Skip or Close.
fn default_id(places: &[Place]) -> i32 {
    match places.iter().any(|p| p.id == ID_CARD && p.enabled) {
        true => ID_CARD,
        false => ID_CLOSE,
    }
}

/// Lays the screen out again and fits the window and its buttons to it, never taller than its
/// screen's work area; `to` (WM_DPICHANGED's rectangle) moves it too.
fn relayout(to: Option<RECT>) {
    let Some((h, s, dpi)) = get(|w| (w.hwnd, w.scale, w.dpi)) else { return };
    let work = work_area(h, to);
    let cap = work.map_or(f32::INFINITY, |r| client_cap(r.bottom - r.top, dpi, s));
    let Some((height, places, buttons)) = set(|w| {
        let (fit, places) = layout(w, cap);
        w.scroll = w.scroll.clamp(0.0, (fit.content - fit.shown).max(0.0));
        w.fit = fit;
        w.places.clone_from(&places);
        (fit.height, places, w.buttons.clone())
    }) else {
        return;
    };
    let px = |v: f32| (v * s).round() as i32;
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: px(W), bottom: px(height) };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let mut now = RECT::default();
        let _ = GetWindowRect(h, &mut now);
        let (x, mut y) = to.map_or((now.left, now.top), |t| (t.left, t.top));
        // A window that grew past its screen's bottom moves up, so its buttons stay on it.
        if let Some(wk) = work.filter(|_| wh > now.bottom - now.top || to.is_some()) {
            y = y.min(wk.bottom - wh).max(wk.top);
        }
        let _ = SetWindowPos(h, None, x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
        for (id, b) in &buttons {
            match places.iter().find(|p| p.id == *id) {
                Some(p) => {
                    let _ = SetWindowTextW(*b, &HSTRING::from(p.label));
                    let _ = EnableWindow(*b, p.enabled);
                    let (bx, by) = (px(p.x - RING), px(p.y - RING));
                    let _ = MoveWindow(*b, bx, by, px(p.x + p.w + RING) - bx, px(p.y + p.h + RING) - by, true);
                    let _ = ShowWindow(*b, SW_SHOWNA);
                    let _ = InvalidateRect(Some(*b), None, false);
                }
                None => {
                    let _ = ShowWindow(*b, SW_HIDE);
                }
            }
        }
        let _ = InvalidateRect(Some(h), None, false);
        // A refresh that hid or disabled the focused button moves focus to the default one.
        let default = buttons.iter().find(|(id, _)| *id == default_id(&places)).map(|(_, b)| *b);
        let focused = GetFocus();
        let usable = |b: HWND| buttons.iter().any(|(_, x)| *x == b) && IsWindowVisible(b).as_bool() && IsWindowEnabled(b).as_bool();
        if let Some(d) = default {
            if GetActiveWindow() == h && !usable(focused) {
                let _ = SetFocus(Some(d));
            }
            let _ = set(|w| {
                if w.focus.is_some_and(|f| !usable(f)) {
                    w.focus = Some(d);
                }
            });
        }
    }
}

/// Reads the state again and redraws, if the screen is open. Also on WM_TOUCHPADS_CHANGED.
pub fn refresh() {
    let Some(first) = get(|w| w.first) else { return };
    let view = read_view(first);
    if set(|w| w.view = view).is_some() {
        relayout(None);
    }
}

/// Main loop: the screen's keys (Tab, Enter, Esc, Space) through the dialog manager, and
/// Page Up and Page Down scrolling a short window's middle.
pub fn key(msg: &MSG) -> bool {
    let Some(h) = hwnd() else { return false };
    unsafe {
        if msg.message == WM_KEYDOWN && (msg.hwnd == h || IsChild(h, msg.hwnd).as_bool()) {
            let page = get(|w| (w.fit.shown - 40.0).max(20.0)).unwrap_or(0.0);
            let dy = match VIRTUAL_KEY(msg.wParam.0 as u16) {
                VK_PRIOR => Some(-page),
                VK_NEXT => Some(page),
                _ => None,
            };
            if let Some(dy) = dy {
                scroll_by(dy);
                return true;
            }
        }
        IsDialogMessageW(h, msg).as_bool()
    }
}

/// Opens the screen, or brings the open one to the front. `first`: the first start, so Skip
/// writes an empty touchpad.txt and the screen never opens on its own again.
pub fn show(first: bool) {
    if let Some(h) = hwnd() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        return; // getting active refreshes it
    }
    gdiplus();
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when the screen opens a second time
        let (mon, _) = crate::ws::under_mouse();
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work = mi.rcWork;
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("des-k"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return eprintln!("welcome: no window: {e}"),
        };
        let style = WINDOW_STYLE(WS_CHILD.0 | WS_TABSTOP.0 | BS_OWNERDRAW as u32);
        let make = |id: i32| {
            let menu = HMENU(id as isize as *mut core::ffi::c_void);
            CreateWindowExW(WINDOW_EX_STYLE(0), w!("BUTTON"), w!(""), style, 0, 0, 0, 0, Some(h), Some(menu), Some(inst.into()), None).ok().map(|b| (id, b))
        };
        let buttons: Vec<(i32, HWND)> = IDS.into_iter().filter_map(make).collect();
        let dpi = GetDpiForWindow(h);
        let scale = dpi as f32 / 96.0;
        let theme = theme();
        dark_title(h, &theme);
        crate::ui::transitions(h, false); // `fastest` turns off its open animation (spec 0017 AC-6)
        let view = read_view(first);
        WIN.with_borrow_mut(|w| *w = Some(Win { hwnd: h, first, dpi, scale, theme, fonts: Fonts::new(scale), view, places: vec![], buttons, focus: None, fit: Fit::default(), scroll: 0.0 }));
        relayout(None);
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
    }
}

fn give() {
    if let Some(b) = setup::give_back() {
        LAST.set(Some(b));
    }
    refresh();
}

/// Skip, Close, Esc and the title bar's X.
fn close(h: HWND) {
    if get(|w| w.first) == Some(true) {
        setup::skip();
    }
    unsafe { let _ = DestroyWindow(h); }
}

fn on_command(h: HWND, id: i32) {
    match id {
        ID_CARD if LAST.get().is_some_and(|b| b.failed()) => give(),
        ID_CARD => {
            setup::use_gestures();
            refresh();
        }
        ID_CARD2 => setup::open_settings(),
        ID_OWN => {
            // Choose my own also does what the card's working Use or Open button does.
            let card = get(|w| w.places.iter().any(|p| p.id == ID_CARD && p.enabled && p.label != AGAIN));
            if card == Some(true) {
                setup::use_gestures();
            }
            crate::settings_page::open(Some(crate::settings_page::Section::Touchpad));
            refresh();
        }
        ID_GIVE => give(),
        ID_CLOSE => close(h),
        _ => {}
    }
}

fn on_draw_item(dis: &DRAWITEMSTRUCT) {
    let _ = get(|w| {
        let Some(p) = w.places.iter().find(|p| p.id == dis.CtlID as i32) else { return };
        let r = dis.rcItem;
        let (pw, ph) = (r.right - r.left, r.bottom - r.top);
        let state = dis.itemState;
        buffered(dis.hDC, r.left, r.top, pw, ph, |dc| {
            let c = Canvas::new(dc, true, w.scale, w.theme, &w.fonts);
            let (bw, bh) = (pw as f32 / w.scale, ph as f32 / w.scale);
            c.boxed((0.0, 0.0, bw + 1.0, bh + 1.0), 0.0, p.back, None);
            let focus = state.0 & ODS_FOCUS.0 != 0 && state.0 & ODS_NOFOCUSRECT.0 == 0;
            let (pressed, disabled) = (state.0 & ODS_SELECTED.0 != 0, state.0 & ODS_DISABLED.0 != 0);
            paint_button(&c, p, (RING, RING, bw - 2.0 * RING, bh - 2.0 * RING), pressed, disabled, focus);
        });
    });
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
                        let mut c = Pass { c: Canvas::new(mem, true, w.scale, w.theme, &w.fonts), places: vec![] };
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, w.theme.surface, None);
                        compose(&mut c, &w.view, w.fit.short.then_some(w.fit.height), w.scroll);
                    })
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // A button with focus hands the wheel on to here.
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_DRAWITEM => {
                on_draw_item(&*(lp.0 as *const DRAWITEMSTRUCT));
                LRESULT(1)
            }
            WM_COMMAND if (wp.0 >> 16) & 0xFFFF == BN_CLICKED as usize => {
                on_command(h, (wp.0 & 0xFFFF) as i32);
                LRESULT(0)
            }
            // The dialog manager asks which button Enter presses: the focused one, like any
            // Windows dialog, else the default one.
            DM_GETDEFID => {
                let focused = GetFocus();
                let id = get(|w| w.buttons.iter().find(|(_, b)| *b == focused).map_or_else(|| default_id(&w.places), |(id, _)| *id));
                LRESULT(((DC_HASDEFID as isize) << 16) | id.unwrap_or(ID_CLOSE) as isize)
            }
            // Back from Settings: read the state again, and give focus back.
            WM_ACTIVATE => {
                if wp.0 & 0xFFFF == WA_INACTIVE as usize {
                    let focused = GetFocus();
                    let _ = set(|w| w.focus = w.buttons.iter().any(|(_, b)| *b == focused).then_some(focused));
                } else if (wp.0 >> 16) & 0xFFFF == 0 {
                    refresh();
                    let target = get(|w| w.focus.or_else(|| w.buttons.iter().find(|(id, _)| *id == default_id(&w.places)).map(|(_, b)| *b))).flatten();
                    if let Some(b) = target {
                        let _ = SetFocus(Some(b));
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                close(h);
                LRESULT(0)
            }
            WM_DESTROY => {
                drop(WIN.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xFFFF) as u32;
                let to = *(lp.0 as *const RECT);
                let _ = set(|w| {
                    w.dpi = dpi;
                    w.scale = dpi as f32 / 96.0;
                    w.fonts = Fonts::new(w.scale);
                });
                relayout(Some(to));
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                crate::ui::refresh();
                let t = theme();
                if set(|w| std::mem::replace(&mut w.theme, t) != t) == Some(true) {
                    dark_title(h, &t);
                    relayout(None);
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

    // Not a check that can fail: draws every state of the design file offscreen into
    // target/welcome/*.png, so the drawing can be compared with the approved design without
    // opening the window. Run with `cargo test render -- --ignored`.
    #[test]
    #[ignore]
    fn render_every_state() {
        assert!(gdiplus());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("welcome");
        std::fs::create_dir_all(&dir).unwrap();
        let (n, o, x) = (Row::Nothing, Row::StillOn, Row::CantRead);
        let st = |pad, enabled, rows: [Row; 4], saved| State { pad, enabled, rows, off: rows == [n; 4], saved };
        let gave = |wrote, cleared| Some(GaveBack { wrote, cleared });
        let (a, p, z) = (Wrote::All, Wrote::Part, Wrote::NotOne);
        let states = [
            ("s1", View { st: st(true, true, [o; 4], false), first: true, back: None }),
            ("s2", View { st: st(true, true, [n, n, o, n], true), first: false, back: None }),
            ("s3", View { st: st(true, true, [n; 4], true), first: false, back: None }),
            ("s4", View { st: st(true, true, [n; 4], false), first: false, back: None }),
            ("s5", View { st: st(false, true, [o; 4], false), first: true, back: None }),
            ("s6", View { st: st(true, false, [n; 4], true), first: false, back: None }),
            ("s7", View { st: st(true, true, [o, o, n, n], true), first: false, back: gave([a, p, z, z], false) }),
            ("s7b", View { st: st(true, true, [o; 4], true), first: false, back: gave([z; 4], false) }),
            ("s8", View { st: st(true, true, [o; 4], false), first: false, back: gave([a; 4], true) }),
            ("s8b", View { st: st(true, true, [o; 4], true), first: false, back: gave([a; 4], false) }),
            ("s9", View { st: st(true, true, [n, n, n, x], true), first: false, back: None }),
        ];
        let night_sky = Theme { hc: true, ..hc_like(0x000000, 0xFFFFFF, 0x1AEBFF, 0x3FF23F) };
        let mut heights = vec![];
        // "short": a 1080p laptop screen at 150% (672 dip of work area under the taskbar), so
        // the window is capped and its middle scrolls; "short-end" is scrolled to the end.
        for (tname, theme) in [("light", ui::LIGHT), ("dark", ui::DARK), ("hc", night_sky)] {
            for (scale, fit) in [(1.0f32, ""), (1.5, ""), (1.5, "short"), (1.5, "short-end")] {
                for (sname, v) in &states {
                    let wanted = match fit {
                        "" => scale == 1.0 || matches!(*sname, "s1" | "s7"),
                        "short" => matches!(*sname, "s1" | "s2" | "s6"),
                        _ => *sname == "s1",
                    };
                    if !wanted {
                        continue;
                    }
                    let fonts = Fonts::new(scale);
                    let dpi = (96.0 * scale) as u32;
                    let scroll = if fit == "short-end" { f32::MAX } else { 0.0 };
                    let fake = Win { hwnd: HWND::default(), first: v.first, dpi, scale, theme, fonts, view: *v, places: vec![], buttons: vec![], focus: None, fit: Fit::default(), scroll };
                    let cap = if fit.is_empty() { f32::INFINITY } else { client_cap((672.0 * scale) as i32, dpi, scale) };
                    let (got, places) = layout(&fake, cap);
                    let h = got.height;
                    let note = if got.short { format!(", capped at {cap} dip, middle {} of {} dip shows", got.shown, got.content) } else { String::new() };
                    heights.push(format!("{tname} {sname}{} {}% {h} dip{note}", if fit.is_empty() { String::new() } else { format!(" {fit}") }, scale * 100.0));
                    let (pw, ph) = ((W * scale).round() as i32, (h * scale).round() as i32);
                    let file = if fit.is_empty() { format!("{sname}-{tname}-{}.png", scale * 100.0) } else { format!("{sname}-{fit}-{tname}-{}.png", scale * 100.0) };
                    ui::tests::save_png(pw, ph, &dir.join(file), |dc| {
                        let mut c = Pass { c: Canvas::new(dc, true, scale, theme, &fake.fonts), places: vec![] };
                        c.boxed((0.0, 0.0, W + 1.0, h + 1.0), 0.0, theme.surface, None);
                        compose(&mut c, v, got.short.then_some(h), scroll);
                        let default = default_id(&places);
                        for pl in &places {
                            let focus = *sname == "s1" && pl.id == default;
                            paint_button(&c, pl, (pl.x, pl.y, pl.w, pl.h), false, !pl.enabled, focus);
                        }
                    });
                }
            }
        }
        std::fs::write(dir.join("heights.txt"), heights.join("\n")).unwrap();
    }

    fn hc_like(bg: u32, fg: u32, hi: u32, gray: u32) -> Theme {
        Theme { dark: true, ..crate::ui::high_contrast([bg, fg, hi, bg, gray]) }
    }
}
