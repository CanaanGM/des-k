// Cloak/uncloak any top-level window through the shell's undocumented IApplicationView.
// Layout from Ciantic's AltTabAccessor (MIT), verified on 25H2 by spikes/cloak.
use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use windows::{
    Win32::{
        Foundation::HWND,
        System::Com::{CLSCTX_ALL, CoCreateInstance, IServiceProvider},
        UI::{
            Shell::{ITaskbarList, TaskbarList},
            WindowsAndMessaging::IsWindow,
        },
    },
    core::{Error, GUID, HRESULT, Interface, Result},
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

/// Whether a Windows update most likely moved the undocumented layout: several refusals and
/// not one SetCloak since start. One odd window that refuses while others work is not that.
pub fn broken() -> bool {
    CLOAKS.load(Ordering::Relaxed) == 0 && MISMATCHES.load(Ordering::Relaxed) >= 3
}

const CLSID_IMMERSIVE_SHELL: GUID = GUID::from_u128(0xC2F03A33_21F5_47FA_B4BB_156362A2F239);
const IID_VIEW_COLLECTION: GUID = GUID::from_u128(0x1841C6D7_4F9D_42C0_AF41_8747538F10E5);

// vtable slots (IUnknown = 0..2)
const RELEASE: usize = 2;
const COLL_GET_VIEW_FOR_HWND: usize = 6;
const VIEW_GET_THUMBNAIL_WINDOW: usize = 9;
const VIEW_SET_CLOAK: usize = 12;

unsafe fn slot(obj: *mut c_void, i: usize) -> *const c_void {
    unsafe { *(*(obj as *const *const *const c_void)).add(i) }
}

unsafe fn release(obj: *mut c_void) {
    let f: extern "system" fn(*mut c_void) -> u32 = unsafe { std::mem::transmute(slot(obj, RELEASE)) };
    f(obj);
}

/// One per operation: explorer can restart, so don't keep it around.
pub struct Shell(*mut c_void, ITaskbarList);

impl Shell {
    pub fn new() -> Result<Self> {
        let sp: IServiceProvider = unsafe { CoCreateInstance(&CLSID_IMMERSIVE_SHELL, None, CLSCTX_ALL)? };
        let mut coll = std::ptr::null_mut();
        unsafe { (sp.vtable().QueryService)(sp.as_raw(), &IID_VIEW_COLLECTION, &IID_VIEW_COLLECTION, &mut coll).ok()? };
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
        let mut view = std::ptr::null_mut();
        unsafe {
            let get_view: extern "system" fn(*mut c_void, HWND, *mut *mut c_void) -> HRESULT =
                std::mem::transmute(slot(self.0, COLL_GET_VIEW_FOR_HWND));
            get_view(self.0, hwnd, &mut view).ok()?;

            // Refuse to call SetCloak if a Windows update shifted the vtable.
            let mut thumb = HWND::default();
            let get_thumb: extern "system" fn(*mut c_void, *mut HWND) -> HRESULT =
                std::mem::transmute(slot(view, VIEW_GET_THUMBNAIL_WINDOW));
            let hr = get_thumb(view, &mut thumb);
            let result = if hr.is_err() || thumb != hwnd {
                MISMATCHES.fetch_add(1, Ordering::Relaxed);
                Err(Error::new(HRESULT(0x8000FFFFu32 as i32), "IApplicationView layout mismatch"))
            } else {
                CLOAKS.fetch_add(1, Ordering::Relaxed);
                let set: extern "system" fn(*mut c_void, u32, i32) -> HRESULT =
                    std::mem::transmute(slot(view, VIEW_SET_CLOAK));
                set(view, 1, if cloak { 2 } else { 0 }).ok()
            };
            release(view);
            result
        }
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        unsafe { release(self.0) };
    }
}
