// The settings page (spec 0012), opened from the tray's Settings. For now the tracer of build
// step 2: one toggle, [indicator] enabled, plus Open settings.toml, on plain Windows controls.
// It proves page → file → reload → live effect; the designed page (steps 3 to 6) replaces
// these controls. Lives on the main thread: its keys come through main's loop and
// IsDialogMessageW (Tab, Space, Enter, Esc). Sizes are in dip, scaled to the screen's dpi.
use std::cell::{Cell, RefCell};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Controls::{BST_CHECKED, BST_UNCHECKED},
        HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow},
        Input::KeyboardAndMouse::SetFocus,
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, w};

use crate::settings::{self, Edit};

const CLASS: PCWSTR = w!("pdesk-settings");
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0);
const KEY: &str = "indicator.enabled";

const ID_HEAD: i32 = 201;
const ID_DOTS: i32 = 202;
const ID_WHEN: i32 = 203;
const ID_NOTE: i32 = 204;
const ID_FILE: i32 = 205;

/// Each control: id, class, text, style, and its box in dip (x, y, w, h).
const CONTROLS: [(i32, PCWSTR, PCWSTR, u32, [f32; 4]); 5] = [
    (ID_HEAD, w!("STATIC"), w!("Indicator"), 0, [16.0, 14.0, 368.0, 20.0]),
    (ID_DOTS, w!("BUTTON"), w!("Show the workspace dots when you switch"), WS_TABSTOP.0 | BS_CHECKBOX as u32, [16.0, 38.0, 368.0, 24.0]),
    (ID_WHEN, w!("STATIC"), w!("Applies now."), 0, [36.0, 62.0, 348.0, 20.0]),
    (ID_NOTE, w!("STATIC"), w!(""), 0, [16.0, 90.0, 368.0, 40.0]),
    (ID_FILE, w!("BUTTON"), w!("Open settings.toml"), WS_TABSTOP.0 | BS_PUSHBUTTON as u32, [16.0, 136.0, 168.0, 30.0]),
];
const CLIENT: (f32, f32) = (400.0, 182.0);

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static FONT: Cell<Option<HFONT>> = const { Cell::new(None) };
    // The last save that failed, shown until the next good reload.
    static ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn item(id: i32) -> Option<HWND> {
    unsafe { GetDlgItem(WND.get(), id).ok() }
}

fn set_text(id: i32, text: &str) {
    if let Some(c) = item(id) {
        unsafe { let _ = SetWindowTextW(c, &HSTRING::from(text)); }
    }
}

/// Opens the page on the screen under the mouse, or brings the open one to the front (AC-1).
pub fn open() {
    if let Some(h) = WND.get() {
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: inst.into(),
            lpszClassName: CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
            ..Default::default()
        };
        RegisterClassW(&wc); // fails harmlessly when the page opens a second time
        let (mon, _) = crate::ws::under_mouse();
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work = mi.rcWork;
        // Made on the screen it opens on, so it starts at that screen's dpi.
        let h = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("pdesk settings"), STYLE, work.left, work.top, 100, 100, None, None, Some(inst.into()), None) {
            Ok(h) => h,
            Err(e) => return eprintln!("settings page: no window: {e}"),
        };
        WND.set(Some(h));
        for (id, class, text, style, _) in CONTROLS {
            let menu = HMENU(id as isize as *mut core::ffi::c_void);
            let style = WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style);
            if let Err(e) = CreateWindowExW(WINDOW_EX_STYLE(0), class, text, style, 0, 0, 0, 0, Some(h), Some(menu), Some(inst.into()), None) {
                eprintln!("settings page: control {id} not made: {e}");
            }
        }
        layout(h, None);
        refresh();
        let mut r = RECT::default();
        let _ = GetWindowRect(h, &mut r);
        let (ww, wh) = (r.right - r.left, r.bottom - r.top);
        let (x, y) = ((work.left + work.right - ww) / 2, ((work.top + work.bottom - wh) / 2).max(work.top));
        let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = ShowWindow(h, SW_SHOW);
        let _ = SetForegroundWindow(h);
        if let Some(dots) = item(ID_DOTS) {
            let _ = SetFocus(Some(dots));
        }
    }
}

/// Sizes the window and its controls for its dpi; `to` is where WM_DPICHANGED puts it.
fn layout(h: HWND, to: Option<&RECT>) {
    unsafe {
        let dpi = GetDpiForWindow(h);
        let s = dpi as f32 / 96.0;
        let px = |v: f32| (v * s).round() as i32;
        // 9 pt Segoe UI, the size of Windows' own dialogs.
        let font = CreateFontW(-px(12.0), 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS, CLEARTYPE_QUALITY, (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32, w!("Segoe UI"));
        for (id, _, _, _, [x, y, cw, ch]) in CONTROLS {
            if let Some(c) = item(id) {
                let _ = SetWindowPos(c, None, px(x), px(y), px(cw), px(ch), SWP_NOZORDER | SWP_NOACTIVATE);
                SendMessageW(c, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            }
        }
        if let Some(old) = FONT.replace(Some(font)) {
            let _ = DeleteObject(old.into());
        }
        let mut r = RECT { left: 0, top: 0, right: px(CLIENT.0), bottom: px(CLIENT.1) };
        let _ = AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi);
        let (x, y) = to.map_or((0, 0), |t| (t.left, t.top));
        let keep = if to.is_some() { SWP_NOZORDER } else { SWP_NOMOVE | SWP_NOZORDER };
        let _ = SetWindowPos(h, None, x, y, r.right - r.left, r.bottom - r.top, keep | SWP_NOACTIVATE);
    }
}

/// Shows what pdesk read back from the file: the page never keeps its own copy of a value.
/// Called after every reload.
pub fn refresh() {
    let Some(dots) = item(ID_DOTS) else { return };
    let on = settings::get().indicator_enabled;
    unsafe { SendMessageW(dots, BM_SETCHECK, Some(WPARAM(if on { BST_CHECKED.0 } else { BST_UNCHECKED.0 } as usize)), None) };
    let problem = settings::problems().iter().find(|p| p.key == KEY).map(|p| p.message.clone());
    let note = ERROR.take().map(|e| format!("Not saved: {e}")).or(problem).unwrap_or_default();
    set_text(ID_NOTE, &note);
}

fn toggle() {
    let on = !settings::get().indicator_enabled;
    if let Err(why) = settings::write(Edit::Value(KEY, on.into())) {
        ERROR.set(Some(why));
        refresh();
    }
}

/// Main loop: the page's keys (Tab, Space, Enter, Esc) through the dialog manager.
pub fn key(msg: &MSG) -> bool {
    WND.get().is_some_and(|h| unsafe { IsDialogMessageW(h, msg) }.as_bool())
}

unsafe extern "system" fn wndproc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match m {
            WM_COMMAND if (wp.0 >> 16) & 0xFFFF == BN_CLICKED as usize => {
                match (wp.0 & 0xFFFF) as i32 {
                    ID_DOTS => toggle(),
                    ID_FILE => crate::tray::open(&settings::path()),
                    id if id == IDCANCEL.0 => { let _ = DestroyWindow(h); } // Esc
                    _ => {}
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                WND.set(None);
                ERROR.set(None);
                if let Some(f) = FONT.take() {
                    let _ = DeleteObject(f.into());
                }
                LRESULT(0)
            }
            WM_DPICHANGED => {
                layout(h, Some(&*(lp.0 as *const RECT)));
                LRESULT(0)
            }
            _ => DefWindowProcW(h, m, wp, lp),
        }
    }
}
