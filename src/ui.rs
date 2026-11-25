// The drawing layer des-k's own windows share: the theme (Windows' light, dark or high contrast
// colors, read once and checked again each second) and look B's tokens (spec 0017), the fonts and
// icons, motion, the rounded popup, the Canvas that draws shapes through GDI+ and text through
// GDI, the back buffer, per pixel alpha popups, and the try_borrow accessors for window state
// that a message can re-enter. Sizes on the Canvas are in dip (1/96 inch).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::thread::LocalKey;
use windows::Win32::{
    Foundation::{COLORREF, ERROR_SUCCESS, HWND, LPARAM, POINT, SIZE, WPARAM},
    Graphics::{
        Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_TRANSITIONS_FORCEDISABLED, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
            DwmSetWindowAttribute,
        },
        Gdi::*,
        GdiPlus as gp,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RegGetValueW},
        Threading::GetCurrentThreadId,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

use crate::settings::Accent;

pub(crate) mod canvas;
pub(crate) mod color;
pub(crate) mod fonts;

pub(crate) use canvas::{Bx, Canvas, Picture, buffered, gdiplus, layered};
pub(crate) use color::{Hsv, alpha, contrast, cref, distance, hsv_of, mix, op, rgb_of, ring_color};
use color::hue;
pub(crate) use fonts::{F, Fonts, font, fonts, variable};

/// The look des-k's windows draw in: light or dark, high contrast, and look B's tokens.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Theme {
    pub(crate) dark: bool,
    pub(crate) hc: bool,
    /// Look B's tokens (spec 0017); the old look's went with the first run, its last screen.
    pub(crate) b: B,
}

/// Look B's tokens, as 0xRRGGBB: the approved first run's `:root` (pdesk-design
/// restart/first-run-r11.html), the same in every look B screen.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct B {
    pub(crate) bg: u32,
    pub(crate) card: u32,
    pub(crate) line: u32,
    pub(crate) tx: u32,
    pub(crate) tx2: u32,
    pub(crate) tx3: u32, // greyed
    /// The accent: Windows' own or yours (spec 0020), des-k purple when neither can be read.
    pub(crate) acc: u32,
    pub(crate) on_acc: u32,
    /// The accent's neighbors, 30 degrees either side: `acc_a` tints headers and bands,
    /// `acc_b` fills count badges, chips and picker marks (spec 0020 AC-3).
    pub(crate) acc_a: u32,
    pub(crate) acc_b: u32,
    pub(crate) on_acc_b: u32,
    pub(crate) track: u32,
    pub(crate) hi: u32,
    pub(crate) warn: u32,
    pub(crate) good: u32,
    /// A key line's band, and a key cap with its edge.
    pub(crate) band: u32,
    pub(crate) key: u32,
    pub(crate) key_edge: u32,
    /// A popup's 1 px edge: 10% black (light) or white (dark) over the card.
    pub(crate) edge: u32,
    /// The indicator's bars and dots that are not lit.
    pub(crate) bar_off: u32,
    /// The nine workspace colors, and the ink on each (spec 0020 AC-4, AC-5).
    pub(crate) ws: [u32; 9],
    pub(crate) on_ws: [u32; 9],
    /// The window marks on each workspace tile: white unless white is too faint on it (AC-16).
    pub(crate) mk_ws: [u32; 9],
    /// In light, a workspace colored block and bar get an edge this much darker (0: none).
    pub(crate) ws_edge: f32,
    pub(crate) bar_edge: f32,
    /// A see through popup's opacity (the indicator's card).
    pub(crate) alpha: f32,
    /// A key cap held while recording, and a picker's mark box: a light tint of the accent.
    pub(crate) held: u32,
    /// des-k's mark, four squares: top left, top right, bottom left, bottom right.
    pub(crate) mark: [u32; 4],
    /// A drawn screen's bezel and its stand (the settings page's and first run's pictures).
    pub(crate) bez: u32,
    pub(crate) stand: u32,
    /// A link in a caption, like Reset: the accent, lighter on dark.
    pub(crate) link: u32,
    /// A chart's grid lines (the settings page's Motion).
    pub(crate) grid: u32,
    /// An empty text box's hint: at least 4.5:1 on the box, clearly lighter than typed text
    /// (spec 0023's name boxes).
    pub(crate) hint: u32,
}

/// The mark's colors, the tray icon's (approved with the first run).
const MARK: [u32; 4] = [0xB93C87, 0xE53935, 0x8E3FD9, 0xB93C87];

/// The nine workspace colors: one lightness, the same hex in light and dark (approved 2026-11-04).
pub(crate) const WS: [u32; 9] = [0x79A4F3, 0xDDB43C, 0x36BE9C, 0xBB8DE3, 0xE68A4E, 0x27B5DC, 0xDF80B8, 0xB99F88, 0xF97676];

// What `resolve` makes of des-k purple and look B's workspace colors, so the constants below
// are the theme with no Windows accent and no [colors] (tested).
const ACC_A: u32 = 0x413FD9;
const ACC_B: u32 = 0xD93FD7;
const ON_ACC_B: u32 = 0x14141A;
const ON_WS: [u32; 9] = [0x14141A; 9];

const B_LIGHT: B = B {
    bg: 0xF3F3F3,
    card: 0xFFFFFF,
    line: 0xE3E3E3,
    tx: 0x1A1A1A,
    tx2: 0x5C5C5C,
    tx3: 0x7A7A7A,
    acc: 0x8E3FD9,
    on_acc: 0xFFFFFF,
    acc_a: ACC_A,
    acc_b: ACC_B,
    on_acc_b: ON_ACC_B,
    track: 0xF0F0F0,
    hi: 0xFFFFFF,
    warn: 0x9D5D00,
    good: 0x0F7B0F,
    band: 0xEBEBEB,
    key: 0xFFFFFF,
    key_edge: 0xD1D1D1,
    edge: 0xE6E6E6,
    bar_off: 0x949494,
    ws: WS,
    on_ws: ON_WS,
    mk_ws: [0xFFFFFF; 9],
    ws_edge: 0.10,
    bar_edge: 0.30,
    alpha: 0.92,
    held: 0xF1E8FA,
    mark: MARK,
    bez: 0x3A3A3A,
    stand: 0xC8C8C8,
    link: 0x8E3FD9,
    grid: 0xD0D0D0,
    hint: 0x767676,
};

const B_DARK: B = B {
    bg: 0x202020,
    card: 0x2B2B2B,
    line: 0x3A3A3A,
    tx: 0xF5F5F5,
    tx2: 0xC5C5C5,
    tx3: 0x9A9A9A,
    acc: 0x8E3FD9,
    on_acc: 0xFFFFFF,
    acc_a: ACC_A,
    acc_b: ACC_B,
    on_acc_b: ON_ACC_B,
    track: 0x232323,
    hi: 0x3F3F3F,
    warn: 0xFCE100,
    good: 0x6CCB5F,
    band: 0x1B1B1B,
    key: 0x353535,
    key_edge: 0x4C4C4C,
    edge: 0x404040,
    bar_off: 0x9A9A9A,
    ws: WS,
    on_ws: ON_WS,
    mk_ws: [0xFFFFFF; 9],
    ws_edge: 0.0,
    bar_edge: 0.0,
    alpha: 0.92,
    held: 0x3F2F4E,
    mark: MARK,
    bez: 0x0A0A0A,
    stand: 0x454545,
    link: 0xB27CE5,
    grid: 0x4A4A4A,
    hint: 0x9E9E9E,
};

pub(crate) const DARK: Theme = Theme {
    dark: true,
    hc: false,
    b: B_DARK,
};

pub(crate) const LIGHT: Theme = Theme {
    dark: false,
    hc: false,
    b: B_LIGHT,
};

/// High contrast: Windows' own colors only, no gradient and no tints; borders instead. Look B
/// maps as the designs' HC frames show: every workspace and the accent in Highlight, lines and
/// text in WindowText, greyed text in GrayText, nothing see through.
pub(crate) fn high_contrast(sys: [u32; 5]) -> Theme {
    let [bg, fg, hl, hl_fg, gray] = sys;
    let light = (bg >> 16 & 0xFF) + (bg >> 8 & 0xFF) + (bg & 0xFF) > 3 * 128;
    Theme {
        dark: !light,
        hc: true,
        b: B {
            bg,
            card: bg,
            line: fg,
            tx: fg,
            tx2: fg,
            tx3: gray,
            acc: hl,
            on_acc: hl_fg,
            acc_a: hl,
            acc_b: hl,
            on_acc_b: hl_fg,
            track: bg,
            hi: hl,
            warn: fg,
            good: fg,
            band: bg,
            key: bg,
            key_edge: fg,
            edge: fg,
            bar_off: fg,
            ws: [hl; 9],
            on_ws: [hl_fg; 9],
            mk_ws: [hl_fg; 9],
            ws_edge: 0.0,
            bar_edge: 0.0,
            alpha: 1.0,
            held: hl_fg,
            mark: [fg; 4],
            bez: fg,
            stand: fg,
            link: hl,
            grid: fg,
            hint: gray,
        },
    }
}

/// Windows' look as des-k last read it (spec 0017 AC-2).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Look {
    pub(crate) apps_light: bool,   // des-k's windows: the settings page, the first run, the finder, the overview
    pub(crate) system_light: bool, // what sits with the taskbar: the indicator, the tray popup
    pub(crate) transparency: bool,
    /// High contrast's Window, WindowText, Highlight, HighlightText and GrayText; None when off.
    pub(crate) hc: Option<[u32; 5]>,
    /// Windows' accent shades, Light3 to Dark3 (spec 0020); None until one is read.
    pub(crate) accent: Option<[u32; 8]>,
}

thread_local! {
    static LOOK: Cell<Option<Look>> = const { Cell::new(None) };
    // The accent palette could not be read last time, so the log says so once, not each second.
    static PALETTE_MISSING: Cell<bool> = const { Cell::new(false) };
}

/// `AccentPalette`: 8 shades of 4 bytes, R, G, B and an unused A, Light3 first. Anything but
/// exactly 32 bytes is not a palette.
fn palette_of(bytes: &[u8]) -> Option<[u32; 8]> {
    (bytes.len() == 32).then(|| std::array::from_fn(|i| u32::from(bytes[4 * i]) << 16 | u32::from(bytes[4 * i + 1]) << 8 | u32::from(bytes[4 * i + 2])))
}

/// Windows' accent palette, or `last` when it can't be read, so a half written value never
/// flashes purple.
fn read_palette(last: Option<[u32; 8]>) -> Option<[u32; 8]> {
    let (mut buf, mut size) = ([0u8; 32], 32u32);
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent");
    let e = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, w!("AccentPalette"), RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)) };
    let got = if e == ERROR_SUCCESS { palette_of(&buf[..(size as usize).min(32)]) } else { None };
    if got.is_none() != PALETTE_MISSING.get() {
        PALETTE_MISSING.set(got.is_none());
        match got {
            None => warn!("ui: can't read Windows' accent color ({e:?}), keeping the last one"),
            Some(_) => info!("ui: Windows' accent color can be read again"),
        }
    }
    got.or(last)
}

fn read_look() -> Look {
    let dword = |name: PCWSTR, default: u32| {
        let (mut v, mut size) = (default, 4u32);
        let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
        let e = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, name, RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut size)) };
        if e == ERROR_SUCCESS { v } else { default }
    };
    let mut hc = HIGHCONTRASTW { cbSize: size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let read = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, Some((&mut hc as *mut HIGHCONTRASTW).cast()), Default::default()) };
    let sys = |i: SYS_COLOR_INDEX| {
        let c = unsafe { GetSysColor(i) };
        (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
    };
    let on = read.is_ok() && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0;
    Look {
        apps_light: dword(w!("AppsUseLightTheme"), 1) != 0,
        system_light: dword(w!("SystemUsesLightTheme"), 0) != 0,
        transparency: dword(w!("EnableTransparency"), 1) != 0,
        hc: on.then(|| [sys(COLOR_WINDOW), sys(COLOR_WINDOWTEXT), sys(COLOR_HIGHLIGHT), sys(COLOR_HIGHLIGHTTEXT), sys(COLOR_GRAYTEXT)]),
        accent: read_palette(LOOK.get().and_then(|l| l.accent)),
    }
}

/// Windows' look, read once and then kept until `refresh` sees a change.
pub(crate) fn look() -> Look {
    LOOK.get().unwrap_or_else(|| {
        let l = read_look();
        LOOK.set(Some(l));
        l
    })
}

/// Reads Windows' look again; true when it changed. Main's 1 s timer calls it, since the admin
/// des-k may never get the theme broadcast from the Settings app (UIPI). A window's own
/// WM_SETTINGCHANGE calls it too, the faster path when the broadcast does arrive.
pub(crate) fn refresh() -> bool {
    let l = read_look();
    LOOK.replace(Some(l)) != Some(l)
}

/// After `refresh` saw a change, or a color was saved: every des-k window of this thread hears
/// WM_SYSCOLORCHANGE and repaints in the new look. Never WM_SETTINGCHANGE: it carries a pointer,
/// so Windows refuses to post it (ERROR_MESSAGE_SYNC_ONLY); each window handles the two alike.
pub(crate) fn repaint_all() {
    unsafe extern "system" fn post(h: HWND, _: LPARAM) -> BOOL {
        unsafe { let _ = PostMessageW(Some(h), WM_SYSCOLORCHANGE, WPARAM(0), LPARAM(0)); }
        BOOL(1)
    }
    unsafe { let _ = EnumThreadWindows(GetCurrentThreadId(), Some(post), LPARAM(0)); }
}

fn theme_of(light: bool, l: Look) -> Theme {
    let s = crate::settings::get();
    resolve(light, l, s.accent, s.ws_colors)
}

/// Every color des-k paints with is decided here and nowhere else (spec 0020): high contrast
/// first, then your accent or Windows', its neighbors, your workspace colors, and their inks.
fn resolve(light: bool, l: Look, accent: Accent, ws: [Option<u32>; 9]) -> Theme {
    let mut t = match l.hc {
        Some(sys) => return high_contrast(sys),
        None if light => LIGHT,
        None => DARK,
    };
    if !l.transparency {
        t.b.alpha = 1.0;
    }
    let b = &mut t.b;
    // Dark1 in light and Light2 in dark: the shades Windows' own apps fill accent buttons with.
    b.acc = match accent {
        Accent::Rgb(c) => c,
        Accent::Windows => l.accent.map_or(b.acc, |p| p[if light { 4 } else { 1 }]),
    };
    (b.acc_a, b.acc_b) = (hue(b.acc, -30.0), hue(b.acc, 30.0));
    (b.on_acc, b.on_acc_b) = (ink(b.acc), ink(b.acc_b));
    // Moved toward black (light) or white (dark) 5% at a time until it reads on the card (AC-15).
    let base = if light { b.acc } else { mix(b.acc, 0xFFFFFF, 0.28) };
    let to = if light { 0x000000 } else { 0xFFFFFF };
    b.link = (0..=20).map(|k| mix(base, to, k as f32 * 0.05)).find(|&c| contrast(c, b.card) >= 4.5).unwrap_or(to);
    b.held = mix(b.card, b.acc, if light { 0.12 } else { 0.20 });
    for i in 0..9 {
        b.ws[i] = ws[i].unwrap_or(WS[i]);
        b.on_ws[i] = ink(b.ws[i]);
        b.mk_ws[i] = if contrast(0xFFFFFF, b.ws[i]) >= 1.6 { 0xFFFFFF } else { mix(b.ws[i], 0x14141A, 0.55) };
    }
    t
}

/// The color picker's 12 ready made colors (spec 0021 AC-4): des-k's nine workspace colors in
/// order, then des-k purple, lime and slate, measured in the design pass (tested to start with WS).
pub(crate) const PRESETS: [u32; 12] = [0x79A4F3, 0xDDB43C, 0x36BE9C, 0xBB8DE3, 0xE68A4E, 0x27B5DC, 0xDF80B8, 0xB99F88, 0xF97676, 0x8E3FD9, 0x92BF4F, 0x92A7BD];

/// The page's theme with one color in place of the file's, while the color picker previews
/// it: `ws` is Some(i) for workspace i + 1, None for the accent (spec 0021). The same
/// `resolve`, so the accent's neighbors, link, inks and tile marks follow, and high contrast
/// still returns first.
pub(crate) fn theme_preview(light: bool, l: Look, ws: Option<usize>, color: u32) -> Theme {
    let s = crate::settings::get();
    let (mut accent, mut colors) = (s.accent, s.ws_colors);
    match ws {
        Some(i) => colors[i] = Some(color),
        None => accent = Accent::Rgb(color),
    }
    resolve(light, l, accent, colors)
}

/// The ink for text on `bg`: des-k's near black or white, whichever has the higher WCAG
/// contrast ratio with it (spec 0020 AC-5).
pub(crate) fn ink(bg: u32) -> u32 {
    const DARK_INK: u32 = 0x14141A;
    if contrast(DARK_INK, bg) >= contrast(0xFFFFFF, bg) { DARK_INK } else { 0xFFFFFF }
}

/// Windows' app mode, light or dark, unless high contrast is on: des-k's windows.
pub(crate) fn theme() -> Theme {
    let l = look();
    theme_of(l.apps_light, l)
}

/// Windows' own mode, as the taskbar shows it: the indicator and the tray popup. Opaque when
/// Windows' transparency effects are off.
pub(crate) fn system_theme() -> Theme {
    let l = look();
    theme_of(l.system_light, l)
}

/// The length of a fade or move: none with `fastest` on (spec 0017 AC-6).
pub(crate) fn motion_ms(base: u32) -> u32 {
    if crate::settings::get().fastest { 0 } else { base }
}

/// Windows' own open and close animation on one of des-k's windows: off with `fastest`, and
/// always off on a popup.
pub(crate) fn transitions(h: HWND, popup: bool) {
    let off = BOOL((popup || crate::settings::get().fastest) as i32);
    unsafe { let _ = DwmSetWindowAttribute(h, DWMWA_TRANSITIONS_FORCEDISABLED, &off as *const BOOL as _, size_of::<BOOL>() as u32); }
}

/// `fastest` changed: every des-k window that is not a popup (a tool window) follows it.
pub(crate) fn transitions_all() {
    unsafe extern "system" fn set(h: HWND, _: LPARAM) -> BOOL {
        let ex = unsafe { GetWindowLongW(h, GWL_EXSTYLE) } as u32;
        if ex & WS_EX_TOOLWINDOW.0 == 0 {
            transitions(h, false);
        }
        BOOL(1)
    }
    unsafe { let _ = EnumThreadWindows(GetCurrentThreadId(), Some(set), LPARAM(0)); }
}

/// A des-k popup (spec 0017): a tool window (never in Alt+Tab or the taskbar) with Windows'
/// rounded corners and popup shadow, and no open animation. `ex` adds styles; `layered` keeps
/// it a layered window, faded whole (the indicator). Registers `class` with `proc`.
pub(crate) fn popup(class: PCWSTR, proc: WNDPROC, ex: WINDOW_EX_STYLE, layered: bool) -> Option<HWND> {
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW { lpfnWndProc: proc, hInstance: inst.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc);
        let ex = ex | WS_EX_TOOLWINDOW | if layered { WS_EX_LAYERED } else { WINDOW_EX_STYLE(0) };
        // Clip children: a popup's own paint never covers its text box (the finder's).
        let h = CreateWindowExW(ex, class, w!(""), WS_POPUP | WS_CLIPCHILDREN, 0, 0, 0, 0, None, None, Some(inst.into()), None).ok()?;
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, size_of_val(&round) as u32);
        transitions(h, true);
        Some(h)
    }
}

/// The 1 px edge Windows draws around a rounded popup, in `c`.
pub(crate) fn border(h: HWND, c: u32) {
    let c = cref(c);
    unsafe { let _ = DwmSetWindowAttribute(h, DWMWA_BORDER_COLOR, &c as *const COLORREF as _, size_of::<COLORREF>() as u32); }
}

pub(crate) fn dark_title(h: HWND, t: &Theme) {
    let on = BOOL(t.dark as i32);
    unsafe { let _ = DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const BOOL as _, size_of::<BOOL>() as u32); }
}

/// Reads a window's state, or None when the window is closed. A message can come back into
/// the window while its state is borrowed (see the callers); that read is skipped, and logged
/// as `who`, rather than panicking.
pub(crate) fn get<T: 'static, R>(key: &'static LocalKey<RefCell<Option<T>>>, who: &str, f: impl FnOnce(&T) -> R) -> Option<R> {
    key.with(|cell| match cell.try_borrow() {
        Ok(w) => w.as_ref().map(f),
        Err(_) => {
            warn!("{who}: skipped, the window's state is already borrowed (a message came back in)");
            None
        }
    })
}

/// Changes a window's state, as `get` reads it.
pub(crate) fn set<T: 'static, R>(key: &'static LocalKey<RefCell<Option<T>>>, who: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    key.with(|cell| match cell.try_borrow_mut() {
        Ok(mut w) => w.as_mut().map(f),
        Err(_) => {
            warn!("{who}: skipped, the window's state is already borrowed (a message came back in)");
            None
        }
    })
}

#[cfg(test)]
pub(crate) mod tests;
