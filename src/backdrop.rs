// The pictures behind your desktop icons (spec 0026): one child window per present screen,
// inside the desktop's WorkerW, owned by a thread of its own. A child of explorer's window
// shares input with explorer's thread, so a hung explorer may only ever stall this thread,
// never the main one: main only stores a wish and posts. This thread never takes the ws lock.
// It decodes every workspace's picture for each screen ahead, so a switch only shows a window.
// Your Windows wallpaper is never touched: a hidden window, or none, shows it. It also owns
// every slideshow (spec 0028): which picture each screen's workspace shows, its order and its
// counted seconds, in memory only.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

use crate::settings::{Fit, Pick, parse::name_order};
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
// 1 s checks a screen may be missing from the wish before its slideshows start over: a
// monitor's display sleep doesn't restart them (spec 0028 AC-9).
const GONE_CHECKS: u32 = 30;

/// What one present screen should show.
#[derive(Clone, PartialEq)]
pub(crate) struct Want {
    pub(crate) rect: RECT,    // in screen pixels
    pub(crate) id: String,    // the screen's own id: keys its slideshows across rect changes (spec 0028)
    pub(crate) shown: usize,  // its shown workspace, 0 based
    pub(crate) files: Vec<Option<Source>>, // each workspace's wallpaper, decoded ahead (AC-7)
    pub(crate) intervals: Vec<u16>, // each workspace's slideshow interval, minutes (spec 0028 AC-2)
}

/// A workspace's wallpaper as the backdrop gets it: names in the wallpapers folder, never paths.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Source {
    Picture(String),
    Folder(String),   // a slideshow of the subfolder's top level pictures (spec 0028 AC-1)
    Set(Vec<String>), // a slideshow of these pictures, in name order (AC-14)
}

impl Source {
    pub(crate) fn of(p: &Pick) -> Option<Source> {
        match p {
            Pick::None => None,
            Pick::Picture(f) => Some(Source::Picture(f.clone())),
            Pick::Folder(f) => Some(Source::Folder(f.clone())),
            Pick::Set(v) => Some(Source::Set(v.clone())),
        }
    }

    /// What `wallpaper::problem` files this value's note under, so every screen showing it
    /// agrees: `Pick::key`'s text.
    fn key(&self) -> String {
        match self {
            Source::Picture(f) => Pick::Picture(f.clone()),
            Source::Folder(f) => Pick::Folder(f.clone()),
            Source::Set(v) => Pick::Set(v.clone()),
        }
        .key()
    }

    /// Whether `file`, a path relative to the wallpapers folder, is one of this slideshow's.
    fn holds(&self, file: &str) -> bool {
        match self {
            Source::Picture(_) => false,
            Source::Folder(f) => file.strip_prefix(f.as_str()).is_some_and(|rest| rest.starts_with('\\')),
            Source::Set(v) => v.iter().any(|n| n == file),
        }
    }
}

/// How every picture looks, the same on every screen.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Look {
    pub(crate) fit: Fit,
    pub(crate) dim: u8,    // percent darker, laid over at paint time (AC-18)
    pub(crate) fade: bool, // transition = "fade" and not fastest (AC-4)
    pub(crate) fastest: bool, // a slideshow's change is instant (spec 0028 AC-6)
    pub(crate) shuffle: bool, // slideshows go in random order (spec 0028 AC-3)
}

// The latest wish from main; last one wins.
static WISH: Mutex<Option<(Vec<Want>, Look)>> = Mutex::new(None);
// Each slideshow's picture now, by screen id and workspace, as a path relative to the
// wallpapers folder: the overview and the settings page show it (spec 0028 AC-10, AC-11).
static CURRENT: Mutex<BTreeMap<(String, usize), String>> = Mutex::new(BTreeMap::new());
// Bumped at each change of CURRENT, so a reader repaints only when it moved.
static CURRENT_GEN: AtomicU64 = AtomicU64::new(0);

/// Screen `id`'s slideshow picture now on workspace `i`, relative to the wallpapers folder.
pub(crate) fn current(id: &str, i: usize) -> Option<String> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).get(&(id.to_string(), i)).cloned()
}

/// Every slideshow's picture now, for the settings page, which compares it with its last copy.
pub(crate) fn current_all() -> BTreeMap<(String, usize), String> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Moves on whenever any slideshow's picture changed.
pub(crate) fn current_gen() -> u64 {
    CURRENT_GEN.load(Ordering::Relaxed)
}
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

/// A slideshow: one screen's workspace (spec 0028 AC-5).
type ShowKey = (String, usize);

/// One slideshow's place (spec 0028). Pictures are paths relative to the wallpapers folder.
struct Show {
    source: Source,          // a Folder or a Set
    current: Option<String>, // None: nothing to show yet, or nothing it can show (AC-8)
    next: Option<String>,    // decoded ahead while its workspace shows (AC-7); None: one picture
    bag: Vec<String>,        // shuffle's rest of this round (AC-3)
    seconds: u32,            // counted while its picture showed (AC-4)
}

impl Show {
    fn new(source: Source) -> Show {
        Show { source, current: None, next: None, bag: Vec::new(), seconds: 0 }
    }
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
    shows: HashMap<ShowKey, Show>,
    gone: HashMap<String, u32>, // checks in a row a screen with slideshows was missing from the wish
    stepped: HashSet<isize>,    // windows whose next change is a slideshow's: always a crossfade (AC-6)
    skipped: HashMap<String, Vec<String>>, // each slideshow's unreadable pictures as last logged, by its key
    rng: u64,
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
    let mut b = Backdrop::new(crate::ws::data_dir().join("wallpapers"));
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
    fn new(folder: PathBuf) -> Backdrop {
        Backdrop {
            folder,
            status: Status::Off,
            progman: None,
            worker: None,
            screens: Vec::new(),
            want: Vec::new(),
            look: Look { fit: Fit::Fill, dim: 0, fade: false, fastest: false, shuffle: false },
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
            shows: HashMap::new(),
            gone: HashMap::new(),
            stepped: HashSet::new(),
            skipped: HashMap::new(),
            // Any odd number works for xorshift; the clock makes each run's shuffle its own.
            rng: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1,
        }
    }

    fn set_status(&mut self, status: Status) {
        self.status = status;
        FAILED.store(status == Status::Failed, Ordering::Relaxed);
    }

    fn take(&mut self) {
        let Some((want, look)) = WISH.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        self.want = want;
        let dimmed = look.dim != self.look.dim;
        if look.shuffle != self.look.shuffle {
            // The new order starts from the next change; the picture now stays (spec 0028 AC-9).
            self.shows.values_mut().for_each(|s| s.bag.clear());
        }
        self.look = look;
        DIM.set(look.dim);
        if self.want.iter().all(|w| w.files.iter().all(Option::is_none)) {
            self.off();
            return;
        }
        if self.status == Status::Off {
            self.set_status(Status::Attaching);
        }
        self.sync_shows();
        // Pictures nothing asks for any more are dropped (AC-7). Only new files are stamped
        // here, so a switch reads no file; the check timer reads them all.
        self.prune();
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

    /// The picture workspace `i` of `w` shows now: its own, or its slideshow's current one.
    fn file_of<'a>(&'a self, w: &'a Want, i: usize) -> Option<&'a String> {
        match w.files.get(i)?.as_ref()? {
            Source::Picture(f) => Some(f),
            _ => self.shows.get(&(w.id.clone(), i))?.current.as_ref(),
        }
    }

    /// Every (file, size) any screen may show, the shown ones first, then each shown
    /// slideshow's next picture (spec 0028 AC-7): a hidden one never decodes ahead.
    fn wanted(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = Vec::new();
        let shown = self.want.iter().filter_map(|w| Some((self.file_of(w, w.shown)?, size(&w.rect))));
        let rest = self.want.iter().flat_map(|w| (0..w.files.len()).filter_map(move |i| Some((self.file_of(w, i)?, size(&w.rect)))));
        let next = self.want.iter().filter_map(|w| Some((self.shows.get(&(w.id.clone(), w.shown))?.next.as_ref()?, size(&w.rect))));
        for (f, s) in shown.chain(rest).chain(next) {
            if !keys.iter().any(|k| k.0 == *f && k.1 == s) {
                keys.push((f.clone(), s));
            }
        }
        keys
    }

    /// Drops pictures nothing asks for any more (AC-7), and failures of files no value names.
    /// A slideshow's failures stay while it lasts, so a bad picture isn't tried each round.
    fn prune(&mut self) {
        let wanted = self.wanted();
        self.pictures.retain(|k, _| wanted.contains(k));
        let shows = &self.shows;
        self.bad.retain(|f, _| wanted.iter().any(|k| k.0 == *f) || shows.values().any(|s| s.source.holds(f)));
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
        let keys: Vec<ShowKey> = self.shows.keys().cloned().collect();
        for k in keys {
            self.drop_show(&k);
        }
        self.gone.clear();
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
        // By key, never the file name (AC-16): the first workspace that names this file. A
        // slideshow's picture is no workspace's own, and its failures are counted, not named.
        let ws = self.want.iter().find_map(|w| w.files.iter().position(|f| matches!(f, Some(Source::Picture(p)) if *p == file))).map(|i| i + 1);
        match crate::wallpaper::decode(&path.to_string_lossy(), screen, screen, position(made.1), (w, h), made.2).map(|p| Dib::new(&p)) {
            Some(Some(d)) => {
                self.pictures.insert(key, (made, Rc::new(d)));
                if ws.is_some() {
                    crate::wallpaper::set_problem(&file, None);
                }
            }
            Some(None) => {
                if let Some(ws) = ws {
                    warn!("backdrop: no memory for the picture for wallpaper.ws{ws}, that workspace shows your Windows wallpaper");
                }
                self.pictures.retain(|k, _| k.0 != file);
                self.bad.insert(file.clone(), made.0);
                self.slide_failed(&file);
            }
            None => {
                let problem = if path.is_file() { Problem::Undecodable } else { Problem::Missing };
                // One line per change of the file, however many screen sizes want it (AC-8).
                if let Some(ws) = ws {
                    warn!("backdrop: the picture for wallpaper.ws{ws} {}, that workspace shows your Windows wallpaper", problem.why());
                    crate::wallpaper::set_problem(&file, Some(problem));
                }
                self.pictures.retain(|k, _| k.0 != file);
                self.bad.insert(file.clone(), made.0);
                self.slide_failed(&file);
            }
        }
        self.show();
        self.queue_decode();
        self.pass = self.decode_queued;
    }
}

/// The slideshows (spec 0028).
impl Backdrop {
    /// Keeps each slideshow whose value is the same, starts one whose value is new or changed,
    /// and drops one whose screen is in the wish without it. A screen missing from the wish
    /// keeps its slideshows until `forget_gone` (AC-9).
    fn sync_shows(&mut self) {
        let (mut keep, mut fresh, mut renext) = (HashSet::new(), Vec::new(), Vec::new());
        for w in &self.want {
            for (i, f) in w.files.iter().enumerate() {
                let Some(src @ (Source::Folder(_) | Source::Set(_))) = f else { continue };
                let key = (w.id.clone(), i);
                keep.insert(key.clone());
                match self.shows.get_mut(&key) {
                    Some(s) if s.source == *src => {}
                    // A set edit keeps the picture showing now and its count when it is still in the set.
                    Some(s) if matches!((&s.source, src), (Source::Set(_), Source::Set(_))) && s.current.as_ref().is_some_and(|c| src.holds(c)) => {
                        s.source = src.clone();
                        if !s.next.as_ref().is_some_and(|n| src.holds(n)) {
                            renext.push(key);
                        }
                    }
                    _ => {
                        self.shows.insert(key.clone(), Show::new(src.clone()));
                        fresh.push(key);
                    }
                }
            }
        }
        let present: HashSet<&str> = self.want.iter().map(|w| w.id.as_str()).collect();
        let dropped: Vec<ShowKey> = self.shows.keys().filter(|k| present.contains(k.0.as_str()) && !keep.contains(*k)).cloned().collect();
        for k in &dropped {
            self.drop_show(k);
        }
        for k in &fresh {
            self.start(k);
        }
        for k in &renext {
            self.renext(k);
        }
    }

    fn drop_show(&mut self, key: &ShowKey) {
        self.shows.remove(key);
        publish(key, None);
    }

    /// Drops the slideshows of a screen missing from the wish `GONE_CHECKS` checks in a row: one
    /// gone longer starts over when it comes back (AC-9).
    fn forget_gone(&mut self) {
        let present: HashSet<&str> = self.want.iter().map(|w| w.id.as_str()).collect();
        let away: HashSet<String> = self.shows.keys().filter(|k| !present.contains(k.0.as_str())).map(|k| k.0.clone()).collect();
        self.gone.retain(|id, _| away.contains(id));
        for id in away {
            let checks = self.gone.entry(id.clone()).or_insert(0);
            *checks += 1;
            if *checks >= GONE_CHECKS {
                self.gone.remove(&id);
                let keys: Vec<ShowKey> = self.shows.keys().filter(|k| k.0 == id).cloned().collect();
                for k in &keys {
                    self.drop_show(k);
                }
            }
        }
    }

    /// The slideshow's pictures that can show now, in AC-3's order: its folder's top level
    /// pictures or its set's files, less those that failed as they are now (AC-8). Logs a
    /// change in which ones can't be read, by count, never by name (AC-12).
    fn readable(&mut self, key: &ShowKey) -> Result<Vec<String>, Problem> {
        let Some(source) = self.shows.get(key).map(|s| s.source.clone()) else { return Err(Problem::Missing) };
        let listed = list(&self.folder, &source)?;
        let (ok, bad): (Vec<_>, Vec<_>) = listed.into_iter().partition(|(f, stamp)| self.bad.get(f) != Some(stamp));
        let bad: Vec<String> = bad.into_iter().map(|(f, _)| f).collect();
        let name = source.key();
        if self.skipped.get(&name) != Some(&bad) {
            let n = bad.len();
            if n > 0 {
                let what = match (&source, n) {
                    (Source::Set(_), _) => "of its pictures",
                    (_, 1) => "picture in its folder",
                    _ => "pictures in its folder",
                };
                warn!("backdrop: wallpaper.ws{}: {n} {what} can't be read, skipped", key.1 + 1);
            }
            self.skipped.insert(name, bad);
        }
        match ok.is_empty() {
            true => Err(Problem::Unreadable),
            false => Ok(ok.into_iter().map(|(f, _)| f).collect()),
        }
    }

    /// Starts a slideshow over (AC-9): its first picture by name, or the first of a new random
    /// order with shuffle. One with nothing to show shows your Windows wallpaper (AC-8).
    fn start(&mut self, key: &ShowKey) {
        let names = self.readable(key);
        let (shuffle, rng) = (self.look.shuffle, &mut self.rng);
        let Some(show) = self.shows.get_mut(key) else { return };
        (show.current, show.next, show.seconds) = (None, None, 0);
        show.bag.clear();
        let problem = match names {
            Ok(names) => {
                show.current = if shuffle { draw(&mut show.bag, &names, None, rng) } else { names.first().cloned() };
                show.next = upcoming(&names, show.current.as_deref(), shuffle, &mut show.bag, rng);
                None
            }
            Err(p) => Some(p),
        };
        publish(key, show.current.as_ref());
        note(key.1, &show.source, problem);
    }

    /// Picks the slideshow's next picture again, listing its pictures afresh (AC-8).
    fn renext(&mut self, key: &ShowKey) {
        let names = self.readable(key);
        let (shuffle, rng) = (self.look.shuffle, &mut self.rng);
        let Some(show) = self.shows.get_mut(key) else { return };
        show.next = names.ok().and_then(|n| upcoming(&n, show.current.as_deref(), shuffle, &mut show.bag, rng));
    }

    /// The 1 s check's part (AC-4): one second for each screen's shown slideshow while its
    /// window shows its picture, and a change once its interval is up and the next picture is
    /// ready (AC-7). A slideshow with nothing to show looks again, so a folder filled later
    /// shows within 2 s (AC-8).
    fn slides(&mut self) {
        if self.shows.is_empty() {
            return; // AC-13
        }
        self.forget_gone();
        let idle: Vec<ShowKey> = self.shows.iter().filter(|(_, s)| s.current.is_none()).map(|(k, _)| k.clone()).collect();
        let mut moved = !idle.is_empty();
        for k in &idle {
            self.start(k);
        }
        for j in 0..self.want.len().min(self.screens.len()) {
            let w = &self.want[j];
            let (key, size) = ((w.id.clone(), w.shown), size(&w.rect));
            let limit = u32::from(w.intervals.get(w.shown).copied().unwrap_or(30)) * 60;
            let (hwnd, fading) = (self.screens[j].hwnd.0 as isize, self.screens[j].fade.is_some());
            let Some(show) = self.shows.get(&key) else { continue };
            let Some(current) = &show.current else { continue };
            let painted = PAINTS.with_borrow(|p| p.get(&hwnd).map(|p| p.pic.clone()));
            let decoded = |f: &String| self.pictures.get(&(f.clone(), size)).map(|(_, d)| d.clone());
            let on = self.status == Status::Shown && painted.zip(decoded(current)).is_some_and(|(a, b)| Rc::ptr_eq(&a, &b));
            if !on {
                continue; // not drawn: can't draw, or its picture is still decoding
            }
            let ready = show.next.as_ref().map(|n| decoded(n).is_some());
            let show = self.shows.get_mut(&key).expect("read above");
            show.seconds += 1;
            // A change due during a fade on that screen waits for its end.
            if show.seconds < limit || fading {
                continue;
            }
            match ready {
                Some(true) => {
                    (show.current, show.seconds) = (show.next.take(), 0);
                    publish(&key, show.current.as_ref());
                    self.stepped.insert(hwnd);
                    self.renext(&key);
                    moved = true;
                }
                Some(false) => {} // it changes as soon as the next picture is decoded (AC-7)
                None => {
                    // One picture: look for more each interval.
                    self.renext(&key);
                    if let Some(show) = self.shows.get_mut(&key).filter(|s| s.next.is_none()) {
                        show.seconds = 0;
                    }
                    moved = true;
                }
            }
        }
        if moved {
            self.prune();
            self.restamp(false);
            self.queue_decode();
            self.show();
        }
    }

    /// `file` can't be decoded as it is now: a slideshow showing it moves to the following
    /// picture, and one about to show it picks another (AC-8).
    fn slide_failed(&mut self, file: &str) {
        let keys: Vec<ShowKey> = self.shows.iter().filter(|(_, s)| s.current.as_deref() == Some(file) || s.next.as_deref() == Some(file)).map(|(k, _)| k.clone()).collect();
        for key in &keys {
            let names = self.readable(key);
            let (shuffle, rng) = (self.look.shuffle, &mut self.rng);
            let Some(show) = self.shows.get_mut(key) else { continue };
            if show.current.as_deref() != Some(file) {
                show.next = names.ok().and_then(|n| upcoming(&n, show.current.as_deref(), shuffle, &mut show.bag, rng));
                continue;
            }
            show.seconds = 0;
            let problem = match names {
                Ok(names) => {
                    show.current = if shuffle { draw(&mut show.bag, &names, Some(file), rng) } else { after(&names, file) };
                    show.next = upcoming(&names, show.current.as_deref(), shuffle, &mut show.bag, rng);
                    None
                }
                Err(p) => {
                    (show.current, show.next) = (None, None);
                    Some(p)
                }
            };
            publish(key, show.current.as_ref());
            note(key.1, &show.source, problem);
        }
        if !keys.is_empty() {
            self.prune();
            self.restamp(false);
        }
    }
}

/// Sets slideshow `key`'s picture now for the overview and the settings page.
fn publish(key: &ShowKey, current: Option<&String>) {
    let mut all = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    let changed = match current {
        Some(c) => all.insert(key.clone(), c.clone()).as_ref() != Some(c),
        None => all.remove(key).is_some(),
    };
    if changed {
        CURRENT_GEN.fetch_add(1, Ordering::Relaxed);
    }
}

/// Files why slideshow `source` of workspace `ws` (0 based) can't show, for its card row, and
/// logs it once per change by key, never by name (AC-8, AC-12). Screens showing the same value
/// share one note, so the line is not repeated per screen.
fn note(ws: usize, source: &Source, problem: Option<Problem>) {
    let name = source.key();
    if crate::wallpaper::problem(&name) == problem {
        return;
    }
    let ws = ws + 1;
    let set = matches!(source, Source::Set(_));
    match (problem, set) {
        (None, _) => {}
        (Some(Problem::Missing), false) => warn!("backdrop: the folder for wallpaper.ws{ws} is not in the wallpapers folder, that workspace shows your Windows wallpaper"),
        (Some(Problem::Missing), true) => warn!("backdrop: none of the pictures for wallpaper.ws{ws} are in the wallpapers folder, that workspace shows your Windows wallpaper"),
        (Some(Problem::Unreadable), true) => warn!("backdrop: none of the pictures for wallpaper.ws{ws} can be read, that workspace shows your Windows wallpaper"),
        (Some(p), _) => warn!("backdrop: the folder for wallpaper.ws{ws} has {}, that workspace shows your Windows wallpaper", match p {
            Problem::Empty => "no pictures in it",
            _ => "no picture that can be read",
        }),
    }
    crate::wallpaper::set_problem(&name, problem);
}

/// A slideshow's pictures as paths relative to the wallpapers folder, with their stamps, in
/// AC-3's order: a folder's top level files with a picture's name, or a set's names that are
/// files there now. Lists afresh each time, so pictures added or removed count (AC-8).
fn list(folder: &std::path::Path, source: &Source) -> Result<Vec<(String, Stamp)>, Problem> {
    let mut out: Vec<(String, Stamp)> = match source {
        Source::Picture(_) => Vec::new(),
        Source::Folder(f) => {
            let dir = std::fs::read_dir(folder.join(f)).map_err(|_| Problem::Missing)?;
            dir.flatten()
                .filter_map(|e| {
                    let name = e.file_name().into_string().ok().filter(|n| crate::settings::parse::is_picture_name(n))?;
                    let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
                    Some((format!("{f}\\{name}"), meta.modified().ok()))
                })
                .collect()
        }
        Source::Set(names) => names
            .iter()
            .filter_map(|n| {
                let meta = std::fs::metadata(folder.join(n)).ok().filter(std::fs::Metadata::is_file)?;
                Some((n.clone(), meta.modified().ok()))
            })
            .collect(),
    };
    if out.is_empty() {
        return Err(if matches!(source, Source::Folder(_)) { Problem::Empty } else { Problem::Missing });
    }
    out.sort_by(|a, b| name_order(&a.0, &b.0));
    Ok(out)
}

/// The picture after `current` in AC-3's order among `names` (sorted that way), wrapping. A
/// `current` no longer there still has its place by name.
fn after(names: &[String], current: &str) -> Option<String> {
    names.iter().find(|n| name_order(n, current).is_gt()).or(names.first()).cloned()
}

/// The picture to show after `current`, or None when there is no other (AC-7).
fn upcoming(names: &[String], current: Option<&str>, shuffle: bool, bag: &mut Vec<String>, rng: &mut u64) -> Option<String> {
    let current = current?;
    if !names.iter().any(|n| n != current) {
        return None;
    }
    if !shuffle {
        return after(names, current);
    }
    match draw(bag, names, Some(current), rng)? {
        n if n == current => draw(bag, names, Some(current), rng),
        n => Some(n),
    }
}

/// Shuffle's next picture (AC-3): the rest of this round, skipping pictures gone since; an
/// empty round refills with a new random order whose first isn't `current`.
fn draw(bag: &mut Vec<String>, names: &[String], current: Option<&str>, rng: &mut u64) -> Option<String> {
    bag.retain(|n| names.contains(n));
    if bag.is_empty() {
        *bag = names.to_vec();
        for i in (1..bag.len()).rev() {
            bag.swap(i, (xorshift(rng) % (i as u64 + 1)) as usize);
        }
        if bag.len() >= 2 && current.is_some_and(|c| bag[0] == c) {
            let j = 1 + (xorshift(rng) % (bag.len() as u64 - 1)) as usize;
            bag.swap(0, j);
        }
    }
    (!bag.is_empty()).then(|| bag.remove(0))
}

/// A small random number generator: plenty for an order of pictures, and no crate.
fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
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

    /// A wallpapers folder of its own for one test, filled with empty files (listing reads
    /// names only), and removed again when dropped.
    struct Folder(PathBuf);

    impl Folder {
        fn new(test: &str, files: &[&str]) -> Folder {
            let root = std::env::temp_dir().join(format!("des-k-{test}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            for f in files {
                let path = root.join(f);
                std::fs::create_dir_all(path.parent().expect("inside root")).expect("temp folder");
                if !f.ends_with('\\') {
                    std::fs::write(&path, b"x").expect("temp file");
                }
            }
            std::fs::create_dir_all(&root).expect("temp folder");
            Folder(root)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(list: &[(String, Stamp)]) -> Vec<&str> {
        list.iter().map(|(f, _)| f.as_str()).collect()
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn want(id: &str, shown: usize, files: Vec<Option<Source>>) -> Want {
        let n = files.len();
        Want { rect: RECT::default(), id: id.into(), shown, files, intervals: vec![1; n] }
    }

    // Spec 0028 AC-1, AC-3, AC-8: a folder plays its top level pictures by name with case
    // ignored, never a text file or a picture in a folder inside it; a set plays the names
    // that are files there now. A missing folder and an empty one get their own notes.
    #[test]
    fn a_slideshow_lists_only_its_own_pictures_by_name() {
        let dir = Folder::new("list", &["beach\\b.jpg", "beach\\A.png", "beach\\c.JPG", "beach\\notes.txt", "beach\\deep\\d.jpg", "empty\\read me.txt", "x.jpg", "y.png"]);
        let beach = list(&dir.0, &Source::Folder("beach".into())).expect("pictures");
        assert_eq!(names(&beach), ["beach\\A.png", "beach\\b.jpg", "beach\\c.JPG"]);
        assert_eq!(list(&dir.0, &Source::Folder("trips".into())).err(), Some(Problem::Missing));
        assert_eq!(list(&dir.0, &Source::Folder("empty".into())).err(), Some(Problem::Empty));
        let set = list(&dir.0, &Source::Set(strings(&["gone.jpg", "x.jpg", "y.png"]))).expect("two there");
        assert_eq!(names(&set), ["x.jpg", "y.png"]);
        assert_eq!(list(&dir.0, &Source::Set(strings(&["gone.jpg", "lost.png"]))).err(), Some(Problem::Missing));
        // A picture named like the folder but outside it is not one of its pictures.
        assert!(Source::Folder("beach".into()).holds("beach\\b.jpg") && !Source::Folder("bea".into()).holds("beach\\b.jpg"));
    }

    // AC-3: name order walks on and wraps; a picture removed while it showed keeps its place,
    // so the next one is the one after it by name. One picture has nothing to change to.
    #[test]
    fn name_order_wraps_and_keeps_a_removed_pictures_place() {
        let list = strings(&["a.jpg", "C.jpg", "d.jpg"]);
        let (mut bag, mut rng) = (Vec::new(), 1u64);
        assert_eq!(upcoming(&list, Some("a.jpg"), false, &mut bag, &mut rng).as_deref(), Some("C.jpg"));
        assert_eq!(upcoming(&list, Some("d.jpg"), false, &mut bag, &mut rng).as_deref(), Some("a.jpg"));
        assert_eq!(upcoming(&list, Some("b.jpg"), false, &mut bag, &mut rng).as_deref(), Some("C.jpg"), "b.jpg was removed");
        assert_eq!(upcoming(&strings(&["a.jpg"]), Some("a.jpg"), false, &mut bag, &mut rng), None);
    }

    // AC-3: shuffle shows every picture once per round, and a new round never starts with the
    // picture just shown, so nothing shows twice in a row.
    #[test]
    fn shuffle_shows_each_picture_once_a_round_and_never_twice_in_a_row() {
        let list = strings(&["a.jpg", "b.jpg", "c.jpg", "d.jpg", "e.jpg"]);
        for seed in [1u64, 7, 12345, 0xDEAD_BEEF] {
            let (mut bag, mut rng) = (Vec::new(), seed);
            let mut shown = vec![draw(&mut bag, &list, None, &mut rng).expect("a first picture")];
            for _ in 0..(5 * 20 - 1) {
                let next = upcoming(&list, shown.last().map(String::as_str), true, &mut bag, &mut rng).expect("five pictures");
                assert_ne!(Some(&next), shown.last(), "seed {seed}");
                shown.push(next);
            }
            for round in shown.chunks(5) {
                let mut round = round.to_vec();
                round.sort();
                assert_eq!(round, list, "seed {seed}");
            }
        }
    }

    // AC-5, AC-9: each screen keeps its own slideshow; the same value keeps the picture and its
    // count; another folder starts over; a set edit keeps the picture showing now while it is
    // still ticked, else starts over; a workspace whose value is gone drops its slideshow.
    #[test]
    fn a_new_wish_keeps_or_restarts_slideshows_as_promised() {
        let dir = Folder::new("wish", &["beach\\a.jpg", "beach\\b.jpg", "trips\\t.jpg", "x.jpg", "y.jpg", "z.jpg"]);
        let mut b = Backdrop::new(dir.0.clone());
        let beach = || Some(Source::Folder("beach".into()));
        b.want = vec![want("T-wish-1", 0, vec![None, beach()]), want("T-wish-2", 1, vec![None, beach()])];
        b.sync_shows();
        let key = |id: &str, i| (id.to_string(), i);
        assert_eq!(b.shows[&key("T-wish-1", 1)].current.as_deref(), Some("beach\\a.jpg"));
        assert_eq!(b.shows[&key("T-wish-1", 1)].next.as_deref(), Some("beach\\b.jpg"));
        assert_eq!(current("T-wish-2", 1).as_deref(), Some("beach\\a.jpg"), "the overview reads it");
        b.shows.get_mut(&key("T-wish-2", 1)).expect("screen 2").seconds = 40;
        b.sync_shows();
        assert_eq!(b.shows[&key("T-wish-2", 1)].seconds, 40, "the same value keeps its count");
        assert_eq!(b.shows[&key("T-wish-1", 1)].seconds, 0, "screens count on their own");

        b.want[1].files[1] = Some(Source::Folder("trips".into()));
        b.sync_shows();
        assert_eq!((b.shows[&key("T-wish-2", 1)].current.as_deref(), b.shows[&key("T-wish-2", 1)].seconds), (Some("trips\\t.jpg"), 0));
        assert_eq!(b.shows[&key("T-wish-2", 1)].next, None, "one picture never changes");

        b.want[0].files[1] = Some(Source::Set(strings(&["x.jpg", "y.jpg"])));
        b.sync_shows();
        let set = |b: &Backdrop| (b.shows[&key("T-wish-1", 1)].current.clone(), b.shows[&key("T-wish-1", 1)].seconds);
        assert_eq!(set(&b), (Some("x.jpg".into()), 0));
        b.shows.get_mut(&key("T-wish-1", 1)).expect("set").seconds = 30;
        b.want[0].files[1] = Some(Source::Set(strings(&["x.jpg", "y.jpg", "z.jpg"])));
        b.sync_shows();
        assert_eq!(set(&b), (Some("x.jpg".into()), 30), "ticking another keeps the picture and its count");
        b.want[0].files[1] = Some(Source::Set(strings(&["y.jpg", "z.jpg"])));
        b.sync_shows();
        assert_eq!(set(&b), (Some("y.jpg".into()), 0), "unticking the one showing starts over");

        b.want[0].files[1] = Some(Source::Picture("x.jpg".into()));
        b.sync_shows();
        assert!(!b.shows.contains_key(&key("T-wish-1", 1)) && current("T-wish-1", 1).is_none());
    }

    // AC-9: a screen missing from the wish keeps its slideshows through a monitor's display
    // sleep, and loses them after 30 checks in a row.
    #[test]
    fn a_missing_screen_keeps_its_slideshows_for_30_checks() {
        let dir = Folder::new("gone", &["beach\\a.jpg", "beach\\b.jpg"]);
        let mut b = Backdrop::new(dir.0.clone());
        b.want = vec![want("T-gone", 0, vec![Some(Source::Folder("beach".into()))])];
        b.sync_shows();
        b.want.clear();
        b.sync_shows();
        for _ in 0..GONE_CHECKS - 1 {
            b.forget_gone();
        }
        assert!(b.shows.contains_key(&("T-gone".to_string(), 0)));
        b.want = vec![want("T-gone", 0, vec![Some(Source::Folder("beach".into()))])];
        b.forget_gone();
        b.want.clear();
        for _ in 0..GONE_CHECKS - 1 {
            b.forget_gone();
        }
        assert!(b.shows.contains_key(&("T-gone".to_string(), 0)), "coming back resets the count");
        b.forget_gone();
        assert!(b.shows.is_empty() && current("T-gone", 0).is_none());
    }

    // AC-8: a picture that can't be read is skipped for the next one; when none can, the
    // slideshow shows your Windows wallpaper and its row gets the note.
    #[test]
    fn an_unreadable_picture_is_skipped_and_none_readable_is_a_note() {
        let dir = Folder::new("bad", &["bad\\a.jpg", "bad\\b.jpg", "bad\\c.jpg"]);
        let mut b = Backdrop::new(dir.0.clone());
        b.want = vec![want("T-bad", 0, vec![Some(Source::Folder("bad".into()))])];
        b.sync_shows();
        let key = ("T-bad".to_string(), 0);
        let fail = |b: &mut Backdrop, f: &str| {
            let stamp = std::fs::metadata(dir.0.join(f)).and_then(|m| m.modified()).ok();
            b.bad.insert(f.to_string(), stamp);
            b.slide_failed(f);
        };
        fail(&mut b, "bad\\a.jpg");
        assert_eq!((b.shows[&key].current.as_deref(), b.shows[&key].next.as_deref()), (Some("bad\\b.jpg"), Some("bad\\c.jpg")));
        fail(&mut b, "bad\\c.jpg");
        assert_eq!((b.shows[&key].current.as_deref(), b.shows[&key].next.as_deref()), (Some("bad\\b.jpg"), None), "the next is picked again");
        fail(&mut b, "bad\\b.jpg");
        assert_eq!(b.shows[&key].current, None);
        assert_eq!(crate::wallpaper::problem("bad"), Some(Problem::Unreadable));
        assert_eq!(b.skipped["bad"].len(), 3, "counted, for one log line per change");
    }

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
