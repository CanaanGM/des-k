// Tests for where the color picker's card goes.
use super::*;

/// The card never covers what you clicked (ring and hover edge included), stays in the
/// window past the side list, keeps what you clicked in view 8 dip from the edges and below
/// the notes, and scrolls only within the page's limits.
fn sound(room: &Room, anchor: Bx, (at, d): (Place, f32)) {
    let card = (PICK_W, PICK_H);
    assert!(at.x >= room.lo - 0.5 && at.x <= room.hi + 0.5, "x {at:?}");
    assert!(at.y >= MARGIN - 0.5 && at.y + PICK_H <= room.vh - MARGIN + 0.5, "y {at:?}");
    let a = grown(anchor, d);
    assert!(!covers(a, (at.x, at.y), card), "covers what you clicked: {at:?} {a:?}");
    assert!(a.1 >= room.fixed + MARGIN - 0.5 && a.1 + a.3 <= room.vh - MARGIN + 0.5, "what you clicked is out of view: {a:?}");
    assert!(room.scroll + d >= -0.5 && room.scroll + d <= room.smax + 0.5, "scrolled past the page: {d}");
}

fn room(vh: f32, fixed: f32, scroll: f32, smax: f32) -> Room {
    Room { vh, fixed, lo: X0, hi: X0 + (W - X0 - PAD_R) - PICK_W, scroll, smax }
}

fn tile_levels(rows: &[Bx], row: usize, tile: Bx) -> Vec<Level> {
    let one = |b: Bx| Level { next_to: b, keep: vec![b] };
    vec![one(rows.iter().copied().reduce(union).unwrap()), one(rows[row]), one(tile)]
}

// AC-14, the design pass's tight cases: the card finds a place without covering what you
// clicked, scrolling the least it must, and over the notes only when nothing else fits.
#[test]
fn the_card_never_covers_what_you_clicked() {
    let card = (PICK_W, PICK_H);
    // A 528 window with both notes (210 dip of them): below the notes there is no room for
    // the card, so it opens beside the tiles, over the notes.
    let (rows, tile) = ([(744.0, 300.0, 264.0, 34.0), (744.0, 360.0, 264.0, 34.0)], (798.0, 360.0, 48.0, 34.0));
    let r = room(MIN_H, 210.0, 400.0, 1800.0);
    let got = place(&r, &tile_levels(&rows, 1, tile), tile, true, card, true);
    sound(&r, tile, got);
    assert!(got.0.y < 210.0, "over the notes: {got:?}");
    // 9 workspaces, the page scrolled to its end: what you clicked sits 8 dip from the bottom.
    let (rows, tile) = ([(632.0, 560.0, 376.0, 34.0), (632.0, 600.0, 376.0, 34.0)], (920.0, 600.0, 40.0, 34.0));
    let r = room(H, 0.0, 1800.0, 1800.0);
    let got = place(&r, &tile_levels(&rows, 1, tile), tile, true, card, true);
    sound(&r, tile, got);
    // Your accent with both notes in 648: the swatch always stays clear, the legend when it can.
    let (sw, legend) = ((828.0, 420.0, 48.0, 32.0), (604.0, 300.0, 300.0, 132.0));
    let levels = [Level { next_to: sw, keep: vec![sw, legend] }, Level { next_to: sw, keep: vec![sw] }];
    let r = room(H, 230.0, 900.0, 1800.0);
    let got = place(&r, &levels, sw, false, card, true);
    sound(&r, sw, got);
    // A swatch near the window's bottom: below doesn't fit, above does, nothing scrolls.
    let sw = (828.0, 560.0, 48.0, 32.0);
    let r = room(H, 0.0, 900.0, 1800.0);
    let (at, d) = place(&r, &[Level { next_to: sw, keep: vec![sw] }], sw, false, card, true);
    assert_eq!((d, at.y), (0.0, 560.0 - MARGIN - PICK_H));
    // A tile 10 dip into the bottom margin: the page scrolls the least that brings it in view
    // with its ring, and no more.
    let (rows, tile) = ([(744.0, 608.0, 264.0, 34.0)], (798.0, 608.0, 48.0, 34.0));
    let r = room(H, 0.0, 300.0, 1800.0);
    let (at, d) = place(&r, &tile_levels(&rows, 0, tile), tile, true, card, true);
    assert_eq!(d, 608.0 + 34.0 + OUTSET - (H - MARGIN));
    sound(&r, tile, (at, d));
    // Placed again without the scroll step, the page never moves.
    assert_eq!(place(&r, &tile_levels(&rows, 0, tile), tile, true, card, false).1, 0.0);
}

// AC-14: once placed, the card keeps its window position while it still fits; when what
// it keeps clear moves under it, it is placed again.
#[test]
fn the_card_stays_put_while_it_fits() {
    let card = (PICK_W, PICK_H);
    let sw = (828.0, 200.0, 48.0, 32.0);
    let r = room(H, 0.0, 0.0, 1800.0);
    let levels = [Level { next_to: sw, keep: vec![sw] }];
    let (at, _) = place(&r, &levels, sw, false, card, true);
    assert!(kept(&r, &levels, at, card));
    let moved = [Level { next_to: (828.0, 300.0, 48.0, 32.0), keep: vec![(828.0, 300.0, 48.0, 32.0)] }];
    assert!(!kept(&r, &moved, at, card), "the swatch moved under the card");
}
