// The color math tests: hue turns, the HSV round trip, CIE76 closeness and the picker's ring.
use super::*;

// AC-3: the neighbors keep the accent's saturation and lightness, so a rose brings a red
// (hue plus 30) and a purple (hue minus 30); a grey has no hue to turn.
#[test]
fn neighbors_are_thirty_degrees_either_side() {
    let rose = 0xE0457B;
    let (purple, red) = (hue(rose, -30.0), hue(rose, 30.0));
    let rgb = |c: u32| (c >> 16 & 0xFF, c >> 8 & 0xFF, c & 0xFF);
    let (r, g, b) = rgb(purple);
    assert!(b > g && r > g && b >= 0x99, "{purple:06X}");
    let (r, g, b) = rgb(red);
    assert!(r == 0xE0 && g > b && g < 0x80, "{red:06X}");
    assert_eq!(hue(0x808080, 30.0), 0x808080);
    assert_eq!(hue(hue(rose, 30.0), -30.0), rose);
    assert_eq!(hue(0xFF0000, 120.0), 0x00FF00);
}

// Spec 0021 AC-5, AC-7: opening the picker or picking a swatch turns a color into HSV, and
// only a move on the ring or square turns HSV back. Every #RRGGBB must survive that trip,
// or a preset or your own color would be saved one step off without you touching it.
#[test]
fn every_color_survives_hsv_and_back() {
    let mut keep = (0.0, 0.0, 0.0);
    for c in 0..=0xFF_FFFFu32 {
        keep = hsv_of(c, keep);
        assert_eq!(rgb_of(keep), c, "#{c:06X} via {keep:?}");
    }
}

// A grey or black has no hue of its own: the picker's marks stay where they were instead of
// jumping to red at the top of the ring.
#[test]
fn a_grey_keeps_the_last_hue() {
    let was = hsv_of(0xDDB43C, (0.0, 0.0, 0.0));
    let grey = hsv_of(0x808080, was);
    assert_eq!((grey.0, grey.1), (was.0, 0.0));
    let black = hsv_of(0x000000, was);
    assert_eq!((black.0, black.1, black.2), (was.0, was.1, 0.0));
    assert_eq!(hsv_of(0x000000, (0.0, 0.0, 0.0)), (0.0, 0.0, 0.0));
}

// AC-8: "very close" is a CIE76 distance under 10, so two blues one step apart are close and
// blue and gold are not.
#[test]
fn closeness_is_cie76_under_ten() {
    assert_eq!(distance(0x79A4F3, 0x79A4F3), 0.0);
    assert!(distance(0x79A4F3, 0x7AA5F2) < 10.0);
    assert!(distance(0x79A4F3, 0xDDB43C) >= 10.0);
    // The design pass's measure: lime and slate sit apart from every other preset.
    for (i, &a) in PRESETS.iter().enumerate() {
        for &b in &PRESETS[i + 1..] {
            assert!(distance(a, b) >= 10.0, "#{a:06X} #{b:06X}");
        }
    }
}

// The ring is toned to the presets' lightness: every hue comes out, and none is the grey
// fallback, so the ring never has a dead patch.
#[test]
fn the_ring_has_every_hue() {
    for h in 0..360 {
        assert_ne!(ring_color(h as f32), 0xB0B0B0, "hue {h}");
    }
}
