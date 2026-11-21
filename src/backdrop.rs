// The pictures behind your desktop icons (spec 0026): one child window per present screen,
// inside the desktop's WorkerW, owned by a thread of its own. A child of explorer's window
// shares input with explorer's thread, so a hung explorer may only ever stall this thread,
// never the main one: main only stores a wish and posts. This thread never takes the ws lock.
// It decodes every workspace's picture for each screen ahead, so a switch only shows a window.
// Your Windows wallpaper is never touched: a hidden window, or none, shows it.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, EndPaint, HBITMAP, HDC, HGDIOBJ,
        InvalidateRect, MapWindowPoints, PAINTSTRUCT, SRCCOPY, SelectObject, UpdateWindow,
    },
    System::{
        Com::{COINIT_MULTITHREADED, CoInitializeEx},
        LibraryLoader::GetModuleHandleW,
        Threading::GetCurrentThreadId,
    },
    UI::{
        Shell::{DESKTOP_WALLPAPER_POSITION, DWPOS_CENTER, DWPOS_FILL, DWPOS_FIT, DWPOS_STRETCH},
        WindowsAndMessaging::*,
    },
};
use windows::core::w;

use crate::settings::Fit;
use crate::ui::Picture;
use crate::wallpaper::Problem;

const CLASS: windows::core::PCWSTR = w!("des-k-backdrop");
const WM_WISH: u32 = WM_USER; // main stored a new wish
const WM_RETRY: u32 = WM_USER + 1; // explorer restarted: attach again now, even while failed
const WM_DECODE: u32 = WM_USER + 2; // decode the next picture still missing
// How long the desktop may exist without a usable layer before it counts as failed (AC-12).
const ATTACH_GRACE: Duration = Duration::from_secs(5);
const FAILED_RETRY: Duration = Duration::from_secs(5);
const FADE: Duration = Duration::from_millis(150); // AC-4
const FADE_TICK: u32 = 16; // ms between a fade's steps

/// What one present screen should show.
#[derive(Clone, PartialEq)]
pub(crate) struct Want {
    pub(crate) rect: RECT,    // in screen pixels
    pub(crate) shown: usize,  // its shown workspace, 0 based
    pub(crate) files: Vec<Option<String>>, // each workspace's picture, decoded ahead (AC-7)
}

/// How every picture looks, the same on every screen.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Look {
    pub(crate) fit: Fit,
    pub(crate) dim: u8,    // percent darker, laid over at paint time (AC-18)
    pub(crate) fade: bool, // transition = "fade" and not fastest (AC-4)
}

// The latest wish from main; last one wins.
static WISH: Mutex<Option<(Vec<Want>, Look)>> = Mutex::new(None);
// The backdrop thread's id, set once its message queue exists.
static THREAD: OnceLock<u32> = OnceLock::new();
static SPAWNED: OnceLock<()> = OnceLock::new();
// The backdrop can't draw behind the icons now: the settings page's note (AC-12).
static FAILED: AtomicBool = AtomicBool::new(false);

/// Whether des-k can't draw behind your desktop icons right now (AC-12), for the settings page.
pub(crate) fn failed() -> bool {
    FAILED.load(Ordering::Relaxed)
}

/// Main thread: what each present screen should show. Stores the wish and wakes the backdrop
/// thread, starting it on first use; never waits on it.
pub(crate) fn want(wants: Vec<Want>, look: Look) {
    // AC-17: until some workspace has a picture there is no thread, no window, no decode.
    if SPAWNED.get().is_none() && wants.iter().all(|w| w.files.iter().all(Option::is_none)) {
        return;
    }
    *WISH.lock().unwrap_or_else(|e| e.into_inner()) = Some((wants, look));
    if SPAWNED.set(()).is_ok()
        && let Err(e) = std::thread::Builder::new().name("backdrop".into()).spawn(run)
    {
        error!("backdrop: no thread to draw pictures ({e}), every screen shows your Windows wallpaper");
    }
    // Not there yet: the thread reads the wish itself once its queue exists.
    post(WM_WISH);
}

/// Main thread, on the tray's TaskbarCreated: explorer restarted, attach again at once (AC-11).
pub(crate) fn explorer_restarted() {
    post(WM_RETRY);
}

fn post(msg: u32) {
    if let Some(&tid) = THREAD.get() {
        unsafe { let _ = PostThreadMessageW(tid, msg, WPARAM(0), LPARAM(0)); }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Off,       // no picture anywhere: no window, nothing sent to the desktop (AC-17)
    Attaching, // looking for the layer behind the icons
    Shown,     // a window per screen
    Failed,    // the desktop is there but gives no layer: every screen shows your wallpaper
}

/// A decoded picture: its file in the wallpapers folder and the screen size it was made for.
type Key = (String, (i32, i32));
/// A file's modified time as last read; None: not in the folder.
type Stamp = Option<SystemTime>;
/// What a decode was made from besides its key: the file's stamp, the fit and the bar color.
/// Any of them changing decodes again, and the old picture shows until then.
type Made = (Stamp, Fit, u32);

struct Screen {
    hwnd: HWND,
    rect: RECT,                    // where it was last placed, in screen pixels
    fade: Option<(Fade, Instant)>, // a fade under way, and when it started
}

#[derive(Clone, Copy, PartialEq)]
enum Fade {
    Cross, // from one picture to another, painted blended
    In,    // from your wallpaper: the window's alpha rises
    Out,   // to your wallpaper: the window's alpha falls, then it hides
}

/// A decoded picture in a memory DC of its own, kept selected for its life, so a paint, a
/// fade step and the dim layer are each one GDI call.
struct Dib {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    w: i32,
    h: i32,
}

impl Dib {
    fn new(p: &Picture) -> Option<Dib> {
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

/// What a window paints: its picture, over the one it fades from.
#[derive(Clone)]
struct Paint {
    pic: Rc<Dib>,
    from: Option<Rc<Dib>>, // a crossfade under way: drawn first, `pic` over it at `alpha`
    alpha: u8,
}

struct Backdrop {
    folder: PathBuf,
    status: Status,
    progman: Option<HWND>, // the desktop window the last try found
    worker: Option<HWND>,  // the layer behind the icons, once attached
    screens: Vec<Screen>,  // one per `want`, in order, while attached
    want: Vec<Want>,
    look: Look,
    stuck_since: Option<Instant>, // the first failed try with the desktop there
    last_try: Option<Instant>,
    waiting: bool, // the "no desktop" line is out for this outage
    origin: POINT,         // where the layer's client area started at the last placing, in screen pixels
    pictures: HashMap<Key, (Made, Rc<Dib>)>, // an older one shows until its new decode is ready
    stamps: HashMap<String, Stamp>, // each wanted file's, read every second (AC-8)
    bad: HashMap<String, Stamp>,    // a file that failed, and its stamp then: logged once, tried again when it changes
    decode_queued: bool,
    pass: bool,    // a run of decodes is under way; the bar color is read as one starts
    back: u32,     // your Windows background color, 0xRRGGBB: the bars (AC-5)
    ticker: usize, // the fade timer while any fade runs, else 0
}

thread_local! {
    // What each window paints, by hwnd; read by its wndproc on this thread.
    static PAINTS: RefCell<HashMap<isize, Paint>> = RefCell::new(HashMap::new());
    static DIM: Cell<u8> = const { Cell::new(0) };
    static BLACK: Option<Dib> = Dib::new(&Picture { w: 1, h: 1, px: vec![0xFF00_0000] });
}

fn run() {
    let mut msg = MSG::default();
    // Makes this thread's message queue, so a post from main lands from here on.
    unsafe { let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE); }
    let _ = THREAD.set(unsafe { GetCurrentThreadId() });
    // MTA, like main: the bar color is asked of explorer, and an STA would pump messages there.
    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    crate::ui::gdiplus();
    unsafe {
        if let Ok(inst) = GetModuleHandleW(None) {
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() });
        }
    }
    // Explorer restarting destroys the WorkerW and every window in it: checked each second.
    let check = unsafe { SetTimer(None, 0, 1000, None) };
    let mut b = Backdrop {
        folder: crate::ws::data_dir().join("wallpapers"),
        status: Status::Off,
        progman: None,
        worker: None,
        screens: Vec::new(),
        want: Vec::new(),
        look: Look { fit: Fit::Fill, dim: 0, fade: false },
        stuck_since: None,
        last_try: None,
        waiting: false,
        origin: POINT::default(),
        pictures: HashMap::new(),
        stamps: HashMap::new(),
        bad: HashMap::new(),
        decode_queued: false,
        pass: false,
        back: 0, // black until read
        ticker: 0,
    };
    b.take();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        match msg.message {
            WM_WISH if msg.hwnd.is_invalid() => b.take(),
            WM_RETRY if msg.hwnd.is_invalid() => b.retry(),
            WM_DECODE if msg.hwnd.is_invalid() => b.decode_next(),
            WM_TIMER if msg.hwnd.is_invalid() && msg.wParam.0 == check => b.check(),
            WM_TIMER if msg.hwnd.is_invalid() && b.ticker != 0 && msg.wParam.0 == b.ticker => b.tick(),
            _ => unsafe {
                DispatchMessageW(&msg);
            },
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

fn size(r: &RECT) -> (i32, i32) {
    (r.right - r.left, r.bottom - r.top)
}

pub(crate) fn position(fit: Fit) -> DESKTOP_WALLPAPER_POSITION {
    match fit {
        Fit::Fill => DWPOS_FILL,
        Fit::Whole => DWPOS_FIT,
        Fit::Stretch => DWPOS_STRETCH,
        Fit::Center => DWPOS_CENTER,
    }
}

/// The dim layer's constant alpha: `round(dim × 255 / 100)` (AC-18).
pub(crate) fn dim_alpha(dim: u8) -> u8 {
    ((dim as u32 * 255 + 50) / 100).min(255) as u8
}

impl Backdrop {
    fn set_status(&mut self, status: Status) {
        self.status = status;
        FAILED.store(status == Status::Failed, Ordering::Relaxed);
    }

    fn take(&mut self) {
        let Some((want, look)) = WISH.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        self.want = want;
        let dimmed = look.dim != self.look.dim;
        self.look = look;
        DIM.set(look.dim);
        if self.want.iter().all(|w| w.files.iter().all(Option::is_none)) {
            self.off();
            return;
        }
        if self.status == Status::Off {
            self.set_status(Status::Attaching);
        }
        // Pictures nothing asks for any more are dropped (AC-7). Only new files are stamped
        // here, so a switch reads no file; the check timer reads them all.
        let wanted = self.wanted();
        self.pictures.retain(|k, _| wanted.contains(k));
        self.bad.retain(|f, _| wanted.iter().any(|k| k.0 == *f));
        self.restamp(false);
        self.refit();
        if self.worker.is_none() {
            self.attach(false);
        }
        self.show();
        if dimmed {
            // Dim is laid over at paint time, so a change only paints again (AC-18).
            self.repaint();
        }
        self.queue_decode();
    }

    /// Every (file, size) any screen may show, the shown ones first.
    fn wanted(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = Vec::new();
        let shown = self.want.iter().filter_map(|w| Some((w.files.get(w.shown)?.as_ref()?, size(&w.rect))));
        let rest = self.want.iter().flat_map(|w| w.files.iter().flatten().map(move |f| (f, size(&w.rect))));
        for (f, s) in shown.chain(rest) {
            if !keys.iter().any(|k| k.0 == *f && k.1 == s) {
                keys.push((f.clone(), s));
            }
        }
        keys
    }

    fn off(&mut self) {
        if self.status != Status::Off {
            info!("backdrop: no workspace has a picture, every screen shows your Windows wallpaper");
        }
        self.detach();
        self.set_status(Status::Off);
        self.progman = None;
        self.stuck_since = None;
        self.pictures.clear();
        self.stamps.clear();
        self.bad.clear();
    }

    /// Reads the modified time of each wanted file (`all`), or only of files not read yet.
    /// Returns whether any changed: a file replaced, removed or put back (AC-8).
    fn restamp(&mut self, all: bool) -> bool {
        let files: HashSet<String> = self.wanted().into_iter().map(|k| k.0).collect();
        self.stamps.retain(|f, _| files.contains(f));
        let mut changed = false;
        for f in files {
            if !all && self.stamps.contains_key(&f) {
                continue;
            }
            let now = std::fs::metadata(self.folder.join(&f)).and_then(|m| m.modified()).ok();
            changed |= self.stamps.insert(f, now) != Some(now);
        }
        changed
    }

    fn stamp(&self, file: &str) -> Stamp {
        self.stamps.get(file).copied().flatten()
    }

    fn made(&self, file: &str) -> Made {
        (self.stamp(file), self.look.fit, self.back)
    }

    /// Whether `k` still needs a decode: none yet, or its file, the fit or the bar color
    /// changed since, and it did not already fail as it is now.
    fn missing(&self, k: &Key) -> bool {
        let stamp = self.stamp(&k.0);
        self.bad.get(&k.0) != Some(&stamp) && self.pictures.get(k).is_none_or(|(m, _)| *m != self.made(&k.0))
    }

    /// Reads your Windows background color, at each attach and each run of decodes; a change
    /// decodes every picture again with the new bars (AC-5). A failed read keeps the last one.
    fn read_back(&mut self) {
        if let Some(back) = crate::wallpaper::background()
            && back != self.back
        {
            self.back = back;
            self.queue_decode();
        }
    }

    /// Places the windows again; a window Windows refuses means attaching again.
    fn refit(&mut self) {
        if self.worker.is_some() && let Err(why) = self.fit() {
            warn!("backdrop: {why}, attaching again");
            self.detach();
        }
    }

    /// Destroys every window; with the layer gone they may already be.
    fn detach(&mut self) {
        for s in self.screens.drain(..) {
            unsafe { let _ = DestroyWindow(s.hwnd); }
        }
        PAINTS.with_borrow_mut(HashMap::clear);
        self.worker = None;
    }

    fn retry(&mut self) {
        if self.status != Status::Off && self.worker.is_none() {
            self.attach(true);
            self.show();
        }
    }

    fn check(&mut self) {
        if self.status == Status::Off {
            return;
        }
        if self.restamp(true) {
            self.queue_decode();
        }
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
    fn attach(&mut self, now: bool) {
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
    fn show(&mut self) {
        for (s, w) in self.screens.iter_mut().zip(&self.want) {
            let id = s.hwnd.0 as isize;
            let pic = w.files.get(w.shown).and_then(|f| self.pictures.get(&(f.clone()?, size(&w.rect)))).map(|(_, p)| p.clone()).filter(|_| s.rect == w.rect);
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
            let fade = self.look.fade;
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
    fn tick(&mut self) {
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
    fn repaint(&self) {
        for s in &self.screens {
            if PAINTS.with_borrow(|p| p.contains_key(&(s.hwnd.0 as isize))) {
                unsafe {
                    let _ = InvalidateRect(Some(s.hwnd), None, false);
                    let _ = UpdateWindow(s.hwnd);
                }
            }
        }
    }

    fn queue_decode(&mut self) {
        if !self.decode_queued && self.wanted().iter().any(|k| self.missing(k)) {
            self.decode_queued = true;
            post(WM_DECODE);
        }
    }

    /// Decodes one missing picture, then queues the next behind any waiting message, so a
    /// switch never waits for more than the one decode under way.
    fn decode_next(&mut self) {
        self.decode_queued = false;
        if !self.pass {
            self.pass = true;
            self.read_back();
        }
        let Some(key) = self.wanted().into_iter().find(|k| self.missing(k)) else {
            self.pass = false;
            return;
        };
        let (file, (w, h)) = key.clone();
        let made = self.made(&file);
        let path = self.folder.join(&file);
        let screen = RECT { left: 0, top: 0, right: w, bottom: h };
        // By key, never the file name (AC-16): the first workspace that names this file.
        let ws = self.want.iter().find_map(|w| w.files.iter().position(|f| f.as_ref() == Some(&file))).unwrap_or(0) + 1;
        match crate::wallpaper::decode(&path.to_string_lossy(), screen, screen, position(made.1), (w, h), made.2).map(|p| Dib::new(&p)) {
            Some(Some(d)) => {
                self.pictures.insert(key, (made, Rc::new(d)));
                crate::wallpaper::set_problem(&file, None);
            }
            Some(None) => {
                warn!("backdrop: no memory for the picture for wallpaper.ws{ws}, that workspace shows your Windows wallpaper");
                self.pictures.retain(|k, _| k.0 != file);
                self.bad.insert(file, made.0);
            }
            None => {
                let problem = if path.is_file() { Problem::Undecodable } else { Problem::Missing };
                // One line per change of the file, however many screen sizes want it (AC-8).
                warn!("backdrop: the picture for wallpaper.ws{ws} {}, that workspace shows your Windows wallpaper", problem.why());
                self.pictures.retain(|k, _| k.0 != file);
                crate::wallpaper::set_problem(&file, Some(problem));
                self.bad.insert(file, made.0);
            }
        }
        self.show();
        self.queue_decode();
        self.pass = self.decode_queued;
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

/// `src` stretched over `w` by `h` at `dc`'s top left, at `alpha` (255: a plain copy).
fn blend(dc: HDC, src: &Dib, (w, h): (i32, i32), alpha: u8) {
    unsafe {
        if alpha == 255 && (src.w, src.h) == (w, h) {
            let _ = BitBlt(dc, 0, 0, w, h, Some(src.dc), 0, 0, SRCCOPY);
        } else {
            let f = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, SourceConstantAlpha: alpha, ..Default::default() };
            let _ = AlphaBlend(dc, 0, 0, w, h, src.dc, 0, 0, src.w, src.h, f);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            if let Some(p) = PAINTS.with_borrow(|m| m.get(&(hwnd.0 as isize)).cloned()) {
                let size = (p.pic.w, p.pic.h);
                match &p.from {
                    Some(from) => {
                        blend(dc, from, size, 255);
                        blend(dc, &p.pic, size, p.alpha);
                    }
                    None => blend(dc, &p.pic, size, 255),
                }
                // Black over all of it, bars included; the pictures themselves stay undimmed,
                // so a dim change never decodes (AC-18).
                let dim = DIM.get();
                if dim > 0 {
                    BLACK.with(|b| b.as_ref().map(|b| blend(dc, b, size, dim_alpha(dim))));
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        },
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spec 0026 AC-18: the layer's alpha is round(dim × 255 / 100), so each channel ends near
    // c × (100 - dim) / 100: 40 leaves white at 60%, and 0 lays nothing over.
    #[test]
    fn dim_leaves_the_promised_share() {
        assert_eq!((dim_alpha(0), dim_alpha(40), dim_alpha(80)), (0, 102, 204));
        for dim in 0..=80u8 {
            let white = 255 * (255 - dim_alpha(dim) as u32) / 255;
            let promised = 255 * (100 - dim as u32) / 100;
            assert!(white.abs_diff(promised) <= 1, "dim {dim}: {white} vs {promised}");
        }
    }
}
