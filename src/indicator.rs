// On-screen workspace indicator: a row of dots centered on the switched monitor,
// the active one drawn as a wider bright pill, and the workspace a window was just sent to
// in an accent color, or every other one for a window just pinned. Holds briefly, then fades out.
// A note (spec 0016: a screen unplugged or back) adds lines of text under the dots and holds 3 s.
// Lives on the main thread (its messages come through main's loop).
use std::cell::{Cell, RefCell};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
        Gdi::*,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        WindowsAndMessaging::*,
    },
};
use windows::core::w;

use crate::ui;

use crate::settings;

/// Also how `--quit` and the single-instance check find the running des-k.
pub const CLASS: windows::core::PCWSTR = w!("des-k-indicator");
/// Posted to the main loop on WM_DISPLAYCHANGE (handled there, never inside a sent message).
pub const WM_DISPLAY_CHANGED: u32 = WM_APP + 1;

const FADE_TIMER: usize = 1;
const FADE_STEP_MS: u32 = 16;
const OPAQUE: u8 = 235;
pub(crate) const ACCENT: COLORREF = COLORREF(0x00FA_A560); // RGB(96, 165, 250), COLORREF is 0x00BBGGRR

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
    static COUNT: Cell<usize> = const { Cell::new(0) };
    static SENT: Cell<Option<usize>> = const { Cell::new(None) };
    static ALL: Cell<bool> = const { Cell::new(false) }; // a window was just pinned: every workspace lit
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
    static ALPHA: Cell<u8> = const { Cell::new(0) };
    static STEP: Cell<u8> = const { Cell::new(OPAQUE) }; // alpha lost per fade tick
    static NOTE: RefCell<String> = const { RefCell::new(String::new()) }; // lines under the dots
}

// Sizes at 96 dpi.
const DOT: f32 = 10.0;
const ACTIVE_W: f32 = 28.0;
const GAP: f32 = 12.0;
const PAD: f32 = 18.0;
const TEXT: f32 = 14.0; // a note's text height
const LINE: f32 = 20.0; // and its line height
const NOTE_MS: u32 = 3000; // long enough to read a line

fn px(v: f32) -> i32 {
    (v * SCALE.get()).round() as i32
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let bg = CreateSolidBrush(COLORREF(0x00202020));
                FillRect(hdc, &rc, bg);
                let _ = DeleteObject(bg.into());

                SelectObject(hdc, GetStockObject(NULL_PEN));
                let on = CreateSolidBrush(COLORREF(0x00F0F0F0));
                let off = CreateSolidBrush(COLORREF(0x00707070));
                let sent = CreateSolidBrush(ACCENT);
                let (top, d) = (px(PAD), px(DOT));
                let mut x = px(PAD);
                for i in 0..COUNT.get() {
                    let w = if i == ACTIVE.get() { px(ACTIVE_W) } else { d };
                    let brush = if i == ACTIVE.get() { on } else if ALL.get() || SENT.get() == Some(i) { sent } else { off };
                    SelectObject(hdc, brush.into());
                    let _ = RoundRect(hdc, x, top, x + w, top + d, d, d);
                    x += w + px(GAP);
                }
                let _ = DeleteObject(on.into());
                let _ = DeleteObject(off.into());
                let _ = DeleteObject(sent.into());
                let note = NOTE.with_borrow(|n| n.clone());
                if !note.is_empty() {
                    let font = ui::font(w!("Segoe UI"), TEXT * SCALE.get(), FW_NORMAL.0 as i32);
                    let old = SelectObject(hdc, font.into());
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, COLORREF(0x00F0F0F0));
                    let mut y = top + d + px(GAP);
                    for line in note.lines() {
                        let mut wide: Vec<u16> = line.encode_utf16().collect();
                        let mut r = RECT { left: px(PAD), top: y, right: rc.right - px(PAD), bottom: y + px(LINE) };
                        DrawTextW(hdc, &mut wide, &mut r, DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX);
                        y += px(LINE);
                    }
                    SelectObject(hdc, old);
                    let _ = DeleteObject(font.into());
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == FADE_TIMER => {
                // First tick ends the hold; then fade a step per tick.
                SetTimer(Some(hwnd), FADE_TIMER, FADE_STEP_MS, None);
                let a = ALPHA.get().saturating_sub(STEP.get());
                ALPHA.set(a);
                if a == 0 {
                    let _ = KillTimer(Some(hwnd), FADE_TIMER);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else {
                    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), a, LWA_ALPHA);
                }
                LRESULT(0)
            }
            // Screens can take a while to light up after a wake (spec 0016 AC-18).
            WM_POWERBROADCAST if wp.0 as u32 == PBT_APMRESUMEAUTOMATIC => {
                crate::ws::woke();
                LRESULT(1)
            }
            WM_DISPLAYCHANGE => {
                let _ = PostMessageW(Some(hwnd), WM_DISPLAY_CHANGED, WPARAM(0), LPARAM(0));
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// Create the window up front, so it exists for --quit and display-change messages.
pub fn init() {
    let _ = window();
}

fn window() -> Option<HWND> {
    if let Some(h) = WND.get() {
        return Some(h);
    }
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
        // Tool window + no-activate: never in Alt+Tab, never managed by des-k, never steals focus.
        let ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
        let h = CreateWindowExW(ex, CLASS, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(inst.into()), None).ok()?;
        // An admin des-k would drop `des-k --quit` from a normal terminal otherwise (spec 0008).
        if let Err(e) = ChangeWindowMessageFilterEx(h, WM_CLOSE, MSGFLT_ALLOW, None) {
            eprintln!("indicator: WM_CLOSE not let in from normal programs: {e}");
        }
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(h, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, size_of_val(&round) as u32);
        WND.set(Some(h));
        Some(h)
    }
}

/// Show `active` (0-based) of `count` workspaces, centered on `mon`'s work area, with
/// `sent` (0-based, never `active`) in the accent color.
pub fn show(mon: HMONITOR, active: usize, count: usize, sent: Option<usize>) {
    ALL.set(false);
    NOTE.with_borrow_mut(String::clear);
    show_now(mon, active, count, sent, None);
}

/// A screen unplugged or back (spec 0016 AC-13): `active` of `count` on `mon` with `text`, one
/// line or more, under the dots, held 3 s. Replaces whatever the indicator shows.
pub fn show_note(mon: HMONITOR, active: usize, count: usize, text: &str) {
    ALL.set(false);
    NOTE.set(text.to_string());
    show_now(mon, active, count, None, Some(NOTE_MS));
}

/// After a pin (spec 0014): every workspace but `active` in the accent color while `pinned`,
/// the window being on all of them; plain dots after an unpin.
pub fn show_pin(mon: HMONITOR, active: usize, count: usize, pinned: bool) {
    ALL.set(pinned);
    NOTE.with_borrow_mut(String::clear);
    show_now(mon, active, count, None, None);
}

fn show_now(mon: HMONITOR, active: usize, count: usize, sent: Option<usize>, hold: Option<u32>) {
    let st = settings::get();
    if !st.indicator_enabled {
        return;
    }
    let Some(h) = window() else { return };
    // Enough steps to fade from OPAQUE to 0 in about fade_ms; 0 hides on the first tick.
    let ticks = (st.fade_ms / FADE_STEP_MS).max(1);
    STEP.set((OPAQUE as u32).div_ceil(ticks) as u8);
    unsafe {
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        SCALE.set(dx as f32 / 96.0);
        ACTIVE.set(active);
        COUNT.set(count);
        SENT.set(sent);
        ALPHA.set(OPAQUE);

        let mut w = px(PAD * 2.0 + ACTIVE_W + (count as f32 - 1.0) * (DOT + GAP));
        let mut ht = px(PAD * 2.0 + DOT);
        let note = NOTE.with_borrow(|n| n.clone());
        if !note.is_empty() {
            let dc = GetDC(None);
            let font = ui::font(w!("Segoe UI"), TEXT * SCALE.get(), FW_NORMAL.0 as i32);
            let old = SelectObject(dc, font.into());
            for line in note.lines() {
                let wide: Vec<u16> = line.encode_utf16().collect();
                let mut size = windows::Win32::Foundation::SIZE::default();
                let _ = GetTextExtentPoint32W(dc, &wide, &mut size);
                w = w.max(size.cx + px(PAD * 2.0));
            }
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            ReleaseDC(None, dc);
            ht += px(GAP) + note.lines().count() as i32 * px(LINE);
        }
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let r = mi.rcWork;
        let (x, y) = ((r.left + r.right - w) / 2, (r.top + r.bottom - ht) / 2);

        let _ = SetLayeredWindowAttributes(h, COLORREF(0), OPAQUE, LWA_ALPHA);
        let _ = SetWindowPos(h, Some(HWND_TOPMOST), x, y, w, ht, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        let _ = InvalidateRect(Some(h), None, true);
        SetTimer(Some(h), FADE_TIMER, hold.unwrap_or(st.hold_ms), None);
    }
}
