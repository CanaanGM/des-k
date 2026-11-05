// The drawing layer des-k's own windows share: the theme (Windows' light, dark or high contrast
// colors, read once and checked again each second) and look B's tokens (spec 0017), the fonts and
// icons, motion, the rounded popup, the Canvas that draws shapes through GDI+ and text through
// GDI, the back buffer, plain GDI text for the finder and overview, and the try_borrow accessors
// for window state that a message can re-enter. Sizes on the Canvas are in dip (1/96 inch).
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::thread::LocalKey;
use windows::Win32::{
    Foundation::{COLORREF, ERROR_SUCCESS, HWND, LPARAM, RECT, SIZE, WPARAM},
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
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        Threading::GetCurrentThreadId,
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

/// Colors as 0xRRGGBB: the design file's tokens (provisional, the indicator's).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Theme {
    pub(crate) dark: bool,
    pub(crate) hc: bool,
    pub(crate) surface: u32,
    pub(crate) text: u32,
    pub(crate) dim: u32,
    pub(crate) stroke: u32,
    pub(crate) accent: u32,
    pub(crate) on_accent: u32,
    pub(crate) band_a: u32,
    pub(crate) band_b: u32,
    pub(crate) on_band: u32,
    pub(crate) card: u32,
    pub(crate) card_edge: u32,
    pub(crate) tile: u32,
    pub(crate) tile_ico: u32,
    pub(crate) tile_ico_fg: u32,
    pub(crate) chip_on: u32,
    pub(crate) chip_on_fg: u32,
    pub(crate) chip_off: u32,
    pub(crate) chip_off_fg: u32,
    pub(crate) chip_err: u32,
    pub(crate) chip_err_fg: u32,
    pub(crate) note: u32,
    pub(crate) note_edge: u32,
    pub(crate) warn: u32,
    pub(crate) warn_edge: u32,
    pub(crate) btn: u32,
    pub(crate) btn_edge: u32,
    pub(crate) foot: u32,
    pub(crate) disabled: u32,
    /// Look B (spec 0017). The fields above are the old look, each deleted with the last screen
    /// that draws it.
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
    pub(crate) acc: u32,
    pub(crate) on_acc: u32,
    pub(crate) track: u32,
    pub(crate) hi: u32,
    pub(crate) warn: u32,
    pub(crate) good: u32,
    /// A popup's 1 px edge: 10% black (light) or white (dark) over the card.
    pub(crate) edge: u32,
    /// The indicator's bars and dots that are not lit.
    pub(crate) bar_off: u32,
    /// The nine workspace colors, and the text on them.
    pub(crate) ws: [u32; 9],
    pub(crate) on_ws: u32,
    /// In light, a workspace colored block and bar get an edge this much darker (0: none).
    pub(crate) ws_edge: f32,
    pub(crate) bar_edge: f32,
    /// A see through popup's opacity (the indicator's card).
    pub(crate) alpha: f32,
}

/// The nine workspace colors: one lightness, the same hex in light and dark (approved 2026-11-04).
const WS: [u32; 9] = [0x79A4F3, 0xDDB43C, 0x36BE9C, 0xBB8DE3, 0xE68A4E, 0x27B5DC, 0xDF80B8, 0xB99F88, 0xF97676];

const B_LIGHT: B = B {
    bg: 0xF3F3F3,
    card: 0xFFFFFF,
    line: 0xE3E3E3,
    tx: 0x1A1A1A,
    tx2: 0x5C5C5C,
    tx3: 0x7A7A7A,
    acc: 0x8E3FD9,
    on_acc: 0xFFFFFF,
    track: 0xF0F0F0,
    hi: 0xFFFFFF,
    warn: 0x9D5D00,
    good: 0x0F7B0F,
    edge: 0xE6E6E6,
    bar_off: 0x949494,
    ws: WS,
    on_ws: 0x14141A,
    ws_edge: 0.10,
    bar_edge: 0.30,
    alpha: 0.92,
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
    track: 0x232323,
    hi: 0x3F3F3F,
    warn: 0xFCE100,
    good: 0x6CCB5F,
    edge: 0x404040,
    bar_off: 0x9A9A9A,
    ws: WS,
    on_ws: 0x14141A,
    ws_edge: 0.0,
    bar_edge: 0.0,
    alpha: 0.92,
};

pub(crate) const DARK: Theme = Theme {
    dark: true,
    hc: false,
    surface: 0x202020,
    text: 0xF2F2F2,
    dim: 0xA3A3A3,
    stroke: 0x555555,
    accent: 0x60A5FA,
    on_accent: 0x0A1A30,
    band_a: 0x1D4ED8,
    band_b: 0x6D28D9,
    on_band: 0xFFFFFF,
    card: 0x2A2A2E,
    card_edge: 0x3A3A40,
    tile: 0x26262B,
    tile_ico: 0x1E3A5F,
    tile_ico_fg: 0x93C5FD,
    chip_on: 0x4A3A10,
    chip_on_fg: 0xFDE68A,
    chip_off: 0x123D24,
    chip_off_fg: 0x86EFAC,
    chip_err: 0x3A2A2A,
    chip_err_fg: 0xFCA5A5,
    note: 0x1E2A3D,
    note_edge: 0x2F4A73,
    warn: 0x3A2E12,
    warn_edge: 0x6B5418,
    btn: 0x34343A,
    btn_edge: 0x4A4A52,
    foot: 0x1A1A1D,
    disabled: 0x8A8A8A,
    b: B_DARK,
};

pub(crate) const LIGHT: Theme = Theme {
    dark: false,
    hc: false,
    surface: 0xFFFFFF,
    text: 0x1A1A1A,
    dim: 0x555555,
    stroke: 0xB8B8B8,
    accent: 0x1F6FDB,
    on_accent: 0xFFFFFF,
    band_a: 0x1F6FDB,
    band_b: 0x7C3AED,
    on_band: 0xFFFFFF,
    card: 0xF5F7FB,
    card_edge: 0xE3E8F2,
    tile: 0xF5F7FB,
    tile_ico: 0xDBEAFE,
    tile_ico_fg: 0x1D4ED8,
    chip_on: 0xFEF3C7,
    chip_on_fg: 0x7A4A00,
    chip_off: 0xDCFCE7,
    chip_off_fg: 0x14532D,
    chip_err: 0xFEE2E2,
    chip_err_fg: 0x7F1D1D,
    note: 0xEFF6FF,
    note_edge: 0xBFDBFE,
    warn: 0xFFFBEB,
    warn_edge: 0xFCD34D,
    btn: 0xFFFFFF,
    btn_edge: 0xC9CED8,
    foot: 0xF5F7FB,
    disabled: 0x767676,
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
        surface: bg,
        text: fg,
        dim: fg,
        stroke: fg,
        accent: hl,
        on_accent: hl_fg,
        band_a: bg,
        band_b: bg,
        on_band: fg,
        card: bg,
        card_edge: fg,
        tile: bg,
        tile_ico: bg,
        tile_ico_fg: fg,
        chip_on: bg,
        chip_on_fg: fg,
        chip_off: bg,
        chip_off_fg: fg,
        chip_err: bg,
        chip_err_fg: fg,
        note: bg,
        note_edge: fg,
        warn: bg,
        warn_edge: fg,
        btn: bg,
        btn_edge: fg,
        foot: bg,
        disabled: gray,
        b: B {
            bg,
            card: bg,
            line: fg,
            tx: fg,
            tx2: fg,
            tx3: gray,
            acc: hl,
            on_acc: hl_fg,
            track: bg,
            hi: hl,
            warn: fg,
            good: fg,
            edge: fg,
            bar_off: fg,
            ws: [hl; 9],
            on_ws: hl_fg,
            ws_edge: 0.0,
            bar_edge: 0.0,
            alpha: 1.0,
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
}

thread_local! {
    static LOOK: Cell<Option<Look>> = const { Cell::new(None) };
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

/// After `refresh` saw a change: every des-k window of this thread hears WM_SETTINGCHANGE and
/// repaints in the new look.
pub(crate) fn repaint_all() {
    unsafe extern "system" fn post(h: HWND, _: LPARAM) -> BOOL {
        unsafe { let _ = PostMessageW(Some(h), WM_SETTINGCHANGE, WPARAM(0), LPARAM(0)); }
        BOOL(1)
    }
    unsafe { let _ = EnumThreadWindows(GetCurrentThreadId(), Some(post), LPARAM(0)); }
}

fn theme_of(light: bool, l: Look) -> Theme {
    let mut t = match l.hc {
        Some(sys) => return high_contrast(sys),
        None if light => LIGHT,
        None => DARK,
    };
    if !l.transparency {
        t.b.alpha = 1.0;
    }
    t
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
        let h = CreateWindowExW(ex, class, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None).ok()?;
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

/// The finder's and overview's chosen color until their look B slices (spec 0017).
pub(crate) const ACCENT: COLORREF = COLORREF(0x00FA_A560); // RGB(96, 165, 250), COLORREF is 0x00BBGGRR

pub(crate) fn cref(c: u32) -> COLORREF {
    COLORREF((c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF))
}

/// Opaque GDI+ color.
pub(crate) fn op(c: u32) -> u32 {
    0xFF00_0000 | c
}

/// GDI+ color at `a` (0 to 1) opacity.
pub(crate) fn alpha(c: u32, a: f32) -> u32 {
    ((a * 255.0).round() as u32) << 24 | c
}

/// `a` moved toward `b` by `f`.
pub(crate) fn mix(a: u32, b: u32, f: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = ((a >> s & 0xFF) as f32, (b >> s & 0xFF) as f32);
        ((x + (y - x) * f).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

pub(crate) fn dark_title(h: HWND, t: &Theme) {
    let on = BOOL(t.dark as i32);
    unsafe { let _ = DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const BOOL as _, size_of::<BOOL>() as u32); }
}

/// The fonts, as `F as usize` indexes.
#[derive(Clone, Copy)]
pub(crate) enum F {
    Body,
    Bold,
    Small,
    SmallBold,
    Glyph,
    H1,
    H2,
    Title,  // the settings page's headings
    Mono,   // setting names as the file spells them
    Number, // the indicator's workspace number
    Icon,   // Segoe Fluent Icons
}

/// Per F: face (0 text, 1 display, 2 mono, 3 icons), size in dip, weight: 400 or 600 only
/// (spec 0017 AC-3).
const FACES: [(u8, f32, i32); 11] =
    [(0, 14.0, 400), (0, 14.0, 600), (0, 12.0, 400), (0, 12.0, 600), (0, 14.0, 600), (1, 28.0, 600), (1, 16.0, 600), (1, 20.0, 600), (2, 12.0, 400), (1, 36.0, 600), (3, 16.0, 400)];

/// Every F at one scale; deleted when dropped.
pub(crate) struct Fonts([HFONT; FACES.len()]);

thread_local! {
    static GDIPLUS: Cell<Option<bool>> = const { Cell::new(None) };
    static VARIABLE: Cell<Option<bool>> = const { Cell::new(None) };
    static FLUENT: Cell<Option<bool>> = const { Cell::new(None) };
    static SHARED: RefCell<HashMap<u32, Rc<Fonts>>> = RefCell::new(HashMap::new());
}

/// Every F at `scale`, made once per scale and kept for des-k's life, so a screen that opens
/// often never makes its fonts again.
pub(crate) fn fonts(scale: f32) -> Rc<Fonts> {
    let key = (scale * 100.0).round() as u32;
    SHARED.with_borrow_mut(|m| m.entry(key).or_insert_with(|| Rc::new(Fonts::new(scale))).clone())
}

impl Fonts {
    pub(crate) fn new(scale: f32) -> Fonts {
        let variable = variable();
        Fonts(FACES.map(|(kind, size, weight)| {
            let face = match (variable, kind) {
                (_, 2) => "Consolas",
                (_, 3) if fluent() => "Segoe Fluent Icons",
                (_, 3) => "Segoe MDL2 Assets", // the same codes

                (true, 1) => "Segoe UI Variable Display",
                (true, _) => "Segoe UI Variable Text",
                (false, _) => "Segoe UI",
            };
            // GDI fakes a 600 weight from the regular face (heavier and wider); the real
            // semibold is its own family.
            let face = if weight == 600 { format!("{face} Semibold") } else { face.to_string() };
            // Icons are thin lines: ClearType gives them colored fringes, so they get grey smoothing.
            let quality = if kind == 3 { ANTIALIASED_QUALITY } else { CLEARTYPE_QUALITY };
            font_with(PCWSTR(HSTRING::from(face).as_ptr()), size * scale, weight, quality)
        }))
    }

    pub(crate) fn get(&self, f: F) -> HFONT {
        self.0[f as usize]
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for f in self.0 {
            unsafe { let _ = DeleteObject(f.into()); }
        }
    }
}

/// Whether Segoe UI Variable is installed. A missing face is silently swapped for another
/// font: check once, and use plain Segoe UI then.
pub(crate) fn variable() -> bool {
    VARIABLE.get().unwrap_or_else(|| {
        let has = has_face(w!("Segoe UI Variable Text"));
        if !has {
            eprintln!("fonts: Segoe UI Variable is not installed, des-k uses Segoe UI");
        }
        VARIABLE.set(Some(has));
        has
    })
}

/// Whether Segoe Fluent Icons is installed, checked once like `variable`.
fn fluent() -> bool {
    FLUENT.get().unwrap_or_else(|| {
        let has = has_face(w!("Segoe Fluent Icons"));
        if !has {
            eprintln!("fonts: Segoe Fluent Icons is not installed, des-k uses Segoe MDL2 Assets");
        }
        FLUENT.set(Some(has));
        has
    })
}

/// A font `px` pixels tall; the caller deletes it.
pub(crate) fn font(face: PCWSTR, px: f32, weight: i32) -> HFONT {
    font_with(face, px, weight, CLEARTYPE_QUALITY)
}

fn font_with(face: PCWSTR, px: f32, weight: i32, quality: FONT_QUALITY) -> HFONT {
    unsafe {
        CreateFontW(
            -(px.round() as i32),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            quality,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            face,
        )
    }
}

fn has_face(face: PCWSTR) -> bool {
    let f = font(face, 14.0, 400);
    let mut name = [0u16; 64];
    unsafe {
        let dc = GetDC(None);
        let old = SelectObject(dc, f.into());
        let n = GetTextFaceW(dc, Some(&mut name)).max(1) as usize - 1;
        SelectObject(dc, old);
        ReleaseDC(None, dc);
        let _ = DeleteObject(f.into());
        name[..n] == *face.as_wide()
    }
}

/// GDI+ starts once, the first time a screen that uses it opens, and lives as long as des-k.
pub(crate) fn gdiplus() -> bool {
    if let Some(ok) = GDIPLUS.get() {
        return ok;
    }
    let mut token = 0usize;
    let input = gp::GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
    let status = unsafe { gp::GdiplusStartup(&mut token, &input, std::ptr::null_mut()) };
    let ok = status.0 == 0;
    if !ok {
        eprintln!("welcome: GDI+ did not start (status {}), so the screen draws its text only", status.0);
    }
    GDIPLUS.set(Some(ok));
    ok
}

/// A box: x, y, width, height, in dip.
pub(crate) type Bx = (f32, f32, f32, f32);
/// A run of text: the text, its font and color.
type Run<'a> = (&'a str, F, u32);

/// Draws (or, without `draw`, only measures) in dip. Shapes go through GDI+, text through GDI.
pub(crate) struct Canvas<'a> {
    pub(crate) dc: HDC,
    g: *mut gp::GpGraphics, // null: measuring, or GDI+ did not start
    pub(crate) draw: bool,
    pub(crate) s: f32,
    pub(crate) t: Theme,
    fonts: &'a Fonts,
}

impl Drop for Canvas<'_> {
    fn drop(&mut self) {
        if !self.g.is_null() {
            unsafe { gp::GdipDeleteGraphics(self.g) };
        }
    }
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(dc: HDC, draw: bool, s: f32, t: Theme, fonts: &'a Fonts) -> Canvas<'a> {
        let mut g = std::ptr::null_mut();
        if draw && GDIPLUS.get() == Some(true) {
            unsafe {
                gp::GdipCreateFromHDC(dc, &mut g);
                if !g.is_null() {
                    gp::GdipSetSmoothingMode(g, gp::SmoothingModeAntiAlias);
                    gp::GdipSetPixelOffsetMode(g, gp::PixelOffsetModeHalf);
                }
            }
        }
        Canvas { dc, g, draw, s, t, fonts }
    }

    /// The same canvas, measuring only: a layout can size a part before drawing it.
    pub(crate) fn measuring(&self) -> Canvas<'a> {
        Canvas { dc: self.dc, g: std::ptr::null_mut(), draw: false, s: self.s, t: self.t, fonts: self.fonts }
    }

    pub(crate) fn shapes(&self) -> bool {
        self.draw && !self.g.is_null()
    }

    /// GDI on a DC GDI+ draws on goes through GDI+'s own handle to it.
    fn with_dc<R>(&self, f: impl FnOnce(HDC) -> R) -> R {
        if self.g.is_null() {
            return f(self.dc);
        }
        let mut dc = HDC::default();
        unsafe { gp::GdipGetDC(self.g, &mut dc) };
        let r = f(dc);
        unsafe { gp::GdipReleaseDC(self.g, dc) };
        r
    }

    pub(crate) fn width(&self, text: &str, f: F) -> f32 {
        let wide: Vec<u16> = text.encode_utf16().collect();
        self.with_dc(|dc| unsafe {
            let old = SelectObject(dc, self.fonts.get(f).into());
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            SelectObject(dc, old);
            size.cx as f32 / self.s
        })
    }

    /// `text` cut with … to fit `w`, or whole when it fits.
    pub(crate) fn fit(&self, text: &str, f: F, w: f32) -> String {
        if self.width(text, f) <= w {
            return text.to_string();
        }
        let chars: Vec<char> = text.chars().collect();
        let cut = |n: usize| chars[..n].iter().collect::<String>().trim_end().to_string() + "…";
        let (mut lo, mut hi) = (0, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.width(&cut(mid), f) <= w { lo = mid } else { hi = mid - 1 }
        }
        cut(lo)
    }

    /// One run of text, centered in a line box `lh` tall whose top is `y`.
    pub(crate) fn text(&self, x: f32, y: f32, lh: f32, text: &str, f: F, color: u32) {
        if !self.draw {
            return;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        self.with_dc(|dc| unsafe {
            let old = SelectObject(dc, self.fonts.get(f).into());
            let mut tm = TEXTMETRICW::default();
            let _ = GetTextMetricsW(dc, &mut tm);
            SetTextColor(dc, cref(color));
            SetBkMode(dc, TRANSPARENT);
            let top = (y * self.s + (lh * self.s - tm.tmHeight as f32) / 2.0).round() as i32;
            let _ = TextOutW(dc, (x * self.s).round() as i32, top, &wide);
            SelectObject(dc, old);
        });
    }

    /// Wrapped text from runs (text, font, color) that flow on; breaks only at spaces.
    /// Returns its height.
    pub(crate) fn para(&self, x: f32, y: f32, w: f32, lh: f32, runs: &[(&str, F, u32)]) -> f32 {
        // Words glued to the one before (no space between) never break from it.
        let mut words: Vec<(bool, Vec<Run>)> = vec![];
        let mut space = false;
        for &(text, f, c) in runs {
            for (i, piece) in text.split(' ').enumerate() {
                space |= i > 0;
                if piece.is_empty() {
                    continue;
                }
                match words.last_mut() {
                    Some(last) if !space => last.1.push((piece, f, c)),
                    _ => words.push((space && !words.is_empty(), vec![(piece, f, c)])),
                }
                space = false;
            }
        }
        let (mut line, mut cx) = (0.0, 0.0);
        for (space, pieces) in words {
            let ww: f32 = pieces.iter().map(|(t, f, _)| self.width(t, *f)).sum();
            let sw = if space { self.width(" ", pieces[0].1) } else { 0.0 };
            if cx > 0.0 && cx + sw + ww > w + 0.01 {
                line += 1.0;
                cx = 0.0;
            } else if cx > 0.0 {
                cx += sw;
            }
            for (t, f, c) in pieces {
                self.text(x + cx, y + line * lh, lh, t, f, c);
                cx += self.width(t, f);
            }
        }
        (line + 1.0) * lh
    }

    fn brush(&self, argb: u32) -> *mut gp::GpBrush {
        let mut b = std::ptr::null_mut();
        unsafe { gp::GdipCreateSolidFill(argb, &mut b) };
        b.cast()
    }

    fn pen(&self, argb: u32, width: f32) -> *mut gp::GpPen {
        let mut p = std::ptr::null_mut();
        unsafe {
            gp::GdipCreatePen1(argb, width * self.s, gp::UnitPixel, &mut p);
            gp::GdipSetPenLineCap197819(p, gp::LineCapRound, gp::LineCapRound, gp::DashCapFlat);
            gp::GdipSetPenLineJoin(p, gp::LineJoinRound);
        }
        p
    }

    fn rr_path(&self, (x, y, w, h): Bx, r: f32) -> *mut gp::GpPath {
        let s = self.s;
        let (x, y, w, h) = (x * s, y * s, w * s, h * s);
        let d = (2.0 * r * s).min(w).min(h);
        let mut p = std::ptr::null_mut();
        unsafe {
            gp::GdipCreatePath(gp::FillModeAlternate, &mut p);
            if d < 0.5 {
                gp::GdipAddPathRectangle(p, x, y, w, h);
            } else {
                gp::GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
                gp::GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
                gp::GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
                gp::GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
            }
            gp::GdipClosePathFigure(p);
        }
        p
    }

    pub(crate) fn fill_rr(&self, (x, y, w, h): Bx, r: f32, argb: u32) {
        if !self.shapes() || w <= 0.0 || h <= 0.0 {
            return;
        }
        let (p, b) = (self.rr_path((x, y, w, h), r), self.brush(argb));
        unsafe {
            gp::GdipFillPath(self.g, b, p);
            gp::GdipDeleteBrush(b);
            gp::GdipDeletePath(p);
        }
    }

    /// Stroked along the shape itself (half inside, half outside), like SVG.
    pub(crate) fn stroke_rr(&self, (x, y, w, h): Bx, r: f32, width: f32, argb: u32) {
        if !self.shapes() {
            return;
        }
        let (p, pen) = (self.rr_path((x, y, w, h), r), self.pen(argb, width));
        unsafe {
            gp::GdipDrawPath(self.g, pen, p);
            gp::GdipDeletePen(pen);
            gp::GdipDeletePath(p);
        }
    }

    /// A box with an optional one pixel border inside its edge, like CSS.
    pub(crate) fn boxed(&self, (x, y, w, h): Bx, r: f32, fill: u32, edge: Option<u32>) {
        match edge {
            Some(e) => {
                let b = self.s.floor().max(1.0) / self.s;
                self.fill_rr((x, y, w, h), r, op(e));
                self.fill_rr((x + b, y + b, w - 2.0 * b, h - 2.0 * b), (r - b).max(0.0), op(fill));
            }
            None => self.fill_rr((x, y, w, h), r, op(fill)),
        }
    }

    pub(crate) fn circle(&self, cx: f32, cy: f32, r: f32, fill: Option<u32>, edge: Option<(u32, f32)>) {
        if !self.shapes() {
            return;
        }
        let s = self.s;
        let (x, y, d) = ((cx - r) * s, (cy - r) * s, 2.0 * r * s);
        unsafe {
            if let Some(argb) = fill {
                let b = self.brush(argb);
                gp::GdipFillEllipse(self.g, b, x, y, d, d);
                gp::GdipDeleteBrush(b);
            }
            if let Some((argb, width)) = edge {
                let pen = self.pen(argb, width);
                gp::GdipDrawEllipse(self.g, pen, x, y, d, d);
                gp::GdipDeletePen(pen);
            }
        }
    }

    pub(crate) fn lines(&self, pts: &[(f32, f32)], width: f32, argb: u32) {
        if !self.shapes() {
            return;
        }
        let pts: Vec<gp::PointF> = pts.iter().map(|(x, y)| gp::PointF { X: x * self.s, Y: y * self.s }).collect();
        let pen = self.pen(argb, width);
        unsafe {
            gp::GdipDrawLines(self.g, pen, pts.as_ptr(), pts.len() as i32);
            gp::GdipDeletePen(pen);
        }
    }

    pub(crate) fn gradient(&self, x: f32, y: f32, w: f32, h: f32, a: u32, b: u32) {
        if !self.shapes() {
            return;
        }
        let s = self.s;
        let rect = gp::RectF { X: x * s, Y: y * s, Width: w * s, Height: h * s };
        let mut brush = std::ptr::null_mut();
        unsafe {
            // CSS 120deg: toward the bottom right, 30 degrees below the x axis.
            gp::GdipCreateLineBrushFromRectWithAngle(&rect, op(a), op(b), 30.0, true, gp::WrapModeTile, &mut brush);
            gp::GdipFillRectangle(self.g, brush.cast(), rect.X, rect.Y, rect.Width, rect.Height);
            gp::GdipDeleteBrush(brush.cast());
        }
    }

    /// A check mark in a `size` square at x, y.
    pub(crate) fn check(&self, x: f32, y: f32, size: f32, width: f32, argb: u32) {
        self.lines(&[(x, y + size * 0.55), (x + size * 0.36, y + size * 0.9), (x + size, y + size * 0.12)], width, argb);
    }

    /// The circled i of an information banner.
    pub(crate) fn info(&self, cx: f32, cy: f32, argb: u32) {
        self.circle(cx, cy, 6.5, None, Some((argb, 1.25)));
        self.circle(cx, cy - 3.0, 0.95, Some(argb), None);
        self.lines(&[(cx, cy - 0.75), (cx, cy + 3.25)], 1.4, argb);
    }
}

/// Paints into a back buffer `w` by `h` px, then copies it to `dc` at x, y in one step.
pub(crate) fn buffered(dc: HDC, x: i32, y: i32, w: i32, h: i32, f: impl FnOnce(HDC)) {
    unsafe {
        let mem = CreateCompatibleDC(Some(dc));
        let bmp = CreateCompatibleBitmap(dc, w.max(1), h.max(1));
        let old = SelectObject(mem, bmp.into());
        f(mem);
        let _ = BitBlt(dc, x, y, w, h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
    }
}

/// One line of GDI text in `r`, cut with … when too long: the finder's and overview's.
pub(crate) fn text(dc: HDC, s: &str, mut r: RECT, color: COLORREF, align: DRAW_TEXT_FORMAT) {
    let mut wide: Vec<u16> = s.encode_utf16().collect();
    unsafe {
        SetTextColor(dc, color);
        DrawTextW(dc, &mut wide, &mut r, align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX);
    }
}

/// Reads a window's state, or None when the window is closed. A message can come back into
/// the window while its state is borrowed (see the callers); that read is skipped, and logged
/// as `who`, rather than panicking.
pub(crate) fn get<T: 'static, R>(key: &'static LocalKey<RefCell<Option<T>>>, who: &str, f: impl FnOnce(&T) -> R) -> Option<R> {
    key.with(|cell| match cell.try_borrow() {
        Ok(w) => w.as_ref().map(f),
        Err(_) => {
            eprintln!("{who}: skipped, the window's state is already borrowed (a message came back in)");
            None
        }
    })
}

/// Changes a window's state, as `get` reads it.
pub(crate) fn set<T: 'static, R>(key: &'static LocalKey<RefCell<Option<T>>>, who: &str, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    key.with(|cell| match cell.try_borrow_mut() {
        Ok(mut w) => w.as_mut().map(f),
        Err(_) => {
            eprintln!("{who}: skipped, the window's state is already borrowed (a message came back in)");
            None
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use windows::core::GUID;

    /// Draws `f` into a `w` by `h` bitmap and saves it as a PNG: the render tests' output.
    pub(crate) fn save_png(w: i32, h: i32, path: &std::path::Path, f: impl FnOnce(HDC)) {
        unsafe {
            let screen = GetDC(None);
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp.into());
            f(mem);
            SelectObject(mem, old);
            let mut image = std::ptr::null_mut();
            gp::GdipCreateBitmapFromHBITMAP(bmp, HPALETTE::default(), &mut image);
            let png = GUID::from_u128(0x557cf406_1a04_11d3_9a73_0000f81ef32e);
            let status = gp::GdipSaveImageToFile(image.cast(), &HSTRING::from(path.as_os_str()), &png, std::ptr::null());
            assert_eq!(status.0, 0, "{}", path.display());
            gp::GdipDisposeImage(image.cast());
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
        }
    }
}
