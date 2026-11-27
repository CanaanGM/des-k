use super::*;
use windows::Win32::UI::Input::KeyboardAndMouse::{VK_F13, VK_NUMPAD5, VK_OEM_PLUS, VK_UP};

const A: u16 = b'A' as u16;
const D5: u16 = b'5' as u16;

// A digit set is saved as modifiers alone, so a digit with nothing held, or with Shift alone
// (typing !, @, #), would take over plain typing.
#[test]
fn a_digit_needs_win_ctrl_or_alt() {
    assert_eq!(check(D5, WIN, Kind::Digit, &WORDS), Ok(()));
    assert_eq!(check(D5, CTRL | SHIFT, Kind::Digit, &WORDS), Ok(()));
    assert_eq!(check(D5, 0, Kind::Digit, &WORDS), Err(WORDS.digit_mods));
    assert_eq!(check(D5, SHIFT, Kind::Digit, &WORDS), Err(WORDS.shift_digit));
}

// Any digit stands for all nine, the numpad's too; 0 and letters aren't digits of a set.
#[test]
fn only_digits_1_to_9_record_a_digit_set() {
    assert_eq!(check(VK_NUMPAD5.0, ALT, Kind::Digit, &WORDS), Ok(()));
    assert_eq!(check(b'0' as u16, ALT, Kind::Digit, &WORDS), Err(WORDS.not_digit));
    assert_eq!(check(A, ALT, Kind::Digit, &WORDS), Err(WORDS.not_digit));
}

// A pair is saved as its modifiers over ← and →, so only those two arrows say which.
#[test]
fn a_pair_takes_left_or_right_with_a_modifier() {
    assert_eq!(check(VK_LEFT.0, WIN | CTRL, Kind::Arrow, &WORDS), Ok(()));
    assert_eq!(check(VK_RIGHT.0, SHIFT, Kind::Arrow, &WORDS), Ok(()));
    assert_eq!(check(VK_UP.0, WIN, Kind::Arrow, &WORDS), Err(WORDS.not_arrow));
    assert_eq!(check(VK_LEFT.0, 0, Kind::Arrow, &WORDS), Err(WORDS.arrow_mods));
}

// A combo must be one settings.toml can write back and read again, and a bare key would
// swallow plain typing.
#[test]
fn a_combo_is_a_nameable_key_with_a_modifier() {
    assert_eq!(check(A, WIN, Kind::Combo, &WORDS), Ok(()));
    assert_eq!(check(VK_F13.0, SHIFT, Kind::Combo, &WORDS), Ok(()));
    assert_eq!(check(VK_OEM_PLUS.0, CTRL, Kind::Combo, &WORDS), Err(WORDS.not_savable));
    assert_eq!(check(A, 0, Kind::Combo, &WORDS), Err(WORDS.no_mods));
}

// Each screen keeps its own refusal words: the rule is shared, the wording isn't.
#[test]
fn a_refusal_comes_in_the_screens_own_words() {
    let mine = Words { no_mods: ("mine", "my line"), ..WORDS };
    assert_eq!(check(A, 0, Kind::Combo, &mine), Err(("mine", "my line")));
    assert_eq!(check(D5, 0, Kind::Digit, &mine), Err(WORDS.digit_mods));
}
