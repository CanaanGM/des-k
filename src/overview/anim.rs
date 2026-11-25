// The overview's motion: easing, the fade in as it opens, and the popups fading out as it closes.
use super::*;
use super::layout::Screen;

/// The popups of a closed session while they fade out: no focus, and clicks go through them.
pub(super) struct Leaving {
    pub(super) screens: Vec<Screen>,
    start: Instant,
    from: u8, // the alpha the fade starts at
}

/// Ease out: fast at first, then settling. 0 at the start, exactly 1 at the end. Squared,
/// not cubed: a cube covers most of the way in the first frames and looks like no animation.
fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(2)
}

/// `[overview] fade_ms` long (spec 0017 AC-7); done at once with `fastest` on (AC-6).
fn progress(start: Instant) -> f32 {
    let ms = ui::motion_ms(settings::get().overview_fade_ms);
    if ms == 0 {
        return 1.0;
    }
    ease((start.elapsed().as_secs_f32() * 1000.0 / ms as f32).min(1.0))
}

/// Off: every popup goes at once. Fade: they stay a moment, without focus and letting clicks
/// through, while they fade out (AC-11).
pub(super) fn leave(s: Session) {
    drop(LEAVING.take());
    if settings::get().overview_animation == Animation::Off {
        return drop(s);
    }
    let Session { screens, drag, alpha, .. } = s;
    if drag.is_some() {
        unsafe { let _ = ReleaseCapture(); }
    }
    drop(drag); // and its ghost
    let Some(first) = screens.first().map(|sc| sc.popup) else { return };
    for sc in &screens {
        unsafe {
            let _ = ShowWindow(sc.close_btn, SW_HIDE);
            for id in [CLOSING_TIMER, NOTE_TIMER, REFRESH_TIMER, ANIM_TIMER, WALL_TIMER] {
                let _ = KillTimer(Some(sc.popup), id);
            }
            SetWindowLongPtrW(sc.popup, GWL_EXSTYLE, GetWindowLongPtrW(sc.popup, GWL_EXSTYLE) | WS_EX_TRANSPARENT.0 as isize);
        }
    }
    LEAVING.set(Some(Leaving { screens, start: Instant::now(), from: alpha }));
    unsafe { SetTimer(Some(first), ANIM_TIMER, FRAME_MS, None) };
}

/// The whole popup at `alpha`, previews too (Windows may not carry the popup's alpha over to
/// the previews it draws into it).
fn set_alpha(sc: &Screen, alpha: u8) {
    unsafe { let _ = SetLayeredWindowAttributes(sc.popup, COLORREF(0), alpha, LWA_ALPHA); }
    for t in sc.all().filter_map(|x| x.thumb.as_ref()) {
        t.set_opacity(alpha);
    }
}

/// One frame of the session's fade in; true when it just ended.
pub(super) fn step(s: &mut Session) -> bool {
    let Some(start) = s.fading_in else { return false };
    let e = progress(start);
    s.alpha = (255.0 * e).round() as u8;
    for sc in &s.screens {
        set_alpha(sc, s.alpha);
    }
    let done = e >= 1.0;
    if done {
        s.fading_in = None;
    }
    done
}

/// The animation timer: a frame of the open session, or of the popups leaving.
pub(super) fn anim_tick(popup: HWND) {
    if !is_ours(popup) {
        return leaving_tick();
    }
    if SESSION.with_borrow_mut(|s| s.as_mut().is_some_and(step)) {
        unsafe { let _ = KillTimer(Some(popup), ANIM_TIMER); }
    }
}

fn leaving_tick() {
    let done = LEAVING.with_borrow(|l| {
        let Some(l) = l else { return true };
        let e = progress(l.start);
        let alpha = (l.from as f32 * (1.0 - e)).round() as u8;
        for sc in &l.screens {
            set_alpha(sc, alpha);
        }
        e >= 1.0
    });
    if done {
        drop(LEAVING.take()); // outside the borrow: destroying a popup sends it messages
    }
}


#[cfg(test)]
mod tests;
