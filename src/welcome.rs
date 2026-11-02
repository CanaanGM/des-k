// The welcome screen (spec 0011, 0011-first-run-setup, Welcome screen): what pdesk is, Windows'
// three and four finger gestures row by row with what pdesk does for each, and the Use, Choose
// my own and Give back buttons, built to the approved H2 design
// (pdesk-design/welcome/micro-suite-r4.html) plus its 2026-11-01 change. Lives on the main thread: its
// messages come through main's loop, which passes them through IsDialogMessageW so Tab,
// Enter and Esc work. GDI draws the text, GDI+ the smooth shapes, into a back buffer.
// Sizes are in dip (1/96 inch) and scaled to the screen's dpi when drawn.
use std::cell::{Cell, RefCell};
use windows::Win32::{
    Foundation::{COLORREF, ERROR_SUCCESS, HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM},
    Graphics::{
        Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
        Gdi::*,
        GdiPlus as gp,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    },
    UI::{
        Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
        Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_NOFOCUSRECT, ODS_SELECTED},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::{EnableWindow, GetActiveWindow, GetFocus, IsWindowEnabled, SetFocus},
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

use crate::settings::Motion;
use crate::setup::{self, GaveBack, Row, State, Wrote};

const CLASS: PCWSTR = w!("pdesk-welcome");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_CLIPCHILDREN.0);

// The buttons, created in Tab order. Close is IDCANCEL, so Esc presses it.
const ID_CARD: i32 = 101; // Use / Open Touchpad settings / Try giving back again
const ID_CARD2: i32 = 102; // Open Touchpad settings, beside the disabled Use
const ID_OWN: i32 = 103;
const ID_GIVE: i32 = 104;
const ID_CLOSE: i32 = IDCANCEL.0;
const IDS: [i32; 5] = [ID_CARD, ID_CARD2, ID_OWN, ID_GIVE, ID_CLOSE];

const USE: &str = "Use these gestures";
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
}

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
};

/// High contrast: Windows' own colors only, no gradient and no tints; borders instead.
fn high_contrast() -> Theme {
    let sys = |i: SYS_COLOR_INDEX| {
        let c = unsafe { GetSysColor(i) };
        (c & 0xFF) << 16 | (c & 0xFF00) | (c >> 16 & 0xFF)
    };
    let (bg, fg) = (sys(COLOR_WINDOW), sys(COLOR_WINDOWTEXT));
    let light = (bg >> 16 & 0xFF) + (bg >> 8 & 0xFF) + (bg & 0xFF) > 3 * 128;
    Theme {
        dark: !light,
        hc: true,
        surface: bg,
        text: fg,
        dim: fg,
        stroke: fg,
        accent: sys(COLOR_HIGHLIGHT),
        on_accent: sys(COLOR_HIGHLIGHTTEXT),
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
        disabled: sys(COLOR_GRAYTEXT),
    }
}

/// Windows' app mode, light or dark, unless high contrast is on.
pub(crate) fn theme() -> Theme {
    let mut hc = HIGHCONTRASTW { cbSize: size_of::<HIGHCONTRASTW>() as u32, ..Default::default() };
    let read = unsafe { SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, Some((&mut hc as *mut HIGHCONTRASTW).cast()), Default::default()) };
    if read.is_ok() && hc.dwFlags.0 & HCF_HIGHCONTRASTON.0 != 0 {
        return high_contrast();
    }
    let (mut light, mut size) = (1u32, 4u32);
    let key = w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let e = unsafe { RegGetValueW(HKEY_CURRENT_USER, key, w!("AppsUseLightTheme"), RRF_RT_REG_DWORD, None, Some((&mut light as *mut u32).cast()), Some(&mut size)) };
    if e == ERROR_SUCCESS && light == 0 { DARK } else { LIGHT }
}

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
    Title, // the settings page's headings
    Mono,  // setting names as the file spells them
}

/// Per F: face (0 text, 1 display, 2 mono), size in dip, weight.
const FACES: [(u8, f32, i32); 9] = [(0, 14.0, 400), (0, 14.0, 600), (0, 12.0, 400), (0, 12.0, 600), (0, 14.0, 700), (1, 28.0, 600), (1, 16.0, 600), (1, 20.0, 600), (2, 12.0, 400)];

pub(crate) struct Fonts([HFONT; 9]);

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
    /// The last give back's result: kept until pdesk exits, a later give back replaces it, or
    /// Windows' gestures read off again, so reopening the screen shows the same rows.
    static LAST: Cell<Option<GaveBack>> = const { Cell::new(None) };
    static GDIPLUS: Cell<Option<bool>> = const { Cell::new(None) };
    static VARIABLE: Cell<Option<bool>> = const { Cell::new(None) };
}

impl Fonts {
    pub(crate) fn new(scale: f32) -> Fonts {
        // A missing face is silently swapped for another font: check once, and use plain Segoe UI then.
        let variable = VARIABLE.get().unwrap_or_else(|| {
            let has = has_face(w!("Segoe UI Variable Text"));
            VARIABLE.set(Some(has));
            has
        });
        Fonts(FACES.map(|(kind, size, weight)| {
            let face = match (variable, kind) {
                (_, 2) => "Consolas",
                (true, 1) => "Segoe UI Variable Display",
                (true, _) => "Segoe UI Variable Text",
                (false, _) => "Segoe UI",
            };
            // GDI fakes a 600 weight from the regular face (heavier and wider); the real
            // semibold is its own family.
            let face = if weight == 600 { format!("{face} Semibold") } else { face.to_string() };
            font(PCWSTR(HSTRING::from(face).as_ptr()), size * scale, weight)
        }))
    }

    pub(crate) fn get(&self, f: F) -> HFONT {
        self.0[f as usize]
    }

    pub(crate) fn delete(&self) {
        for f in self.0 {
            unsafe { let _ = DeleteObject(f.into()); }
        }
    }
}

fn font(face: PCWSTR, px: f32, weight: i32) -> HFONT {
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
            CLEARTYPE_QUALITY,
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

/// GDI+ starts once, the first time the screen opens, and lives as long as pdesk.
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
}

// Never hold the borrow across a call that sends this window a message: a button redraws
// itself at once on focus, enable and text changes, and that comes back as WM_DRAWITEM.
fn get<R>(f: impl FnOnce(&Win) -> R) -> Option<R> {
    WIN.with(|w| w.try_borrow().ok().and_then(|w| w.as_ref().map(f)))
}

fn set<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WIN.with(|w| w.try_borrow_mut().ok().and_then(|mut w| w.as_mut().map(f)))
}

fn hwnd() -> Option<HWND> {
    get(|w| w.hwnd)
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
    places: Vec<Place>,
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
        Canvas { dc, g, draw, s, t, fonts, places: vec![] }
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
pub(crate) fn measure(c: &mut Canvas, f: impl FnOnce(&mut Canvas) -> f32) -> f32 {
    let draw = std::mem::replace(&mut c.draw, false);
    let h = f(c);
    c.draw = draw;
    h
}

/// The whole screen, top to bottom. Returns its height.
fn compose(c: &mut Canvas, v: &View) -> f32 {
    let t = c.t;
    // The band.
    if t.hc {
        c.boxed((0.0, 0.0, W, BAND), 0.0, t.surface, None);
        c.boxed((0.0, BAND - 1.0, W, 1.0), 0.0, t.stroke, None);
    } else {
        c.gradient(0.0, 0.0, W, BAND, t.band_a, t.band_b);
    }
    c.text(SIDE, 14.0, 36.0, "Welcome to pdesk!", F::H1, t.on_band);
    c.text(SIDE, 52.0, 20.0, "Every screen gets its own desktops.", F::Body, t.on_band);
    art(c, W - 18.0 - 172.0, BAND - 8.0 - 62.0, t.on_band);
    let mut y = BAND + 16.0;
    let intro = "pdesk gives each of your screens its own set of workspaces. You switch the screen under your mouse, and your other screens stay where they are. pdesk never moves or resizes your windows. It only hides and shows them.";
    y += c.para(SIDE, y, IN, 20.0, &[(intro, F::Body, t.text)]) + 16.0;
    tiles(c, y);
    y += 56.0 + 16.0;
    // The heading, with the count at its right.
    pad_icon(c, SIDE, y + 1.0, op(t.text));
    c.text(SIDE + 28.0, y, 22.0, "Your touchpad gestures", F::H2, t.text);
    if v.st.enabled && v.back.is_none() {
        let n = v.st.rows.iter().filter(|r| **r == Row::Nothing).count();
        let count = format!("Windows: {n} of 4 off");
        let cw = c.width(&count, F::SmallBold);
        c.text(SIDE + IN - cw, y + 3.0, 16.0, &count, F::SmallBold, t.dim);
    }
    y += 22.0 + 8.0;
    // The card: border 1, padding 12 on top and 14 below.
    let h = measure(c, |c| card(c, v, SIDE + 15.0, y + 13.0)) + 28.0;
    c.boxed((SIDE, y, IN, h), 12.0, t.card, Some(t.card_edge));
    card(c, v, SIDE + 15.0, y + 13.0);
    y += h + 24.0;
    // The footer: border 1 on top, padding 14.
    let fh = measure(c, |c| foot(c, v, y + 15.0)) + 29.0;
    c.boxed((0.0, y, W, fh), 0.0, t.foot, None);
    c.boxed((0.0, y, W, 1.0), 0.0, t.card_edge, None);
    foot(c, v, y + 15.0);
    y + fh
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

/// The card's contents at x, top. Returns their height.
fn card(c: &mut Canvas, v: &View, x: f32, top: f32) -> f32 {
    let t = c.t;
    let st = &v.st;
    let all = st.rows.iter().all(|r| *r == Row::Nothing);
    let mut y = top;
    // Reading off wins over no touchpad: then pdesk runs nothing, so the rows would mislead.
    let (primary, open) = if !st.enabled {
        let sub = "Windows' three and four finger gestures are set to Nothing, so right now they do nothing. Turn reading back on with Choose my own, or set them back in Windows Settings.";
        let sub = st.off.then_some(sub);
        y += banner(c, x, y, CARD_IN, st.off, "Touchpad reading is off in pdesk's settings.", sub.as_slice());
        (Some((USE, false)), st.off)
    } else {
        match v.back {
            Some(b) => {
                let again = "Your saved values are kept, so you can try again. Settings is open if you want gestures now.";
                let (warn, title, subs) = if !b.failed() {
                    let mut subs = vec!["To have them now, set them in Settings (it's open). pdesk has stopped reading gestures.".to_string()];
                    if !b.cleared {
                        subs.push("pdesk couldn't clear its copy of your values (the log says why). Press Give gestures back again to clear it.".into());
                    }
                    (false, "Your gestures come back at your next sign in.", subs)
                } else if b.wrote.iter().all(|w| *w == Wrote::NotOne) {
                    (true, "Giving back didn't work.", vec![format!("Nothing was written back (the log says why). {again}")])
                } else {
                    let sub = format!("Some values were written back for your next sign in, some weren't (the log names them). {again}");
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
                        let text = format!("pdesk can't read the {} setting, so it leaves every gesture to Windows. The log has the details.", setup::ROWS[i].0);
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
        y += 14.0;
        y += rows(c, v, x, y);
        if v.back.is_none() {
            // Only while pdesk reads the touchpad: after a give back it runs nothing.
            let edits = "What pdesk runs now. Choose my own changes them, and changes apply at once.";
            y += 12.0 + c.para(x, y + 12.0, CARD_IN, 16.0, &[(edits, F::Small, t.dim)]);
            let after = match all {
                _ if !st.pad => None,
                true => (!st.saved).then_some("You set them to Nothing yourself, so Quit has nothing to give back."),
                false => Some("pdesk runs them once all four are off, even over admin windows. After that, quitting pdesk gives them back from your next sign in."),
            };
            if let Some(a) = after {
                y += 6.0 + c.para(x, y + 6.0, CARD_IN, 16.0, &[(a, F::Small, t.dim)]);
            }
        }
        let primary = match v.back {
            Some(b) if b.failed() => Some((AGAIN, true)),
            _ if !st.pad => Some((USE, false)),
            _ if all => None,
            None if st.saved => Some((OPEN, true)),
            _ => Some((USE, true)),
        };
        (primary, false)
    };
    // The buttons on one line, Choose my own wrapping below when it doesn't fit.
    y += 12.0;
    let mut bx = x;
    if let Some((label, enabled)) = primary {
        let w = c.pill_w(label, Kind::Primary);
        c.place(Place { id: ID_CARD, kind: Kind::Primary, x: bx, y, w, h: 32.0, back: t.card, label, enabled });
        bx += w + 8.0;
    }
    if open {
        let w = c.pill_w(OPEN, Kind::Quiet);
        c.place(Place { id: ID_CARD2, kind: Kind::Quiet, x: bx, y, w, h: 32.0, back: t.card, label: OPEN, enabled: true });
        bx += w + 8.0;
    }
    let w = c.pill_w(OWN, Kind::Quiet);
    if bx + w > x + CARD_IN {
        (bx, y) = (x, y + 32.0 + 8.0);
    }
    c.place(Place { id: ID_OWN, kind: Kind::Quiet, x: bx, y, w, h: 32.0, back: t.card, label: OWN, enabled: true });
    y + 32.0 - top
}

/// "pdesk runs these gestures", with a big check. Returns its height.
fn ok_block(c: &mut Canvas, x: f32, y: f32) -> f32 {
    let t = c.t;
    let (tx, tw) = (x + 42.0, CARD_IN - 42.0);
    let line = "Three and four finger swipes and taps now do what settings.toml says.";
    let th = 20.0 + measure(c, |c| c.para(tx, y, tw, 16.0, &[(line, F::Small, t.dim)]));
    let h = th.max(32.0);
    let cy = y + h / 2.0;
    c.circle(x + 16.0, cy, 16.0, Some(op(t.chip_off)), None);
    if t.hc {
        c.circle(x + 16.0, cy, 15.5, None, Some((op(t.stroke), 1.0)));
    }
    c.check(x + 9.5, cy - 6.0, 13.0, 2.0, op(t.chip_off_fg));
    let ty = y + (h - th) / 2.0;
    c.text(tx, ty, 20.0, "pdesk runs these gestures.", F::Bold, t.text);
    c.para(tx, ty + 20.0, tw, 16.0, &[(line, F::Small, t.dim)]);
    h
}

/// An information (or, with `warn`, a warning) banner. Returns its height.
fn banner(c: &mut Canvas, x: f32, y: f32, w: f32, warn: bool, title: &str, subs: &[&str]) -> f32 {
    let t = c.t;
    let (tx, tw) = (x + 1.0 + 10.0 + 28.0, w - 2.0 - 20.0 - 28.0);
    let body = |c: &mut Canvas| {
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

/// What pdesk does for this gesture: its once rule's label, and its twice rule's if any.
pub(crate) fn does(fingers: usize, motion: Motion) -> String {
    let g = &crate::settings::get().gestures;
    let (once, twice) = crate::settings::rules_for(fingers, motion);
    let mut text = once.map_or("nothing".into(), |i| g[i].action.label());
    if let Some(i) = twice {
        text += &format!(" · twice: {}", g[i].action.label());
    }
    text
}

/// The four rows: each one's name and chip, then what pdesk does for it (swipes two per
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
            None => match v.st.rows[i] {
                Row::Nothing => ("Nothing", true, t.chip_off, t.chip_off_fg),
                Row::StillOn => ("Still on", false, t.chip_on, t.chip_on_fg),
                Row::CantRead => ("Can't read", false, t.chip_err, t.chip_err_fg),
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
fn foot(c: &mut Canvas, v: &View, top: f32) -> f32 {
    let t = c.t;
    // While a failure banner shows, the card holds Try giving back again instead.
    let give = v.st.saved && !v.back.is_some_and(|b| b.failed());
    let left = |c: &mut Canvas, y: f32| {
        let mut h = 0.0;
        if v.st.saved {
            if give {
                let w = c.pill_w(GIVE, Kind::Quiet);
                c.place(Place { id: ID_GIVE, kind: Kind::Quiet, x: SIDE, y, w, h: 32.0, back: t.foot, label: GIVE, enabled: true });
                h += 36.0;
            }
            let line = "Gives back what you had the first time you chose Use, from your next sign in.";
            h += c.para(SIDE, y + h, 300.0, 16.0, &[(line, F::Small, t.dim)]);
        } else if v.first {
            let runs = [("You can open this again from the tray: ", F::Small, t.dim), ("Touchpad setup", F::SmallBold, t.dim), (".", F::Small, t.dim)];
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

fn read_view(first: bool) -> View {
    let st = setup::state();
    if st.off {
        LAST.set(None); // Windows' gestures are off again: the give back is over
    }
    View { st, first, back: LAST.get() }
}

/// The screen's height in dip and its buttons, measured with a screen DC.
fn layout(w: &Win) -> (f32, Vec<Place>) {
    unsafe {
        let dc = GetDC(None);
        let mut c = Canvas::new(dc, false, w.scale, w.theme, &w.fonts);
        let h = compose(&mut c, &w.view);
        let places = std::mem::take(&mut c.places);
        drop(c);
        ReleaseDC(None, dc);
        (h, places)
    }
}

/// The card button when it shows and works, else Skip or Close.
fn default_id(places: &[Place]) -> i32 {
    match places.iter().any(|p| p.id == ID_CARD && p.enabled) {
        true => ID_CARD,
        false => ID_CLOSE,
    }
}

/// Lays the screen out again and fits the window and its buttons to it; `to` moves it too.
fn relayout(to: Option<(i32, i32)>) {
    let Some((h, s, dpi, height, places, buttons)) = set(|w| {
        let (height, places) = layout(w);
        w.places.clone_from(&places);
        (w.hwnd, w.scale, w.dpi, height, places, w.buttons.clone())
    }) else {
        return;
    };
    let px = |v: f32| (v * s).round() as i32;
    unsafe {
        let mut r = RECT { left: 0, top: 0, right: px(W), bottom: px(height) };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        let (x, y) = to.unwrap_or_default();
        let flags = SWP_NOZORDER | SWP_NOACTIVATE | if to.is_none() { SWP_NOMOVE } else { SET_WINDOW_POS_FLAGS(0) };
        let _ = SetWindowPos(h, None, x, y, r.right - r.left, r.bottom - r.top, flags);
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

/// Main loop: the screen's keys (Tab, Enter, Esc, Space) through the dialog manager.
pub fn key(msg: &MSG) -> bool {
    hwnd().is_some_and(|h| unsafe { IsDialogMessageW(h, msg) }.as_bool())
}

pub(crate) fn dark_title(h: HWND, t: &Theme) {
    let on = BOOL(t.dark as i32);
    unsafe { let _ = DwmSetWindowAttribute(h, DWMWA_USE_IMMERSIVE_DARK_MODE, &on as *const BOOL as _, size_of::<BOOL>() as u32); }
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
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("pdesk"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
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
        let view = read_view(first);
        WIN.with_borrow_mut(|w| *w = Some(Win { hwnd: h, first, dpi, scale, theme, fonts: Fonts::new(scale), view, places: vec![], buttons, focus: None }));
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
                        let mut c = Canvas::new(mem, true, w.scale, w.theme, &w.fonts);
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, w.theme.surface, None);
                        compose(&mut c, &w.view);
                    })
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
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
                if let Some(w) = WIN.with_borrow_mut(Option::take) {
                    w.fonts.delete();
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xFFFF) as u32;
                let to = *(lp.0 as *const RECT);
                let _ = set(|w| {
                    w.fonts.delete();
                    w.dpi = dpi;
                    w.scale = dpi as f32 / 96.0;
                    w.fonts = Fonts::new(w.scale);
                });
                relayout(Some((to.left, to.top)));
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
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
pub(crate) mod tests {
    use super::*;
    use windows::core::GUID;

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
        for (tname, theme) in [("light", LIGHT), ("dark", DARK), ("hc", night_sky)] {
            for scale in [1.0f32, 1.5] {
                for (sname, v) in &states {
                    if scale > 1.0 && !matches!(*sname, "s1" | "s7") {
                        continue;
                    }
                    let fonts = Fonts::new(scale);
                    let fake = Win { hwnd: HWND::default(), first: v.first, dpi: (96.0 * scale) as u32, scale, theme, fonts, view: *v, places: vec![], buttons: vec![], focus: None };
                    let (h, places) = layout(&fake);
                    heights.push(format!("{tname} {sname} {}% {h} dip", scale * 100.0));
                    let (pw, ph) = ((W * scale).round() as i32, (h * scale).round() as i32);
                    save_png(pw, ph, &dir.join(format!("{sname}-{tname}-{}.png", scale * 100.0)), |dc| {
                        let mut c = Canvas::new(dc, true, scale, theme, &fake.fonts);
                        c.boxed((0.0, 0.0, W + 1.0, h + 1.0), 0.0, theme.surface, None);
                        compose(&mut c, v);
                        let default = default_id(&places);
                        for pl in &places {
                            let focus = *sname == "s1" && pl.id == default;
                            paint_button(&c, pl, (pl.x, pl.y, pl.w, pl.h), false, !pl.enabled, focus);
                        }
                    });
                    fake.fonts.delete();
                }
            }
        }
        std::fs::write(dir.join("heights.txt"), heights.join("\n")).unwrap();
    }

    fn hc_like(bg: u32, fg: u32, hi: u32, gray: u32) -> Theme {
        Theme { dark: true, hc: true, surface: bg, text: fg, dim: fg, stroke: fg, accent: hi, on_accent: bg, band_a: bg, band_b: bg, on_band: fg, card: bg, card_edge: fg, tile: bg, tile_ico: bg, tile_ico_fg: fg, chip_on: bg, chip_on_fg: fg, chip_off: bg, chip_off_fg: fg, chip_err: bg, chip_err_fg: fg, note: bg, note_edge: fg, warn: bg, warn_edge: fg, btn: bg, btn_edge: fg, foot: bg, disabled: gray }
    }

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
