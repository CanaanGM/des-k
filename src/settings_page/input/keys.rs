// The focus list and the keys: Tab order, arrows, Esc, the menu keys, and the hit test
// (spec 0012 AC-16).
use super::*;
use crate::settings_page::cards::colors::{ACCENT, enter_hex, hex_key, leave_hex, set_accent};
use crate::settings_page::cards::place::{MODE, is_place, set_mode, step_place, walk_picture};
use crate::settings_page::choosers::chooser_key;
use crate::settings_page::edit_box::place_box;
use crate::settings_page::menu::{Menu, choose, close_menu, menu_lines, menu_open, open_menu};
use crate::settings_page::recording::{Prompt, give_key, remove};
use crate::settings_page::rows::{Kind, drawn_order, find_row, shortcut, unused, value};
use super::act::{act, row_step, step_key};

/// What Esc does: it backs out of the newest thing first, like a dialog. An open menu closes;
/// the clash prompt presses Cancel, so a stray Esc never closes the page with a choice still
/// owed; then each Dismiss banner goes, save error first; None when nothing is left: it closes
/// the page.
fn escape(prompt: Option<&Prompt>, error: bool, edited: bool) -> Option<Ctl> {
    match prompt {
        Some(p) => Some(Ctl::Cancel(p.key)),
        None if error => Some(Ctl::Dismiss),
        None if edited => Some(Ctl::DismissEdit),
        None => None,
    }
}

/// The keyboard stops in one area, in order, each with its top: the side list, or the rest.
pub(in crate::settings_page) fn stops(w: &Win, side: bool) -> Vec<(Ctl, f32)> {
    let mut out: Vec<(Ctl, f32)> = vec![];
    // The Overview picture's tiles take clicks only, never the keyboard (spec 0021).
    let mut hits: Vec<&Hit> = w.hits.iter().filter(|h| h.ctl.in_side() == side && h.group != 3 && !matches!(h.ctl, Ctl::ShowTile(..))).collect();
    // Banners first, then the column, each in page order.
    hits.sort_by_key(|h| if h.group == 2 { 0 } else { 1 });
    for h in hits {
        let c = h.ctl.stop();
        if !out.iter().any(|(x, _)| *x == c) {
            out.push((c, h.at.1));
        }
    }
    out
}

/// Focuses `c` and shows it; a side list item also jumps to its section.
fn focus_on(c: Ctl) {
    if let Some(Some(Ctl::Hex(k))) = get(|w| w.focus)
        && Ctl::Hex(k) != c
    {
        leave_hex(k);
    }
    let _ = set(|w| w.focus = Some(c));
    if let Ctl::Hex(k) = c {
        enter_hex(k, None);
    }
    match c {
        Ctl::Side(sec) => {
            jump(sec);
            reveal();
        }
        _ => reveal(),
    }
    place_box();
}

/// Where the keyboard goes next, as in Windows Settings: Tab between the side list and the
/// settings, Up and Down from stop to stop.
fn next_stop(w: &Win, vk: VIRTUAL_KEY) -> Option<Ctl> {
    let side = w.focus.is_some_and(Ctl::in_side);
    let first_seen = || {
        let list = stops(w, false);
        list.iter().find(|(_, y)| *y >= w.fixed).or(list.first()).map(|(c, _)| *c)
    };
    match vk {
        VK_TAB if side => first_seen(),
        VK_TAB => current(w.scroll, w.height, w.vh - w.fixed, &w.tops).map(Ctl::Side),
        _ if w.focus.is_none() => first_seen(),
        _ => {
            let list = stops(w, side);
            let i = list.iter().position(|(c, _)| Some(*c) == w.focus)?;
            let j = match vk {
                VK_UP => i.checked_sub(1)?,
                VK_DOWN => i + 1,
                VK_HOME => 0,
                _ => list.len().checked_sub(1)?, // VK_END
            };
            list.get(j).map(|(c, _)| *c)
        }
    }
}

/// Left and Right on the focused stop: a switch goes off or on, a number steps, a choice moves,
/// a shortcut row picks des-k or Windows, the tiles add or remove.
fn change(focus: Ctl, right: bool) {
    if settings::store::parse_error().is_some() {
        return; // read only
    }
    let s = settings::get();
    match focus {
        Ctl::Toggle(k) => {
            if value(&s, k) != right as i64 && unused(&s, k).is_empty() {
                save(k, right as i64);
            }
        }
        Ctl::Stepper(k) if is_place(k) => step_place(k, right),
        Ctl::Pick(ACCENT) => set_accent(right),
        Ctl::Pick(MODE) => set_mode(right),
        Ctl::Picture => walk_picture(right),
        Ctl::Stepper(k) => step_key(k, right),
        Ctl::Tiles(r) => row_step(r, right),
        Ctl::Pick(k) => match find_row(k).map(|r| r.kind) {
            Some(Kind::Pair(..)) if value(&s, k) != !right as i64 => save(k, !right as i64),
            Some(Kind::Choice(words)) if unused(&s, k).is_empty() => {
                let i = value(&s, k) as usize;
                let n = if right { (i + 1).min(words.len() - 1) } else { i.saturating_sub(1) };
                if n != i {
                    save(k, n as i64);
                }
            }
            _ => {}
        },
        Ctl::KeyRow(k) => give_key(k, !right),
        _ => return,
    }
    reveal();
}

/// The open menu's keys: Up and Down move, Enter or Space picks, Esc closes or goes back.
fn menu_key(vk: VIRTUAL_KEY) {
    let Some(m) = get(|w| w.menu).flatten() else { return };
    let n = menu_lines(&m, &settings::get()).len();
    match vk {
        VK_ESCAPE if m.family.is_some() || m.times == 2 => open_menu(m.row, 1, None),
        VK_ESCAPE => close_menu(),
        VK_UP | VK_DOWN | VK_HOME | VK_END => {
            let f = match vk {
                VK_UP => m.focus.saturating_sub(1),
                VK_DOWN => (m.focus + 1).min(n - 1),
                VK_HOME => 0,
                _ => n - 1,
            };
            let _ = set(|w| w.menu = Some(Menu { focus: f, ..m }));
            layout();
        }
        VK_RETURN | VK_SPACE => choose(m.focus),
        _ => {}
    }
}

pub(super) fn on_key(h: HWND, vk: VIRTUAL_KEY) {
    let _ = set(|w| w.kb = true);
    if get(|w| w.chooser.is_some()) == Some(true) {
        return chooser_key(vk);
    }
    if get(|w| w.menu.is_some()) == Some(true) {
        return menu_key(vk);
    }
    let focus = get(|w| w.focus).flatten();
    if let Some(Ctl::Hex(k)) = focus
        && hex_key(k, vk)
    {
        return;
    }
    match vk {
        VK_ESCAPE => match get(|w| escape(w.prompt.as_ref(), w.error.is_some(), w.edited.is_some())).flatten() {
            Some(c) => {
                act(c);
                reveal();
            }
            None => unsafe { let _ = DestroyWindow(h); },
        },
        VK_TAB | VK_UP | VK_DOWN | VK_HOME | VK_END => match get(|w| next_stop(w, vk)).flatten() {
            Some(c) => focus_on(c),
            None => repaint(), // at the end already: just show the ring
        },
        VK_LEFT | VK_RIGHT => {
            if let Some(f) = focus {
                change(f, vk == VK_RIGHT);
            }
        }
        VK_SPACE | VK_RETURN => match focus {
            // A number or a choice changes with Left and Right only, so a stray Space never does.
            Some(Ctl::Stepper(_) | Ctl::Pick(_) | Ctl::Tiles(_) | Ctl::Picture) | None => {}
            // The color picker: nothing on the page moves once it is open (spec 0021).
            Some(f @ Ctl::Swatch(_)) => act(f),
            Some(f) => {
                act(f);
                reveal();
            }
        },
        // On the tiles: removes the last workspace, as × does; on a shortcut row, its last key.
        VK_DELETE => match focus {
            Some(Ctl::Tiles(r)) => change(Ctl::Tiles(r), false),
            Some(Ctl::KeyRow(k)) if settings::store::parse_error().is_none() => {
                let s = settings::get();
                if let Some(sh) = shortcut(k)
                    && let Some(&i) = drawn_order(&s, sh).last()
                {
                    remove(k, i);
                }
            }
            _ => {}
        },
        VK_PRIOR | VK_NEXT => {
            let page = get(|w| w.vh - w.fixed - 40.0).unwrap_or(0.0);
            scroll_by(if vk == VK_PRIOR { -page } else { page });
        }
        _ => {}
    }
}

pub(in crate::settings_page) fn point(lp: LPARAM, scale: f32) -> (f32, f32) {
    ((lp.0 & 0xFFFF) as i16 as f32 / scale, ((lp.0 >> 16) & 0xFFFF) as i16 as f32 / scale)
}

/// The control under a point. Column controls count only inside the column's view; while the
/// menu is open, only its lines do.
pub(in crate::settings_page) fn hit_at(w: &Win, (x, y): (f32, f32)) -> Option<Ctl> {
    let inside = |(hx, hy, hw, hh): Bx| x >= hx && x < hx + hw && y >= hy && y < hy + hh;
    let menu = menu_open(w);
    let shown = |h: &Hit| if menu { h.group == 3 } else { h.group != 1 || (x > NAV && y > w.fixed) };
    w.hits.iter().find(|h| shown(h) && inside(h.at)).map(|h| h.ctl)
}

#[cfg(test)]
mod tests;
