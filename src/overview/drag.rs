// The overview's mouse: a press, a click, hover with its X, and a drag with its ghost and the
// drop that moves a window to another workspace or screen.
use super::*;
use super::layout::{Hit, Screen, Thumb, inside, thumb};

/// Where a drag would drop, when the drop changes anything.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Aim {
    Tile(usize),
    Spread,
}

/// The small live preview under the mouse during a drag (AC-7); its own popup, above every
/// screen's overview, with Windows' rounded corners, shadow, and a 1 px edge in the text color
/// (AC-OVR-5). Taken down when dropped.
pub(super) struct Ghost {
    hwnd: HWND,
    size: (i32, i32),
    real: (i32, i32), // the window's own size
    small: bool, // over a band: the size it will have in a tile
    thumb: Option<Thumb>, // none: a plain card
}

impl Drop for Ghost {
    fn drop(&mut self) {
        self.thumb = None; // before the popup it draws into
        unsafe { let _ = DestroyWindow(self.hwnd); }
    }
}

/// A `w` wide picture of a `real` sized window.
fn ghost_size(real: (i32, i32), w: i32) -> (i32, i32) {
    (w, (w as i64 * real.1.max(1) as i64 / real.0.max(1) as i64) as i32)
}

pub(super) enum Stage {
    Pressed,        // not yet past the drag distance: still a click
    Moving(Ghost),
    Cancelled,      // Esc or a right click: the release does nothing
}

/// A press on a window, from its press to the button's release. The pressed popup holds the
/// mouse capture throughout, so the drag can cross screens.
pub(super) struct Drag {
    pub(super) hwnd: HWND,
    pub(super) from: (String, usize), // where it lives: screen and workspace
    pub(super) start: POINT,          // screen coordinates
    pub(super) scale: f32,            // the pressed screen's, for the ghost's size
    pub(super) stage: Stage,
    pub(super) aim: Option<(usize, Aim)>, // (screen, where) a drop now would land
}

pub(super) fn aim_rects(sc: &Screen, a: Aim) -> Vec<RECT> {
    match a {
        Aim::Tile(i) => sc.tile_ring(i).into_iter().collect(),
        Aim::Spread => sc.spread_ring().to_vec(),
    }
}

/// The drag ghost for `src`: 240 dip wide at `scale`, the window's aspect, not shown yet.
fn ghost(src: HWND, scale: f32) -> Option<Ghost> {
    let real = ws::real_rect(src);
    let real = ((real.right - real.left).max(1), (real.bottom - real.top).max(1));
    let (w, h) = ghost_size(real, (GHOST_W * scale).round() as i32);
    let hwnd = ui::popup(GHOST_CLASS, Some(ghost_wndproc), WS_EX_TOPMOST | WS_EX_NOACTIVATE, false)?;
    ui::border(hwnd, ui::theme().b.tx);
    unsafe { let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE); }
    let full = RECT { left: 0, top: 0, right: w, bottom: h };
    Some(Ghost { hwnd, size: (w, h), real, small: false, thumb: thumb(hwnd, src, full, full) })
}

fn on_click(popup: HWND, pt: POINT) {
    let hit = SESSION.with_borrow(|s| {
        let sc = s.as_ref()?.screens.iter().find(|sc| sc.popup == popup)?;
        Some((sc.device.clone(), sc.shown, sc.hit(pt)))
    });
    match hit {
        Some((_, _, Hit::Slot(h) | Hit::Mini(_, h))) => go(h),
        Some((device, shown, Hit::Tile(i))) => show(&device, shown, i),
        Some((.., Hit::Background)) => close(true),
        None => {}
    }
}

pub(super) fn on_middle_click(popup: HWND, pt: POINT) {
    // During a drag the point is relative to the pressed popup, whichever screen it is on.
    let hit = SESSION.with_borrow(|s| s.as_ref().filter(|s| s.drag.is_none())?.screens.iter().find(|sc| sc.popup == popup).map(|sc| sc.hit(pt)));
    if let Some(Hit::Slot(h)) = hit {
        close_window(popup, h);
    }
}

/// A press on a window may become a drag: the popup takes the mouse capture, so the moves
/// and the release come back to it from any screen.
pub(super) fn on_press(popup: HWND, pt: POINT) {
    let start = cursor();
    let started = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.iter().find(|sc| sc.popup == popup)?;
        let (hwnd, ws) = match sc.hit(pt) {
            Hit::Slot(h) => (h, sc.shown),
            Hit::Mini(i, h) => (h, i),
            _ => return None,
        };
        s.drag = Some(Drag { hwnd, from: (sc.device.clone(), ws), start, scale: sc.scale, stage: Stage::Pressed, aim: None });
        Some(())
    });
    if started.is_some() {
        unsafe { SetCapture(popup) };
    }
}

/// The dragged window's preview at 40% while it moves, as it was after (AC-OVR-4). High
/// contrast keeps it whole and outlines it instead.
pub(super) fn dim_source(hwnd: HWND, dim: bool) {
    let opacity = if dim && !ui::theme().hc { 102 } else { 255 };
    SESSION.with_borrow(|s| {
        for sc in s.iter().flat_map(|s| &s.screens) {
            for t in sc.all().filter(|x| x.hwnd == hwnd).filter_map(|x| x.thumb.as_ref()) {
                t.set_opacity(opacity);
            }
        }
    });
}

/// Where a drop at `pt` (screen coordinates) would land when it changes anything, for a window
/// living at `from`: a tile, or a spread. Never its own tile or spread (AC-OVR-4). Also the
/// screen under the mouse and whether `pt` is over that screen's band.
fn aim_at(s: &Session, pt: POINT, from: &(String, usize)) -> Option<(usize, Option<Aim>, bool)> {
    let i = s.screens.iter().position(|sc| inside(&sc.rect, pt))?;
    let sc = &s.screens[i];
    let local = POINT { x: pt.x - sc.rect.left, y: pt.y - sc.rect.top };
    let own = |ws: usize| (sc.device.as_str(), ws) == (from.0.as_str(), from.1);
    let aim = match sc.tiles.iter().position(|t| inside(&t.rect, local)) {
        Some(k) => (!own(k)).then_some(Aim::Tile(k)),
        None => (inside(&sc.spread_area(), local) && !own(sc.shown)).then_some(Aim::Spread),
    };
    Some((i, aim, local.y < sc.band_h()))
}

/// A move with a window pressed: past the drag distance the ghost appears, then follows the
/// mouse, and the place a drop would land gets its ring. False when nothing is pressed, so the
/// move is an ordinary one.
pub(super) fn drag_move() -> bool {
    let pt = cursor();
    let pressed = SESSION.with_borrow(|s| {
        let d = s.as_ref()?.drag.as_ref()?;
        let moving = match &d.stage {
            Stage::Pressed => false,
            Stage::Moving(_) => true,
            Stage::Cancelled => return Some(None),
        };
        Some(Some((d.hwnd, d.start, d.scale, moving)))
    });
    let Some(pressed) = pressed else { return false };
    // Cancelled: nothing moves until the release.
    let Some((hwnd, start, scale, moving)) = pressed else { return true };
    if !moving {
        let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
        if (pt.x - start.x).abs() <= cx && (pt.y - start.y).abs() <= cy {
            return true;
        }
        let Some(g) = ghost(hwnd, scale) else {
            warn!("overview: drag preview for {} not created", ws::who(hwnd));
            return true;
        };
        // The X would sit on a window that is now moving.
        let old = SESSION.with_borrow_mut(|s| {
            let s = s.as_mut()?;
            s.drag.as_mut()?.stage = Stage::Moving(g);
            s.hover.take()
        });
        hover_changed(old);
        dim_source(hwnd, true);
        // The chosen ring hides and the bar says what a drop does.
        invalidate_rects(SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default());
    }
    // The bar speaks on the screen under the mouse.
    let on = SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.screens.iter().find(|sc| inside(&sc.rect, pt)).map(|sc| sc.popup)));
    if let Some(p) = on {
        follow_mouse(p);
    }
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let from = s.drag.as_ref()?.from.clone();
        let (i, aim, over_band) = aim_at(s, pt, &from).unwrap_or((usize::MAX, None, false));
        let before = s.drag.as_ref()?.aim;
        let now = aim.map(|a| (i, a));
        let rects: Vec<(HWND, RECT)> = [before, now].into_iter().flatten().filter(|_| before != now).flat_map(|(k, a)| aim_rects(&s.screens[k], a).into_iter().map(move |r| (k, r))).map(|(k, r)| (s.screens[k].popup, r)).collect();
        // Over a band the picture takes its size in that screen's tiles, so the tile it aims
        // at stays in view; it changes size only on crossing the band's rule (AC-OVR-5).
        let tile = s.screens.get(i).map(|sc| (sc.tile_size().0, sc.size().0, sc.scale));
        let d = s.drag.as_mut()?;
        d.aim = now;
        let Stage::Moving(g) = &mut d.stage else { return None };
        let mut resized = None;
        if let Some((tw, sw, sc_scale)) = tile
            && over_band != g.small
        {
            g.small = over_band;
            let w = if over_band { ((g.real.0 as i64 * tw as i64 / sw.max(1) as i64) as i32).max((GHOST_MIN * sc_scale).round() as i32) } else { (GHOST_W * d.scale).round() as i32 };
            g.size = ghost_size(g.real, w);
            if let Some(t) = &g.thumb {
                t.place(RECT { left: 0, top: 0, right: g.size.0, bottom: g.size.1 });
            }
            resized = Some(g.size);
        }
        Some((g.hwnd, g.size, resized, rects))
    });
    let Some((ghost_hwnd, (w, h), resized, rects)) = changed else { return true };
    invalidate_rects(rects);
    let flags = if resized.is_some() { SWP_NOACTIVATE | SWP_SHOWWINDOW } else { SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW };
    unsafe { let _ = SetWindowPos(ghost_hwnd, Some(HWND_TOPMOST), pt.x - w / 2, pt.y - h / 2, w, h, flags); }
    true
}

/// The release: a drop after a drag, a click otherwise, nothing after a cancel.
pub(super) fn on_release(popup: HWND, pt: POINT) {
    let marks = SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default();
    let drag = SESSION.with_borrow_mut(|s| s.as_mut()?.drag.take());
    // After the take, so the capture change finds no drag to cancel.
    unsafe { let _ = ReleaseCapture(); }
    if let Some(d) = &drag
        && !matches!(d.stage, Stage::Pressed)
    {
        dim_source(d.hwnd, false);
        invalidate_rects(marks);
    }
    match drag {
        Some(Drag { stage: Stage::Cancelled, .. }) => {}
        Some(d @ Drag { stage: Stage::Moving(_), .. }) => drop_at(d),
        _ => on_click(popup, pt),
    }
}

/// Esc or a right click during a drag: the ghost goes, and the release does nothing. False
/// when no drag is under way, so Esc closes the overview as usual.
pub(super) fn cancel_drag() -> bool {
    let marks = SESSION.with_borrow(|s| s.as_ref().map(Session::drag_rects)).unwrap_or_default();
    let old = SESSION.with_borrow_mut(|s| {
        let d = s.as_mut()?.drag.as_mut()?;
        d.aim = None;
        matches!(d.stage, Stage::Moving(_)).then(|| (d.hwnd, std::mem::replace(&mut d.stage, Stage::Cancelled)))
    });
    // The ghost goes here, outside the borrow.
    let Some((hwnd, _)) = old else { return false };
    dim_source(hwnd, false);
    invalidate_rects(marks);
    true
}

/// Move the dragged window to the workspace under the mouse, on any screen. Its own place,
/// or anywhere that is neither a tile nor a spread, changes nothing.
fn drop_at(d: Drag) {
    let pt = cursor();
    let target = SESSION.with_borrow(|s| {
        s.as_ref()?.screens.iter().find_map(|sc| {
            let r = sc.rect;
            inside(&r, pt).then(|| (sc.popup, sc.device.clone(), sc.drop_target(POINT { x: pt.x - r.left, y: pt.y - r.top })))
        })
    });
    let (hwnd, from) = (d.hwnd, d.from.clone());
    drop(d); // the ghost goes before the window moves
    let Some((popup, device, Some(to))) = target else { return };
    if (device.as_str(), to) == (from.0.as_str(), from.1) {
        return;
    }
    match ws::place(hwnd, &device, to) {
        Ok(()) => {
            // Hidden now: Esc must not hand focus to a window you can't see.
            if to != ws::active(&device) {
                SESSION.with_borrow_mut(|s| {
                    if let Some(s) = s.as_mut().filter(|s| s.came_from == hwnd) {
                        s.came_from = HWND::default();
                    }
                });
            }
            rebuild();
        }
        Err(Miss::Gone) => {
            info!("overview: a window is gone");
            rebuild();
        }
        Err(Miss::CantMove | Miss::Unplugged) => note(popup, Note::CantMove),
    }
}

/// The selection follows the mouse onto another screen, at that screen's first window, so
/// the keys always act on the screen you are looking at; the bar moves with it.
fn follow_mouse(popup: HWND) {
    let rects = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let i = s.screens.iter().position(|sc| sc.popup == popup)?;
        if i == s.selected.0 {
            return None;
        }
        let marks = |s: &Session| {
            let sc = s.screens.get(s.selected.0)?;
            Some([Some(sc.bar_strip()), s.selected.1.and_then(|h| sc.mark_rect(h))].into_iter().flatten().map(|r| (sc.popup, r)).collect::<Vec<_>>())
        };
        let mut out = marks(s).unwrap_or_default();
        s.selected = (i, s.screens[i].first());
        out.extend(marks(s).unwrap_or_default());
        Some(out)
    });
    invalidate_rects(rects.unwrap_or_default());
}

/// Track the spread window and the tile under the mouse, for their marks, the X and the
/// `"hover"` labels.
pub(super) fn on_move(popup: HWND, pt: POINT) {
    follow_mouse(popup);
    let changed = SESSION.with_borrow_mut(|s| {
        let s = s.as_mut()?;
        let sc = s.screens.iter().find(|sc| sc.popup == popup)?;
        let hit = sc.hit(pt);
        let new = match hit {
            Hit::Slot(h) => Some((popup, h)),
            _ => None,
        };
        let tile = match hit {
            Hit::Tile(i) | Hit::Mini(i, _) => Some((popup, i)),
            _ => None,
        };
        let old_tile = std::mem::replace(&mut s.tile_hover, tile);
        let tiles: Vec<(HWND, RECT)> = if old_tile != tile {
            [old_tile, tile].into_iter().flatten().filter_map(|(p, i)| s.screens.iter().find(|sc| sc.popup == p)?.tile_ring(i).map(|r| (p, r))).collect()
        } else {
            Vec::new()
        };
        let old = std::mem::replace(&mut s.hover, new);
        Some(((old != new).then_some(old), tiles))
    });
    let Some((hover, tiles)) = changed else { return };
    invalidate_rects(tiles);
    if let Some(old) = hover {
        hover_changed(old);
    }
}

/// After `hover` changed from `old`: the X moves to the hovered window (or hides), and the
/// old and new rings and labels redraw. Every screen, since the mouse may have crossed onto
/// another one.
pub(super) fn hover_changed(old: Option<(HWND, HWND)>) {
    let Some((buttons, marks)) = SESSION.with_borrow(|s| {
        let s = s.as_ref()?;
        let at = |sc: &Screen| s.hover.filter(|(p, _)| *p == sc.popup).and_then(|(_, h)| sc.close_spot(h));
        // A window no longer laid out repaints its whole popup.
        let mark = |(p, h): (HWND, HWND)| {
            let sc = s.screens.iter().find(|sc| sc.popup == p);
            (p, sc.and_then(|sc| sc.mark_rect(h)))
        };
        Some((s.screens.iter().map(|sc| (sc.close_btn, at(sc))).collect::<Vec<_>>(), [old, s.hover].into_iter().flatten().map(mark).collect::<Vec<_>>()))
    }) else {
        return;
    };
    unsafe {
        for (b, at) in buttons {
            match at {
                Some(pt) => { let _ = SetWindowPos(b, Some(HWND_TOPMOST), pt.x, pt.y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW); }
                None => { let _ = ShowWindow(b, SW_HIDE); }
            }
        }
        for (p, r) in marks {
            let _ = InvalidateRect(Some(p), r.as_ref().map(|r| r as *const RECT), false);
        }
    }
}

/// The X's click closes the window it sits on.
pub(super) fn on_close_click() {
    if let Some((popup, h)) = SESSION.with_borrow(|s| s.as_ref().and_then(|s| s.hover)) {
        close_window(popup, h);
    }
}

unsafe extern "system" fn ghost_wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
            // What a window Windows draws nothing for shows: a plain card.
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let dc = BeginPaint(hwnd, &mut ps);
                let card = CreateSolidBrush(ui::color::cref(ui::theme().b.card));
                FillRect(dc, &ps.rcPaint, card);
                let _ = DeleteObject(card.into());
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::border(hwnd, ui::theme().b.tx);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
