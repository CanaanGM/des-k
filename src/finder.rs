// Window finder (spec 0007, look B in spec 0017): a search popup listing every window on every
// screen and workspace, hidden ones included. Enter goes to the window, Shift+Enter brings it
// here, Ctrl+W closes it. Lives on the main thread; where a window is only ever changes through
// `ws`, and the finder never cloaks anything itself. Drawn on the shared Canvas to the approved
// design (pdesk-design restart/finder-r3.html); the search text is a native EDIT, so the popup
// is never layered.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;
use nucleo_matcher::{
    Config, Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use windows::Management::Deployment::PackageManager;
use windows::Win32::{
    Foundation::{CloseHandle, ERROR_ACCESS_DENIED, ERROR_SUCCESS, HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::Gdi::*,
    Storage::{
        FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW},
        Packaging::Appx::GetPackageFullName,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
    UI::{
        Accessibility::{UIA_ListControlTypeId, UIA_ListItemControlTypeId, UIA_PaneControlTypeId, UIA_TextControlTypeId},
        HiDpi::{GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI},
        Input::KeyboardAndMouse::*,
        WindowsAndMessaging::*,
    },
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::restore;
use crate::ui::{self, Canvas, F, alpha, mix, op};
use crate::uia::{self, Node, Tree};
use crate::ws::{self, Miss};

const CLASS: PCWSTR = w!("des-k-finder");
const CLOSING_TIMER: usize = 1;
const CLOSING_POLL_MS: u32 = 200;
const ICON_WAIT_MS: u32 = 50; // a frozen app must not stall the finder
const WHO: &str = "des-k finder";
const PLACEHOLDER: &str = "Find a window";

// Sizes in dip, from the design.
const WIDTH: f32 = 720.0;
const BOX: f32 = 48.0; // the search box, its rule included
const SIDE: f32 = 16.0; // the search box's and the key line's side padding
const TEXT_X: f32 = 44.0; // the search text, after its glyph
const TEXT_H: f32 = 20.0;
const COUNT_W: f32 = 100.0; // kept free for the count right of the text box
const LIST_PAD: f32 = 4.0; // above and below the rows
const ROW: f32 = 36.0;
const ROWS: usize = 9;
const ROW_X: f32 = 4.0;
const ROW_W: f32 = WIDTH - ROW_X - 10.0; // the scroll thumb sits in the 10 on the right
const ROW_PAD: f32 = 12.0;
const GAP: f32 = 12.0;
const ICON: f32 = 16.0;
const PROGRAM_X: f32 = ROW_X + ROW_PAD + ICON + GAP;
const PROGRAM_W: f32 = 140.0;
const TITLE_X: f32 = PROGRAM_X + PROGRAM_W + GAP;
const CHIP_RIGHT: f32 = ROW_X + ROW_W - ROW_PAD;
const SCREEN_RIGHT: f32 = CHIP_RIGHT - 24.0 - GAP;
const SCREEN_W: f32 = 140.0;
const TITLE_W: f32 = SCREEN_RIGHT - SCREEN_W - GAP - TITLE_X;
const FOOT: f32 = 40.0; // the key line, its rule included
const WHEEL_ROWS: isize = 3;

// Edit messages, from winuser.h: the `windows` crate keeps them in Win32_UI_Controls, a big feature for four numbers.
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_GETMARGINS: u32 = 0x00D4;
const WM_MOUSELEAVE: u32 = 0x02A3; // winuser.h, also in Win32_UI_Controls

// Keys without a VK_ name.
const KEY_N: VIRTUAL_KEY = VIRTUAL_KEY(b'N' as u16);
const KEY_P: VIRTUAL_KEY = VIRTUAL_KEY(b'P' as u16);
const KEY_W: VIRTUAL_KEY = VIRTUAL_KEY(b'W' as u16);

struct Row {
    hwnd: HWND,
    title: String,
    program: String,
    icon: HICON,   // the app's or the class's: borrowed, never destroyed
    screen: usize, // 1 based, left to right: the AC-4 order
    place: String, // its screen's name
    workspace: usize, // 1 based
    on_screen: bool,
    pinned: bool,
    unplugged: bool, // hidden on a screen that just left
    used: Option<Instant>,
    rank: usize, // place in the AC-4 order
    gone: bool,  // closed while the finder is open
    marks: Vec<u32>, // the query's matched letters, nucleo's indexes into "program title"
}

/// A note above the key line (AC-FND-7); it clears on the next key or move.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Note {
    CantMove,
    CantClose,
    Gone,
    Unplugged,
}

impl Note {
    /// Its icon, its line, the grey line under it, and whether it warns.
    fn words(self, main: &str) -> (&'static str, &'static str, Option<String>, bool) {
        let main = if main.is_empty() { "the main display" } else { main };
        match self {
            Note::CantMove => ("\u{E7BA}", "Can't move this window", Some("It stays where it was.".into()), true),
            Note::CantClose => ("\u{E7BA}", "Can't close an admin window", Some("It stays open.".into()), true),
            Note::Gone => ("\u{E946}", "That window is gone", None, false),
            Note::Unplugged => ("\u{E7F4}", "Its screen was just unplugged", Some(format!("In a moment it moves to {main}; try again then.")), false),
        }
    }
}

struct Session {
    rows: Vec<Row>,
    shown: Vec<(usize, u32)>, // row index, score
    selected: usize,          // index into shown
    scroll: usize,
    hover: Option<usize>, // index into shown, under the mouse
    came_from: HWND,
    screen: String, // the device the finder opened on: where Shift+Enter brings windows
    screens: usize, // present screens: one hides the screen names
    main: String,   // the main display's name, for the unplugged note
    query: String,
    note: Option<Note>,
    closing: Vec<HWND>, // sent WM_CLOSE, waiting to go
    matcher: Matcher,
    scale: f32,
    said: Option<(isize, String, Option<Note>)>, // what Narrator last heard: row, query, note
}

thread_local! {
    static WND: Cell<Option<HWND>> = const { Cell::new(None) };
    static EDIT: Cell<Option<HWND>> = const { Cell::new(None) };
    static EDIT_PROC: Cell<isize> = const { Cell::new(0) }; // the EDIT's own window procedure
    static BRUSH: Cell<Option<(HBRUSH, u32)>> = const { Cell::new(None) }; // the text box's, and its color
    // Root owner window -> last time it came to the front. Memory only (spec 0007).
    static RECENT: RefCell<HashMap<isize, Instant>> = RefCell::default();
    // Window -> its icon, asked for once (see `icon`). Borrowed handles, never destroyed.
    static ICONS: RefCell<HashMap<isize, HICON>> = RefCell::default();
    // Exe path -> the app's name, read once while des-k runs (AC-FND-4).
    static NAMES: RefCell<HashMap<String, Option<String>>> = RefCell::default();
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static TREE: Tree = Tree::default();
}

/// Never hold the borrow across a call that can send a message to the finder: its
/// window procedure borrows the session too.
fn with<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    SESSION.with_borrow_mut(|s| s.as_mut().map(f))
}

impl Session {
    fn new(rows: Vec<Row>, came_from: HWND, screen: String, screens: usize, main: String, scale: f32) -> Session {
        let mut s = Session {
            rows,
            shown: Vec::new(),
            selected: 0,
            scroll: 0,
            hover: None,
            came_from,
            screen,
            screens,
            main,
            query: String::new(),
            note: None,
            closing: Vec::new(),
            matcher: Matcher::new(Config::DEFAULT),
            scale,
            said: None,
        };
        s.refilter(false);
        s
    }

    fn px(&self, v: f32) -> i32 {
        (v * self.scale).round() as i32
    }

    /// Recompute what is shown. `keep` holds on to the selected window; when it went, the
    /// row that took its place is selected.
    fn refilter(&mut self, keep: bool) {
        let kept = self.shown.get(self.selected).map(|&(i, _)| self.rows[i].hwnd);
        let scores: Vec<Option<u32>> = if self.query.trim().is_empty() {
            self.rows.iter_mut().map(|r| {
                r.marks.clear();
                (!r.gone).then_some(0)
            }).collect()
        } else {
            let pattern = Pattern::new(&self.query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
            let (mut buf, mut marks) = (Vec::new(), Vec::new());
            let matcher = &mut self.matcher;
            self.rows
                .iter_mut()
                .map(|r| {
                    r.marks.clear();
                    if r.gone {
                        return None;
                    }
                    marks.clear();
                    let score = pattern.indices(Utf32Str::new(&format!("{} {}", r.program, r.title), &mut buf), matcher, &mut marks)?;
                    marks.sort_unstable();
                    marks.dedup();
                    r.marks.clone_from(&marks);
                    Some(score)
                })
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
        self.hover = None;
        self.scroll_to_selection();
    }

    fn scroll_to_selection(&mut self) {
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + ROWS {
            self.scroll = self.selected + 1 - ROWS;
        }
        self.scroll_by(0);
    }

    fn scroll_by(&mut self, rows: isize) {
        let most = self.shown.len().saturating_sub(ROWS);
        self.scroll = (self.scroll as isize + rows).clamp(0, most as isize) as usize;
    }

    /// What the list says when it lists nothing.
    fn empty_text(&self) -> Option<&'static str> {
        match () {
            _ if !self.shown.is_empty() => None,
            _ if self.rows.iter().all(|r| r.gone) => Some("No windows"),
            _ => Some("No match"),
        }
    }

    /// "12 windows", or "2 of 12" while filtering; none with no windows at all.
    fn count(&self) -> Option<String> {
        let all = self.rows.iter().filter(|r| !r.gone).count();
        match all {
            0 => None,
            _ if !self.query.trim().is_empty() => Some(format!("{} of {all}", self.shown.len())),
            1 => Some("1 window".into()),
            _ => Some(format!("{all} windows")),
        }
    }

    fn visible_rows(&self) -> usize {
        self.shown.len().min(ROWS)
    }

    fn list_h(&self) -> f32 {
        let inner = match (self.visible_rows(), self.empty_text()) {
            (0, Some("No match")) => 52.0, // two lines
            (0, _) => ROW,
            (n, _) => n as f32 * ROW,
        };
        2.0 * LIST_PAD + inner
    }

    fn note_h(&self) -> f32 {
        // 8 above and below its words, 8 under the box.
        self.note.map_or(0.0, |n| 8.0 + 20.0 + if n.words("").2.is_some() { 16.0 } else { 0.0 } + 8.0 + 8.0)
    }

    fn height(&self) -> f32 {
        BOX + self.list_h() + self.note_h() + FOOT
    }

    /// The index into `shown` of the row at a client pixel row.
    fn row_at(&self, y: i32) -> Option<usize> {
        let k = (y as f32 / self.scale - BOX - LIST_PAD) / ROW;
        (k >= 0.0 && (k as usize) < self.visible_rows()).then(|| self.scroll + k as usize)
    }

    /// A row as Narrator reads it: the painted words, then where it is (AC-FND-9).
    fn words(&self, i: usize) -> String {
        let r = &self.rows[i];
        let mut w = format!("{}, {}", r.title, r.program);
        if self.screens > 1 || r.unplugged {
            w += &format!(", {}", r.place);
        }
        w += &format!(", workspace {}", r.workspace);
        w += match () {
            _ if self.closing.contains(&r.hwnd) => ", closing",
            _ if r.unplugged => ", its screen was unplugged",
            _ if r.pinned => ", pinned, on every workspace",
            _ if r.on_screen => ", shown",
            _ => ", hidden",
        };
        w
    }

    /// What Narrator should hear after this change, if anything, and whether it waits for what
    /// Narrator is saying (the first words after the finder opens wait for the text box's).
    fn speech(&mut self) -> Option<(String, bool)> {
        let now = (self.shown.get(self.selected).map_or(0, |&(i, _)| self.rows[i].hwnd.0 as isize), self.query.clone(), self.note);
        if self.said.as_ref() == Some(&now) {
            return None;
        }
        let before = self.said.replace(now);
        if let Some(n) = self.note
            && before.as_ref().map(|b| b.2) != Some(Some(n))
        {
            let (_, line, under, _) = n.words(&self.main);
            return Some((under.map_or_else(|| line.to_string(), |u| format!("{line}. {u}")), false));
        }
        let what = match self.shown.get(self.selected) {
            Some(&(i, _)) => self.words(i),
            None => self.empty_text().unwrap_or_default().to_string(),
        };
        let filtered = before.as_ref().is_none_or(|b| b.1 != self.query);
        let say = match self.count() {
            Some(c) if filtered && !self.shown.is_empty() => format!("{c}, {what}"),
            _ => what,
        };
        Some((say, before.is_none()))
    }

    /// The finder as Narrator sees it, in client pixels: the count, the list of shown rows
    /// with the chosen one selected, and the note or the empty list's words.
    fn tree(&self) -> Node {
        let rect = |x: f32, y: f32, w: f32, h: f32| RECT { left: self.px(x), top: self.px(y), right: self.px(x + w), bottom: self.px(y + h) };
        let text = |name: String, r: RECT| Node { role: UIA_TextControlTypeId, name, rect: r, ..Node::default() };
        let mut kids = vec![];
        if let Some(c) = self.count() {
            kids.push(text(c, rect(WIDTH - SIDE - COUNT_W, 0.0, COUNT_W, BOX)));
        }
        let rows = (self.shown.iter().enumerate().skip(self.scroll).take(ROWS))
            .map(|(k, &(i, _))| Node {
                role: UIA_ListItemControlTypeId,
                name: self.words(i),
                rect: rect(ROW_X, BOX + LIST_PAD + (k - self.scroll) as f32 * ROW, ROW_W, ROW),
                selected: Some(k == self.selected),
                ..Node::default()
            })
            .collect();
        kids.push(Node { role: UIA_ListControlTypeId, name: "Windows".into(), rect: rect(0.0, BOX, WIDTH, self.list_h()), selected: None, kids: rows, ..Node::default() });
        if let Some(e) = self.empty_text() {
            kids.push(text(e.into(), rect(ROW_X, BOX + LIST_PAD, ROW_W, self.list_h() - 2.0 * LIST_PAD)));
        }
        if let Some(n) = self.note {
            let (_, line, under, _) = n.words(&self.main);
            kids.push(text(under.map_or_else(|| line.to_string(), |u| format!("{line}. {u}")), rect(8.0, BOX + self.list_h(), WIDTH - 16.0, self.note_h() - 8.0)));
        }
        Node { role: UIA_PaneControlTypeId, name: PLACEHOLDER.into(), rect: rect(0.0, 0.0, WIDTH, self.height()), selected: None, kids, ..Node::default() }
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

/// A row's matched letters split back into its two columns, as char indexes (AC-FND-6). nucleo
/// counts a letter with combining marks as one; a column where that happened gets none.
fn split_marks(r: &Row) -> (Vec<usize>, Vec<usize>) {
    let mut buf = Vec::new();
    let plen = Utf32Str::new(&r.program, &mut buf).len();
    let tlen = Utf32Str::new(&r.title, &mut buf).len();
    let (pok, tok) = (plen == r.program.chars().count(), tlen == r.title.chars().count());
    let (mut p, mut t) = (vec![], vec![]);
    for &i in &r.marks {
        let i = i as usize;
        if i < plen {
            if pok {
                p.push(i);
            }
        } else if i > plen && i - plen - 1 < tlen && tok {
            t.push(i - plen - 1);
        }
    }
    (p, t)
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

fn stem(exe: &str) -> String {
    Path::new(exe).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

/// The app's name (AC-FND-4): its exe's file description, else the exe name without .exe. A
/// Store app's frame names the hosted app's package, else shows the window title. Read once per
/// exe while des-k runs.
pub(crate) fn program(hwnd: HWND) -> String {
    let exe = |h: HWND| {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        restore::exe_path(pid).map(|p| (p, pid))
    };
    let Some((path, _)) = exe(hwnd) else { return String::new() };
    if stem(&path).eq_ignore_ascii_case("ApplicationFrameHost") {
        // The hosted app lives in a CoreWindow child while it runs (not while suspended).
        let core = unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) }.ok().and_then(exe);
        return core.and_then(|(path, pid)| named(&path, || package_name(pid))).unwrap_or_else(|| ws::title(hwnd));
    }
    named(&path, || Some(describe(&path).unwrap_or_else(|| stem(&path)))).unwrap_or_default()
}

fn named(exe: &str, find: impl FnOnce() -> Option<String>) -> Option<String> {
    if let Some(n) = NAMES.with_borrow(|m| m.get(exe).cloned()) {
        return n;
    }
    let n = find();
    NAMES.with_borrow_mut(|m| m.insert(exe.to_string(), n.clone()));
    n
}

/// The FileDescription in an exe's version info: in its own language first, then US English.
fn describe(exe: &str) -> Option<String> {
    let path = HSTRING::from(exe);
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;
        let query = |sub: &str| {
            let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
            VerQueryValueW(data.as_ptr().cast(), &HSTRING::from(sub), &mut p, &mut len).as_bool().then_some((p as *const u16, len as usize))
        };
        let mut langs: Vec<(u16, u16)> = match query(r"\VarFileInfo\Translation") {
            Some((p, bytes)) => std::slice::from_raw_parts(p, bytes / 2).as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect(),
            None => vec![],
        };
        langs.push((0x0409, 0x04B0));
        langs.into_iter().find_map(|(lang, cp)| {
            let (p, chars) = query(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\FileDescription"))?;
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, chars));
            let s = s.trim_end_matches('\0').trim();
            (!s.is_empty()).then(|| s.to_string())
        })
    }
}

/// A Store app's name, as its package gives it.
fn package_name(pid: u32) -> Option<String> {
    let full = unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let e = GetPackageFullName(proc, &mut len, Some(PWSTR(buf.as_mut_ptr())));
        let _ = CloseHandle(proc);
        if e != ERROR_SUCCESS {
            return None;
        }
        String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)])
    };
    // An empty user means the user des-k runs as.
    let name = PackageManager::new().and_then(|m| m.FindPackageByUserSecurityIdPackageFullName(&HSTRING::new(), &HSTRING::from(&full))).and_then(|p| p.DisplayName());
    match name {
        Ok(n) if !n.is_empty() => Some(n.to_string()),
        Ok(_) => None,
        Err(e) => {
            warn!("finder: no name for the package {full}: {e}");
            None
        }
    }
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
                place: p.screen,
                workspace: p.workspace + 1,
                on_screen: p.on_screen,
                pinned: p.pinned,
                unplugged: p.unplugged,
                used: recent.get(&(p.hwnd.0 as isize)).copied(),
                rank: 0,
                gone: false,
                marks: vec![],
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
    let Some((popup, edit)) = window() else { return warn!("finder: window not created") };
    ui::gdiplus();
    let (mon, device) = ws::under_mouse();
    let came_from = unsafe { GetForegroundWindow() };
    used(came_from);
    RECENT.with_borrow_mut(|r| r.retain(|&h, _| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool()));
    let (mut dpi, mut dy) = (96u32, 96u32);
    unsafe { let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dpi, &mut dy); }
    let (screens, main) = ws::present();
    let s = Session::new(snapshot(), came_from, device.clone(), screens, main, dpi as f32 / 96.0);
    let (n, w, h, scale) = (s.rows.len(), s.px(WIDTH), s.px(s.height()), s.scale);
    unsafe {
        let _ = SetWindowTextW(edit, w!("")); // before the session exists, so its EN_CHANGE does nothing
        SESSION.set(Some(s));
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let r = mi.rcWork;
        let (x, y) = ((r.left + r.right - w) / 2, r.top + (r.bottom - r.top) / 5);
        ui::border(popup, ui::theme().b.edge);
        // Placed first: moving to another screen can change the popup's dpi, then the font matches it.
        let _ = SetWindowPos(popup, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(ui::fonts(scale).get(F::Body).0 as usize)), Some(LPARAM(0)));
        refresh();
        let _ = ShowWindow(popup, SW_SHOW);
        let _ = SetForegroundWindow(popup);
        let _ = SetFocus(Some(edit));
        if GetForegroundWindow() != popup {
            close(false);
            return warn!("finder: Windows refused focus");
        }
    }
    info!("finder: opened on {device}, {n} windows");
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

/// Resize to what is shown now, repaint, and tell Narrator what changed.
fn refresh() {
    let (Some(popup), Some(edit)) = (WND.get(), EDIT.get()) else { return };
    let Some((w, h, edit_at, tree, say)) = with(|s| {
        let e = RECT { left: s.px(TEXT_X), top: s.px((BOX - 1.0 - TEXT_H) / 2.0), right: s.px(WIDTH - SIDE - COUNT_W), bottom: 0 };
        (s.px(WIDTH), s.px(s.height()), RECT { bottom: e.top + s.px(TEXT_H), ..e }, s.tree(), s.speech())
    }) else {
        return;
    };
    unsafe {
        let _ = SetWindowPos(popup, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
        let _ = MoveWindow(edit, edit_at.left, edit_at.top, edit_at.right - edit_at.left, edit_at.bottom - edit_at.top, true);
        let _ = InvalidateRect(Some(popup), None, false);
    }
    TREE.with(|t| {
        if let Ok(mut n) = t.try_lock() {
            *n = tree;
        }
    });
    if let Some((words, queue)) = say {
        uia::notify(popup, &words, WHO, queue);
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

fn set_note(note: Note) {
    with(|s| s.note = Some(note));
    refresh();
}

/// The row of a window that went away disappears; the finder stays open.
fn drop_row(hwnd: HWND, note: Option<Note>) {
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
    let Some((h, screen)) = with(|s| {
        let &(i, _) = s.shown.get(s.selected)?;
        Some((s.rows[i].hwnd, s.screen.clone()))
    })
    .flatten() else {
        return;
    };
    let done = match how {
        Act::Go => ws::go_to(h),
        Act::Bring => ws::bring(h, &screen),
        Act::Close => return close_window(h),
    };
    match done {
        // Focus has moved to the window already, which closed the finder; this is the backstop.
        Ok(()) => {
            used(h);
            close(false);
            if matches!(how, Act::Go) {
                crate::welcome::tried(crate::welcome::Try::Find);
            }
        }
        Err(Miss::Gone) => {
            info!("finder: {} is gone", ws::who(h));
            drop_row(h, Some(Note::Gone));
        }
        Err(Miss::CantMove) => set_note(Note::CantMove),
        Err(Miss::Unplugged) => {
            info!("finder: {} waits for its unplugged screen to fold", ws::who(h));
            set_note(Note::Unplugged);
        }
    }
}

/// Ctrl+W: ask it to close as its X button would, then watch for it to go.
fn close_window(h: HWND) {
    if !unsafe { IsWindow(Some(h)) }.as_bool() {
        info!("finder: {} is gone", ws::who(h));
        return drop_row(h, Some(Note::Gone));
    }
    match unsafe { PostMessageW(Some(h), WM_CLOSE, WPARAM(0), LPARAM(0)) } {
        Ok(()) => {
            with(|s| {
                if !s.closing.contains(&h) {
                    s.closing.push(h);
                }
            });
            if let Some(popup) = WND.get() {
                unsafe { SetTimer(Some(popup), CLOSING_TIMER, CLOSING_POLL_MS, None) };
            }
            refresh();
        }
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => {
            warn!("finder: can't close {}, it runs as admin", ws::who(h));
            set_note(Note::CantClose);
        }
        Err(e) => warn!("finder: can't close {}: {e}", ws::who(h)),
    }
}

/// Rows whose window closed or hid itself (apps that close to the tray) go away.
fn closing_tick() {
    let gone = with(|s| {
        let gone: Vec<(HWND, String)> = s
            .closing
            .iter()
            .filter(|&&h| unsafe { !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() })
            .map(|&h| (h, ws::who(h)))
            .collect();
        s.closing.retain(|h| !gone.iter().any(|(g, _)| g == h));
        (gone, s.closing.is_empty())
    });
    let Some((gone, none_left)) = gone else { return };
    for (h, app) in gone {
        info!("finder: closed {app}");
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
    // The placeholder comes and goes with the text: the edit repaints only what it typed.
    unsafe { let _ = InvalidateRect(Some(edit), None, true); }
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
        let k = s.row_at(y)?;
        s.selected = k;
        s.note = None;
        Some(())
    });
    if hit.flatten().is_some() {
        refresh();
        act(Act::Go);
    }
}

fn on_hover(hwnd: HWND, y: Option<i32>) {
    let changed = with(|s| {
        let k = y.and_then(|y| s.row_at(y));
        std::mem::replace(&mut s.hover, k) != k
    });
    if changed == Some(true) {
        unsafe { let _ = InvalidateRect(Some(hwnd), None, false); }
    }
    if y.is_some() {
        let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
        unsafe { let _ = TrackMouseEvent(&mut t); }
    }
}

fn on_wheel(delta: i16) {
    with(|s| s.scroll_by(-(delta as isize) * WHEEL_ROWS / WHEEL_DELTA as isize));
    refresh();
}

/// The text box's background brush, made again when the theme changes its color.
fn edit_brush(color: u32) -> HBRUSH {
    if let Some((b, c)) = BRUSH.get()
        && c == color
    {
        return b;
    }
    let b = unsafe { CreateSolidBrush(ui::cref(color)) };
    if let Some((old, _)) = BRUSH.replace(Some((b, color))) {
        unsafe { let _ = DeleteObject(old.into()); }
    }
    b
}

/// One column of a row: `text` with the letters at `marks` in 600 and `mark` color, cut with …
/// to `w` (DT_END_ELLIPSIS works on one font only).
#[allow(clippy::too_many_arguments)]
fn column(c: &Canvas, x: f32, y: f32, w: f32, text: &str, marks: &[usize], color: u32, mark: u32) {
    let mut runs: Vec<(String, bool)> = vec![];
    for (i, ch) in text.chars().enumerate() {
        let on = marks.binary_search(&i).is_ok();
        match runs.last_mut() {
            Some((t, b)) if *b == on => t.push(ch),
            _ => runs.push((ch.to_string(), on)),
        }
    }
    let font = |on: bool| if on { F::Bold } else { F::Body };
    let total: f32 = runs.iter().map(|(t, on)| c.width(t, font(*on))).sum();
    let room = if total <= w { w } else { w - c.width("…", F::Body) };
    let mut cx = x;
    for (t, on) in &runs {
        let (f, ink) = (font(*on), if *on { mark } else { color });
        let tw = c.width(t, f);
        if cx + tw <= x + room {
            c.text(cx, y, ROW, t, f, ink);
            cx += tw;
            continue;
        }
        // The cut falls in this run: as many letters as fit, then the ….
        let chars: Vec<char> = t.chars().collect();
        let part = |n: usize| chars[..n].iter().collect::<String>();
        let (mut lo, mut hi) = (0, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if cx + c.width(&part(mid), f) <= x + room { lo = mid } else { hi = mid - 1 }
        }
        let kept = part(lo);
        let kept = kept.trim_end();
        c.text(cx, y, ROW, kept, f, ink);
        c.text(cx + c.width(kept, f), y, ROW, "…", F::Body, color);
        return;
    }
}

/// A screen's name and `then` cut to `w`: a twin's " #n" stays (AC-FND-3).
fn fit_place(c: &Canvas, name: &str, then: &str, w: f32) -> String {
    let whole = format!("{name}{then}");
    if c.width(&whole, F::Small) <= w {
        return whole;
    }
    let (base, end) = match name.rsplit_once(" #") {
        Some((base, n)) if !n.is_empty() && n.chars().all(|d| d.is_ascii_digit()) => (base, format!(" #{n}{then}")),
        _ => (name, then.to_string()),
    };
    c.fit(base, F::Small, w - c.width(&end, F::Small)) + &end
}

/// A row's workspace chip, its right edge at CHIP_RIGHT: full when the window is not hidden,
/// soft (40%) when des-k hid it, a pin when pinned, a grey outline when its screen just left.
fn chip(c: &Canvas, r: &Row, y: f32, chosen: bool, bg: u32) {
    let (b, hc) = (c.t.b, c.t.hc);
    let color = b.ws[(r.workspace.max(1) - 1).min(8)];
    let (label, f) = if r.pinned { ("\u{E718}".to_string(), F::IconSmall) } else { (r.workspace.to_string(), F::SmallBold) };
    let tw = c.width(&label, f);
    let w = (tw + 10.0).max(20.0);
    let at = (CHIP_RIGHT - w, y + (ROW - 18.0) / 2.0, w, 18.0);
    let full = r.on_screen && !r.unplugged;
    let soft = !r.on_screen && !r.unplugged;
    let (fill, edge, ink) = match (hc, chosen) {
        // High contrast: a full chip in Highlight, a soft one outlined; on the chosen row both
        // turn to HighlightText, and an unplugged one is filled like a full one.
        (true, true) if soft => (bg, Some(b.on_acc), b.on_acc),
        (true, true) => (b.on_acc, None, b.acc),
        (true, false) if full => (color, None, b.on_ws),
        (true, false) if soft => (bg, Some(b.tx), b.tx),
        (true, false) => (bg, Some(b.tx3), b.tx2),
        _ if full => (color, (b.ws_edge > 0.0).then(|| mix(color, 0, b.ws_edge)), b.on_ws),
        _ if soft => (mix(b.card, color, 0.4), Some(mix(b.card, color, 0.7)), b.tx),
        _ => (bg, Some(b.tx3), b.tx2),
    };
    c.boxed(at, 4.0, fill, edge);
    c.text(at.0 + (w - tw) / 2.0, at.1, 18.0, &label, f, ink);
}

fn row(c: &Canvas, s: &Session, r: &Row, y: f32, chosen: bool, hover: bool) {
    let (b, hc) = (c.t.b, c.t.hc);
    let closing = s.closing.contains(&r.hwnd);
    let at = (ROW_X, y, ROW_W, ROW);
    let bg = match () {
        _ if chosen && hc => b.acc,
        _ if chosen => mix(b.card, b.acc, 0.10),
        _ if hover && !hc => b.track,
        _ => b.card,
    };
    if hover && hc && !chosen {
        c.boxed(at, 6.0, bg, Some(b.acc));
    } else if bg != b.card {
        c.fill_rr(at, 6.0, op(bg));
    }
    if chosen {
        // The settings side list's bar.
        c.fill_rr((ROW_X, y + 10.0, 3.0, 16.0), 1.5, op(if hc { b.on_acc } else { b.acc }));
    }
    let ink = |color: u32| if hc && chosen { b.on_acc } else { color };
    // A window Ctrl+W was sent to fades until it goes.
    let fade = |color: u32| if closing { mix(bg, color, 0.45) } else { color };
    let icon_y = y + (ROW - ICON) / 2.0;
    c.icon(ROW_X + ROW_PAD, icon_y, ICON, r.icon);
    if closing {
        c.fill_rr((ROW_X + ROW_PAD, icon_y, ICON, ICON), 0.0, alpha(bg, 0.55));
    }
    let (pm, tm) = split_marks(r);
    column(c, PROGRAM_X, y, PROGRAM_W, &r.program, &pm, fade(ink(b.tx2)), fade(ink(b.tx)));
    column(c, TITLE_X, y, TITLE_W, &r.title, &tm, fade(ink(b.tx)), fade(ink(b.tx)));
    let place = match () {
        _ if closing => "Closing…".to_string(),
        _ if r.unplugged => fit_place(c, &r.place, " · unplugged", SCREEN_W),
        _ if s.screens > 1 => fit_place(c, &r.place, "", SCREEN_W),
        _ => String::new(),
    };
    c.text(SCREEN_RIGHT - c.width(&place, F::Small), y, ROW, &place, F::Small, ink(b.tx2));
    if !closing {
        chip(c, r, y, chosen, bg);
    }
}

/// A key line item: its key caps, then what it does. Returns where it ends.
fn key_item(c: &Canvas, x: f32, y: f32, keys: &[&str], label: &str, grey: bool) -> f32 {
    let b = c.t.b;
    let fade = |color: u32| if grey { mix(b.band, color, 0.45) } else { color };
    let ky = y + (FOOT - 20.0) / 2.0;
    let mut x = x;
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            x += 4.0;
        }
        let tw = c.width(k, F::Small);
        let w = (tw + 12.0).max(20.0);
        c.fill_rr((x, ky + 1.0, w, 20.0), 4.0, op(fade(b.key_edge))); // its 1 px shadow below
        c.boxed((x, ky, w, 20.0), 4.0, fade(b.key), Some(fade(b.key_edge)));
        c.text(x + (w - tw) / 2.0, ky, 20.0, k, F::Small, fade(b.tx));
        x += w;
    }
    x += 8.0;
    c.text(x, y, FOOT, label, F::Small, fade(b.tx2));
    x + c.width(label, F::Small)
}

/// The whole finder at 0, 0. `typed` paints the search text and caret as the EDIT would, for
/// the render tests; on screen the EDIT draws them itself.
fn draw(c: &Canvas, s: &Session, typed: Option<&str>) {
    let (b, hc) = (c.t.b, c.t.hc);
    let height = s.height();
    c.fill_rr((0.0, 0.0, WIDTH, height), 0.0, op(b.card));
    // The search box.
    c.text(SIDE, 0.0, BOX - 1.0, "\u{E721}", F::Icon, b.tx2);
    if let Some(n) = s.count() {
        c.text(WIDTH - SIDE - c.width(&n, F::Small), 0.0, BOX - 1.0, &n, F::Small, b.tx2);
    }
    if let Some(q) = typed {
        let y = (BOX - 1.0 - TEXT_H) / 2.0;
        let caret = if q.is_empty() {
            c.text(TEXT_X + 1.0, y, TEXT_H, PLACEHOLDER, F::Body, b.tx2);
            TEXT_X
        } else {
            c.text(TEXT_X, y, TEXT_H, q, F::Body, b.tx);
            TEXT_X + c.width(q, F::Body) + 1.0
        };
        c.fill_rr((caret, y, 1.0, TEXT_H), 0.0, op(b.tx));
    }
    c.rule(0.0, BOX - 1.0, WIDTH, op(b.line));
    // The list.
    let top = BOX + LIST_PAD;
    match s.empty_text() {
        Some("No match") => {
            c.text(ROW_X + 40.0, top + 8.0, 20.0, "No match", F::Body, b.tx2);
            c.text(ROW_X + 40.0, top + 28.0, 16.0, "Windows on Windows' own desktops and paused Store apps aren't listed.", F::Small, b.tx2);
        }
        Some(e) => c.text(ROW_X + 40.0, top, ROW, e, F::Body, b.tx2),
        None => {}
    }
    for (k, &(i, _)) in s.shown.iter().enumerate().skip(s.scroll).take(ROWS) {
        row(c, s, &s.rows[i], top + (k - s.scroll) as f32 * ROW, k == s.selected, s.hover == Some(k));
    }
    let n = s.shown.len();
    if n > ROWS {
        let track = ROWS as f32 * ROW - 12.0;
        let h = track * ROWS as f32 / n as f32;
        let y = top + (track - h) * s.scroll as f32 / (n - ROWS) as f32;
        c.fill_rr((WIDTH - 6.0, y, 3.0, h), 1.5, op(b.tx3));
    }
    // The note.
    if let Some(note) = s.note {
        let (icon, line, under, warn) = note.words(&s.main);
        let y = BOX + s.list_h();
        c.boxed((8.0, y, WIDTH - 16.0, s.note_h() - 8.0), 6.0, b.bg, Some(b.line));
        c.text(20.0, y + 8.0, 20.0, icon, F::Icon, if warn { b.warn } else { b.tx2 });
        c.text(48.0, y + 8.0, 20.0, line, F::Bold, b.tx);
        if let Some(u) = under {
            c.text(48.0, y + 28.0, 16.0, &c.fit(&u, F::Small, WIDTH - 16.0 - 52.0), F::Small, b.tx2);
        }
    }
    // The key line; with no rows only Esc does anything.
    let y = height - FOOT;
    c.fill_rr((0.0, y, WIDTH, FOOT), 0.0, op(if hc { b.card } else { b.band }));
    c.rule(0.0, y, WIDTH, op(b.line));
    let none = s.shown.is_empty();
    let mut x = SIDE;
    for (keys, label) in [(&["Enter"][..], "Go to"), (&["Shift", "Enter"][..], "Bring here"), (&["Ctrl", "W"][..], "Close window")] {
        x = key_item(c, x, y, keys, label, none) + 20.0;
    }
    let esc = key_item(&c.measuring(), 0.0, y, &["Esc"], "Cancel", false);
    key_item(c, WIDTH - SIDE - esc, y, &["Esc"], "Cancel", false);
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        // Drawn off screen first: the list repaints on every key, and would flicker otherwise.
        ui::buffered(hdc, 0, 0, rc.right, rc.bottom, |mem| {
            SESSION.with_borrow(|s| {
                if let Some(s) = s {
                    let fonts = ui::fonts(s.scale);
                    draw(&Canvas::new(mem, true, s.scale, ui::theme(), &fonts), s, None);
                }
            });
        });
        let _ = EndPaint(hwnd, &ps);
    }
}

/// "Find a window" in grey while the text box is empty: an EDIT without common controls 6 has
/// no placeholder of its own.
unsafe fn placeholder(edit: HWND) {
    unsafe {
        let dc = GetDC(Some(edit));
        let font = HFONT(SendMessageW(edit, WM_GETFONT, None, None).0 as *mut _);
        let old = SelectObject(dc, font.into());
        SetTextColor(dc, ui::cref(ui::theme().b.tx2));
        SetBkMode(dc, TRANSPARENT);
        let left = (SendMessageW(edit, EM_GETMARGINS, None, None).0 & 0xFFFF) as i32;
        let one = (GetDpiForWindow(edit) as i32 + 95) / 96; // after the caret
        let wide: Vec<u16> = PLACEHOLDER.encode_utf16().collect();
        let _ = HideCaret(Some(edit));
        let _ = TextOutW(dc, left + one, 0, &wide);
        let _ = ShowCaret(Some(edit));
        SelectObject(dc, old);
        ReleaseDC(Some(edit), dc);
    }
}

unsafe extern "system" fn edit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        let own: WNDPROC = std::mem::transmute(EDIT_PROC.get());
        // The wheel scrolls the list, whichever of the two the mouse is over.
        if msg == WM_MOUSEWHEEL
            && let Some(popup) = WND.get()
        {
            return SendMessageW(popup, msg, Some(wp), Some(lp));
        }
        let r = CallWindowProcW(own, hwnd, msg, wp, lp);
        if msg == WM_PAINT && GetWindowTextLengthW(hwnd) == 0 {
            placeholder(hwnd);
        }
        r
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
                let (dc, b) = (HDC(wp.0 as *mut _), ui::theme().b);
                SetTextColor(dc, ui::cref(b.tx));
                SetBkColor(dc, ui::cref(b.card));
                LRESULT(edit_brush(b.card).0 as isize)
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
            WM_MOUSEMOVE => {
                on_hover(hwnd, Some((lp.0 >> 16) as i16 as i32));
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                on_hover(hwnd, None);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                on_wheel((wp.0 >> 16) as i16);
                LRESULT(0)
            }
            WM_TIMER if wp.0 == CLOSING_TIMER => {
                closing_tick();
                LRESULT(0)
            }
            // Windows' mode or contrast changed (or main's 1 s check saw it): repaint in it.
            WM_SETTINGCHANGE | WM_SYSCOLORCHANGE => {
                ui::refresh();
                ui::border(hwnd, ui::theme().b.edge);
                let _ = InvalidateRect(Some(hwnd), None, false);
                if let Some(edit) = EDIT.get() {
                    let _ = InvalidateRect(Some(edit), None, true);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_GETOBJECT => TREE.with(|t| uia::answer_tree(hwnd, wp, lp, t)),
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
        // It must take focus, so no WS_EX_NOACTIVATE; and it holds an EDIT, so never layered.
        let popup = ui::popup(CLASS, Some(wndproc), WS_EX_TOPMOST, false)?;
        let _ = SetWindowTextW(popup, &HSTRING::from(PLACEHOLDER)); // what Narrator calls the window
        let edit = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), WS_CHILD | WS_VISIBLE | WINDOW_STYLE(ES_AUTOHSCROLL as u32), 0, 0, 0, 0, Some(popup), None, Some(inst.into()), None).ok()?;
        EDIT_PROC.set(SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as isize));
        uia::name_control(edit, PLACEHOLDER);
        WND.set(Some(popup));
        EDIT.set(Some(edit));
        Some((popup, edit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{DARK, Fonts, LIGHT, Theme};
    use std::time::Duration;

    fn row(on_screen: bool, screen: usize, workspace: usize, used: Option<Instant>) -> Row {
        Row {
            hwnd: HWND::default(),
            title: String::new(),
            program: String::new(),
            icon: HICON::default(),
            screen,
            place: String::new(),
            workspace,
            on_screen,
            pinned: false,
            unplugged: false,
            used,
            rank: 0,
            gone: false,
            marks: vec![],
        }
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

    fn named(program: &str, title: &str, rank: usize) -> Row {
        Row { program: program.into(), title: title.into(), rank, ..row(true, 1, 1, None) }
    }

    fn session(rows: Vec<Row>) -> Session {
        Session::new(rows, HWND::default(), String::new(), 2, "Built-in display".into(), 1.0)
    }

    fn filter(s: &mut Session, q: &str) {
        s.query = q.into();
        s.refilter(false);
    }

    // AC-5: each typed word must appear, letters in order, any case, in "program title"; the
    // best match is selected, and an empty result says why.
    #[test]
    fn refilter_matches_every_word_fuzzily_and_selects_the_best() {
        let mut s = session(vec![named("explorer", "Downloads", 0), named("Code", "finder.rs - pdesk - Visual Studio Code", 1), named("chrome", "pdesk issues", 2)]);
        let firsts = |s: &mut Session, q: &str| {
            filter(s, q);
            (s.shown.iter().map(|&(i, _)| i).collect::<Vec<_>>(), s.selected, s.empty_text())
        };
        assert_eq!(firsts(&mut s, "vsc"), (vec![1], 0, None));
        assert_eq!(firsts(&mut s, "PDESK visual").0, [1]);
        assert_eq!(firsts(&mut s, "pdesk").0.len(), 2);
        assert_eq!(firsts(&mut s, "zzzq"), (vec![], 0, Some("No match")));
        assert_eq!(s.count().as_deref(), Some("0 of 3"));
        s.rows.iter_mut().for_each(|r| r.gone = true);
        assert_eq!(firsts(&mut s, "").2, Some("No windows"));
        assert_eq!(s.count(), None);
    }

    // AC-FND-6: the bold letters must be the ones that matched, each in the column it sits in;
    // an index off by one would bold the letter next to it, or one in the other column.
    #[test]
    fn matched_letters_split_into_the_program_and_title_columns() {
        let mut s = session(vec![named("Google Chrome", "des-k issues", 0), named("Code", "notes", 1)]);
        filter(&mut s, "chrome");
        let (p, t) = split_marks(&s.rows[0]);
        assert_eq!((p, t), (vec![7, 8, 9, 10, 11, 12], vec![]));
        filter(&mut s, "code des");
        let (p, t) = split_marks(&s.rows[0]);
        let pick = |text: &str, at: &[usize]| at.iter().map(|&i| text.chars().nth(i).unwrap()).collect::<String>();
        assert_eq!(pick("Google Chrome", &p).to_lowercase().chars().filter(|c| "code".contains(*c)).count(), p.len());
        assert_eq!(pick("des-k issues", &t), "des");
        // A row the query no longer matches keeps no bold letters.
        filter(&mut s, "");
        assert!(s.rows.iter().all(|r| r.marks.is_empty()));
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

    // AC-FND-9: Narrator hears the same words the row paints, plus where the window is; and it
    // hears each change once, not again on every repaint.
    #[test]
    fn narrator_reads_the_chosen_row_once_per_change() {
        let mut rows = vec![named("Google Chrome", "des-k issues", 0), named("Notepad", "notes.txt", 1)];
        rows[0].place = "DELL U2720Q".into();
        rows[0].hwnd = HWND(10 as *mut _);
        rows[1].hwnd = HWND(11 as *mut _);
        rows[1].on_screen = false;
        rows[1].workspace = 3;
        rows[1].place = "Built-in display".into();
        let mut s = session(rows);
        assert_eq!(s.speech(), Some(("2 windows, notes.txt, Notepad, Built-in display, workspace 3, hidden".into(), true)));
        assert_eq!(s.speech(), None);
        s.selected = 0;
        assert_eq!(s.speech(), Some(("des-k issues, Google Chrome, DELL U2720Q, workspace 1, shown".into(), false)));
        s.note = Some(Note::Unplugged);
        assert_eq!(s.speech().unwrap().0, "Its screen was just unplugged. In a moment it moves to Built-in display; try again then.");
        let tree = s.tree();
        let list = tree.kids.iter().find(|n| n.role == UIA_ListControlTypeId).unwrap();
        assert_eq!(list.kids.iter().map(|k| k.selected).collect::<Vec<_>>(), [Some(true), Some(false)]);
    }

    /// A window list like the design's: [icon, program, title, screen, workspace, flag].
    fn design_rows(set: &str) -> Vec<Row> {
        const DELL: &str = "DELL U2720Q";
        const BUILT: &str = "Built-in display";
        let all: Vec<(&str, &str, &str, usize, &str)> = match set {
            "long" => vec![
                ("Visual Studio Code", "settings_page.rs - des-k - a very long workspace folder name that goes on - Visual Studio Code", "LG ULTRAGEAR 27GR95QE OLED Gaming Monitor #1", 9, ""),
                ("Microsoft Edge WebView2", "A web page title that is far too long to fit in the row at all", "LG ULTRAGEAR 27GR95QE OLED Gaming Monitor #2", 7, "shown"),
                ("Windows Terminal", "Administrator: PowerShell", BUILT, 6, ""),
            ],
            _ => vec![
                ("Visual Studio Code", "finder.rs - des-k - Visual Studio Code", DELL, 2, "shown"),
                ("Google Chrome", "des-k issues · GitHub - Google Chrome", DELL, 3, if set == "closing" { "closing" } else if set == "away" { "away" } else { "" }),
                ("File Explorer", "Downloads", BUILT, 1, "shown"),
                ("Windows Terminal", "PowerShell", DELL, 1, ""),
                ("Notepad", "notes.txt - Notepad", BUILT, 3, if set == "away" { "away" } else { "" }),
                ("Microsoft Teams", "Chat | Microsoft Teams", BUILT, 1, "pin"),
                ("Outlook", "Inbox - Outlook", DELL, 4, ""),
                ("Settings", "Display", BUILT, 2, ""),
                ("Paint", "Untitled - Paint", DELL, 2, "shown"),
                ("Google Chrome", "Pull requests · des-k - Google Chrome", BUILT, 4, ""),
                ("Spotify", "Spotify Premium", BUILT, 2, ""),
                ("File Explorer", "des-k", DELL, 4, ""),
            ],
        };
        let app = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
        all.into_iter()
            .enumerate()
            .map(|(i, (program, title, place, ws, flag))| Row {
                hwnd: HWND((i + 1) as *mut _),
                place: if flag == "away" { "LG HDR 4K".into() } else { place.into() },
                on_screen: matches!(flag, "shown" | "pin"),
                pinned: flag == "pin",
                unplugged: flag == "away",
                icon: app,
                workspace: ws,
                ..named(program, title, i)
            })
            .collect()
    }

    /// Draws every state the design shows to target/finder/ PNGs, to compare by eye with
    /// pdesk-design/restart/captures/fdr3-* (spec 0017 AC-1, AC-FND-1 to AC-FND-7).
    #[test]
    #[ignore]
    fn render() {
        ui::gdiplus();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target").join("finder");
        std::fs::create_dir_all(&dir).unwrap();
        type Setup = fn(&mut Session);
        let states: Vec<(&str, &str, &str, Setup)> = vec![
            ("opened", "all", "", |_| {}),
            ("chrome", "all", "chrome", |_| {}),
            ("two-words", "all", "code des", |_| {}),
            ("no-match", "all", "zzzq", |_| {}),
            ("no-windows", "none", "", |_| {}),
            ("scrolled", "all", "", |s| {
                s.selected = 11;
                s.scroll_to_selection();
            }),
            ("one-screen", "all", "", |s| s.screens = 1),
            ("hover", "all", "", |s| s.hover = Some(4)),
            ("long", "long", "", |s| s.selected = 0),
            ("closing", "closing", "", |s| s.closing.push(HWND(2 as *mut _))),
            ("closed", "all", "", |s| {
                s.rows[1].gone = true;
                s.refilter(true);
            }),
            ("cant-move", "long", "", |s| {
                s.selected = 2;
                s.note = Some(Note::CantMove);
            }),
            ("cant-close", "long", "", |s| {
                s.selected = 2;
                s.note = Some(Note::CantClose);
            }),
            ("gone", "all", "", |s| {
                s.rows[1].gone = true;
                s.refilter(true);
                s.note = Some(Note::Gone);
            }),
            ("unplugged", "away", "", |_| {}),
            ("unplugged-note", "away", "", |s| s.note = Some(Note::Unplugged)),
        ];
        let aquatic = ui::high_contrast([0x202020, 0xFFFFFF, 0x8EE3F0, 0x263B50, 0xA6A6A6]);
        let desert = ui::high_contrast([0xFFFAEF, 0x3D3D3D, 0x903909, 0xFFF5E3, 0x6D6D6D]);
        let themes: [(&str, Theme, f32); 5] = [("light", LIGHT, 1.0), ("dark", DARK, 1.0), ("hc", aquatic, 1.0), ("desert", desert, 1.0), ("light-150", LIGHT, 1.5)];
        for (tname, t, scale) in themes {
            let fonts = Fonts::new(scale);
            for (name, set, query, setup) in &states {
                let rows = if *set == "none" { vec![] } else { design_rows(set) };
                let mut s = Session::new(rows, HWND::default(), String::new(), 2, "Built-in display".into(), scale);
                filter(&mut s, query);
                setup(&mut s);
                let (w, h) = (s.px(WIDTH), s.px(s.height()));
                let path = dir.join(format!("{name}-{tname}.png"));
                ui::tests::save_png(w, h, &path, |dc| {
                    let c = Canvas::new(dc, true, scale, t, &fonts);
                    draw(&c, &s, Some(query));
                    // Windows' 1 px edge, as ui::border asks for it.
                    c.stroke_rr((0.5 / scale, 0.5 / scale, WIDTH - 1.0 / scale, s.height() - 1.0 / scale), 8.0, 1.0 / scale, op(t.b.edge));
                });
            }
        }
    }
}
