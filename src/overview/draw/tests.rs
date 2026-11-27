// The drawing tests: the shadow, the bar's greyed keys, and a partial repaint against the whole.
use super::*;
use crate::overview::tests::{design_windows, laid_out, pixels, screen};
use crate::ui::LIGHT;

// AC-OVR-3: the shadow must fade to nothing at its edge (no hard line where the picture
// ends) and reach full strength under the window, getting lighter the farther out.
#[test]
fn the_shadow_fades_out_and_is_full_under_the_box() {
    let (e, sigma) = (18, 6.0);
    let a = shadow_alphas(e, sigma, 0.5);
    let n = (4 * e + 1) as usize;
    let at = |x: usize, y: usize| a[y * n + x];
    assert_eq!(at(0, 0), 0);
    assert_eq!(at(0, n / 2), 0, "the outer edge");
    assert!((126..=128).contains(&at(n / 2, n / 2)), "the middle, under the box: {}", at(n / 2, n / 2));
    let row: Vec<u8> = (0..n / 2).map(|x| at(x, n / 2)).collect();
    assert!(row.windows(2).all(|p| p[0] <= p[1]), "{row:?}");
    // Symmetric, so stretching the middle never shows a seam.
    assert_eq!(at(3, n / 2), at(n - 4, n / 2));
}

// AC-OVR-7: Enter and Ctrl+W need a window in the spread, Drag a window anywhere on the
// screen; greying them tells you before you try.
#[test]
fn the_bar_greys_what_cannot_act() {
    let mut sc = screen(1280, 720, 1.0, 0, 4);
    let grey = |sc: &Screen| bar_items(sc, Bar::Keys, &LIGHT).0.iter().map(|(_, g)| *g).collect::<Vec<_>>();
    assert_eq!(grey(&sc), [true, true, false, true, false]);
    sc.tiles[2].minis.push(Slot { hwnd: HWND::default(), rect: RECT::default(), title: String::new(), icon: HICON::default(), pinned: false, card: false, thumb: None });
    assert_eq!(grey(&sc), [true, false, false, true, false], "a hidden window can still be dragged");
}

// AC-OVR-10: hover, selection and drags repaint only the rectangle that changed, into a
// buffer that size. That is only safe if a part painted alone matches the same part of a
// whole paint, pixel for pixel: shapes, text, icons and shadows all shifted alike.
#[test]
fn a_partial_repaint_matches_the_whole_paint() {
    ui::canvas::gdiplus();
    let code = HWND(std::ptr::without_provenance_mut(13));
    let web = HWND(std::ptr::without_provenance_mut(12));
    let sc = laid_out(1.5, 1, 4, &design_windows(), &[], Labels::Hover);
    let m = Marks { selected: Some(code), hover: Some(web), tile_hover: Some(2), bar: Some(Bar::Keys), ..Marks::default() };
    let (w, h) = sc.size();
    let whole = pixels(&sc, &m, LIGHT, (w, h), (0, 0));
    // A hover mark, a tile's ring and the bar: text, shapes and a shadow.
    for part in [sc.mark_rect(web).unwrap(), sc.tile_ring(2).unwrap(), sc.bar_strip()] {
        let (pw, ph) = (part.right - part.left, part.bottom - part.top);
        let alone = pixels(&sc, &m, LIGHT, (pw, ph), (part.left, part.top));
        for y in 0..ph {
            for x in 0..pw {
                let (a, b) = (alone[(y * pw + x) as usize], whole[((part.top + y) * w + part.left + x) as usize]);
                assert_eq!(a & 0xFFFFFF, b & 0xFFFFFF, "{part:?} at {x}, {y}");
            }
        }
    }
}
