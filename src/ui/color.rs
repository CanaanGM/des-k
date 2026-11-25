// Color math for des-k's windows: HSV for the color picker, CIELAB distance, the WCAG
// contrast ratio, hue turns and mixes, and the GDI and GDI+ color forms.
use super::*;

/// `c` turned `deg` degrees round the color wheel, its HSL saturation and lightness kept; a
/// grey stays the same grey.
pub(super) fn hue(c: u32, deg: f32) -> u32 {
    let [r, g, b] = [c >> 16 & 0xFF, c >> 8 & 0xFF, c & 0xFF].map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if d == 0.0 {
        return c;
    }
    let h = 60.0
        * if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
    let h = (h + deg).rem_euclid(360.0) / 60.0;
    // Same lightness and saturation means the same chroma (d) and the same min.
    let x = d * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (d, x, 0.0),
        1 => (x, d, 0.0),
        2 => (0.0, d, x),
        3 => (0.0, x, d),
        4 => (x, 0.0, d),
        _ => (d, 0.0, x),
    };
    let ch = |v: f32| ((v + min) * 255.0).round().clamp(0.0, 255.0) as u32;
    ch(r) << 16 | ch(g) << 8 | ch(b)
}

/// Hue 0 to 360, saturation and value 0 to 1: where the picker's ring and square sit.
pub(crate) type Hsv = (f32, f32, f32);

/// `c` as HSV (spec 0021's color model). A grey keeps `keep`'s hue, and black keeps its
/// saturation too, so the picker's marks don't jump.
pub(crate) fn hsv_of(c: u32, keep: Hsv) -> Hsv {
    let [r, g, b] = [c >> 16 & 0xFF, c >> 8 & 0xFF, c & 0xFF].map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    if max == 0.0 {
        return (keep.0, keep.1, 0.0);
    }
    if d == 0.0 {
        return (keep.0, 0.0, max);
    }
    let h = 60.0
        * if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
    (h.rem_euclid(360.0), d / max, max)
}

/// The `#RRGGBB` of an HSV point, each channel rounded.
pub(crate) fn rgb_of((h, s, v): Hsv) -> u32 {
    let f = |n: f32| {
        let k = (n + h / 60.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    let ch = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u32;
    ch(f(5.0)) << 16 | ch(f(3.0)) << 8 | ch(f(1.0))
}

/// A channel as linear light, 0 to 1.
fn linear(c: u32, shift: u32) -> f32 {
    let v = (c >> shift & 0xFF) as f32 / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// CIELAB (D65 white) of a color.
fn lab(c: u32) -> [f32; 3] {
    let (r, g, b) = (linear(c, 16), linear(c, 8), linear(c, 0));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| if t > 216.0 / 24389.0 { t.cbrt() } else { (24389.0 / 27.0 * t + 16.0) / 116.0 };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// How far apart two colors look: CIE76, the plain CIELAB distance. Under 10 reads as very
/// close (spec 0021 AC-8).
pub(crate) fn distance(a: u32, b: u32) -> f32 {
    let (p, q) = (lab(a), lab(b));
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

/// The color the picker's ring shows at hue `h`: that hue at the presets' lightness (OKLCH L
/// .74, chroma .13, lowered only as far as sRGB needs), so the ring sits with the presets.
/// Picking on the ring still sets only the hue.
pub(crate) fn ring_color(h: f32) -> u32 {
    let pure = rgb_of((h, 1.0, 1.0));
    let (r, g, b) = (linear(pure, 16), linear(pure, 8), linear(pure, 0));
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    let angle = (0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s).atan2(1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s);
    let enc = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        ((if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }) * 255.0).round().clamp(0.0, 255.0) as u32
    };
    for step in 0..=26 {
        let c = 0.13 - step as f32 * 0.005;
        let (a, b) = (c * angle.cos(), c * angle.sin());
        let l = (0.74 + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m = (0.74 - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s = (0.74 - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        let rgb = [4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s, -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s, -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s];
        if rgb.iter().all(|x| (-0.001..=1.001).contains(x)) {
            return enc(rgb[0]) << 16 | enc(rgb[1]) << 8 | enc(rgb[2]);
        }
    }
    0xB0B0B0
}

/// The WCAG contrast ratio of two colors, 1 to 21.
pub(crate) fn contrast(a: u32, b: u32) -> f32 {
    let lum = |c: u32| {
        let ch = |s: u32| {
            let v = (c >> s & 0xFF) as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
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

#[cfg(test)]
mod tests;
