// The layout tests: the strip, the spread, minis, arrows and Tab, the first window, a tile's
// words, and where a drop lands.
use super::*;
use crate::overview::draw::bar_box;
use crate::overview::tests::screen;
use crate::ui::{LIGHT, fonts::Fonts};

fn overlap(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

// AC-2: N tiles must all fit on the screen and still look like little copies of it, so a
// window's place inside a tile can match its place on the screen.
#[test]
fn strip_tiles_keep_the_screen_aspect_and_fit_between_the_margins() {
    let tiles = strip(1920, 1080, 4, 1.0);
    assert_eq!(tiles.len(), 4);
    assert_eq!(tiles[0], RECT { left: 474, top: 24, right: 705, bottom: 154 });
    // Nine tiles on a narrow screen: shrunk to fit, aspect kept, still centered.
    let tiles = strip(1080, 1920, 9, 1.0);
    assert!(tiles[0].left >= 48 && tiles[8].right <= 1080 - 48, "{:?}", (tiles[0], tiles[8]));
    let (w, h) = (tiles[0].right - tiles[0].left, tiles[0].bottom - tiles[0].top);
    assert!((w as f64 / h as f64 - 1080.0 / 1920.0).abs() < 0.02);
    assert!((tiles[0].left - (1080 - tiles[8].right)).abs() <= 1);
}

// AC-3: every window must be visible at once and none may hide another, or the spread
// fails at what it is for; and a small window is never blown up past its real size.
fn sized(w: i32, h: i32) -> RECT {
    RECT { left: 0, top: 0, right: w, bottom: h }
}

#[test]
fn spread_never_overlaps_never_enlarges_and_stays_inside() {
    let area = RECT { left: 48, top: 200, right: 1872, bottom: 1032 };
    let reals = [sized(1920, 1080), sized(800, 600), sized(300, 200), sized(1200, 1000), sized(640, 480)];
    let (got, cell_w) = spread(area, &reals, 32, 22);
    assert_eq!(got.len(), reals.len());
    for (i, r) in got.iter().enumerate() {
        // The label band under each window stays inside too (AC-5), and a hover label as
        // wide as the cell never runs into the next one (AC-OVR-6).
        assert!(r.left >= area.left && r.right <= area.right && r.top >= area.top && r.bottom + 22 <= area.bottom, "{r:?}");
        assert!(r.right - r.left <= reals[i].right && r.bottom - r.top <= reals[i].bottom);
        assert!(r.right - r.left <= cell_w);
        let with_label = |r: &RECT| RECT { bottom: r.bottom + 22, ..*r };
        assert!(got.iter().skip(i + 1).all(|o| !overlap(&with_label(r), &with_label(o))), "{r:?}");
    }
    assert_eq!(got[2].right - got[2].left, 300); // small enough to show at real size
}

// Four screen sized windows on a wide screen read best as 2 by 2, not as a thin row of four.
#[test]
fn spread_picks_the_row_count_that_shows_windows_largest() {
    let area = RECT { left: 0, top: 0, right: 1600, bottom: 900 };
    let (got, _) = spread(area, &[sized(1920, 1080); 4], 0, 0);
    assert_eq!(got[0].top, got[1].top);
    assert!(got[2].top > got[0].top);
    assert_eq!(got[0].right - got[0].left, 800);
    // One window alone gets the whole area, centered.
    let (one, _) = spread(area, &[sized(3200, 900)], 0, 0);
    assert_eq!(one, [RECT { left: 0, top: 225, right: 1600, bottom: 675 }]);
    assert!(spread(area, &[], 0, 0).0.is_empty());
}

// AC-3: you find a window in the spread by where you left it, so a window on the right of
// the screen must land right of one on the left, whatever order the z order lists them in.
#[test]
fn spread_keeps_each_window_on_its_side_of_the_screen() {
    let area = RECT { left: 0, top: 0, right: 1600, bottom: 900 };
    let at = |x: i32, y: i32| RECT { left: x, top: y, right: x + 800, bottom: y + 600 };
    let (got, _) = spread(area, &[at(1400, 0), at(0, 0), at(0, 700)], 0, 0);
    assert!(got[0].left > got[1].left && got[0].left > got[2].left, "{got:?}");
    // Four in a 2 by 2: the lower windows take the lower row.
    let (got, _) = spread(area, &[at(0, 700), at(1000, 700), at(1000, 0), at(0, 0)], 0, 0);
    assert!(got[0].top > got[3].top && got[1].top > got[2].top, "{got:?}");
    assert!(got[1].left > got[0].left && got[2].left > got[3].left, "{got:?}");
}

// AC-2: a tile is a small copy of its screen, so a window filling the right half of the
// second screen fills the right half of the tile, not some place offset by the screen's position.
#[test]
fn a_mini_sits_where_its_window_sits_on_its_own_screen() {
    let screen = RECT { left: 1920, top: 0, right: 3840, bottom: 1080 };
    let tile = RECT { left: 100, top: 24, right: 292, bottom: 132 };
    let right_half = RECT { left: 2880, top: 0, right: 3840, bottom: 1080 };
    assert_eq!(place_in(right_half, screen, tile), RECT { left: 196, top: 24, right: 292, bottom: 132 });
}

// A maximized window hangs a few pixels past its screen; cut at the tile's edge, its preview
// must show the matching part of the picture, or it would draw onto the next tile.
#[test]
fn a_cut_mini_shows_the_matching_part_of_the_picture() {
    let full = RECT { left: -10, top: 0, right: 90, bottom: 100 };
    let shown = intersect(full, RECT { left: 0, top: 0, right: 200, bottom: 200 }).unwrap();
    assert_eq!(shown, RECT { left: 0, top: 0, right: 90, bottom: 100 });
    assert_eq!(source_rect(full, shown, SIZE { cx: 200, cy: 200 }), RECT { left: 20, top: 0, right: 200, bottom: 200 });
    assert_eq!(intersect(full, RECT { left: 90, top: 0, right: 200, bottom: 200 }), None);
}

// AC-8: the arrows walk the spread the way it looks. Right from the top left window must
// reach its neighbour in the same row, not a closer looking one in the row below, and an
// arrow with nothing that way keeps the selection where it is.
#[test]
fn arrows_move_to_the_nearest_window_that_way() {
    let at = |x: i32, y: i32, w: i32, h: i32| RECT { left: x, top: y, right: x + w, bottom: y + h };
    // Two rows; from the top left one, the window below is closer as the crow flies than
    // its row mate on the right.
    let rects = [at(0, 0, 400, 300), at(600, 100, 400, 100), at(100, 400, 400, 300), at(700, 400, 300, 300)];
    assert_eq!(nearest(&rects, 0, (1, 0)), Some(1));
    assert_eq!(nearest(&rects, 0, (0, 1)), Some(2));
    assert_eq!(nearest(&rects, 1, (0, 1)), Some(3));
    assert_eq!(nearest(&rects, 3, (-1, 0)), Some(2));
    assert_eq!(nearest(&rects, 0, (-1, 0)), None);
    assert_eq!(nearest(&rects, 0, (0, -1)), None);
}

// Spec 0023 AC-15: Narrator hears a tile's name where it is painted, after its number; an
// unnamed tile reads as it always has.
#[test]
fn narrator_reads_a_tiles_name_after_its_number() {
    let tile = |name: &str| Tile { rect: RECT::default(), minis: Vec::new(), name: name.into(), file: None, picture: None };
    assert_eq!(tile_words(1, &tile("Mail"), true), "Workspace 2, Mail, shown, no windows");
    assert_eq!(tile_words(2, &tile(""), false), "Workspace 3, not shown, no windows");
}

// AC-7: a drop lands where you let go: on a tile, that workspace; anywhere in the spread,
// the one the screen shows; on the margins nothing, so a missed drop never moves a window.
#[test]
fn a_drop_lands_on_the_tile_or_the_spread_under_the_mouse() {
    let sc = screen(1920, 1080, 1.0, 2, 4);
    let mid = |r: RECT| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 };
    assert_eq!(sc.drop_target(mid(sc.tiles[0].rect)), Some(0));
    assert_eq!(sc.drop_target(mid(sc.tiles[3].rect)), Some(3));
    assert_eq!(sc.drop_target(POINT { x: 960, y: 700 }), Some(2), "the spread means the shown workspace");
    assert_eq!(sc.drop_target(POINT { x: 10, y: 700 }), None, "a side margin is neither");
    assert_eq!(sc.drop_target(POINT { x: 10, y: 10 }), None);
    assert_eq!(sc.drop_target(POINT { x: 960, y: 1080 - 30 }), None, "the bar's room is not the spread");
}

// AC-OVR-2: the spread starts 24 dip under the band (tiles, chips, 16 under them) and ends
// 64 dip above the bottom, where the bar sits, at every scale; a window there never runs
// under the band or the bar.
#[test]
fn the_spread_leaves_room_for_the_band_and_the_bar() {
    for scale in [1.0, 1.5] {
        let (w, h) = ((1280.0 * scale) as i32, (720.0 * scale) as i32);
        let sc = screen(w, h, scale, 1, 4);
        let px = |v: f32| (v * scale).round() as i32;
        let tile = sc.tiles[0].rect;
        assert_eq!(sc.band_h(), tile.bottom + px(8.0 + 18.0 + 16.0));
        let area = sc.spread_area();
        assert_eq!(area.top, sc.band_h() + px(24.0));
        assert_eq!(area.bottom, h - px(64.0));
        assert_eq!((area.left, area.right), (px(48.0), w - px(48.0)));
        // The bar's box sits inside those 64 dip.
        let fonts = Fonts::new(scale);
        let dc = unsafe { CreateCompatibleDC(None) };
        let c = Canvas::new(dc, false, scale, LIGHT, &fonts);
        let (_, y, _, bh) = bar_box(&c, &sc, Bar::Keys);
        assert!(y * scale >= area.bottom as f32 && (y + bh) * scale <= h as f32, "{y} {bh}");
        unsafe { let _ = DeleteDC(dc); }
    }
}

// AC-8: with no window you came from on that screen, the selection starts where you start
// reading, the top left window, whatever order the z order listed them in.
#[test]
fn the_first_window_is_the_top_left_one() {
    let at = |x: i32, y: i32| RECT { left: x, top: y, right: x + 100, bottom: y + 100 };
    assert_eq!(first(&[at(500, 400), at(500, 0), at(0, 400), at(0, 0)]), Some(3));
    assert_eq!(first(&[]), None);
}

// AC-8: Tab reads one screen like a page and wraps there, never on to another screen,
// since mixing every screen's windows into one cycle was what you rejected.
#[test]
fn tab_reads_one_screen_like_a_page_and_wraps_around_on_it() {
    // Row one centers at y 50 and 49 (a pixel of rounding), so the right one must
    // still come after the left one, not first; then row two.
    let r = |x: i32, top: i32, h: i32| RECT { left: x, top, right: x + 100, bottom: top + h };
    let rects = [r(0, 300, 100), r(200, 0, 99), r(0, 0, 100)];
    assert_eq!(tab_step(&rects, None, 1), Some(2));
    assert_eq!(tab_step(&rects, Some(2), 1), Some(1));
    assert_eq!(tab_step(&rects, Some(1), 1), Some(0));
    assert_eq!(tab_step(&rects, Some(0), 1), Some(2), "Tab wraps to the top of this screen");
    assert_eq!(tab_step(&rects, Some(2), -1), Some(0), "Shift+Tab wraps to the bottom");
    assert_eq!(tab_step(&[], None, 1), None, "an empty screen has no stop");
}
