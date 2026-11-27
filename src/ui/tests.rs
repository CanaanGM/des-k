// The theme tests: look B's constants, the accent's neighbors and inks, links and tile marks,
// your colors and high contrast, plus the helpers other modules' render tests use.
use super::*;
use windows::core::GUID;

const PLAIN: Look = Look { apps_light: true, system_light: true, transparency: true, hc: None, accent: None };

/// For the render tests (spec 0020): a rose accent, and your own colors for workspaces 1
/// and 2, a deep blue that takes white ink and a pale yellow that takes black.
pub(crate) fn rose(light: bool) -> Theme {
    let mut ws = [None; 9];
    (ws[0], ws[1]) = (Some(0x1E2A78), Some(0xFFE066));
    resolve(light, PLAIN, Accent::Rgb(0xE0457B), ws)
}

// With no Windows accent and no [colors], des-k looks exactly as look B was approved, so
// the LIGHT and DARK constants the render tests draw with are what users see.
#[test]
fn the_constants_are_look_b_unchanged() {
    let t = resolve(true, PLAIN, Accent::Windows, [None; 9]).b;
    let hexes = |v: &[u32]| v.iter().map(|c| format!("{c:06X}")).collect::<Vec<_>>();
    assert_eq!(hexes(&[t.acc_a, t.acc_b, t.on_acc_b, t.link, t.held]), hexes(&[ACC_A, ACC_B, ON_ACC_B, 0x8E3FD9, 0xF1E8FA]));
    let d = resolve(false, PLAIN, Accent::Windows, [None; 9]).b;
    assert_eq!(hexes(&[d.link, d.held]), hexes(&[0xB27CE5, 0x3F2F4E]));
    assert!(resolve(true, PLAIN, Accent::Windows, [None; 9]) == LIGHT);
    assert!(resolve(false, PLAIN, Accent::Windows, [None; 9]) == DARK);
}

// AC-5: a pale yellow takes the near black ink, a deep blue takes white.
#[test]
fn ink_is_the_higher_contrast_of_black_or_white() {
    assert_eq!(ink(0xFFE066), 0x14141A);
    assert_eq!(ink(0x1E2A78), 0xFFFFFF);
    assert_eq!(ink(0x8E3FD9), 0xFFFFFF);
    assert!(WS.iter().all(|c| ink(*c) == 0x14141A));
}

// AC-15: a small link must read on its card for any accent: des-k purple's dark link moves
// to #B27CE5, and rose in light (3.96:1 as the plain accent) is darkened past 4.5:1.
#[test]
fn links_reach_4_5_to_1_on_the_card() {
    assert_eq!(resolve(false, PLAIN, Accent::Windows, [None; 9]).b.link, 0xB27CE5);
    for (light, acc) in [(true, 0xE0457B), (false, 0xE0457B), (true, 0xFFE066), (false, 0x1E2A78), (true, 0x8E3FD9)] {
        let b = resolve(light, PLAIN, Accent::Rgb(acc), [None; 9]).b;
        assert!(contrast(b.link, b.card) >= 4.5, "{acc:06X} light {light}: {:06X}", b.link);
    }
}

// AC-16: white marks vanish on a pale tile you chose, so they take its ink mix; des-k's nine
// were approved with white marks and keep them.
#[test]
fn tile_marks_stay_readable() {
    let mut ws = [None; 9];
    ws[4] = Some(0xFFF4B0);
    let b = resolve(true, PLAIN, Accent::Windows, ws).b;
    assert_eq!(b.mk_ws[4], mix(0xFFF4B0, 0x14141A, 0.55));
    assert!((0..9).filter(|&i| i != 4).all(|i| b.mk_ws[i] == 0xFFFFFF));
    assert!(WS.iter().all(|c| contrast(0xFFFFFF, *c) >= 1.6));
}

// AC-1: light windows fill with Dark1, dark ones with Light2; anything but 32 bytes is no
// palette, and the last good one stays (purple when there never was one).
#[test]
fn the_windows_accent_is_its_shade_for_each_mode() {
    let mut bytes = [0u8; 32];
    bytes[16..19].copy_from_slice(&[0xC4, 0x2B, 0x1C]);
    bytes[4..7].copy_from_slice(&[0xF0, 0x9A, 0x8C]);
    let p = palette_of(&bytes).unwrap();
    let with = Look { accent: Some(p), ..PLAIN };
    assert_eq!(resolve(true, with, Accent::Windows, [None; 9]).b.acc, 0xC42B1C);
    assert_eq!(resolve(false, with, Accent::Windows, [None; 9]).b.acc, 0xF09A8C);
    assert_eq!(palette_of(&bytes[..31]), None);
    // AC-2: your color wins in both modes, exactly as typed.
    assert_eq!(resolve(false, with, Accent::Rgb(0xE0457B), [None; 9]).b.acc, 0xE0457B);
}

// AC-4, AC-5, AC-6: your workspace 3 changes only workspace 3, with its own ink; high
// contrast ignores your colors and Windows' accent alike.
#[test]
fn your_workspace_colors_and_high_contrast() {
    let mut ws = [None; 9];
    ws[2] = Some(0x1E2A78);
    let b = resolve(true, PLAIN, Accent::Rgb(0xFFE066), ws).b;
    assert_eq!((b.ws[2], b.on_ws[2], b.ws[0], b.on_ws[0]), (0x1E2A78, 0xFFFFFF, WS[0], 0x14141A));
    assert_eq!(b.on_acc, 0x14141A);
    let hc = Look { hc: Some([0x000000, 0xFFFFFF, 0x1AEBFF, 0x000001, 0x3FF23F]), accent: Some([0xC42B1C; 8]), ..PLAIN };
    let b = resolve(true, hc, Accent::Rgb(0xE0457B), ws).b;
    assert_eq!((b.acc, b.acc_a, b.acc_b, b.ws[2], b.on_ws), (0x1AEBFF, 0x1AEBFF, 0x1AEBFF, 0x1AEBFF, [0x000001; 9]));
    // The mark is des-k's logo, never an accent (AC-14).
    assert_eq!(resolve(true, PLAIN, Accent::Rgb(0xE0457B), ws).b.mark, MARK);
}

/// The look the color picker's render states preview in: no Windows accent, no contrast.
pub(crate) const fn plain() -> Look {
    PLAIN
}


// AC-4: the first nine presets are des-k's own workspace colors in order, so a workspace's
// default sits where its number says (2 on gold) and picking it takes the key out.
#[test]
fn the_presets_start_with_the_workspace_colors() {
    assert_eq!(PRESETS[..9], WS);
    assert_eq!(PRESETS[9..], [0x8E3FD9, 0x92BF4F, 0x92A7BD]);
}


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
