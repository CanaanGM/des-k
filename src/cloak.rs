// Cloak/uncloak any top-level window through the shell's undocumented IApplicationView.
// Layout from Ciantic's AltTabAccessor (MIT), verified on 25H2 by spikes/cloak. No slot is
// called before Windows confirms the interface's id; on a layout it does not, hiding is off
// for the run (spec 0030).
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU8, AtomicU32, Ordering};
use windows::{
    Win32::{
        Foundation::{E_NOINTERFACE, E_NOTIMPL, HWND},
        System::{
            Com::{CLSCTX_ALL, CoCreateInstance, IServiceProvider},
            SystemInformation::GetTickCount,
        },
        UI::{
            Shell::{ITaskbarList, TaskbarList},
            WindowsAndMessaging::IsWindow,
        },
    },
    core::{Error, GUID, HRESULT, IUnknown, Interface, Result},
};

// Every window des-k cloaked and has not shown again, kept apart from ws's state and never
// held across a call: the panic hook reads it when a panic left ws's lock held.
static OURS: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Record a window cloaked by an earlier des-k (a restart adopts it).
pub fn adopt(hwnd: HWND) {
    mark(hwnd, true);
}

/// The windows des-k cloaked, for the panic hook only.
pub fn ours() -> Vec<isize> {
    OURS.try_lock().map(|v| v.clone()).unwrap_or_default()
}

fn mark(hwnd: HWND, cloaked: bool) {
    let mut v = OURS.lock().unwrap_or_else(|e| e.into_inner());
    // Closed windows drop out here, so a reused handle is never shown by mistake.
    v.retain(|&h| h != hwnd.0 as isize && unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool());
    if cloaked {
        v.push(hwnd.0 as isize);
    }
}

// Calls that passed the layout check, and refusals for a mismatch, since des-k started.
static CLOAKS: AtomicU32 = AtomicU32::new(0);
static MISMATCHES: AtomicU32 = AtomicU32::new(0);

/// Why hiding is off, if it is: the tray picks its warning by it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cause {
    /// Windows answered no to an interface id: a layout des-k does not know (spec 0030 AC-7).
    Unknown(Check, HRESULT),
    /// The id matched, but GetThumbnailWindow refused three times and SetCloak never ran (AC-8).
    Refused,
}

/// Which id Windows refused.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Check {
    Collection,
    View,
}

/// Why hiding is off, or None while it works: an id Windows refused, or several refusals and
/// not one SetCloak since start (one odd window is not that). Lock free, read by the tray.
pub fn cause() -> Option<Cause> {
    match LAYOUT.get() {
        UNKNOWN_COLLECTION => Some(Cause::Unknown(Check::Collection, LAYOUT.why())),
        UNKNOWN_VIEW => Some(Cause::Unknown(Check::View, LAYOUT.why())),
        _ if CLOAKS.load(Ordering::Relaxed) == 0 && MISMATCHES.load(Ordering::Relaxed) >= 3 => Some(Cause::Refused),
        _ => None,
    }
}

/// Whether Windows refused an interface id this run: then no slot is called and hiding is off
/// until des-k restarts (spec 0030 AC-4). Every path that hides or shows checks it first.
pub fn unknown() -> bool {
    matches!(LAYOUT.get(), UNKNOWN_COLLECTION | UNKNOWN_VIEW)
}

/// At start, before `ws::load` (AC-3): checks the view id on the first of `windows` the shell
/// has a view for. Never waits: with explorer not up, or no view yet, the first hide decides.
pub fn check_at_start(windows: &[HWND]) {
    let shell = match Shell::new() {
        Ok(shell) => shell,
        Err(e) => return info!("cloak: shell not ready ({e}), the first hide checks the layout"),
    };
    if unknown() || LAYOUT.get() == KNOWN {
        return;
    }
    if !windows.iter().any(|&h| shell.view(h).is_ok() || unknown()) {
        info!("cloak: no window to check the layout on yet, the first hide checks it");
    }
}

/// A debug build's `--pretend-unknown-windows` (AC-14): this run treats the layout as unknown.
#[cfg(debug_assertions)]
pub fn pretend_unknown() {
    became(UNKNOWN_VIEW, E_NOINTERFACE);
}

/// What an answer to an id means (AC-11, AC-12): only a refused id is a layout des-k does not
/// know. `E_NOTIMPL` is QueryService's answer for a service id it has never heard of (spec 0030
/// build step 1); anything else, such as explorer still starting, is worth another try.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Answer {
    Known,
    Unknown,
    TryAgain,
}

fn classify(hr: HRESULT) -> Answer {
    match hr {
        _ if hr.is_ok() => Answer::Known,
        E_NOINTERFACE | E_NOTIMPL => Answer::Unknown,
        _ => Answer::TryAgain,
    }
}

const UNCHECKED: u8 = 0;
const KNOWN: u8 = 1;
const UNKNOWN_COLLECTION: u8 = 2;
const UNKNOWN_VIEW: u8 = 3;

/// The layout as this run found it, in atomics so nothing on the panic path takes a lock.
/// It leaves `UNCHECKED` once and never moves again (AC-4).
struct Layout {
    now: AtomicU8,
    /// The answer that made it unknown, stored before `now`.
    why: AtomicI32,
    /// Tick count of the collection's first refusal, 0 for none (AC-2).
    first_refusal: AtomicU32,
}

impl Layout {
    const fn new() -> Self {
        Self { now: AtomicU8::new(UNCHECKED), why: AtomicI32::new(0), first_refusal: AtomicU32::new(0) }
    }

    fn get(&self) -> u8 {
        self.now.load(Ordering::Acquire)
    }

    fn why(&self) -> HRESULT {
        HRESULT(self.why.load(Ordering::Acquire))
    }

    /// Moves to `to` if nothing was decided yet; whether it moved.
    fn settle(&self, to: u8, hr: HRESULT) -> bool {
        if self.get() != UNCHECKED {
            return false;
        }
        if to != KNOWN {
            self.why.store(hr.0, Ordering::Release);
        }
        self.now.compare_exchange(UNCHECKED, to, Ordering::AcqRel, Ordering::Acquire).is_ok()
    }

    /// The collection's id was refused at tick `now`: unknown only when it was refused before,
    /// 1 s or more ago, so an explorer still starting is never mistaken for a new Windows.
    fn collection_refused(&self, hr: HRESULT, now: u32) -> bool {
        let first = self.first_refusal.load(Ordering::Relaxed);
        if first == 0 {
            self.first_refusal.store(now.max(1), Ordering::Relaxed);
            return false;
        }
        now.wrapping_sub(first) >= 1000 && self.settle(UNKNOWN_COLLECTION, hr)
    }

    fn collection_known(&self) {
        self.first_refusal.store(0, Ordering::Relaxed);
    }
}

static LAYOUT: Layout = Layout::new();

/// Settles the run's layout and says so once (AC-9).
fn became(to: u8, hr: HRESULT) -> bool {
    let moved = LAYOUT.settle(to, hr);
    if moved {
        log_layout();
    }
    moved
}

fn log_layout() {
    let build = crate::log::windows_build();
    match cause() {
        Some(Cause::Unknown(check, hr)) => {
            let what = if check == Check::View { "view" } else { "view collection" };
            error!("cloak: {build} refused the {what} id ({:#010X}): des-k does not know this Windows, switching hides nothing until des-k is updated", hr.0 as u32);
        }
        _ => info!("cloak: {build} confirmed the view id, hiding is on"),
    }
}

/// `hide`'s answer on an unknown layout: no slot was called.
fn unknown_layout() -> Error {
    Error::new(E_NOINTERFACE, "unknown Windows layout")
}

const CLSID_IMMERSIVE_SHELL: GUID = GUID::from_u128(0xC2F03A33_21F5_47FA_B4BB_156362A2F239);
const IID_VIEW_COLLECTION: GUID = GUID::from_u128(0x1841C6D7_4F9D_42C0_AF41_8747538F10E5);
const IID_APPLICATION_VIEW: GUID = GUID::from_u128(0x372E1D3B_38D3_42E4_A15B_8AB2B178F513);

// vtable slots (IUnknown = 0..2)
const COLL_GET_VIEW_FOR_HWND: usize = 6;
const VIEW_GET_THUMBNAIL_WINDOW: usize = 9;
const VIEW_SET_CLOAK: usize = 12;

unsafe fn slot(obj: *mut c_void, i: usize) -> *const c_void {
    unsafe { *(*(obj as *const *const *const c_void)).add(i) }
}

/// One per operation: explorer can restart, so don't keep it around. The view collection is
/// None on an unknown layout: the Shell still builds, so a switch still switches (AC-5).
pub struct Shell(Option<IUnknown>, ITaskbarList);

impl Shell {
    pub fn new() -> Result<Self> {
        let sp: IServiceProvider = unsafe { CoCreateInstance(&CLSID_IMMERSIVE_SHELL, None, CLSCTX_ALL)? };
        let coll = if unknown() { None } else { collection(&sp)? };
        let taskbar: ITaskbarList = unsafe { CoCreateInstance(&TaskbarList, None, CLSCTX_ALL)? };
        unsafe { taskbar.HrInit()? };
        Ok(Self(coll, taskbar))
    }

    /// Cloaking leaves the taskbar button behind, so remove/re-add it too. An error from the
    /// taskbar step means the cloak itself DID change: callers must undo it or track the window.
    pub fn hide(&self, hwnd: HWND, hide: bool) -> Result<()> {
        self.set_cloak(hwnd, hide)?;
        mark(hwnd, hide);
        unsafe {
            if hide { self.1.DeleteTab(hwnd) } else { self.1.AddTab(hwnd) }
        }
    }

    fn set_cloak(&self, hwnd: HWND, cloak: bool) -> Result<()> {
        let view = self.view(hwnd)?;
        let view = view.as_raw();
        unsafe {
            // Refuse to call SetCloak if a Windows update shifted the vtable (AC-13).
            let mut thumb = HWND::default();
            let get_thumb: extern "system" fn(*mut c_void, *mut HWND) -> HRESULT =
                std::mem::transmute(slot(view, VIEW_GET_THUMBNAIL_WINDOW));
            let hr = get_thumb(view, &mut thumb);
            if hr.is_err() || thumb != hwnd {
                MISMATCHES.fetch_add(1, Ordering::Relaxed);
                return Err(Error::new(HRESULT(0x8000FFFFu32 as i32), "IApplicationView layout mismatch"));
            }
            CLOAKS.fetch_add(1, Ordering::Relaxed);
            let set: extern "system" fn(*mut c_void, u32, i32) -> HRESULT = std::mem::transmute(slot(view, VIEW_SET_CLOAK));
            set(view, 1, if cloak { 2 } else { 0 }).ok()
        }
    }

    /// `hwnd`'s view, as the pointer Windows returned for the IApplicationView id (AC-1). Before
    /// any view matched, a refusal means a layout des-k does not know; after, one odd window,
    /// counted like a GetThumbnailWindow refusal.
    fn view(&self, hwnd: HWND) -> Result<IUnknown> {
        let Some(coll) = self.0.as_ref().filter(|_| !unknown()) else { return Err(unknown_layout()) };
        let mut raw = std::ptr::null_mut();
        let view = unsafe {
            let get_view: extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT =
                std::mem::transmute(slot(coll.as_raw(), COLL_GET_VIEW_FOR_HWND));
            get_view(coll.as_raw(), hwnd, &mut raw).ok()?;
            IUnknown::from_raw(raw) // released when dropped
        };
        let mut app = std::ptr::null_mut();
        let hr = unsafe { view.query(&IID_APPLICATION_VIEW, &mut app) };
        match classify(hr) {
            Answer::Known if !app.is_null() => {
                became(KNOWN, hr);
                Ok(unsafe { IUnknown::from_raw(app) })
            }
            Answer::Unknown if became(UNKNOWN_VIEW, hr) || unknown() => Err(unknown_layout()),
            Answer::Unknown => {
                MISMATCHES.fetch_add(1, Ordering::Relaxed);
                Err(Error::new(hr, "this window's view refused the IApplicationView id"))
            }
            _ => Err(hr.into()),
        }
    }
}

/// The view collection, None once its id is refused for good (AC-2). A first refusal fails like
/// explorer not being up, so callers try again as they always do.
fn collection(sp: &IServiceProvider) -> Result<Option<IUnknown>> {
    let mut raw = std::ptr::null_mut();
    let hr = unsafe { (sp.vtable().QueryService)(sp.as_raw(), &IID_VIEW_COLLECTION, &IID_VIEW_COLLECTION, &mut raw) };
    match classify(hr) {
        Answer::Known if !raw.is_null() => {
            LAYOUT.collection_known();
            Ok(Some(unsafe { IUnknown::from_raw(raw) }))
        }
        Answer::Unknown if LAYOUT.collection_refused(hr, unsafe { GetTickCount() }) => {
            log_layout();
            Ok(None)
        }
        _ => Err(Error::from(hr)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Codes Windows gave in spec 0030's spike (25H2, 2026-11-24).
    const NO_VIEW_YET: HRESULT = HRESULT(0x8002802Bu32 as i32);
    const CLASS_NOT_REGISTERED: HRESULT = HRESULT(0x80040154u32 as i32); // explorer down

    #[test]
    fn only_a_refused_id_means_a_layout_des_k_does_not_know() {
        assert_eq!(classify(HRESULT(0)), Answer::Known);
        assert_eq!(classify(E_NOINTERFACE), Answer::Unknown, "a known service asked for an id it lacks");
        assert_eq!(classify(E_NOTIMPL), Answer::Unknown, "QueryService for a service id it never heard of");
        // A passing failure must never turn hiding off for the whole run (AC-11).
        for hr in [NO_VIEW_YET, CLASS_NOT_REGISTERED, HRESULT(0x80004005u32 as i32), HRESULT(0x800706BAu32 as i32)] {
            assert_eq!(classify(hr), Answer::TryAgain, "{:#010X}", hr.0 as u32);
        }
    }

    #[test]
    fn the_layout_is_decided_once() {
        // Known stays known: a later refusal is one odd window, never a new Windows (AC-4).
        let l = Layout::new();
        assert!(l.settle(KNOWN, HRESULT(0)));
        assert!(!l.settle(UNKNOWN_VIEW, E_NOINTERFACE));
        assert!(!l.collection_refused(E_NOTIMPL, 1000) && !l.collection_refused(E_NOTIMPL, 5000));
        assert_eq!(l.get(), KNOWN);
        // Unknown stays unknown, with the answer that made it so.
        let l = Layout::new();
        assert!(l.settle(UNKNOWN_VIEW, E_NOINTERFACE));
        assert!(!l.settle(KNOWN, HRESULT(0)));
        assert!(!l.settle(UNKNOWN_COLLECTION, E_NOTIMPL));
        assert_eq!((l.get(), l.why()), (UNKNOWN_VIEW, E_NOINTERFACE));
    }

    #[test]
    fn the_collection_must_be_refused_twice_a_second_apart() {
        // Explorer still starting may refuse once; a slow one must not look like a new Windows (AC-2).
        let l = Layout::new();
        assert!(!l.collection_refused(E_NOTIMPL, 10_000));
        assert!(!l.collection_refused(E_NOTIMPL, 10_999));
        assert_eq!(l.get(), UNCHECKED);
        assert!(l.collection_refused(E_NOTIMPL, 11_000));
        assert_eq!((l.get(), l.why()), (UNKNOWN_COLLECTION, E_NOTIMPL));
        // A success in between starts the count again.
        let l = Layout::new();
        assert!(!l.collection_refused(E_NOTIMPL, 10_000));
        l.collection_known();
        assert!(!l.collection_refused(E_NOTIMPL, 12_000));
        assert_eq!(l.get(), UNCHECKED);
        // The tick count wraps every 49.7 days.
        let l = Layout::new();
        assert!(!l.collection_refused(E_NOTIMPL, u32::MAX - 500));
        assert!(l.collection_refused(E_NOTIMPL, 600));
    }

    #[test]
    fn interface_ids_change_only_on_purpose() {
        // A wrong id turns hiding off on every Windows, and leaves windows an earlier des-k
        // hid stuck until you sign out: each is written twice, so a slip in one fails here.
        let view = GUID::from_values(0x372E1D3B, 0x38D3, 0x42E4, [0xA1, 0x5B, 0x8A, 0xB2, 0xB1, 0x78, 0xF5, 0x13]);
        let coll = GUID::from_values(0x1841C6D7, 0x4F9D, 0x42C0, [0xAF, 0x41, 0x87, 0x47, 0x53, 0x8F, 0x10, 0xE5]);
        assert_eq!(IID_APPLICATION_VIEW, view);
        assert_eq!(IID_VIEW_COLLECTION, coll);
    }
}
