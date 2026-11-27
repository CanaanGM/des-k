// The fonts des-k's windows draw with: each F's face, size and weight, made once per scale,
// and the checks for Segoe UI Variable and Segoe Fluent Icons.
use super::*;

/// The fonts, as `F as usize` indexes.
#[derive(Clone, Copy)]
pub(crate) enum F {
    Body,
    Bold,
    Small,
    SmallBold,
    Title,  // the settings page's headings
    Mono,   // setting names as the file spells them
    Number,    // the indicator's workspace number
    Icon,      // Segoe Fluent Icons
    IconSmall, // a 12 px icon, like the finder's pin chip
    Heading,   // a window's 24 px title, like Shortcut hints'
    IconTiny,  // a 10 px icon: an arrow on a key cap
    TinyBold,  // a small screen's number in the tray menu's map
    Code,      // the settings page's hex boxes: Cascadia Mono 14 (spec 0020)
}

/// Per F: face (0 text, 1 display, 2 mono, 3 icons), size in dip, weight: 400 or 600 only
/// (spec 0017 AC-3).
const FACES: [(u8, f32, i32); 13] = [
    (0, 14.0, 400),
    (0, 14.0, 600),
    (0, 12.0, 400),
    (0, 12.0, 600),
    (1, 20.0, 600),
    (2, 12.0, 400),
    (1, 36.0, 600),
    (3, 16.0, 400),
    (3, 12.0, 400),
    (1, 24.0, 600),
    (3, 10.0, 400),
    (0, 10.0, 600),
    (4, 14.0, 400),
];

/// Every F at one scale, and the widths of text measured in each; deleted when dropped.
pub(crate) struct Fonts([HFONT; FACES.len()], pub(super) [RefCell<HashMap<String, i32>>; FACES.len()]);

thread_local! {
    static VARIABLE: Cell<Option<bool>> = const { Cell::new(None) };
    static FLUENT: Cell<Option<bool>> = const { Cell::new(None) };
    static SHARED: RefCell<HashMap<u32, Rc<Fonts>>> = RefCell::new(HashMap::new());
}

/// Every F at `scale`, made once per scale and kept for des-k's life, so a screen that opens
/// often never makes its fonts again.
pub(crate) fn fonts(scale: f32) -> Rc<Fonts> {
    let key = (scale * 100.0).round() as u32;
    SHARED.with_borrow_mut(|m| m.entry(key).or_insert_with(|| Rc::new(Fonts::new(scale))).clone())
}

impl Fonts {
    pub(crate) fn new(scale: f32) -> Fonts {
        let variable = variable();
        let fonts = FACES.map(|(kind, size, weight)| {
            let face = match (variable, kind) {
                (_, 2) => "Consolas",
                (_, 4) => "Cascadia Mono",
                (_, 3) if fluent() => "Segoe Fluent Icons",
                (_, 3) => "Segoe MDL2 Assets", // the same codes

                (true, 1) => "Segoe UI Variable Display",
                (true, _) => "Segoe UI Variable Text",
                (false, _) => "Segoe UI",
            };
            // GDI fakes a 600 weight from the regular face (heavier and wider); the real
            // semibold is its own family.
            let face = if weight == 600 { format!("{face} Semibold") } else { face.to_string() };
            // Icons are thin lines: ClearType gives them colored fringes, so they get grey smoothing.
            let quality = if kind == 3 { ANTIALIASED_QUALITY } else { CLEARTYPE_QUALITY };
            font_with(PCWSTR(HSTRING::from(face).as_ptr()), size * scale, weight, quality)
        });
        Fonts(fonts, std::array::from_fn(|_| RefCell::default()))
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
        if !has {
            warn!("fonts: Segoe UI Variable is not installed, des-k uses Segoe UI");
        }
        VARIABLE.set(Some(has));
        has
    })
}

/// Whether Segoe Fluent Icons is installed, checked once like `variable`.
fn fluent() -> bool {
    FLUENT.get().unwrap_or_else(|| {
        let has = has_face(w!("Segoe Fluent Icons"));
        if !has {
            warn!("fonts: Segoe Fluent Icons is not installed, des-k uses Segoe MDL2 Assets");
        }
        FLUENT.set(Some(has));
        has
    })
}

/// A font `px` pixels tall; the caller deletes it.
pub(crate) fn font(face: PCWSTR, px: f32, weight: i32) -> HFONT {
    font_with(face, px, weight, CLEARTYPE_QUALITY)
}

fn font_with(face: PCWSTR, px: f32, weight: i32, quality: FONT_QUALITY) -> HFONT {
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
            quality,
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
