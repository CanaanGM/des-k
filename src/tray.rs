// Tray icon near the clock: shows des-k is running, with Settings, Open log and Quit.
// Lives on the main thread (its messages come through main's loop) and adds itself again
// whenever explorer restarts.
use std::cell::Cell;
use std::path::Path;
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    Graphics::Gdi::{CreateBitmap, DeleteObject},
    System::{
        Com::{CLSCTX_LOCAL_SERVER, CoCreateInstance, IDispatch, IServiceProvider},
        LibraryLoader::GetModuleHandleW,
        Variant::{VARIANT, VT_I4},
    },
    UI::{
        Shell::{
            CSIDL_DESKTOP, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellWindows, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
            NIM_MODIFY, NOTIFYICONDATAW, SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellExecuteW, ShellWindows,
            Shell_NotifyIconW,
        },
        WindowsAndMessaging::*,
    },
};
use windows::core::{BOOL, BSTR, HSTRING, Interface, PCWSTR, w};

const CLASS: PCWSTR = w!("des-k-tray");
/// Window message (not a thread message): the shell's callback for clicks on the icon.
const WM_TRAY: u32 = WM_APP + 3;
const ID_SETTINGS: usize = 1;
const ID_LOG: usize = 2;
const ID_QUIT: usize = 3;

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static ICON: Cell<Option<HICON>> = const { Cell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TRAY if matches!(lp.0 as u32, WM_LBUTTONUP | WM_RBUTTONUP) => {
                menu(hwnd);
                LRESULT(0)
            }
            // Explorer restarted (or the taskbar changed scale): the old icon is gone.
            _ if msg == TASKBAR_CREATED.get() => {
                add(hwnd);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// A hidden top level window, since TaskbarCreated never reaches message only windows.
/// Tool window, so des-k never manages it.
pub fn init() {
    unsafe {
        let Ok(inst) = GetModuleHandleW(None) else { return };
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
        match CreateWindowExW(WS_EX_TOOLWINDOW, CLASS, w!("des-k"), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None) {
            Ok(h) => {
                // Explorer runs without admin; an admin des-k would never hear its clicks or
                // its restart otherwise (spec 0008). Only these two, nothing else.
                for m in [WM_TRAY, TASKBAR_CREATED.get()] {
                    if let Err(e) = ChangeWindowMessageFilterEx(h, m, MSGFLT_ALLOW, None) {
                        eprintln!("tray: message {m:#x} not let in from explorer: {e}");
                    }
                }
                WND.set(Some(h));
                add(h);
            }
            Err(e) => eprintln!("tray: window not created: {e}"),
        }
    }
}

/// On a clean exit only; after a hard kill the icon lingers until the mouse passes over it.
pub fn remove() {
    if let Some(h) = WND.get() {
        unsafe { let _ = Shell_NotifyIconW(NIM_DELETE, &data(h)); }
    }
}

fn data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

fn add(hwnd: HWND) {
    // Drawn again each time, so a taskbar scale change gets a sharp icon.
    if let Some(old) = ICON.take() {
        unsafe { let _ = DestroyIcon(old); }
    }
    ICON.set(draw_icon());
    let mut nid = data(hwnd);
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = ICON.get().unwrap_or_default();
    for (d, s) in nid.szTip.iter_mut().zip("des-k".encode_utf16()) {
        *d = s;
    }
    unsafe {
        // At login explorer may not be ready yet; its TaskbarCreated brings us back here.
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() && !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
            eprintln!("tray: icon not added");
        }
    }
}

/// Four rounded squares in one blend, purple at the bottom left to red at the top right.
/// Both colors are strong enough to read on light and dark taskbars.
fn draw_icon() -> Option<HICON> {
    const PURPLE: [u32; 3] = [0x8E, 0x3F, 0xD9];
    const RED: [u32; 3] = [0xE5, 0x39, 0x35];
    let s = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let span = (2 * (s - 1)) as u32;
    let blend = |x: i32, y: i32| {
        let t = (x + s - 1 - y) as u32; // 0 at the bottom left, span at the top right
        let c = |i: usize| (PURPLE[i] * (span - t) + RED[i] * t) / span;
        0xFF00_0000 | c(0) << 16 | c(1) << 8 | c(2) // 0xAARRGGBB
    };
    let gap = (s / 8).max(2);
    let q = (s - gap) / 2;
    let off = (s - 2 * q - gap) / 2;
    let mut px = vec![0u32; (s * s) as usize];
    for oy in [off, off + q + gap] {
        for ox in [off, off + q + gap] {
            for dy in 0..q {
                for dx in 0..q {
                    let (ex, ey) = (dx == 0 || dx == q - 1, dy == 0 || dy == q - 1);
                    if ex && ey {
                        continue; // corner pixel: rounds the square
                    }
                    px[((oy + dy) * s + ox + dx) as usize] = blend(ox + dx, oy + dy);
                }
            }
        }
    }
    // Ignored when the color bitmap has alpha, but CreateIconIndirect requires one.
    let mask = vec![0u8; ((s + 15) / 16 * 2 * s) as usize];
    unsafe {
        let color = CreateBitmap(s, s, 1, 32, Some(px.as_ptr().cast()));
        let hmask = CreateBitmap(s, s, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO { fIcon: BOOL(1), hbmMask: hmask, hbmColor: color, ..Default::default() };
        let icon = CreateIconIndirect(&info); // copies both bitmaps
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(hmask.into());
        icon.inspect_err(|e| eprintln!("tray: icon not drawn: {e}")).ok()
    }
}

fn menu(hwnd: HWND) {
    let log = crate::ws::data_dir().join("des-k.log");
    let flag = |ok: bool| if ok { MF_STRING } else { MF_GRAYED };
    unsafe {
        let Ok(m) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(m, MF_STRING, ID_SETTINGS, w!("Settings"));
        // Debug builds with a console log there instead of to the file.
        let _ = AppendMenuW(m, flag(log.exists()), ID_LOG, w!("Open log"));
        let _ = AppendMenuW(m, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(m, MF_STRING, ID_QUIT, w!("Quit (shows every window)"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Without this the menu stays open after a click elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(m, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(m);
        match cmd.0 as usize {
            ID_SETTINGS => crate::settings_page::open(None),
            ID_LOG => open(&log),
            // Main's loop breaks on WM_CLOSE, the same path as `des-k --quit`.
            ID_QUIT => { let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)); }
            _ => {}
        }
    }
}

/// Opened by explorer, so the editor runs as you even when des-k runs as admin (spec 0008).
/// Any failure falls back to opening it from des-k, with des-k's rights. Also takes a
/// settings page like `ms-settings:devices-touchpad`.
pub fn open(path: &Path) {
    if let Err(e) = unsafe { open_by_explorer(path) } {
        eprintln!("tray: explorer did not open {} ({e}), opening it from des-k", path.display());
        open_here(path);
    }
}

/// The desktop's shell view hands out its IShellDispatch2, whose ShellExecute runs inside
/// explorer. Documented COM, the route Raymond Chen describes for starting unelevated.
unsafe fn open_by_explorer(path: &Path) -> windows::core::Result<()> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let mut loc = VARIANT::default();
        (*loc.Anonymous.Anonymous).vt = VT_I4;
        (*loc.Anonymous.Anonymous).Anonymous.lVal = CSIDL_DESKTOP as i32;
        let empty = VARIANT::default();
        let mut hwnd = 0;
        let desktop = windows.FindWindowSW(&loc, &empty, SWC_DESKTOP, &mut hwnd, SWFO_NEEDDISPATCH)?;
        let browser: IShellBrowser = desktop.cast::<IServiceProvider>()?.QueryService(&SID_STopLevelBrowser)?;
        // The view only hands out IDispatch: asking for IShellFolderViewDual directly fails with E_NOINTERFACE.
        let view: IShellFolderViewDual = browser.QueryActiveShellView()?.GetItemObject::<IDispatch>(SVGIO_BACKGROUND)?.cast()?;
        let shell: IShellDispatch2 = view.Application()?.cast()?;
        shell.ShellExecute(&BSTR::from(path.to_string_lossy().as_ref()), &empty, &empty, &empty, &empty)
    }
}

/// The default app, else Notepad: .toml and .log often have no association.
fn open_here(path: &Path) {
    let file = HSTRING::from(path.as_os_str());
    unsafe {
        if ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL).0 as isize <= 32 {
            let quoted = HSTRING::from(format!("\"{}\"", path.display()));
            if ShellExecuteW(None, w!("open"), w!("notepad.exe"), &quoted, None, SW_SHOWNORMAL).0 as isize <= 32 {
                eprintln!("tray: cannot open {}", path.display());
            }
        }
    }
}
