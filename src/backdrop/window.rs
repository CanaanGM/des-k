// The windows behind the desktop icons, one per screen inside the desktop layer: finding that
// layer and attaching, placing each window, showing its picture, and the fades between.
use super::*;
use super::paint::{PAINTS, Paint};

pub(super) struct Screen {
    pub(super) hwnd: HWND,
    rect: RECT,                    // where it was last placed, in screen pixels
    pub(super) fade: Option<(Fade, Instant)>, // a fade under way, and when it started
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Fade {
    Cross, // from one picture to another, painted blended
    In,    // from your wallpaper: the window's alpha rises
    Out,   // to your wallpaper: the window's alpha falls, then it hides
}

/// A decoded picture in a memory DC of its own, kept selected for its life, so a paint, a
/// fade step and the dim layer are each one GDI call.
pub(super) struct Dib {
    pub(super) dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    pub(super) w: i32,
    pub(super) h: i32,
}

impl Dib {
    pub(super) fn new(p: &Picture) -> Option<Dib> {
        let header = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: p.w,
            biHeight: -p.h, // top down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return None;
            }
            let mut bits = std::ptr::null_mut();
            let made = CreateDIBSection(Some(dc), &BITMAPINFO { bmiHeader: header, ..Default::default() }, DIB_RGB_COLORS, &mut bits, None, 0);
            let Some(bmp) = made.ok().filter(|_| !bits.is_null()) else {
                let _ = DeleteDC(dc);
                return None;
            };
            std::ptr::copy_nonoverlapping(p.px.as_ptr(), bits.cast::<u32>(), p.px.len().min((p.w * p.h) as usize));
            let old = SelectObject(dc, bmp.into());
            Some(Dib { dc, bmp, old, w: p.w, h: p.h })
        }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// A debug build only switch for trying AC-12: while a file named `backdrop-fail` sits beside
/// the exe, the layer behind the icons counts as gone and is never found.
#[cfg(debug_assertions)]
fn fail_switch() -> bool {
    std::env::current_exe().is_ok_and(|exe| exe.with_file_name("backdrop-fail").exists())
}

#[cfg(not(debug_assertions))]
fn fail_switch() -> bool {
    false
}

/// The desktop layer behind the icons (24H2 and later): `Progman`'s own `WorkerW` child, which
/// Progman makes on message 0x052C.
fn layer(progman: HWND) -> Result<HWND, &'static str> {
    let sent = unsafe { SendMessageTimeoutW(progman, 0x052C, WPARAM(0xD), LPARAM(1), SMTO_NORMAL, 1000, None) };
    if sent.0 == 0 {
        return Err("the desktop did not answer");
    }
    if fail_switch() {
        return Err("the debug switch backdrop-fail is on");
    }
    unsafe { FindWindowExW(Some(progman), None, w!("WorkerW"), None) }.map_err(|_| "no layer behind the icons")
}

impl Backdrop {
    /// Places the windows again; a window Windows refuses means attaching again.
    pub(super) fn refit(&mut self) {
        if self.worker.is_some() && let Err(why) = self.fit() {
            warn!("backdrop: {why}, attaching again");
            self.detach();
        }
    }

    /// Destroys every window; with the layer gone they may already be.
    pub(super) fn detach(&mut self) {
        for s in self.screens.drain(..) {
            unsafe { let _ = DestroyWindow(s.hwnd); }
        }
        PAINTS.with_borrow_mut(HashMap::clear);
        self.worker = None;
    }

    pub(super) fn retry(&mut self) {
        if self.status != Status::Off && self.worker.is_none() {
            self.attach(true);
            self.show();
        }
    }

    pub(super) fn check(&mut self) {
        if self.status == Status::Off {
            return;
        }
        if self.restamp(true) {
            self.queue_decode();
        }
        self.slides();
        let alive = |h: HWND| unsafe { IsWindow(Some(h)) }.as_bool();
        if let Some(w) = self.worker {
            if alive(w) && self.screens.iter().all(|s| alive(s.hwnd)) && !fail_switch() {
                // The layer moves with the virtual screen's corner (a screen left of the main
                // one went away), and Windows may move it after des-k saw the screens change.
                self.refit();
                self.show();
                return;
            }
            info!("backdrop: the desktop layer went away (explorer restarted?), attaching again");
            self.detach();
            self.set_status(Status::Attaching);
        }
        self.attach(false);
        self.show();
    }

    /// One try at the layer behind the icons (AC-11, AC-12). No desktop at all is explorer
    /// starting or restarting: it only waits. A desktop with no usable layer for
    /// `ATTACH_GRACE` is a failure; then it tries every 5 s, or at once when explorer restarts
    /// (`now`) or the desktop window changes.
    pub(super) fn attach(&mut self, now: bool) {
        let Ok(progman) = (unsafe { FindWindowW(w!("Progman"), None) }) else {
            if !self.waiting {
                info!("backdrop: waiting for the desktop (explorer is starting)");
                self.waiting = true;
            }
            self.progman = None;
            self.stuck_since = None;
            return;
        };
        self.waiting = false;
        let at = Instant::now();
        let same = self.progman == Some(progman);
        if self.status == Status::Failed && same && !now && self.last_try.is_some_and(|t| at - t < FAILED_RETRY) {
            return;
        }
        if !same {
            self.progman = Some(progman);
            self.stuck_since = None;
        }
        self.last_try = Some(at);
        let got = layer(progman).map_err(str::to_string).and_then(|worker| {
            self.worker = Some(worker);
            self.fit().map(|()| worker)
        });
        match got {
            Ok(_) => {
                if self.status == Status::Failed {
                    info!("backdrop: drawing behind the desktop icons again");
                } else {
                    info!("backdrop: attached behind the desktop icons");
                }
                self.set_status(Status::Shown);
                self.stuck_since = None;
                self.read_back();
            }
            Err(why) => {
                self.detach();
                let since = *self.stuck_since.get_or_insert(at);
                if self.status != Status::Failed && at - since >= ATTACH_GRACE {
                    self.set_status(Status::Failed);
                    error!("backdrop: can't draw behind the desktop icons ({why}), every screen shows your Windows wallpaper; trying again every 5 s");
                }
            }
        }
    }

    /// One window per wanted screen inside the layer, each at its screen's place.
    fn fit(&mut self) -> Result<(), String> {
        let worker = self.worker.ok_or("not attached")?;
        while self.screens.len() > self.want.len() {
            let s = self.screens.pop().expect("longer than want");
            PAINTS.with_borrow_mut(|p| p.remove(&(s.hwnd.0 as isize)));
            unsafe { let _ = DestroyWindow(s.hwnd); }
        }
        while self.screens.len() < self.want.len() {
            // Always layered: Progman has no redirection surface (WS_EX_NOREDIRECTIONBITMAP, seen
            // 2026-11-21 on 25H2), so a plain child inside it shows nothing; a layered one gets
            // its own. It shows nothing until its alpha is set.
            let ex = WS_EX_NOPARENTNOTIFY | WS_EX_LAYERED;
            let inst = unsafe { GetModuleHandleW(None) }.ok();
            let hwnd = unsafe { CreateWindowExW(ex, CLASS, w!(""), WS_CHILD | WS_DISABLED | WS_CLIPSIBLINGS, 0, 0, 0, 0, Some(worker), None, inst.map(Into::into), None) }
                .map_err(|e| format!("Windows refused a window behind the desktop icons ({e})"))?;
            unsafe { let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA); }
            self.screens.push(Screen { hwnd, rect: RECT::default(), fade: None });
        }
        let mut origin = [POINT::default()];
        unsafe { MapWindowPoints(Some(worker), None, &mut origin) };
        let moved = origin[0] != self.origin;
        self.origin = origin[0];
        for (s, w) in self.screens.iter_mut().zip(&self.want) {
            if s.rect == w.rect && !moved {
                continue;
            }
            // The virtual screen can start at negative coordinates; the WorkerW's client starts at 0.
            let mut pts = [POINT { x: w.rect.left, y: w.rect.top }, POINT { x: w.rect.right, y: w.rect.bottom }];
            unsafe { MapWindowPoints(None, Some(worker), &mut pts) };
            let (x, y, cx, cy) = (pts[0].x, pts[0].y, pts[1].x - pts[0].x, pts[1].y - pts[0].y);
            match unsafe { SetWindowPos(s.hwnd, None, x, y, cx, cy, SWP_NOACTIVATE | SWP_NOZORDER) } {
                Ok(()) => s.rect = w.rect,
                Err(e) => {
                    s.rect = RECT::default(); // hidden by show, and placed again at the next check
                    warn!("backdrop: a screen's window not placed ({e})");
                }
            }
        }
        Ok(())
    }

    /// Each window shows its screen's shown workspace's picture once it is decoded, else hides,
    /// so your wallpaper shows (AC-3, AC-7). Painted now, so it lands with the switch (AC-4), or
    /// faded over 150 ms with `transition = "fade"`.
    pub(super) fn show(&mut self) {
        let pics: Vec<Option<Rc<Dib>>> = self.want.iter().map(|w| self.file_of(w, w.shown).and_then(|f| self.pictures.get(&(f.clone(), size(&w.rect)))).map(|(_, p)| p.clone())).collect();
        let stepped = std::mem::take(&mut self.stepped);
        for ((s, w), pic) in self.screens.iter_mut().zip(&self.want).zip(pics) {
            let id = s.hwnd.0 as isize;
            let pic = pic.filter(|_| s.rect == w.rect);
            // What the window ends on once any fade under way is done; the same lets it run.
            let out = s.fade.is_some_and(|(f, _)| f == Fade::Out);
            let ends = PAINTS.with_borrow(|p| p.get(&id).map(|p| p.pic.clone())).filter(|_| !out);
            let same = match (&ends, &pic) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            finish(s);
            let now = PAINTS.with_borrow(|p| p.get(&id).map(|p| p.pic.clone()));
            // A slideshow's change always crossfades, whatever transition says (spec 0028 AC-6).
            let fade = if stepped.contains(&id) { !self.look.fastest } else { self.look.fade };
            let start = Some(Instant::now());
            unsafe {
                match (now, pic) {
                    (Some(old), Some(new)) => {
                        PAINTS.with_borrow_mut(|m| m.insert(id, Paint { pic: new, from: fade.then_some(old), alpha: if fade { 0 } else { 255 } }));
                        s.fade = start.filter(|_| fade).map(|t| (Fade::Cross, t));
                        let _ = InvalidateRect(Some(s.hwnd), None, false);
                        let _ = UpdateWindow(s.hwnd);
                    }
                    (None, Some(new)) => {
                        PAINTS.with_borrow_mut(|m| m.insert(id, Paint { pic: new, from: None, alpha: 255 }));
                        if fade {
                            // Clear before it shows, so no frame flashes at full strength.
                            let _ = SetLayeredWindowAttributes(s.hwnd, COLORREF(0), 0, LWA_ALPHA);
                            s.fade = start.map(|t| (Fade::In, t));
                        }
                        let _ = InvalidateRect(Some(s.hwnd), None, false);
                        let _ = ShowWindow(s.hwnd, SW_SHOWNA);
                        let _ = UpdateWindow(s.hwnd);
                    }
                    (Some(_), None) if fade => s.fade = start.map(|t| (Fade::Out, t)),
                    (Some(_), None) => {
                        PAINTS.with_borrow_mut(|m| m.remove(&id));
                        let _ = ShowWindow(s.hwnd, SW_HIDE);
                    }
                    (None, None) => {}
                }
            }
        }
        if self.ticker == 0 && self.screens.iter().any(|s| s.fade.is_some()) {
            self.ticker = unsafe { SetTimer(None, 0, FADE_TICK, None) };
        }
    }

    /// One step of every fade under way; the timer stops with the last one.
    pub(super) fn tick(&mut self) {
        for s in &mut self.screens {
            let Some((f, start)) = s.fade else { continue };
            let t = start.elapsed().as_secs_f32() / FADE.as_secs_f32();
            if t >= 1.0 {
                finish(s);
                continue;
            }
            let a = (t * 255.0) as u8;
            unsafe {
                match f {
                    Fade::Cross => {
                        PAINTS.with_borrow_mut(|m| m.get_mut(&(s.hwnd.0 as isize)).map(|p| p.alpha = a));
                        let _ = InvalidateRect(Some(s.hwnd), None, false);
                        let _ = UpdateWindow(s.hwnd);
                    }
                    Fade::In => {
                        let _ = SetLayeredWindowAttributes(s.hwnd, COLORREF(0), a, LWA_ALPHA);
                    }
                    Fade::Out => {
                        let _ = SetLayeredWindowAttributes(s.hwnd, COLORREF(0), 255 - a, LWA_ALPHA);
                    }
                }
            }
        }
        if self.screens.iter().all(|s| s.fade.is_none()) {
            unsafe { let _ = KillTimer(None, self.ticker); }
            self.ticker = 0;
        }
    }

    /// Paints every window showing a picture again: the dim changed.
    pub(super) fn repaint(&self) {
        for s in &self.screens {
            if PAINTS.with_borrow(|p| p.contains_key(&(s.hwnd.0 as isize))) {
                unsafe {
                    let _ = InvalidateRect(Some(s.hwnd), None, false);
                    let _ = UpdateWindow(s.hwnd);
                }
            }
        }
    }
}

/// Ends `s`'s fade under way at once, as it would end.
fn finish(s: &mut Screen) {
    let Some((f, _)) = s.fade.take() else { return };
    let id = s.hwnd.0 as isize;
    unsafe {
        match f {
            Fade::Cross => {
                PAINTS.with_borrow_mut(|m| m.get_mut(&id).map(|p| (p.from, p.alpha) = (None, 255)));
                let _ = InvalidateRect(Some(s.hwnd), None, false);
                let _ = UpdateWindow(s.hwnd);
            }
            Fade::In => {
                let _ = SetLayeredWindowAttributes(s.hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
            Fade::Out => {
                let _ = ShowWindow(s.hwnd, SW_HIDE);
                let _ = SetLayeredWindowAttributes(s.hwnd, COLORREF(0), 255, LWA_ALPHA);
                PAINTS.with_borrow_mut(|m| m.remove(&id));
            }
        }
    }
}
