// What each window behind the icons paints: its picture, a crossfade's two, and the dim
// layer over them.
use super::*;

/// What a window paints: its picture, over the one it fades from.
#[derive(Clone)]
pub(super) struct Paint {
    pub(super) pic: Rc<Dib>,
    pub(super) from: Option<Rc<Dib>>, // a crossfade under way: drawn first, `pic` over it at `alpha`
    pub(super) alpha: u8,
}

thread_local! {
    // What each window paints, by hwnd; read by its wndproc on this thread.
    pub(super) static PAINTS: RefCell<HashMap<isize, Paint>> = RefCell::new(HashMap::new());
    pub(super) static DIM: Cell<u8> = const { Cell::new(0) };
    static BLACK: Option<Dib> = Dib::new(&Picture { w: 1, h: 1, px: vec![0xFF00_0000] });
}

/// The dim layer's constant alpha: `round(dim × 255 / 100)` (AC-18).
pub(crate) fn dim_alpha(dim: u8) -> u8 {
    ((dim as u32 * 255 + 50) / 100).min(255) as u8
}

/// `src` stretched over `w` by `h` at `dc`'s top left, at `alpha` (255: a plain copy).
fn blend(dc: HDC, src: &Dib, (w, h): (i32, i32), alpha: u8) {
    unsafe {
        if alpha == 255 && (src.w, src.h) == (w, h) {
            let _ = BitBlt(dc, 0, 0, w, h, Some(src.dc), 0, 0, SRCCOPY);
        } else {
            let f = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, SourceConstantAlpha: alpha, ..Default::default() };
            let _ = AlphaBlend(dc, 0, 0, w, h, src.dc, 0, 0, src.w, src.h, f);
        }
    }
}

pub(super) unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            if let Some(p) = PAINTS.with_borrow(|m| m.get(&(hwnd.0 as isize)).cloned()) {
                let size = (p.pic.w, p.pic.h);
                match &p.from {
                    Some(from) => {
                        blend(dc, from, size, 255);
                        blend(dc, &p.pic, size, p.alpha);
                    }
                    None => blend(dc, &p.pic, size, 255),
                }
                // Black over all of it, bars included; the pictures themselves stay undimmed,
                // so a dim change never decodes (AC-18).
                let dim = DIM.get();
                if dim > 0 {
                    BLACK.with(|b| b.as_ref().map(|b| blend(dc, b, size, dim_alpha(dim))));
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests;
