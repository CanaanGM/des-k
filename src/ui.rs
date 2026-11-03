// The drawing layer des-k's own windows share: the theme (Windows' light, dark or high contrast
// colors), the fonts, the Canvas that draws shapes through GDI+ and text through GDI, the back
// buffer, plain GDI text for the finder and overview, and the try_borrow accessors for window
// state that a message can re-enter. Sizes on the Canvas are in dip (1/96 inch), scaled when drawn.
use std::cell::{Cell, RefCell};
use std::thread::LocalKey;
use windows::Win32::{
    Foundation::{COLORREF, ERROR_SUCCESS, HWND, RECT, SIZE},
    Graphics::{
        Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute},
        Gdi::*,
        GdiPlus as gp,
    },
    System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
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
    Title, // the settings page's headings
    Mono,  // setting names as the file spells them
}

/// Per F: face (0 text, 1 display, 2 mono), size in dip, weight.
const FACES: [(u8, f32, i32); 9] = [(0, 14.0, 400), (0, 14.0, 600), (0, 12.0, 400), (0, 12.0, 600), (0, 14.0, 700), (1, 28.0, 600), (1, 16.0, 600), (1, 20.0, 600), (2, 12.0, 400)];

/// Every F at one scale; deleted when dropped.
pub(crate) struct Fonts([HFONT; 9]);

thread_local! {
    static GDIPLUS: Cell<Option<bool>> = const { Cell::new(None) };
    static VARIABLE: Cell<Option<bool>> = const { Cell::new(None) };
}

impl Fonts {
    pub(crate) fn new(scale: f32) -> Fonts {
        let variable = variable();
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
        VARIABLE.set(Some(has));
        has
    })
}

/// A font `px` pixels tall; the caller deletes it.
pub(crate) fn font(face: PCWSTR, px: f32, weight: i32) -> HFONT {
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
