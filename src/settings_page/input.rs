// The page's window messages: paint, mouse, keys, scroll and size (spec 0012).
mod act;
pub(super) mod keys;

use super::*;
use act::act;
use crate::settings_page::cards::colors::{caret_at, enter_hex, enter_if_new, hex_insert, leave_hex};
use crate::settings_page::cards::place::{ACROSS, DOWN, percent, set_place};
use crate::settings_page::cards::wallpaper::wall_wheel;
use keys::{hit_at, on_key, point};
use super::edit_box::{BOX_EDIT, BOX_FONT, BOX_FOR, BOX_HINT, edit_text, place_box};
use super::menu::{close_menu, menu_open};
use super::picker::{EASE_TIMER, How, RING, SQUARE, ease_finish, ease_step, finish_drag, must_close, opens, pick_close, pick_down, pick_key, pick_key_up, pick_lost_keys, pick_move, preview_for};
use super::recording::stop_rec;

pub(super) unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(h, &mut ps);
                let mut r = RECT::default();
                let _ = GetClientRect(h, &mut r);
                let _ = get(|w| {
                    buffered(dc, 0, 0, r.right, r.bottom, |mem| {
                        let t = painted(w);
                        let mut c = Canvas::new(mem, true, w.scale, t, &w.fonts);
                        c.boxed((0.0, 0.0, r.right as f32 / w.scale + 1.0, r.bottom as f32 / w.scale + 1.0), 0.0, t.b.bg, None);
                        compose(&mut c, w);
                    })
                });
                let _ = EndPaint(h, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // The text box: the page's colors; the popup words' preview and a name box's note
            // follow each key typed.
            WM_CTLCOLOREDIT => {
                let (dc, b) = (HDC(wp.0 as *mut _), get(|w| painted(w).b).unwrap_or(theme().b));
                SetTextColor(dc, ui::cref(b.tx));
                SetBkColor(dc, ui::cref(b.card));
                LRESULT(crate::finder::edit_brush(b.card).0 as isize)
            }
            WM_COMMAND if (wp.0 >> 16) as u32 == EN_CHANGE => {
                if let Some(e) = BOX_EDIT.get()
                    && e.0 as isize == lp.0
                    && GetFocus() == e
                {
                    let text = edit_text(e);
                    let words = BOX_FOR.with_borrow(|f| matches!(f, Some((Ctl::Words, _))));
                    let _ = set(|w| if words { w.words = Some(text) } else { w.typed = Some(text) });
                    // The hint shows only while it is empty: paint all of it again, not only
                    // what changed.
                    let _ = InvalidateRect(Some(e), None, true);
                    layout();
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let (vk, repeat) = (VIRTUAL_KEY(wp.0 as u16), lp.0 >> 30 & 1 == 1);
                ease_finish();
                if get(|w| w.pick.is_some()) == Some(true) {
                    pick_key(vk, repeat);
                } else if !(vk == VK_DELETE && repeat) {
                    // A held Delete removes one workspace only, not one per repeat.
                    on_key(h, vk);
                }
                LRESULT(0)
            }
            WM_KEYUP => {
                pick_key_up(VIRTUAL_KEY(wp.0 as u16));
                LRESULT(0)
            }
            WM_KILLFOCUS => {
                pick_lost_keys();
                LRESULT(0)
            }
            WM_TIMER if wp.0 == EASE_TIMER => {
                ease_step();
                LRESULT(0)
            }
            // The hand over what opens the color picker, the arrow elsewhere; the window's edges
            // keep their own cursors (spec 0021 AC-1).
            WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                let _ = ScreenToClient(h, &mut pt);
                let hand = get(|w| opens(w, hit_at(w, (pt.x as f32 / w.scale, pt.y as f32 / w.scale)))) == Some(true);
                let _ = SetCursor(LoadCursorW(None, if hand { IDC_HAND } else { IDC_ARROW }).ok());
                LRESULT(1)
            }
            // TranslateMessage in main's loop makes these: a hex box takes its digits, never
            // while the color picker has the keys.
            WM_CHAR if get(|w| w.pick.is_some()) == Some(true) => LRESULT(0),
            WM_CHAR => {
                if let Some(Some(Ctl::Hex(k))) = get(|w| w.focus)
                    && let Some(c) = char::from_u32(wp.0 as u32).filter(char::is_ascii_hexdigit)
                    && {
                        enter_if_new(k);
                        get(|w| w.hexes.iter().any(|x| x.key == k)) == Some(true)
                    }
                {
                    hex_insert(k, &c.to_string());
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                ease_finish();
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32;
                if get(menu_open) == Some(true) {
                    if get(|w| matches!(w.chooser, Some((Ctl::WallPick(_), ..)))) == Some(true) {
                        wall_wheel(delta);
                    }
                    return LRESULT(0);
                }
                scroll_by(-delta / 120.0 * 48.0);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                // Where it went down on screen, for the color picker's opening guard (AC-15).
                let pos = GetMessagePos();
                let _ = set(|w| w.down_at = ((pos & 0xFFFF) as i16 as i32, (pos >> 16 & 0xFFFF) as i16 as i32));
                ease_finish();
                // A click while recording only cancels it: the note goes, and the page moves
                // under the mouse, so the click must not land on whatever is there now.
                if get(|w| w.rec.is_some()) == Some(true) {
                    stop_rec();
                    return LRESULT(0);
                }
                if get(|w| w.pick.is_some()) == Some(true) {
                    pick_down(h, lp);
                    return LRESULT(0);
                }
                // A click outside the open menu closes it, and does nothing else.
                if get(|w| menu_open(w) && hit_at(w, point(lp, w.scale)).is_none()) == Some(true) {
                    close_menu();
                    return LRESULT(0);
                }
                let _ = SetCapture(h);
                let was = get(|w| w.focus).flatten();
                let _ = set(|w| {
                    let (x, y) = point(lp, w.scale);
                    w.kb = false;
                    match thumb(w.scroll, w.height, w.vh - w.fixed, w.fixed) {
                        Some((ty, th)) if x >= w.vw - 12.0 && y > w.fixed && w.menu.is_none() => {
                            // On the thumb: drag it; beside it: jump there, then drag.
                            let grab = if (ty..ty + th).contains(&y) { y - ty } else { th / 2.0 };
                            w.drag = Some(grab);
                        }
                        _ => {
                            w.press = hit_at(w, (x, y));
                            // Tiles open the color picker and are no keyboard stop: the focus stays.
                            if let Some(p) = w.press.filter(|p| !matches!(p, Ctl::Item(_) | Ctl::ScreenItem(_) | Ctl::WallTick(_) | Ctl::WsTile(..) | Ctl::ShowTile(..))) {
                                w.focus = Some(p.stop());
                            }
                            // Custom: a press on the picture puts the card's center there.
                            if w.press == Some(Ctl::Picture)
                                && settings::get().indicator_enabled
                                && settings::store::parse_error().is_none()
                                && let Some(at) = w.hits.iter().find(|h| h.ctl == Ctl::Picture).map(|h| h.at)
                            {
                                w.dragging = Some(percent(at, (x, y)));
                            }
                        }
                    }
                });
                // Leaving a hex box saves six digits; a click in one puts the caret there.
                let now = get(|w| w.focus).flatten();
                if let Some(Ctl::Hex(k)) = was
                    && now != was
                {
                    leave_hex(k);
                }
                if let Some(Ctl::Hex(k)) = now
                    && let Some(at) = get(|w| w.hits.iter().find(|x| x.ctl == Ctl::Hex(k)).map(|x| x.at)).flatten()
                {
                    let x = get(|w| point(lp, w.scale).0).unwrap_or(0.0);
                    enter_hex(k, Some(caret_at(at, x)));
                }
                place_box();
                let _ = PostMessageW(Some(h), WM_MOUSEMOVE, wp, lp);
                repaint();
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if let Some((part, pt)) = get(|w| w.pick.as_ref().and_then(|p| p.drag).map(|d| (d, point(lp, w.scale)))).flatten() {
                    pick_move(part, pt);
                    return LRESULT(0);
                }
                let drag = get(|w| {
                    let (_, y) = point(lp, w.scale);
                    let view = w.vh - w.fixed;
                    let (_, th) = thumb(w.scroll, w.height, view, w.fixed)?;
                    let grab = w.drag?;
                    Some((y - grab - w.fixed - 4.0) / (view - 8.0 - th) * (w.height - view))
                })
                .flatten();
                if let Some(to) = drag {
                    scroll_to(to);
                } else if set(|w| {
                    let at = w.hits.iter().find(|h| h.ctl == Ctl::Picture).map(|h| h.at);
                    match (w.dragging, at) {
                        (Some(_), Some(at)) => {
                            w.dragging = Some(percent(at, point(lp, w.scale)));
                            true
                        }
                        _ => false,
                    }
                }) == Some(true)
                {
                    layout();
                } else if set(|w| {
                    let hover = hit_at(w, point(lp, w.scale));
                    std::mem::replace(&mut w.hover, hover) != hover
                }) == Some(true)
                {
                    let mut tme = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: h, dwHoverTime: 0 };
                    let _ = TrackMouseEvent(&mut tme);
                    layout(); // pointing at × or Reset shows what a removal moves
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                let _ = set(|w| w.hover = None);
                layout();
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                // A Custom drag ends: Across, then Down, each against the file as it is then.
                if let Some(Some((x, y))) = set(|w| w.dragging.take()) {
                    set_place(ACROSS, x);
                    set_place(DOWN, y);
                }
                let click = set(|w| {
                    let was = w.press.take();
                    let dragged = w.drag.take().is_some();
                    let now = hit_at(w, point(lp, w.scale));
                    (!dragged && was.is_some() && was == now).then_some(was).flatten()
                })
                .flatten();
                if let Some(c) = click {
                    act(c);
                }
                repaint();
                LRESULT(0)
            }
            WM_CAPTURECHANGED => {
                let _ = set(|w| {
                    w.drag = None;
                    w.dragging = None;
                });
                finish_drag();
                LRESULT(0)
            }
            WM_SIZE => {
                if wp.0 as u32 != SIZE_MINIMIZED {
                    ease_finish();
                    if set(|w| {
                        w.vw = (lp.0 & 0xFFFF) as f32 / w.scale;
                        w.vh = ((lp.0 >> 16) & 0xFFFF) as f32 / w.scale;
                        w.menu = None;
                        w.chooser = None;
                    })
                    .is_some()
                    {
                        // A resize (a move to a screen of another scale is one) closes the
                        // color picker and keeps its color; never from inside the set above.
                        pick_close(How::Keep(false));
                        layout();
                    }
                }
                LRESULT(0)
            }
            WM_GETMINMAXINFO => {
                let mm = &mut *(lp.0 as *mut MINMAXINFO);
                let (min_w, min_h) = size(h, W, MIN_H);
                mm.ptMinTrackSize = POINT { x: min_w, y: min_h };
                LRESULT(0)
            }
            WM_ACTIVATE => {
                // Recording ends when the page loses focus, and an open menu closes.
                if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE {
                    if get(|w| w.rec.is_some()) == Some(true) {
                        stop_rec();
                    }
                    // The color picker closes as the menus do, keeping its color (spec 0021 AC-6).
                    pick_close(How::Keep(false));
                    if get(menu_open) == Some(true) {
                        close_menu();
                    }
                }
                refresh();
                LRESULT(0)
            }
            WM_GETOBJECT => match get(|w| w.tree.clone()) {
                Some(tree) => uia::answer_tree(h, wp, lp, &tree),
                None => DefWindowProcW(h, m, wp, lp),
            },
            WM_DESTROY => {
                // Losing focus closed the color picker already; this drops it writing nothing.
                let _ = KillTimer(Some(h), EASE_TIMER);
                RING.set(None);
                SQUARE.set(None);
                BOX_EDIT.set(None);
                BOX_FONT.set(0);
                BOX_FOR.set(None);
                BOX_HINT.set((String::new(), 0));
                crate::keys::record(h, false);
                let _ = UiaReturnRawElementProvider(h, WPARAM(0), LPARAM(0), None);
                drop(PAGE.with_borrow_mut(Option::take));
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let to = *(lp.0 as *const RECT);
                let scale = (wp.0 & 0xFFFF) as f32 / 96.0;
                let (vw, vh) = set(|w| {
                    w.scale = scale;
                    w.fonts = Fonts::new(scale);
                    (w.vw, w.vh)
                })
                .unwrap_or((W, H));
                // The same size in dip, but never taller than the new screen.
                let (ww, wh) = size(h, vw, vh);
                let (work, _) = work_area(MonitorFromRect(&to, MONITOR_DEFAULTTONEAREST));
                let _ = SetWindowPos(h, None, to.left, to.top, ww, wh.min(work.bottom - work.top), SWP_NOZORDER | SWP_NOACTIVATE);
                layout();
                LRESULT(0)
            }
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                crate::ui::refresh();
                let t = theme();
                // The color picker's preview is rebuilt with the theme, and goes once the page shows its color.
                if set(|w| {
                    if let Some(p) = w.pick.as_mut() {
                        p.preview = preview_for(t, p.key, p.color);
                    }
                    std::mem::replace(&mut w.theme, t) != t
                }) == Some(true)
                {
                    dark_title(h, &t);
                    layout();
                }
                // High contrast coming on closes it, writing nothing (spec 0021 AC-10).
                if get(must_close) == Some(true) {
                    pick_close(How::Quiet);
                }
                DefWindowProcW(h, m, wp, lp)
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}
