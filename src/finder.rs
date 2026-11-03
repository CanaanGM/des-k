// Window finder (spec 0007): a search popup listing every window on every screen and
// workspace, hidden ones included. Enter goes to the window, Shift+Enter brings it here,
// Ctrl+W closes it. Lives on the main thread; where a window is only ever changes through
// `ws`, and the finder never cloaks anything itself.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use windows::Win32::{
    Foundation::{COLORREF, ERROR_ACCESS_DENIED, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute},
        Gdi::*,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::indicator::ACCENT;
use crate::restore;
use crate::ui::{self, text};
use crate::ws::{self, Miss};

const CLASS: PCWSTR = w!("des-k-finder");
const CLOSING_TIMER: usize = 1;
const CLOSING_POLL_MS: u32 = 200;
const ICON_WAIT_MS: u32 = 50; // a frozen app must not stall the finder

pub(crate) const BG: COLORREF = COLORREF(0x0020_2020);
pub(crate) const TEXT: COLORREF = COLORREF(0x00F0_F0F0);
pub(crate) const DIM: COLORREF = COLORREF(0x0090_9090);
const LINE: COLORREF = COLORREF(0x0034_3434);
const ON_ACCENT: COLORREF = COLORREF(0x0020_2020); // light text is hard to read on the light blue

// Sizes at 96 dpi.
const WIDTH: f32 = 640.0;
const BOX: f32 = 44.0;
const ROW: f32 = 36.0;
const ROWS: usize = 9;
const NOTE: f32 = 24.0;
const BOTTOM: f32 = 6.0;
const FONT_PX: f32 = 15.0;
const ICON: f32 = 16.0;
const PAD_X: f32 = 16.0;
const PROGRAM_X: f32 = 44.0;
const PROGRAM_W: f32 = 130.0;
const PLACE_W: f32 = 64.0;

// Edit messages, from winuser.h: the `windows` crate keeps them in Win32_UI_Controls, a big feature for three numbers.
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;

// Keys without a VK_ name.
const KEY_N: VIRTUAL_KEY = VIRTUAL_KEY(b'N' as u16);
const KEY_P: VIRTUAL_KEY = VIRTUAL_KEY(b'P' as u16);
const KEY_W: VIRTUAL_KEY = VIRTUAL_KEY(b'W' as u16);

struct Row {
    hwnd: HWND,
    title: String,
    program: String,
    icon: HICON, // the app's or the class's: borrowed, never destroyed
    screen: usize, // 1 based, left to right
    workspace: usize, // 1 based
    on_screen: bool,
    used: Option<Instant>,
    rank: usize, // place in the AC-4 order
    gone: bool,  // closed while the finder is open
}

struct Session {
    rows: Vec<Row>,
    shown: Vec<(usize, u32)>, // row index, score
    selected: usize,          // index into shown
    scroll: usize,
    came_from: HWND,
    screen: String, // the device the finder opened on: where Shift+Enter brings windows
    query: String,
    note: Option<&'static str>,
    closing: Vec<HWND>, // sent WM_CLOSE, waiting to go
    matcher: Matcher,
    scale: f32,
}

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static EDIT: Cell<Option<HWND>> = const { Cell::new(None) };
    static FONT: Cell<Option<(HFONT, u32)>> = const { Cell::new(None) }; // and its dpi
    static BRUSH: Cell<Option<HBRUSH>> = const { Cell::new(None) };
    // Root owner window -> last time it came to the front. Memory only (spec 0007).
    static RECENT: RefCell<HashMap<isize, Instant>> = RefCell::default();
    // Window -> its icon, asked for once (see `icon`). Borrowed handles, never destroyed.
    static ICONS: RefCell<HashMap<isize, HICON>> = RefCell::default();
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// Never hold the borrow across a call that can send a message to the finder: its
/// window procedure borrows the session too.
fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with_borrow_mut(|s| s.as_mut().map(f))
}

impl Session {
    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    /// Recompute what is shown. `keep` holds on to the selected window; when it went, the
    /// row that took its place is selected.
    fn refilter(&mut self, keep: bool) {
        let kept = self.shown.get(self.selected).map(|&(i, _)| self.rows[i].hwnd);
        let scores: Vec<Option<u32>> = if self.query.trim().is_empty() {
            self.rows.iter().map(|r| (!r.gone).then_some(0)).collect()
        } else {
            let pattern = Pattern::new(&self.query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
            let mut buf = Vec::new();
            let matcher = &mut self.matcher;
            self.rows
                .iter()
                .map(|r| if r.gone { None } else { pattern.score(Utf32Str::new(&format!("{} {}", r.program, r.title), &mut buf), matcher) })
                .collect()
        };
        let old = self.selected;
        self.shown = rank(&self.rows, &scores);
        self.selected = if keep {
            let last = self.shown.len().saturating_sub(1);
            kept.and_then(|h| self.shown.iter().position(|&(i, _)| self.rows[i].hwnd == h)).unwrap_or(old.min(last))
        } else {
            self.scroll = 0;
            first_pick(&self.query, self.shown.len())
        };
        self.scroll_to_selection();
    }

    fn scroll_to_selection(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + ROWS {
            self.scroll = self.selected + 1 - ROWS;
        }
        self.scroll = self.scroll.min(self.shown.len().saturating_sub(ROWS));
    }

    fn note_text(&self) -> Option<&'static str> {
        match self.note {
            Some(n) => Some(n),
            None if !self.shown.is_empty() => None,
            None if self.rows.iter().all(|r| r.gone) => Some("No windows"),
            None => Some("No match"),
        }
    }

    fn visible_rows(&self) -> usize {
        self.shown.len().min(ROWS)
    }

    fn height(&self) -> i32 {
        let note = if self.note_text().is_some() { NOTE } else { 0.0 };
        self.px(BOX) + self.visible_rows() as i32 * self.px(ROW) + self.px(note + BOTTOM)
    }
}

/// AC-4: windows used since start, most recent first; then unused ones on screen, front to
/// back; then unused hidden ones by screen, workspace and saved order. `rows` come in
/// `ws::windows()` order, which breaks the remaining ties.
fn recency_rank(rows: &mut [Row]) {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| {
        let (ra, rb) = (&rows[a], &rows[b]);
        rb.used
            .cmp(&ra.used)
            .then(rb.on_screen.cmp(&ra.on_screen))
            .then_with(|| if ra.on_screen { std::cmp::Ordering::Equal } else { (ra.screen, ra.workspace).cmp(&(rb.screen, rb.workspace)) })
            .then(a.cmp(&b))
    });
    for (rank, i) in order.into_iter().enumerate() {
        rows[i].rank = rank;
    }
}

/// AC-5: the matching rows, best score first, equal scores in the AC-4 order.
fn rank(rows: &[Row], scores: &[Option<u32>]) -> Vec<(usize, u32)> {
    let mut shown: Vec<(usize, u32)> = scores.iter().enumerate().filter_map(|(i, s)| Some((i, (*s)?))).collect();
    shown.sort_by(|a, b| b.1.cmp(&a.1).then(rows[a.0].rank.cmp(&rows[b.0].rank)));
    shown
}

/// With nothing typed the second row, so Win+F then Enter flips back like Alt+Tab.
fn first_pick(query: &str, shown: usize) -> usize {
    if query.trim().is_empty() && shown >= 2 { 1 } else { 0 }
}

/// Ctrl+Backspace: where the word before the caret starts. Spaces before the caret go
/// with it; a word is a run of letters and digits, or a run of other symbols.
fn delete_word(before: &[u16]) -> usize {
    let space = |c: u16| c == b' ' as u16;
    // A surrogate half belongs to a character outside the basic plane: count it as a letter.
    let wordy = |c: u16| (0xD800..0xE000).contains(&c) || char::from_u32(c as u32).is_some_and(char::is_alphanumeric);
    let mut i = before.len();
    while i > 0 && space(before[i - 1]) {
        i -= 1;
    }
    if i > 0 {
        let kind = wordy(before[i - 1]);
        while i > 0 && !space(before[i - 1]) && wordy(before[i - 1]) == kind {
            i -= 1;
        }
    }
    i
}

/// The program name: the exe without `.exe`; for a Store app, the hosted app's exe.
fn program(hwnd: HWND) -> String {
    let stem = |h: HWND| {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        restore::exe_path(pid).and_then(|p| Some(Path::new(&p).file_stem()?.to_string_lossy().into_owned()))
    };
    let name = stem(hwnd).unwrap_or_default();
    if name.eq_ignore_ascii_case("ApplicationFrameHost") {
        // The hosted app lives in a CoreWindow child while it runs (not while suspended).
        if let Some(app) = unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) }.ok().and_then(stem) {
            return app;
        }
    }
    name
}

/// A window's icon, asked for once while des-k runs: WM_GETICON waits on the app (a busy one
/// can take the whole ICON_WAIT_MS), and the finder and the overview want every window's
/// icon each time they open. The app's or the class's icon: borrowed, never destroyed.
pub(crate) fn icon(hwnd: HWND) -> HICON {
    if let Some(i) = ICONS.with_borrow(|c| c.get(&(hwnd.0 as isize)).copied()) {
        return i;
    }
    let i = ask_icon(hwnd);
    ICONS.with_borrow_mut(|c| {
        // A window new to the cache is the time to forget the ones that closed.
        c.retain(|&h, _| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool());
        c.insert(hwnd.0 as isize, i);
    });
    i
}

fn ask_icon(hwnd: HWND) -> HICON {
    for kind in [ICON_SMALL2, ICON_SMALL, ICON_BIG] {
        let mut out = 0usize;
        let ok = unsafe { SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(kind as usize), LPARAM(0), SMTO_ABORTIFHUNG, ICON_WAIT_MS, Some(&mut out)) };
        if ok.0 == 0 {
            break; // hung or slow: don't wait on it twice more
        }
        if out != 0 {
            return HICON(out as *mut _);
        }
    }
    for index in [GCLP_HICONSM, GCLP_HICON] {
        let h = unsafe { GetClassLongPtrW(hwnd, index) };
        if h != 0 {
            return HICON(h as *mut _);
        }
    }
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

fn snapshot() -> Vec<Row> {
    let order = ws::screen_order();
    let mut rows: Vec<Row> = RECENT.with_borrow(|recent| {
        ws::windows()
            .into_iter()
            .map(|p| Row {
                title: ws::title(p.hwnd),
                program: program(p.hwnd),
                icon: icon(p.hwnd),
                screen: order.iter().position(|d| *d == p.device).map_or(0, |i| i + 1),
                workspace: p.workspace + 1,
                on_screen: p.on_screen,
                used: recent.get(&(p.hwnd.0 as isize)).copied(),
                rank: 0,
                gone: false,
                hwnd: p.hwnd,
            })
            .collect()
    });
    recency_rank(&mut rows);
    rows
}

/// A window came to the front (the foreground hook, or a finder pick): its root owner,
/// so a dialog counts for the window it belongs to.
pub fn used(hwnd: HWND) {
    let root = unsafe { GetAncestor(hwnd, GA_ROOTOWNER) };
    let root = if root.is_invalid() { hwnd } else { root };
    RECENT.with_borrow_mut(|r| r.insert(root.0 as isize, Instant::now()));
}

/// A touchpad gesture asks (spec 0011).
pub fn is_open() -> bool {
    SESSION.with_borrow(Option::is_some)
}

/// Win+F: open on the screen under the mouse, or close if open.
pub fn toggle() {
    if SESSION.with_borrow(Option::is_some) {
        close(true);
    } else {
        open();
    }
}

fn open() {
    let Some((popup, edit)) = window() else { return eprintln!("finder: window not created") };
    let (mon, device) = ws::under_mouse();
    let came_from = unsafe { GetForegroundWindow() };
    used(came_from);
    RECENT.with_borrow_mut(|r| r.retain(|&h, _| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool()));
    let (mut dpi, mut dy) = (96u32, 96u32);
    unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi, &mut dy); }
    let mut s = Session {
        rows: snapshot(),
        shown: Vec::new(),
        selected: 0,
        scroll: 0,
        came_from,
        screen: device.clone(),
        query: String::new(),
        note: None,
        closing: Vec::new(),
        matcher: Matcher::new(Config::DEFAULT),
        scale: dpi as f32 / 96.0,
    };
    s.refilter(false);
    let (n, w, h, pad, box_h, edit_h) = (s.rows.len(), s.px(WIDTH), s.height(), s.px(PAD_X), s.px(BOX), s.px(FONT_PX + 8.0));
    unsafe {
        let _ = SetWindowTextW(edit, w!("")); // before the session exists, so its EN_CHANGE does nothing
        SESSION.set(Some(s));
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let r = mi.rcWork;
        let (x, y) = ((r.left + r.right - w) / 2, r.top + (r.bottom - r.top) / 5);
        // Placed first: moving to another screen can change the popup's dpi, then the font matches it.
        let _ = SetWindowPos(popup, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(font(dpi).0 as usize)), Some(LPARAM(0)));
        let _ = MoveWindow(edit, pad, (box_h - edit_h) / 2, w - 2 * pad, edit_h, false);
        let _ = ShowWindow(popup, SW_SHOW);
        let _ = SetForegroundWindow(popup);
        let _ = SetFocus(Some(edit));
        if GetForegroundWindow() != popup {
            close(false);
            return eprintln!("finder: Windows refused focus");
        }
    }
    println!("finder: opened on {device}, {n} windows");
}

/// Hide the finder and drop the session. `give_back` focuses the window you came from
/// (the desktop if it is gone); otherwise focus already went where it should.
pub fn close(give_back: bool) {
    let Some(s) = SESSION.with_borrow_mut(Option::take) else { return };
    let Some(popup) = WND.get() else { return };
    unsafe {
        let _ = KillTimer(Some(popup), CLOSING_TIMER);
        // Focus first: hiding the active finder would let Windows pick a window to activate.
        if give_back {
            let to = if IsWindow(Some(s.came_from)).as_bool() { s.came_from } else { GetShellWindow() };
            let _ = SetForegroundWindow(to);
        }
        let _ = ShowWindow(popup, SW_HIDE);
    }
}

/// Resize to what is shown now and repaint.
fn refresh() {
    let (Some(popup), Some((w, h))) = (WND.get(), with(|s| (s.px(WIDTH), s.height()))) else { return };
    unsafe {
        let _ = SetWindowPos(popup, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = InvalidateRect(Some(popup), None, false);
    }
}

fn step(by: isize) {
    with(|s| {
        if let Some(last) = s.shown.len().checked_sub(1) {
            s.selected = (s.selected as isize + by).clamp(0, last as isize) as usize;
            s.note = None;
            s.scroll_to_selection();
        }
    });
    refresh();
}

fn set_note(note: &'static str) {
    with(|s| s.note = Some(note));
    refresh();
}

/// The row of a window that went away disappears; the finder stays open.
fn drop_row(hwnd: HWND, note: Option<&'static str>) {
    with(|s| {
        if let Some(r) = s.rows.iter_mut().find(|r| r.hwnd == hwnd) {
            r.gone = true;
        }
        s.refilter(true);
        s.note = note;
    });
    refresh();
}

enum Act {
    Go,
    Bring,
    Close,
}

/// Act on the selected row. Every action asks `ws` where the window is now: restore may
/// have moved it since the snapshot.
fn act(how: Act) {
    let Some((h, title, screen)) = with(|s| {
        let &(i, _) = s.shown.get(s.selected)?;
        Some((s.rows[i].hwnd, s.rows[i].title.clone(), s.screen.clone()))
    })
    .flatten() else {
        return;
    };
    let done = match how {
        Act::Go => ws::go_to(h),
        Act::Bring => ws::bring(h, &screen),
        Act::Close => return close_window(h, &title),
    };
    match done {
        // Focus has moved to the window already, which closed the finder; this is the backstop.
        Ok(()) => {
            used(h);
            close(false);
        }
        Err(Miss::Gone) => {
            println!("finder: \"{title}\" is gone");
            drop_row(h, Some("That window is gone"));
        }
        Err(Miss::CantMove) => set_note("Can't move this window"),
    }
}

/// Ctrl+W: ask it to close as its X button would, then watch for it to go.
fn close_window(h: HWND, title: &str) {
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        println!("finder: \"{title}\" is gone");
        return drop_row(h, Some("That window is gone"));
    }
    match unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        Ok(()) => {
            with(|s| s.closing.push(h));
            if let Some(popup) = WND.get() {
                unsafe { SetTimer(Some(popup), CLOSING_TIMER, CLOSING_POLL_MS, None) };
            }
        }
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => {
            eprintln!("finder: can't close \"{title}\", it runs as admin");
            set_note("Can't close an admin window");
        }
        Err(e) => eprintln!("finder: can't close \"{title}\": {e}"),
    }
}

/// Rows whose window closed or hid itself (apps that close to the tray) go away.
fn closing_tick() {
    let gone = with(|s| {
        let gone: Vec<(HWND, String)> = s
            .closing
            .iter()
            .filter(|&&h| unsafe { !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() })
            .map(|&h| (h, s.rows.iter().find(|r| r.hwnd == h).map_or_else(String::new, |r| r.title.clone())))
            .collect();
        s.closing.retain(|h| !gone.iter().any(|(g, _)| g == h));
        (gone, s.closing.is_empty())
    });
    let Some((gone, none_left)) = gone else { return };
    for (h, title) in gone {
        println!("finder: closed \"{title}\"");
        drop_row(h, None);
    }
    if none_left && let Some(popup) = WND.get() {
        unsafe { let _ = KillTimer(Some(popup), CLOSING_TIMER); }
    }
}

fn delete_word_before_caret(edit: HWND) {
    unsafe {
        let (mut start, mut end) = (0u32, 0u32);
        SendMessageW(edit, EM_GETSEL, Some(WPARAM(&mut start as *mut _ as usize)), Some(LPARAM(&mut end as *mut _ as isize)));
        if start == end {
            let mut buf = vec![0u16; GetWindowTextLengthW(edit) as usize + 1];
            let n = GetWindowTextW(edit, &mut buf).max(0) as usize;
            start = delete_word(&buf[..(end as usize).min(n)]) as u32;
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(start as usize)), Some(LPARAM(end as isize)));
        }
        // Through the edit itself, so undo and EN_CHANGE work as for any other change.
        SendMessageW(edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(w!("").as_ptr() as isize)));
    }
}

/// Called by the main loop for every message before TranslateMessage. Returns true when
/// the finder took the key (then no WM_CHAR is made for it).
pub fn key(msg: &MSG) -> bool {
    if !matches!(msg.message, WM_KEYDOWN | WM_SYSKEYDOWN) || EDIT.get() != Some(msg.hwnd) || SESSION.with_borrow(Option::is_none) {
        return false;
    }
    let held = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
    let (ctrl, shift) = (held(VK_CONTROL), held(VK_SHIFT));
    match VIRTUAL_KEY(msg.wParam.0 as u16) {
        VK_UP => step(-1),
        VK_DOWN => step(1),
        VK_TAB => step(if shift { -1 } else { 1 }),
        KEY_P if ctrl => step(-1),
        KEY_N if ctrl => step(1),
        VK_RETURN => act(if shift { Act::Bring } else { Act::Go }),
        KEY_W if ctrl => act(Act::Close),
        VK_ESCAPE => close(true),
        VK_BACK if ctrl => delete_word_before_caret(msg.hwnd),
        _ => return false,
    }
    true
}

fn on_text_change() {
    let Some(edit) = EDIT.get() else { return };
    let mut buf = vec![0u16; unsafe { GetWindowTextLengthW(edit) } as usize + 1];
    let n = unsafe { GetWindowTextW(edit, &mut buf) }.max(0) as usize;
    let text = String::from_utf16_lossy(&buf[..n]);
    if with(|s| {
        s.query = text;
        s.note = None;
        s.refilter(false);
    })
    .is_some()
    {
        refresh();
    }
}

fn on_click(y: i32) {
    let hit = with(|s| {
        let row = (y - s.px(BOX)).checked_div(s.px(ROW)).filter(|_| y >= s.px(BOX))? as usize;
        (row < s.visible_rows()).then(|| {
            s.selected = s.scroll + row;
            s.note = None;
        })
    });
    if hit.flatten().is_some() {
        refresh();
        act(Act::Go);
    }
}

/// The edit font at a dpi, made once per dpi.
fn font(dpi: u32) -> HFONT {
    if let Some((f, d)) = FONT.get()
        && d == dpi
    {
        return f;
    }
    let face = if ui::variable() { w!("Segoe UI Variable Text") } else { w!("Segoe UI") };
    let f = ui::font(face, FONT_PX * dpi as f32 / 96.0, FW_NORMAL.0 as i32);
    // The edit gets the new font before the old one goes.
    if let Some((old, _)) = FONT.replace(Some((f, dpi))) {
        if let Some(edit) = EDIT.get() {
            unsafe { SendMessageW(edit, WM_SETFONT, Some(WPARAM(f.0 as usize)), Some(LPARAM(0))) };
        }
        unsafe { let _ = DeleteObject(old.into()); }
    }
    f
}

fn bg_brush() -> HBRUSH {
    BRUSH.get().unwrap_or_else(|| {
        let b = unsafe { CreateSolidBrush(BG) };
        BRUSH.set(Some(b));
        b
    })
}

fn draw(dc: HDC, s: &Session, width: i32) {
    let px = |v: f32| s.px(v);
    let top = px(BOX);
    unsafe {
        SetBkMode(dc, TRANSPARENT);
        if let Some((f, _)) = FONT.get() {
            SelectObject(dc, f.into());
        }
        if s.visible_rows() > 0 {
            let line = CreateSolidBrush(LINE);
            FillRect(dc, &RECT { left: 0, top: top - px(1.0), right: width, bottom: top }, line);
            let _ = DeleteObject(line.into());
        }
        let accent = CreateSolidBrush(ACCENT);
        SelectObject(dc, GetStockObject(NULL_PEN));
        SelectObject(dc, accent.into());
        for (k, &(i, _)) in s.shown.iter().enumerate().skip(s.scroll).take(ROWS) {
            let r = &s.rows[i];
            let y = top + (k - s.scroll) as i32 * px(ROW);
            let selected = k == s.selected;
            if selected {
                let _ = RoundRect(dc, px(6.0), y + px(2.0), width - px(6.0), y + px(ROW - 2.0), px(8.0), px(8.0));
            }
            let (main, dim) = if selected { (ON_ACCENT, ON_ACCENT) } else { (TEXT, DIM) };
            let _ = DrawIconEx(dc, px(PAD_X), y + (px(ROW) - px(ICON)) / 2, r.icon, px(ICON), px(ICON), 0, None, DI_NORMAL);
            let cell = |left: i32, right: i32| RECT { left, top: y, right, bottom: y + px(ROW) };
            let place = format!("{}{} · {}", if r.on_screen { "● " } else { "" }, r.screen, r.workspace);
            text(dc, &r.program, cell(px(PROGRAM_X), px(PROGRAM_X + PROGRAM_W)), dim, DT_LEFT);
            text(dc, &r.title, cell(px(PROGRAM_X + PROGRAM_W + 12.0), width - px(PAD_X + PLACE_W + 8.0)), main, DT_LEFT);
            text(dc, &place, cell(width - px(PAD_X + PLACE_W), width - px(PAD_X)), dim, DT_RIGHT);
        }
        let _ = DeleteObject(accent.into());
        if let Some(note) = s.note_text() {
            let y = top + s.visible_rows() as i32 * px(ROW);
            text(dc, note, RECT { left: px(PAD_X), top: y, right: width - px(PAD_X), bottom: y + px(NOTE) }, DIM, DT_LEFT);
        }
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        // Drawn off screen first: the list repaints on every key, and would flicker otherwise.
        ui::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
            FillRect(mem, &rc, bg_brush());
            SESSION.with_borrow(|s| {
                if let Some(s) = s {
                    draw(mem, s, rc.right);
                }
            });
        });
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_CTLCOLOREDIT => {
                let dc = HDC(wp.0 as *mut _);
                SetTextColor(dc, TEXT);
                SetBkColor(dc, BG);
                LRESULT(bg_brush().0 as isize)
            }
            WM_COMMAND if (wp.0 >> 16) as u32 == EN_CHANGE => {
                on_text_change();
                LRESULT(0)
            }
            // A click outside, or any other window taking focus.
            WM_ACTIVATE if (wp.0 & 0xFFFF) as u32 == WA_INACTIVE => {
                close(false);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                on_click((lp.0 >> 16) as i16 as i32);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == CLOSING_TIMER => {
                closing_tick();
                LRESULT(0)
            }
            WM_DPICHANGED => LRESULT(0), // open sizes the popup for each screen itself
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// The popup and its edit, made on first use.
fn window() -> Option<(HWND, HWND)> {
    if let (Some(w), Some(e)) = (WND.get(), EDIT.get()) {
        return Some((w, e));
    }
    unsafe {
        let inst = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: inst.into(), lpszClassName: CLASS, hCursor: LoadCursorW(None, IDC_ARROW).ok()?, ..Default::default() };
        RegisterClassW(&wc);
        // Tool window: never in Alt+Tab or the taskbar, and never in ws lists. It must take focus, so no WS_EX_NOACTIVATE.
        let popup = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, CLASS, w!("des-k finder"), WS_POPUP | WS_CLIPCHILDREN, 0, 0, 0, 0, None, None, Some(inst.into()), None).ok()?;
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(popup, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as _, size_of_val(&round) as u32);
        let edit = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 0, 0, 0, 0, Some(popup), None, Some(inst.into()), None).ok()?;
        WND.set(Some(popup));
        EDIT.set(Some(edit));
        Some((popup, edit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn row(on_screen: bool, screen: usize, workspace: usize, used: Option<Instant>) -> Row {
        Row { hwnd: HWND::default(), title: String::new(), program: String::new(), icon: HICON::default(), screen, workspace, on_screen, used, rank: 0, gone: false }
    }

    fn ranks(rows: &[Row]) -> Vec<usize> {
        rows.iter().map(|r| r.rank).collect()
    }

    // AC-4: what you used last comes first (that is what makes Win+F, Enter flip back), then
    // what you can see, then hidden windows in a fixed screen and workspace order.
    #[test]
    fn recency_beats_screen_order_and_hidden_windows_follow_screen_then_workspace() {
        let t = Instant::now();
        let mut rows = vec![
            row(true, 1, 1, None),
            row(false, 2, 3, None),
            row(false, 1, 2, Some(t)),
            row(false, 2, 1, None),
            row(true, 3, 1, Some(t + Duration::from_secs(1))),
            row(false, 1, 4, None),
        ];
        recency_rank(&mut rows);
        assert_eq!(ranks(&rows), [2, 5, 1, 4, 0, 3]);
    }

    // AC-5: a better match must win over recency, or typing would not find the window you mean;
    // only equal scores fall back to the AC-4 order. Non matches are not listed at all.
    #[test]
    fn rank_puts_score_first_then_recency_order() {
        let mut rows = vec![row(true, 1, 1, None), row(true, 1, 1, None), row(true, 1, 1, None)];
        rows[0].rank = 2;
        rows[1].rank = 0;
        rows[2].rank = 1;
        assert_eq!(rank(&rows, &[Some(50), Some(20), Some(50)]), [(2, 50), (0, 50), (1, 20)]);
        assert_eq!(rank(&rows, &[None, Some(1), None]), [(1, 1)]);
    }

    // Win+F then Enter must land on the window you were in before, not the one you are in.
    #[test]
    fn empty_query_selects_the_second_row() {
        assert_eq!(first_pick("", 5), 1);
        assert_eq!(first_pick("  ", 2), 1);
        assert_eq!(first_pick("", 1), 0);
        assert_eq!(first_pick("code", 5), 0);
    }

    // AC-5: each typed word must appear, letters in order, any case, in "program title"; the
    // best match is selected, and an empty result says why.
    #[test]
    fn refilter_matches_every_word_fuzzily_and_selects_the_best() {
        let named = |program: &str, title: &str, rank| Row { program: program.into(), title: title.into(), rank, ..row(true, 1, 1, None) };
        let mut s = Session {
            rows: vec![named("explorer", "Downloads", 0), named("Code", "finder.rs - pdesk - Visual Studio Code", 1), named("chrome", "pdesk issues", 2)],
            shown: Vec::new(),
            selected: 0,
            scroll: 0,
            came_from: HWND::default(),
            screen: String::new(),
            query: "vsc".into(),
            note: None,
            closing: Vec::new(),
            matcher: Matcher::new(Config::DEFAULT),
            scale: 1.0,
        };
        let firsts = |s: &mut Session, q: &str| {
            s.query = q.into();
            s.refilter(false);
            (s.shown.iter().map(|&(i, _)| i).collect::<Vec<_>>(), s.selected, s.note_text())
        };
        assert_eq!(firsts(&mut s, "vsc"), (vec![1], 0, None));
        assert_eq!(firsts(&mut s, "PDESK visual").0, [1]);
        assert_eq!(firsts(&mut s, "pdesk").0.len(), 2);
        assert_eq!(firsts(&mut s, "zzzq"), (vec![], 0, Some("No match")));
        s.rows.iter_mut().for_each(|r| r.gone = true);
        assert_eq!(firsts(&mut s, "").2, Some("No windows"));
    }

    // Ctrl+Backspace removes one word, like everywhere else in Windows, not the whole query or one letter.
    #[test]
    fn delete_word_stops_at_a_space_or_punctuation_boundary() {
        let cut = |s: &str| {
            let w: Vec<u16> = s.encode_utf16().collect();
            String::from_utf16(&w[..delete_word(&w)]).unwrap()
        };
        assert_eq!(cut("code pdesk"), "code ");
        assert_eq!(cut("code pdesk  "), "code ");
        assert_eq!(cut("main.rs"), "main.");
        assert_eq!(cut("main."), "main");
        assert_eq!(cut("word"), "");
        assert_eq!(cut(""), "");
    }
}
