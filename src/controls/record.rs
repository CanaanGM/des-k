// Shortcut recording, the half the settings page, the first run and the hints window share
// (spec 0012 AC-7, spec 0035 AC-8): telling the hook which window records, and whether a key it
// heard can be saved. What a recorded key then does, and each screen's focus and layout, stay in
// that screen.
use crate::settings::{self, ALT, CTRL, SHIFT, WIN};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LEFT, VK_NUMPAD1, VK_NUMPAD9, VK_RIGHT};

/// The hook swallows every key and sends the first one to `window` until `stop`. One window
/// records at a time; whoever starts cancels the others first.
pub(crate) fn start(window: Option<HWND>) {
    if let Some(h) = window {
        crate::keys::record(h, true);
    }
}

/// Ends `window`'s recording; it changes nothing once another window took recording over.
pub(crate) fn stop(window: Option<HWND>) {
    if let Some(h) = window {
        crate::keys::record(h, false);
    }
}

/// What a recording waits for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind {
    /// Any digit 1 to 9, with Win, Ctrl or Alt: the digits' modifiers.
    Digit,
    /// ← or →, with a modifier: a previous and next pair.
    Arrow,
    /// A key settings.toml can name, with a modifier.
    Combo,
}

/// Why a key can't be saved, as the screen says it: a title and the line under it.
pub(crate) type Refusal = (&'static str, &'static str);

/// Each refusal's words. A screen passes its own, since they word some differently.
pub(crate) struct Words {
    pub(crate) not_digit: Refusal,
    pub(crate) shift_digit: Refusal, // Shift alone held with a digit
    pub(crate) digit_mods: Refusal,  // no modifier with a digit
    pub(crate) not_arrow: Refusal,
    pub(crate) arrow_mods: Refusal,
    pub(crate) not_savable: Refusal,
    pub(crate) no_mods: Refusal,
}

/// The settings page's and the first run's words.
pub(crate) const WORDS: Words = Words {
    not_digit: ("Press a digit 1 to 9", "Hold the keys, then press any digit. Still recording; Esc alone cancels."),
    shift_digit: ("Shift alone can't be used", "Hold Win, Ctrl or Alt with the digit. Still recording."),
    digit_mods: ("A digit needs Win, Ctrl or Alt", "Hold Win, Ctrl or Alt with the digit. Still recording."),
    not_arrow: ("Press ← or →", "Hold the keys, then press either arrow. Still recording; Esc alone cancels."),
    arrow_mods: ("A key needs Win, Ctrl, Alt or Shift", "Hold one of them with the arrow. Still recording; Esc alone cancels."),
    not_savable: ("This key can't be saved", "Use a letter, a digit, F1 to F24, an arrow, or Home, End, Page Up, Page Down, Insert, Delete, Space, Enter, Tab, or Esc with a modifier. Still recording."),
    no_mods: ("A key needs Win, Ctrl, Alt or Shift", "Hold one of them with the key. Still recording; Esc alone cancels."),
};

/// Whether the key heard, `vk` with the modifiers `mods` held, can be saved for `kind`, or the
/// words saying why not. Esc alone isn't asked about: it cancels before this.
pub(crate) fn check(vk: u16, mods: u8, kind: Kind, words: &Words) -> Result<(), Refusal> {
    match kind {
        Kind::Digit => {
            let digit = (b'1' as u16..=b'9' as u16).contains(&vk) || (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&vk);
            if !digit {
                return Err(words.not_digit);
            }
            if mods & (WIN | CTRL | ALT) == 0 {
                return Err(if mods == SHIFT { words.shift_digit } else { words.digit_mods });
            }
        }
        Kind::Arrow => {
            if vk != VK_LEFT.0 && vk != VK_RIGHT.0 {
                return Err(words.not_arrow);
            }
            if mods == 0 {
                return Err(words.arrow_mods);
            }
        }
        Kind::Combo => {
            if !settings::savable(vk) {
                return Err(words.not_savable);
            }
            if mods == 0 {
                return Err(words.no_mods);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
