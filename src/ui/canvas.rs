// The Canvas that draws shapes through GDI+ and text through GDI, in dip (1/96 inch), plus
// GDI+ startup, the back buffer and per pixel alpha popups.
use super::*;
use super::color::op;
use super::fonts::{F, Fonts, fonts};

thread_local! {
    static GDIPLUS: Cell<Option<bool>> = const { Cell::new(None) };
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
        warn!("welcome: GDI+ did not start (status {}), so the screen draws its text only", status.0);
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
    o: (i32, i32), // the pixel drawn at the DC's 0, 0 (see `shifted`)
    rows: Option<(f32, f32)>, // the DC's pixel rows that show: a long page draws only those
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
        let mut clip = windows::Win32::Foundation::RECT::default();
        let rows = (draw && unsafe { GetClipBox(dc, &mut clip) }.0 > NULLREGION.0).then_some((clip.top as f32, clip.bottom as f32));
        if draw && GDIPLUS.get() == Some(true) {
            unsafe {
                gp::GdipCreateFromHDC(dc, &mut g);
                if !g.is_null() {
                    gp::GdipSetSmoothingMode(g, gp::SmoothingModeAntiAlias);
                    gp::GdipSetPixelOffsetMode(g, gp::PixelOffsetModeHalf);
                }
            }
        }
        Canvas { dc, g, draw, s, t, fonts, o: (0, 0), rows }
    }

    /// The same canvas drawing the part of the picture whose top left pixel is x, y: a back
    /// buffer that holds only the part to repaint.
    pub(crate) fn shifted(mut self, x: i32, y: i32) -> Canvas<'a> {
        if !self.g.is_null() {
            unsafe { gp::GdipTranslateWorldTransform(self.g, -x as f32, -y as f32, gp::MatrixOrderPrepend) };
        }
        self.o = (x, y);
        self
    }

    /// The same canvas, measuring only: a layout can size a part before drawing it.
    pub(crate) fn measuring(&self) -> Canvas<'a> {
        Canvas { dc: self.dc, g: std::ptr::null_mut(), draw: false, s: self.s, t: self.t, fonts: self.fonts, o: self.o, rows: None }
    }

    /// Whether dip rows y to y + h miss what the DC shows, with room for a stroke's edge.
    fn unseen(&self, y: f32, h: f32) -> bool {
        self.rows.is_some_and(|(top, bottom)| {
            let (a, b) = (y * self.s - self.o.1 as f32, (y + h) * self.s - self.o.1 as f32);
            b < top - 8.0 || a > bottom + 8.0
        })
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

    /// Asks Windows once per text and font: a page lays itself out on each scroll step and
    /// hover, measuring the same words every time.
    pub(crate) fn width(&self, text: &str, f: F) -> f32 {
        let known = &self.fonts.1[f as usize];
        if let Some(&cx) = known.borrow().get(text) {
            return cx as f32 / self.s;
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let cx = self.with_dc(|dc| unsafe {
            let old = SelectObject(dc, self.fonts.get(f).into());
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
            SelectObject(dc, old);
            size.cx
        });
        let mut known = known.borrow_mut();
        // Window titles come and go in the finder and overview: forget them now and then.
        if known.len() >= 4096 {
            known.clear();
        }
        known.insert(text.to_string(), cx);
        cx as f32 / self.s
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

    /// A workspace name cut like `fit` (spec 0023, handoff `fitName`), except that a cut leaving
    /// 3 letters or fewer of a word cuts at the space before it ("Research and r…" reads worse
    /// than "Research and…"), and a name with nothing left is dropped instead of a lone ….
    pub(crate) fn fit_name(&self, text: &str, f: F, w: f32) -> String {
        let cut = self.fit(text, f, w);
        let Some(kept) = cut.strip_suffix('…').filter(|_| cut != text) else { return cut };
        if kept.is_empty() {
            return String::new();
        }
        let inside_word = text[kept.len()..].starts_with(|c: char| !c.is_whitespace());
        match kept.rfind(' ') {
            Some(i) if inside_word && i > 0 && kept[i + 1..].chars().count() <= 3 => kept[..i].trim_end().to_string() + "…",
            _ => cut,
        }
    }

    /// One run of text, centered in a line box `lh` tall whose top is `y`.
    pub(crate) fn text(&self, x: f32, y: f32, lh: f32, text: &str, f: F, color: u32) {
        if !self.draw || self.unseen(y, lh) {
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
            let _ = TextOutW(dc, (x * self.s).round() as i32 - self.o.0, top - self.o.1, &wide);
            SelectObject(dc, old);
        });
    }

    /// Wrapped text from runs (text, font, color) that flow on; breaks only at spaces.
    /// Returns its height.
    pub(crate) fn para(&self, x: f32, y: f32, w: f32, lh: f32, runs: &[(&str, F, u32)]) -> f32 {
        self.para_boxes(x, y, w, lh, runs).0
    }

    /// `para`, also giving where each run landed: one box per line it spans, so a link inside
    /// a caption can be clicked and ringed (the settings page's Reset).
    pub(crate) fn para_boxes(&self, x: f32, y: f32, w: f32, lh: f32, runs: &[(&str, F, u32)]) -> (f32, Vec<Vec<Bx>>) {
        // Words glued to the one before (no space between) never break from it.
        let mut words: Vec<(bool, Vec<(usize, Run)>)> = vec![];
        let mut space = false;
        for (r, &(text, f, c)) in runs.iter().enumerate() {
            for (i, piece) in text.split(' ').enumerate() {
                space |= i > 0;
                if piece.is_empty() {
                    continue;
                }
                match words.last_mut() {
                    Some(last) if !space => last.1.push((r, (piece, f, c))),
                    _ => words.push((space && !words.is_empty(), vec![(r, (piece, f, c))])),
                }
                space = false;
            }
        }
        let mut boxes: Vec<Vec<Bx>> = vec![vec![]; runs.len()];
        let (mut line, mut cx) = (0.0, 0.0);
        for (space, pieces) in words {
            let ww: f32 = pieces.iter().map(|(_, (t, f, _))| self.width(t, *f)).sum();
            let sw = if space { self.width(" ", pieces[0].1.1) } else { 0.0 };
            if cx > 0.0 && cx + sw + ww > w + 0.01 {
                line += 1.0;
                cx = 0.0;
            } else if cx > 0.0 {
                // A space inside one run belongs to it, so a two word link is one box.
                if let Some(b) = boxes[pieces[0].0].last_mut().filter(|b| b.1 == y + line * lh) {
                    b.2 += sw;
                }
                cx += sw;
            }
            for (r, (t, f, c)) in pieces {
                self.text(x + cx, y + line * lh, lh, t, f, c);
                let tw = self.width(t, f);
                match boxes[r].last_mut() {
                    Some(b) if b.1 == y + line * lh && (b.0 + b.2 - (x + cx)).abs() < 0.01 => b.2 += tw,
                    _ => boxes[r].push((x + cx, y + line * lh, tw, lh)),
                }
                cx += tw;
            }
        }
        ((line + 1.0) * lh, boxes)
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
        if !self.shapes() || w <= 0.0 || h <= 0.0 || self.unseen(y, h) {
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
        if !self.shapes() || self.unseen(y, h) {
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

    /// Like `stroke_rr`, dotted or dashed: high contrast's marks, a mouse action's key cap.
    pub(crate) fn stroke_dash(&self, bx: Bx, r: f32, width: f32, argb: u32, style: gp::DashStyle) {
        if !self.shapes() {
            return;
        }
        let (p, pen) = (self.rr_path(bx, r), self.pen(argb, width));
        unsafe {
            gp::GdipSetPenLineCap197819(pen, gp::LineCapFlat, gp::LineCapFlat, gp::DashCapFlat);
            gp::GdipSetPenDashStyle(pen, style);
            gp::GdipDrawPath(self.g, pen, p);
            gp::GdipDeletePen(pen);
            gp::GdipDeletePath(p);
        }
    }

    /// A dashed straight line; a path of a zero width rectangle draws nothing in GDI+.
    pub(crate) fn dash_line(&self, (x0, y0): (f32, f32), (x1, y1): (f32, f32), width: f32, argb: u32) {
        if !self.shapes() {
            return;
        }
        let (s, pen) = (self.s, self.pen(argb, width));
        unsafe {
            gp::GdipSetPenDashStyle(pen, gp::DashStyleDot);
            gp::GdipDrawLine(self.g, pen, x0 * s, y0 * s, x1 * s, y1 * s);
            gp::GdipDeletePen(pen);
        }
    }

    pub(crate) fn circle(&self, cx: f32, cy: f32, r: f32, fill: Option<u32>, edge: Option<(u32, f32)>) {
        if !self.shapes() || self.unseen(cy - r, 2.0 * r) {
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

    /// A closed shape, filled, with its edge: the first run's mouse pointer.
    pub(crate) fn polygon(&self, pts: &[(f32, f32)], fill: u32, (edge, width): (u32, f32)) {
        if !self.shapes() {
            return;
        }
        let pts: Vec<gp::PointF> = pts.iter().map(|(x, y)| gp::PointF { X: x * self.s, Y: y * self.s }).collect();
        let (b, pen) = (self.brush(fill), self.pen(edge, width));
        unsafe {
            gp::GdipFillPolygon(self.g, b, pts.as_ptr(), pts.len() as i32, gp::FillModeAlternate);
            gp::GdipDrawPolygon(self.g, pen, pts.as_ptr(), pts.len() as i32);
            gp::GdipDeleteBrush(b);
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

    /// A one pixel line `w` long whose top is the pixel row at `y`: crisp at any scale.
    pub(crate) fn rule(&self, x: f32, y: f32, w: f32, argb: u32) {
        let s = self.s;
        self.fill_rr((x, (y * s).round() / s, w, 1.0 / s), 0.0, argb);
    }

    /// `p` in a box with round corners `r`, its top left at the box's; `p` is already the
    /// box's size in pixels, so nothing is scaled.
    pub(crate) fn picture_rr(&self, (x, y, w, h): Bx, r: f32, p: &Picture) {
        if !self.shapes() || p.px.len() != (p.w * p.h) as usize {
            return;
        }
        unsafe {
            let mut image = std::ptr::null_mut();
            // Reads p's pixels in place; dropped before p can be.
            gp::GdipCreateBitmapFromScan0(p.w, p.h, p.w * 4, ARGB, Some(p.px.as_ptr().cast()), &mut image);
            if image.is_null() {
                return;
            }
            let mut brush = std::ptr::null_mut();
            gp::GdipCreateTexture(image.cast(), gp::WrapModeClamp, &mut brush);
            if !brush.is_null() {
                gp::GdipTranslateTextureTransform(brush, (x * self.s).round(), (y * self.s).round(), gp::MatrixOrderAppend);
                let path = self.rr_path((x, y, w, h), r);
                gp::GdipFillPath(self.g, brush.cast(), path);
                gp::GdipDeletePath(path);
                gp::GdipDeleteBrush(brush.cast());
            }
            gp::GdipDisposeImage(image.cast());
        }
    }

    /// `p`, premultiplied and see through where it says so, its top left at the pixel nearest
    /// x, y; drawn pixel for pixel, never scaled (the color picker's ring and square).
    pub(crate) fn image(&self, x: f32, y: f32, p: &Picture) {
        if !self.shapes() || p.px.len() != (p.w * p.h) as usize {
            return;
        }
        unsafe {
            let mut image = std::ptr::null_mut();
            // Reads p's pixels in place; dropped before p can be.
            gp::GdipCreateBitmapFromScan0(p.w, p.h, p.w * 4, PARGB, Some(p.px.as_ptr().cast()), &mut image);
            if image.is_null() {
                return;
            }
            let mut was = gp::InterpolationModeDefault;
            gp::GdipGetInterpolationMode(self.g, &mut was);
            gp::GdipSetInterpolationMode(self.g, gp::InterpolationModeNearestNeighbor);
            gp::GdipDrawImageRectI(self.g, image.cast(), (x * self.s).round() as i32, (y * self.s).round() as i32, p.w, p.h);
            gp::GdipSetInterpolationMode(self.g, was);
            gp::GdipDisposeImage(image.cast());
        }
    }

    /// An app's icon, `size` dip square, at x, y.
    pub(crate) fn icon(&self, x: f32, y: f32, size: f32, icon: HICON) {
        if !self.draw || icon.is_invalid() {
            return;
        }
        let (px, py, side) = ((x * self.s).round() as i32 - self.o.0, (y * self.s).round() as i32 - self.o.1, (size * self.s).round() as i32);
        self.with_dc(|dc| unsafe { let _ = DrawIconEx(dc, px, py, icon, side, side, 0, None, DI_NORMAL); });
    }

    /// des-k's mark, 16 dip square at x, y: four rounded squares.
    pub(crate) fn mark(&self, x: f32, y: f32) {
        let m = self.t.b.mark;
        for (i, (dx, dy)) in [(0.0, 0.0), (9.0, 0.0), (0.0, 9.0), (9.0, 9.0)].into_iter().enumerate() {
            self.fill_rr((x + dx, y + dy, 7.0, 7.0), 2.0, op(m[i]));
        }
    }

    /// A key cap `h` dip tall (20 in a menu, 22 in a window) at x, y: the key's name, or an
    /// arrow for Left, Right, Up and Down, on the key color with its 1 px edge and the 1 px
    /// shadow under it. `held`: a key held while recording, in the purple. Returns its width.
    pub(crate) fn key_cap(&self, x: f32, y: f32, h: f32, key: &str, held: bool) -> f32 {
        let b = self.t.b;
        let (label, f) = match key {
            "Left" => ("\u{E72B}", F::IconTiny),
            "Right" => ("\u{E72A}", F::IconTiny),
            "Up" => ("\u{E74A}", F::IconTiny),
            "Down" => ("\u{E74B}", F::IconTiny),
            k => (k, F::Small),
        };
        let tw = self.width(label, f);
        let w = (tw + 2.0 * (h / 2.0 - 5.0) + 2.0).max(h);
        let (fill, edge) = if held { (b.held, b.acc) } else { (b.key, b.key_edge) };
        self.fill_rr((x, y + 1.0, w, h), 4.0, op(edge));
        self.boxed((x, y, w, h), 4.0, fill, Some(edge));
        self.text(x + (w - tw) / 2.0, y, h, label, f, b.tx);
        w
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

// GDI+'s pixel formats (gdipluspixelformats.h), which the windows crate leaves out.
const ARGB: i32 = 0x0026_200A;
const PARGB: i32 = 0x000E_200B;

/// A picture in pixels, top row first, each 0xAARRGGBB: opaque for a tile's wallpaper,
/// premultiplied for `Canvas::image`.
pub(crate) struct Picture {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) px: Vec<u32>,
}

/// Paints a popup whose every pixel has its own opacity (`UpdateLayeredWindow`), w by h px at
/// x, y on screen: `f` draws shapes on a clear image. Shapes only: GDI text would come out
/// see through, since GDI writes no alpha.
pub(crate) fn layered(hwnd: HWND, (x, y, w, h): (i32, i32, i32, i32), s: f32, t: Theme, f: impl FnOnce(&Canvas)) {
    if GDIPLUS.get() != Some(true) || w <= 0 || h <= 0 {
        return;
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
        let mut bits = std::ptr::null_mut();
        let Ok(bmp) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            return;
        };
        let old = SelectObject(mem, bmp.into());
        let mut image = std::ptr::null_mut();
        gp::GdipCreateBitmapFromScan0(w, h, w * 4, PARGB, Some(bits.cast_const().cast()), &mut image);
        let mut g = std::ptr::null_mut();
        if !image.is_null() {
            gp::GdipGetImageGraphicsContext(image.cast(), &mut g);
        }
        if !g.is_null() {
            gp::GdipSetSmoothingMode(g, gp::SmoothingModeAntiAlias);
            gp::GdipSetPixelOffsetMode(g, gp::PixelOffsetModeHalf);
            let fonts = fonts(s);
            f(&Canvas { dc: mem, g, draw: true, s, t, fonts: &fonts, o: (0, 0), rows: None }); // its drop deletes g
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8, ..Default::default() };
            let (at, size, from) = (POINT { x, y }, SIZE { cx: w, cy: h }, POINT::default());
            let _ = UpdateLayeredWindow(hwnd, Some(screen), Some(&at), Some(&size), Some(mem), Some(&from), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
        if !image.is_null() {
            gp::GdipDisposeImage(image.cast());
        }
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

#[cfg(test)]
mod tests;
