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
        Packaging::Appx::{FindPackagesByPackageFamily, GetPackageFullName, PACKAGE_FILTER_HEAD},
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
use crate::ui::{self, canvas::Canvas, color::{alpha, mix, op}, fonts::F};
use crate::uia::{self, Node, Tree};
use crate::ws::{self, Miss};

mod draw;
pub(crate) mod names;
mod rank;

use draw::paint;
use names::{icon, program};
use rank::{first_pick, rank, recency_rank};

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
    ws_name: String, // its workspace's name on its screen; empty for none and when pinned (spec 0023)
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
        let (scores, by_name): (Vec<Option<u32>>, Vec<bool>) = if self.query.trim().is_empty() {
            self.rows.iter_mut().map(|r| {
                r.marks.clear();
                ((!r.gone).then_some(0), false)
            }).unzip()
        } else {
            let pattern = Pattern::new(&self.query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy);
            let (mut buf, mut marks) = (Vec::new(), Vec::new());
            let matcher = &mut self.matcher;
            self.rows
                .iter_mut()
                .map(|r| {
                    r.marks.clear();
                    if r.gone {
                        return (None, false);
                    }
                    marks.clear();
                    let Some(score) = pattern.indices(Utf32Str::new(&format!("{} {}", r.program, r.title), &mut buf), matcher, &mut marks) else {
                        // Spec 0023 AC-9: its workspace's name, with no letters marked.
                        let named = !r.pinned && !r.ws_name.is_empty() && pattern.score(Utf32Str::new(&r.ws_name, &mut buf), matcher).is_some();
                        return (None, named);
                    };
                    marks.sort_unstable();
                    marks.dedup();
                    r.marks.clone_from(&marks);
                    (Some(score), false)
                })
                .unzip()
        };
        let old = self.selected;
        self.shown = rank(&self.rows, &scores, &by_name);
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
        // The place column's words in its order: the name, then the screen (spec 0023 AC-8).
        if !r.ws_name.is_empty() {
            w += &format!(", {}", r.ws_name);
        }
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
                ws_name: p.ws_name,
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

/// Win+F: open on the screen `ws::target` picks for `from` (spec 0038), or close if open.
pub fn toggle(from: ws::Source) {
    if SESSION.with_borrow(Option::is_some) {
        close(true);
    } else {
        open(from);
    }
}

fn open(from: ws::Source) {
    let Some((popup, edit)) = window() else { return warn!("finder: window not created") };
    ui::canvas::gdiplus();
    let (mon, device) = ws::target(from);
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
        SendMessageW(edit, WM_SETFONT, Some(WPARAM(ui::fonts::fonts(scale).get(F::Body).0 as usize)), Some(LPARAM(0)));
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
pub(crate) fn edit_brush(color: u32) -> HBRUSH {
    if let Some((b, c)) = BRUSH.get()
        && c == color
    {
        return b;
    }
    let b = unsafe { CreateSolidBrush(ui::color::cref(color)) };
    if let Some((old, _)) = BRUSH.replace(Some((b, color))) {
        unsafe { let _ = DeleteObject(old.into()); }
    }
    b
}

/// "Find a window" in grey while the text box is empty: an EDIT without common controls 6 has
/// no placeholder of its own.
unsafe fn placeholder(edit: HWND) {
    unsafe {
        let dc = GetDC(Some(edit));
        let font = HFONT(SendMessageW(edit, WM_GETFONT, None, None).0 as *mut _);
        let old = SelectObject(dc, font.into());
        SetTextColor(dc, ui::color::cref(ui::theme().b.tx2));
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
                SetTextColor(dc, ui::color::cref(b.tx));
                SetBkColor(dc, ui::color::cref(b.card));
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
mod tests;
