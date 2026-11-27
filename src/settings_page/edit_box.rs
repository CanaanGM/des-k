// The page's text box: a Windows EDIT shared by the popup words and the name boxes, shown over
// the field with focus (spec 0022 AC-10, spec 0023 AC-13).
use super::*;
use crate::settings_page::cards::attention::WORDS;
use crate::settings_page::cards::colors::path_of;
use crate::settings_page::cards::names::{Nm, name_edit, name_held, name_look, name_title};

// ---- The page's text box (spec 0022 AC-10, spec 0023 AC-13) ----
//
// A Windows EDIT, as the finder's search box is, so typing, selection, IME and the clipboard
// all work. It serves whichever text field has the page's focus, the popup words or a name, and
// shows only while one does, over the field the page paints; the rest of the time the page
// paints the text, so a menu or the color picker drawn over the page is never covered by it.

thread_local! {
    pub(super) static BOX_EDIT: std::cell::Cell<Option<HWND>> = const { std::cell::Cell::new(None) };
    static BOX_PROC: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    pub(super) static BOX_FONT: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
    // The field it serves, and for a name the screen it writes for, as it was when the box
    // took that field: leaving saves where you typed, whatever the card shows by then.
    pub(super) static BOX_FOR: RefCell<Option<(Ctl, Option<String>)>> = const { RefCell::new(None) };
    // What it shows while empty, and in which ink: a name box's hint.
    pub(super) static BOX_HINT: RefCell<(String, u32)> = const { RefCell::new((String::new(), 0)) };
}

pub(super) fn make_box(page: HWND, inst: windows::Win32::Foundation::HINSTANCE) {
    unsafe {
        let style = WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32);
        let Ok(e) = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), style, 0, 0, 0, 0, Some(page), None, Some(inst), None) else {
            return warn!("settings page: no text box for the popup words and names");
        };
        BOX_PROC.set(SetWindowLongPtrW(e, GWLP_WNDPROC, box_proc as *const () as isize));
        BOX_EDIT.set(Some(e));
    }
}

pub(super) fn edit_text(e: HWND) -> String {
    let mut buf = [0u16; settings::ATTENTION_MAX + 1];
    let n = unsafe { GetWindowTextW(e, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Puts `text` in the box with the caret at its end.
fn set_edit(e: HWND, text: &str) {
    unsafe {
        let _ = SetWindowTextW(e, &windows::core::HSTRING::from(text));
        let end = GetWindowTextLengthW(e) as usize;
        SendMessageW(e, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
    }
}

/// Shows the box over the field that has the page's focus while it can be typed in, and moves
/// Windows' keyboard focus to match; moving on to another field saves the one it leaves first.
/// It takes the file's text only while it has no focus, so a reload never overwrites what you
/// are typing.
pub(super) fn place_box() {
    let Some(e) = BOX_EDIT.get() else { return };
    let Some((page, want, at, font, held, hint)) = get(|w| {
        let s = settings::get();
        let open = w.menu.is_some() || w.pick.is_some() || w.chooser.is_some();
        let can = settings::store::parse_error().is_none() && !open;
        let (want, held, hint) = match w.focus {
            Some(Ctl::Words) if can && s.attention_enabled => (Some((Ctl::Words, None)), s.attention_text.clone(), (String::new(), 0)),
            Some(Ctl::Name(row)) if can => {
                let l = name_look(w, &s, row);
                (Some((Ctl::Name(row), l.screen)), l.shown, (l.hint, painted(w).b.hint))
            }
            _ => (None, String::new(), (String::new(), 0)),
        };
        let at = want.as_ref().and_then(|(c, _)| w.hits.iter().find(|h| h.ctl == *c && h.group == 1)).map(|h| h.at);
        let at = at.filter(|b| b.1 >= w.fixed && b.1 + b.3 <= w.vh);
        let s = w.scale;
        let px = |(x, y, bw, bh): Bx| ((x * s).round() as i32, (y * s).round() as i32, (bw * s).round() as i32, (bh * s).round() as i32);
        (w.hwnd, want, at.map(|b| px((b.0 + 12.0, b.1 + 6.0, b.2 - 24.0, 20.0))), w.fonts.get(F::Body), held, hint)
    }) else {
        return;
    };
    unsafe {
        let has = GetFocus() == e;
        let Some((x, y, bw, bh)) = at else {
            let _ = ShowWindow(e, SW_HIDE);
            if has {
                let _ = SetFocus(Some(page)); // leaving the box saves it
            }
            return;
        };
        // Another field has it: leaving that one saves it, and lays out, which places it again.
        if has && BOX_FOR.with_borrow(|f| *f != want) {
            let _ = SetFocus(Some(page));
            if GetFocus() != e {
                place_box();
            }
            return;
        }
        if !has {
            if BOX_FOR.with_borrow(|f| *f != want) {
                // Windows counts UTF-16 units, as the parser does (spec 0022 AC-9, spec 0023 AC-2).
                let (limit, name) = match want {
                    Some((Ctl::Name(row), _)) => (settings::NAME_MAX, name_title(row)),
                    _ => (settings::ATTENTION_MAX, "Popup words".to_string()),
                };
                SendMessageW(e, EM_SETLIMITTEXT, Some(WPARAM(limit)), Some(LPARAM(0)));
                uia::name_control(e, &name);
                BOX_FOR.set(want);
            }
            if edit_text(e) != held {
                set_edit(e, &held);
            }
        }
        if BOX_HINT.replace(hint.clone()) != hint {
            let _ = InvalidateRect(Some(e), None, true);
        }
        // A new font (the page's dpi changed) sets the margins back to the font's own.
        if BOX_FONT.replace(font.0 as isize) != font.0 as isize {
            SendMessageW(e, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(0)));
            SendMessageW(e, EM_SETMARGINS, Some(WPARAM((EC_LEFTMARGIN | EC_RIGHTMARGIN) as usize)), Some(LPARAM(0)));
        }
        let _ = SetWindowPos(e, None, x, y, bw, bh, SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        if !has && GetFocus() == page {
            let _ = SetFocus(Some(e));
            let end = GetWindowTextLengthW(e) as usize;
            SendMessageW(e, EM_SETSEL, Some(WPARAM(end)), Some(LPARAM(end as isize)));
        }
    }
}

/// Saves the box's words: empty or the default removes the key (AC-10), anything else is
/// written as typed, and unchanged words write nothing.
fn save_words(text: &str) {
    let saved = settings::get().attention_text.clone();
    if text.trim().is_empty() || text == settings::ATTENTION_TEXT {
        if saved != settings::ATTENTION_TEXT {
            commit(Edit::Remove(path_of(WORDS)), "Popup words");
        }
    } else if text != saved {
        commit(Edit::Value(WORDS, text.into()), "Popup words");
    }
}

/// Saves a name box (spec 0023 AC-12, AC-13). A failed write keeps your text for its row, shown
/// as Not saved, until a good save or Esc; a save, or text that writes nothing, forgets it.
fn save_name(screen: Option<String>, row: Nm, text: &str, enter: bool) {
    let _ = set(|w| w.unsaved.retain(|u| !(u.0 == screen && u.1 == row)));
    match name_edit(&settings::get(), screen.as_deref(), row, text, enter) {
        None => layout(),
        Some(edit) => {
            if !wrote(edit, &name_title(row)) {
                let _ = set(|w| w.unsaved.push((screen, row, text.to_string())));
                layout();
            }
        }
    }
}

/// Saves the field the box serves.
fn save_box(text: &str, enter: bool) {
    match BOX_FOR.with_borrow(|f| f.clone()) {
        Some((Ctl::Name(row), screen)) => save_name(screen, row, text, enter),
        Some((Ctl::Words, _)) => save_words(text),
        _ => {}
    }
}

/// The text the box's field holds in the file, and whether your text for it failed to save.
fn box_saved() -> (String, bool) {
    match BOX_FOR.with_borrow(|f| f.clone()) {
        Some((Ctl::Name(row), screen)) => {
            let failed = get(|w| w.unsaved.iter().any(|u| u.0 == screen && u.1 == row)) == Some(true);
            (name_held(&settings::get(), screen.as_deref(), row).1, failed)
        }
        _ => (settings::get().attention_text.clone(), false),
    }
}

unsafe extern "system" fn box_proc(e: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let own: WNDPROC = std::mem::transmute(BOX_PROC.get());
        let page = GetParent(e).unwrap_or_default();
        match m {
            WM_KEYDOWN => match VIRTUAL_KEY(wp.0 as u16) {
                // The page's own keys: Tab, Up and Down move on, as from any stop.
                VK_TAB | VK_UP | VK_DOWN => {
                    let _ = PostMessageW(Some(page), m, wp, lp);
                    return LRESULT(0);
                }
                // Saves and stays. Text that failed to save stays in the box to try again.
                VK_RETURN => {
                    save_box(&edit_text(e), true);
                    let (now, failed) = box_saved();
                    if !failed && edit_text(e) != now {
                        set_edit(e, &now);
                    }
                    return LRESULT(0);
                }
                // Esc puts back the saved text; with nothing changed it is the page's Esc.
                VK_ESCAPE => {
                    let (saved, failed) = box_saved();
                    if edit_text(e) == saved && !failed {
                        let _ = PostMessageW(Some(page), m, wp, lp);
                    } else {
                        if let Some((Ctl::Name(row), screen)) = BOX_FOR.with_borrow(|f| f.clone()) {
                            let _ = set(|w| w.unsaved.retain(|u| !(u.0 == screen && u.1 == row)));
                        }
                        set_edit(e, &saved);
                        layout();
                    }
                    return LRESULT(0);
                }
                _ => {}
            },
            // Their characters would only beep.
            WM_CHAR if matches!(wp.0, 0x09 | 0x0D | 0x1B) => return LRESULT(0),
            WM_MOUSEWHEEL => return SendMessageW(page, m, Some(wp), Some(lp)),
            WM_KILLFOCUS => {
                let r = CallWindowProcW(own, e, m, wp, lp);
                let text = edit_text(e);
                let _ = set(|w| {
                    w.words = None;
                    w.typed = None;
                });
                save_box(&text, false);
                layout();
                return r;
            }
            // An empty name box shows its hint as the page paints it: without Common Controls 6,
            // which des-k's manifest does not ask for, an EDIT has no cue banner of its own.
            WM_PAINT => {
                let r = CallWindowProcW(own, e, m, wp, lp);
                let (hint, ink) = BOX_HINT.with_borrow(|h| h.clone());
                if !hint.is_empty() && GetWindowTextLengthW(e) == 0 {
                    let dc = GetDC(Some(e));
                    let font = HGDIOBJ(SendMessageW(e, WM_GETFONT, None, None).0 as *mut _);
                    let was = SelectObject(dc, font);
                    SetTextColor(dc, ui::color::cref(ink));
                    SetBkMode(dc, TRANSPARENT);
                    let _ = HideCaret(Some(e));
                    let text: Vec<u16> = hint.encode_utf16().collect();
                    let _ = TextOutW(dc, 0, 0, &text);
                    let _ = ShowCaret(Some(e));
                    SelectObject(dc, was);
                    ReleaseDC(Some(e), dc);
                }
                return r;
            }
            _ => {}
        }
        CallWindowProcW(own, e, m, wp, lp)
    }
}
