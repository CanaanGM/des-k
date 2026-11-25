// Every Windows call ws makes, each behind a safe function that passes it straight through and
// says why it is sound (spec 0035 AC-10). The rest of ws decides; only this file calls Windows.
use super::*;
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME,
    DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QUERY_DISPLAY_CONFIG_FLAGS, QueryDisplayConfig,
};
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, MONITOR_FROM_FLAGS, MonitorFromPoint, MonitorFromRect, MonitorFromWindow};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MONITOR_DPI_TYPE};

// Windows: user32 checks every HWND it is given, so a closed or made up handle only makes the
// call fail. That holds for each window call below, whatever handle a caller passes.

pub(super) fn is_window(h: Option<HWND>) -> BOOL {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { IsWindow(h) }
}

pub(super) fn is_iconic(h: HWND) -> BOOL {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { IsIconic(h) }
}

pub(super) fn is_zoomed(h: HWND) -> BOOL {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { IsZoomed(h) }
}

pub(super) fn is_window_visible(h: HWND) -> BOOL {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { IsWindowVisible(h) }
}

pub(super) fn is_hung_app_window(h: HWND) -> BOOL {
    // SAFETY: any handle is checked (above); it never waits on the window's thread.
    unsafe { IsHungAppWindow(h) }
}

pub(super) fn get_ancestor(h: HWND, flags: GET_ANCESTOR_FLAGS) -> HWND {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { GetAncestor(h, flags) }
}

pub(super) fn get_window(h: HWND, cmd: GET_WINDOW_CMD) -> windows::core::Result<HWND> {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { GetWindow(h, cmd) }
}

pub(super) fn get_shell_window() -> HWND {
    // SAFETY: takes nothing and only reads.
    unsafe { GetShellWindow() }
}

pub(super) fn get_window_long_ptr(h: HWND, index: WINDOW_LONG_PTR_INDEX) -> isize {
    // SAFETY: any handle and index is checked (above); nothing is written.
    unsafe { GetWindowLongPtrW(h, index) }
}

pub(super) fn get_window_thread_process_id(h: HWND, pid: Option<&mut u32>) -> u32 {
    // SAFETY: pid, when given, is a live u32 that Windows fills; any handle is checked (above).
    unsafe { GetWindowThreadProcessId(h, pid.map(|p| p as *mut u32)) }
}

pub(super) fn get_window_text(h: HWND, buf: &mut [u16]) -> i32 {
    // SAFETY: Windows is told the slice's length and writes at most that; any handle is checked (above).
    unsafe { GetWindowTextW(h, buf) }
}

pub(super) fn get_window_text_length(h: HWND) -> i32 {
    // SAFETY: any handle is checked (above); nothing is written.
    unsafe { GetWindowTextLengthW(h) }
}

pub(super) fn get_class_name(h: HWND, buf: &mut [u16]) -> i32 {
    // SAFETY: Windows is told the slice's length and writes at most that; any handle is checked (above).
    unsafe { GetClassNameW(h, buf) }
}

pub(super) fn get_window_rect(h: HWND, r: &mut RECT) -> windows::core::Result<()> {
    // SAFETY: r is a live RECT that Windows fills; any handle is checked (above).
    unsafe { GetWindowRect(h, r) }
}

pub(super) fn get_window_placement(h: HWND, wp: &mut WINDOWPLACEMENT) -> windows::core::Result<()> {
    // SAFETY: wp is a whole live WINDOWPLACEMENT, so Windows writes inside it; any handle is checked (above).
    unsafe { GetWindowPlacement(h, wp) }
}

pub(super) fn set_window_placement(h: HWND, wp: &WINDOWPLACEMENT) -> windows::core::Result<()> {
    // SAFETY: wp is a whole live WINDOWPLACEMENT that Windows only reads; any handle is checked (above).
    unsafe { SetWindowPlacement(h, wp) }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn set_window_pos(h: HWND, after: Option<HWND>, x: i32, y: i32, cx: i32, cy: i32, flags: SET_WINDOW_POS_FLAGS) -> windows::core::Result<()> {
    // SAFETY: plain values only; both handles are checked (above).
    unsafe { SetWindowPos(h, after, x, y, cx, cy, flags) }
}

pub(super) fn show_window(h: HWND, cmd: SHOW_WINDOW_CMD) -> BOOL {
    // SAFETY: plain values only; any handle is checked (above).
    unsafe { ShowWindow(h, cmd) }
}

pub(super) fn dwm_get_window_attribute(h: HWND, attr: DWMWINDOWATTRIBUTE, v: &mut u32) -> windows::core::Result<()> {
    // SAFETY: v is a live u32 and Windows is told its size, so it writes at most those 4 bytes.
    unsafe { DwmGetWindowAttribute(h, attr, v as *mut u32 as *mut _, size_of::<u32>() as u32) }
}

/// Every top level window, top of z order first.
pub(super) fn enum_windows() -> Vec<HWND> {
    unsafe extern "system" fn push(hwnd: HWND, out: LPARAM) -> BOOL {
        // SAFETY: out is the Vec below, which outlives the EnumWindows call that calls this.
        unsafe { (*(out.0 as *mut Vec<HWND>)).push(hwnd) };
        BOOL(1)
    }
    let mut all = Vec::new();
    // SAFETY: push has the WNDENUMPROC shape, and EnumWindows calls it only before it returns.
    unsafe { let _ = EnumWindows(Some(push), LPARAM(&mut all as *mut _ as isize)); }
    all
}

// Screens: user32 checks every HMONITOR it is given too, so a screen that just left only makes
// the call fail.

pub(super) fn monitor_from_window(h: HWND, flags: MONITOR_FROM_FLAGS) -> HMONITOR {
    // SAFETY: plain values only; any handle is checked (above).
    unsafe { MonitorFromWindow(h, flags) }
}

pub(super) fn monitor_from_point(pt: POINT, flags: MONITOR_FROM_FLAGS) -> HMONITOR {
    // SAFETY: plain values only.
    unsafe { MonitorFromPoint(pt, flags) }
}

pub(super) fn monitor_from_rect(r: &RECT, flags: MONITOR_FROM_FLAGS) -> HMONITOR {
    // SAFETY: r is a live RECT that Windows only reads.
    unsafe { MonitorFromRect(r, flags) }
}

pub(super) fn get_monitor_info(m: HMONITOR, mi: &mut MONITORINFO) -> BOOL {
    // cbSize tells Windows how much to write, and a larger form would overrun this MONITORINFO.
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    // SAFETY: mi is a live MONITORINFO and cbSize names exactly its size; any HMONITOR is checked (above).
    unsafe { GetMonitorInfoW(m, mi) }
}

pub(super) fn get_monitor_info_ex(m: HMONITOR, mi: &mut MONITORINFOEXW) -> BOOL {
    // SAFETY: mi is a live MONITORINFOEXW, the largest form, so either size cbSize names fits
    // inside it; any HMONITOR is checked (above).
    unsafe { GetMonitorInfoW(m, mi as *mut MONITORINFOEXW as *mut MONITORINFO) }
}

/// Every present screen with its full rectangle, in the order Windows lists them.
pub(super) fn enum_display_monitors() -> Vec<(HMONITOR, RECT)> {
    unsafe extern "system" fn push(m: HMONITOR, _: HDC, r: *mut RECT, out: LPARAM) -> BOOL {
        // SAFETY: out is the Vec below, which outlives the EnumDisplayMonitors call that calls
        // this, and r is Windows' own RECT for this screen, live for this call.
        unsafe { (*(out.0 as *mut Vec<(HMONITOR, RECT)>)).push((m, *r)) };
        BOOL(1)
    }
    let mut all = Vec::new();
    // SAFETY: push has the MONITORENUMPROC shape, and EnumDisplayMonitors calls it only before it returns.
    unsafe { let _ = EnumDisplayMonitors(None, None, Some(push), LPARAM(&mut all as *mut _ as isize)); }
    all
}

pub(super) fn get_cursor_pos(pt: &mut POINT) -> windows::core::Result<()> {
    // SAFETY: pt is a live POINT that Windows fills.
    unsafe { GetCursorPos(pt) }
}

pub(super) fn get_dpi_for_monitor(m: HMONITOR, kind: MONITOR_DPI_TYPE, dx: &mut u32, dy: &mut u32) -> windows::core::Result<()> {
    // SAFETY: dx and dy are live u32s that Windows fills; any HMONITOR is checked (above).
    unsafe { GetDpiForMonitor(m, kind, dx, dy) }
}

pub(super) fn get_display_config_buffer_sizes(flags: QUERY_DISPLAY_CONFIG_FLAGS, np: &mut u32, nm: &mut u32) -> WIN32_ERROR {
    // SAFETY: np and nm are live u32s that Windows fills.
    unsafe { GetDisplayConfigBufferSizes(flags, np, nm) }
}

/// Fills paths and modes; np and nm come back as how many Windows wrote.
pub(super) fn query_display_config(
    flags: QUERY_DISPLAY_CONFIG_FLAGS,
    np: &mut u32,
    paths: &mut [DISPLAYCONFIG_PATH_INFO],
    nm: &mut u32,
    modes: &mut [DISPLAYCONFIG_MODE_INFO],
) -> WIN32_ERROR {
    // Windows writes as many entries as each count says: never more than its slice holds.
    *np = (*np).min(paths.len() as u32);
    *nm = (*nm).min(modes.len() as u32);
    // SAFETY: each count is at most its slice's length (above), and the counts are live u32s.
    unsafe { QueryDisplayConfig(flags, np, paths.as_mut_ptr(), nm, modes.as_mut_ptr(), None) }
}

pub(super) fn display_config_get_source_name(name: &mut DISPLAYCONFIG_SOURCE_DEVICE_NAME) -> i32 {
    // Windows writes what the header's type asks for, up to the size it gives: make both this struct's.
    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
    name.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
    // SAFETY: the header heads a whole live DISPLAYCONFIG_SOURCE_DEVICE_NAME and names it (above).
    unsafe { DisplayConfigGetDeviceInfo(&mut name.header) }
}

pub(super) fn display_config_get_target_name(name: &mut DISPLAYCONFIG_TARGET_DEVICE_NAME) -> i32 {
    // Windows writes what the header's type asks for, up to the size it gives: make both this struct's.
    name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
    name.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
    // SAFETY: the header heads a whole live DISPLAYCONFIG_TARGET_DEVICE_NAME and names it (above).
    unsafe { DisplayConfigGetDeviceInfo(&mut name.header) }
}

pub(super) fn get_tick_count() -> u32 {
    // SAFETY: takes nothing and only reads the clock.
    unsafe { GetTickCount() }
}
