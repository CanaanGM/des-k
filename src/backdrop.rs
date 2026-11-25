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
use crate::ui::canvas::Picture;
use crate::wallpaper::Problem;

pub(crate) mod paint;
mod slides;
mod window;

use paint::{DIM, wndproc};
use slides::{Show, ShowKey};
use window::{Dib, Screen};

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

fn run() {
    let mut msg = MSG::default();
    // Makes this thread's message queue, so a post from main lands from here on.
    unsafe { let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE); }
    let _ = THREAD.set(unsafe { GetCurrentThreadId() });
    // MTA, like main: the bar color is asked of explorer, and an STA would pump messages there.
    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    crate::ui::canvas::gdiplus();
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
