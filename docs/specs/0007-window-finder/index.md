# 0007. Window finder: a search popup that lands you on any window, on any screen and workspace

**Date**: 2026-10-28
**Status**: Accepted
**Amended by**: [0017](../0017-look-b-screens/0017-finder.md): AC-13 (look) and the `S · W` part of AC-3.

## Summary

Win+F opens a dark search popup on the screen under the mouse. It lists every window on every screen and workspace, hidden ones included, with its icon, program name, title and where it lives. Type a few letters (fuzzy, so "vsc" finds Visual Studio Code), press Enter, and that window's screen switches to its workspace with the window focused. Shift+Enter brings the window to you instead, and Ctrl+W closes it. des-k builds this itself because it is the only thing that knows where its hidden windows are.

## Requirements

**User stories**:
- As the des-k user, I want to find a window by typing part of its title or app name so that I don't have to step through workspaces on three screens to look for it.
- As the des-k user, I want Enter to take me to the window wherever it is, so that the right screen shows the right workspace and the window has focus.
- As the des-k user, I want to pull a window over to where I am, or close it, without going there first.
- As the des-k user, I want Win+F then Enter to flip me back to the window I was in before, like Alt+Tab.

**Acceptance criteria** ("listed windows" means what `ws::app_windows` accepts today: visible, no owner, not a tool window, has a title, not the desktop or a taskbar; "S · W" is screen number S counted from the left, workspace W, both from 1):
- **AC-1**: Win+F (the `finder` setting) opens the finder on the screen under the mouse, centered across, its top at about a fifth of the way down the work area, with keyboard focus in its text box. Win+F is swallowed: neither Feedback Hub nor Start opens. If Windows refuses des-k focus, the finder closes again and the log says so.
- **AC-2**: The finder lists every listed window on every screen: those on screen now and those des-k hid on any workspace, minimized ones included. Windows on Windows' own other virtual desktops, windows the shell cloaked (suspended Store apps), and des-k's own windows are not listed. The list is captured when the finder opens.
- **AC-3**: Each row shows the app's small icon (a generic icon when there is none; a frozen app never delays the finder by more than 50 ms), the program name (the exe file name without `.exe`; for a Store app, the hosted app's exe, else `ApplicationFrameHost`), the title (cut with … when too long), and `S · W`, plus a ● when the window is on screen now.
- **AC-4**: With nothing typed, rows are in recently used order: the latest time each window, or a dialog it owns, came to the front since des-k started, with the window you came from counted as most recent. Recency is kept whether or not `follow_activation` is on, in memory only. Windows not used since start come after, those on screen first in front to back order, then hidden ones by screen, workspace and saved order. The second row is selected (the first when there is only one).
- **AC-5**: Each change to the text filters the list at once. Matching is fuzzy and ignores case: each space separated word must have its letters appear in order somewhere in "program title". Rows sort by match score (letters at word starts and in a row score higher), equal scores in the AC-4 order, and the best match is selected. When nothing matches, the list says "No match"; when there are no windows at all, "No windows".
- **AC-6**: Up/Down, Shift+Tab/Tab and Ctrl+P/Ctrl+N move the selection one row and stop at the ends. At most 9 rows show; the list scrolls to keep the selection visible. The text box supports a caret, Left/Right, Home/End, selecting text, Ctrl+V paste, IME input, and Ctrl+Backspace deleting the word before the caret. A left click on a row picks it like Enter.
- **AC-7**: Enter on a hidden window switches that window's screen straight to its workspace, exactly like a switch (old windows hidden, the indicator shows there), with the window on top, focused, and restored if it was minimized. Other screens keep their workspace, the mouse does not move, the finder closes, and `state.txt` records the switch.
- **AC-8**: Enter on a window on screen now gives it focus (restored if minimized); no screen switches, the finder closes.
- **AC-9**: Shift+Enter brings the window to the screen the finder opened on, onto the workspace that screen shows, focused, and the finder closes; no screen switches and `state.txt` records the change. A window already there acts as AC-8. A window hidden on that same screen leaves its workspace and appears at its own place and size. A window from another screen (on screen there or hidden) lands at the same relative place and size, scaled to the new screen's work area: maximized stays maximized, minimized is restored, a hidden one never flashes on its old screen. If the move fails (for example an admin window), the window stays exactly where it was, still hidden if it was, the finder stays open with the note "Can't move this window", and the log says why.
- **AC-10**: Ctrl+W asks the selected window to close, as its X button would. The finder stays open; once the window is gone or has hidden itself (apps that close to the tray), its row disappears and the next row is selected. An app that asks to save takes focus, which closes the finder. For an admin window the row stays, with the note "Can't close an admin window" and a log line.
- **AC-11**: Esc, or Win+F again, closes the finder and gives focus back to the window you came from (the desktop if it is gone). A click outside the finder closes it and focus stays where you clicked. Any des-k switch, jump, send, or screen change while the finder is open closes it first (focus back), then runs as usual.
- **AC-12**: A listed window that closed after the finder opened: Enter, Shift+Enter, Ctrl+W or a click on it drops its row with the note "That window is gone", and the finder stays open.
- **AC-13**: The finder looks like the indicator: dark grey, rounded corners, light text, the program name and `S · W` dimmer, the selected row in the indicator's accent blue. It is sharp at every screen's scaling, never appears in Alt+Tab or the taskbar, and des-k never hides it.
- **AC-14**: `settings.toml` `[shortcuts]` takes `finder`, a list like `previous`, default `["Win+F"]`, with the spec 0001 rules (a bad entry is dropped with a log line, nothing valid left means the default, a missing key means the default). A combo that is also in a switch or send list turns `finder` off with a log line; the other list keeps it. A `finder` combo on a digit keeps that digit from the digit sets, as spec 0005 logs today.

## Decision

**Chosen option**: Option 1: des-k's own finder popup, fed by des-k's state.

A new `src/finder.rs` owns the popup (a borderless EDIT control inside a frame and list des-k paints), the row snapshot, fuzzy ranking with `nucleo-matcher`, and recency; landing on a window goes through new `ws` functions built on `switch_to`.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Feature design

**Data model sketch**: nothing new in `state.txt`. All finder data lives in memory on the main thread.

| Piece | Fields | Owner | Life |
|---|---|---|---|
| Recency | `HashMap<isize, Instant>`: root owner window (`GA_ROOTOWNER`) → last time it came to the front | `finder.rs`, `thread_local` | from start; dead windows pruned at each open; lost on restart |
| `Row` | `hwnd`, `title: String`, `program: String`, `icon: HICON` (borrowed, never destroyed), `screen: usize`, `workspace: usize` (both 1 based), `on_screen: bool`, `used: Option<Instant>`, `rank: usize` (AC-4 position) | `finder.rs` | snapshot at open, dropped at close |
| Session | `rows: Vec<Row>`, `shown: Vec<(usize, u32)>` (row index, score), `selected`, `scroll`, `came_from: HWND`, `screen: String` (device the finder opened on, the Shift+Enter target), `note: Option<String>`, `closing: Vec<HWND>` (Ctrl+W sent) | `finder.rs` | one per open |
| `ws::Place` | `hwnd`, `device: String`, `workspace: usize` (0 based), `on_screen: bool` | `ws.rs` | returned by `ws::windows()` |
| Setting | `Settings.finder: Vec<Combo>`, default `[Win+F]` | `settings.rs` | read once at start |

`DEFAULT_FILE` gains, under `[shortcuts]` after the send lists:

```toml
# Search every window on every screen and workspace; Enter goes there.
finder = ["Win+F"]
```

**State transitions**: finder closed → open (Win+F) → closed (Enter, Shift+Enter, Esc, Win+F, click outside, focus lost, any des-k switch, jump, send or screen change). Ctrl+W, Shift+Enter failures and gone windows keep it open.

**API surface** (module functions and messages; no network or CLI surface):

| Surface | Kind | Key inputs | Key outputs | Thread | Key errors |
|---|---|---|---|---|---|
| `keys::shortcut` | existing, extended | key event | `settings.finder` joins the `all` lists, posting `WM_FINDER` on key down, swallowed with `VK_MASK` as today | hook | none |
| `WM_FINDER` | `WM_APP + 6`, thread message | none | main loop calls `finder::toggle()` | main | none |
| main loop `WM_FOLLOW` arm | existing, changed | hwnd | `finder::used(hwnd)`, then `ws::follow(hwnd)` only if `follow_activation` | main | none |
| main loop `WM_APP`, `WM_JUMP`, `WM_SEND`, `WM_SEND_TO`, `WM_DISPLAY_CHANGED` arms | existing, changed | as today | `finder::close(true)` first | main | none |
| main loop default arm | existing, changed | `MSG` | `finder::key(&msg)` first; if not handled, `TranslateMessage` then `DispatchMessageW` | main | none |
| foreground hook in `main` | existing, changed | none | installed always (not only with `follow_activation`) | main | not installed: logged, recency only counts finder picks |
| `finder::toggle()` | new | none | opens (snapshot, place, focus) or closes with focus back | main | focus refused: close, log |
| `finder::close(give_back: bool)` | new | whether to focus `came_from` | popup hidden, session dropped | main | none |
| `finder::key(&MSG) -> bool` | new | `WM_KEYDOWN`/`WM_SYSKEYDOWN` to the EDIT | handles Up, Down, Tab, Shift+Tab, Ctrl+N, Ctrl+P, Enter, Shift+Enter, Ctrl+W, Esc, Ctrl+Backspace; true = swallowed (no `WM_CHAR` made) | main | none |
| `finder::used(hwnd)` | new | foreground window | recency of its root owner set to now | main | none |
| `ws::windows() -> Vec<Place>` | new | none | every listed window on screen (front to back) and every live window in the hidden lists (screen, workspace, saved order) | main | none |
| `ws::go_to(hwnd) -> Result<(), Miss>` | new | window | hidden: `switch_to(mon, ws, Some(hwnd), None)`; on screen: `SetForegroundWindow`; then `SW_RESTORE` if `IsIconic` | main | `Miss::Gone`; shell unavailable: logged by `switch_to` |
| `ws::bring(hwnd, device) -> Result<(), Miss>` | new | window, target screen | per AC-9: removes it from any hidden list, moves it if on another screen (while still cloaked when hidden), uncloaks, focuses, saves | main | `Miss::Gone`, `Miss::CantMove` |

Popup details (`finder.rs`):
- Window class `des-k-finder`, `WS_POPUP`, ex style `WS_EX_TOPMOST | WS_EX_TOOLWINDOW` (no `WS_EX_NOACTIVATE`: it must take focus), rounded corners through DWM like the indicator. Child: single line EDIT, no border, dark via `WM_CTLCOLOREDIT`, font set with `WM_SETFONT`; `EN_CHANGE` reaches the popup as `WM_COMMAND` and refilters.
- Sizes at 96 dpi, times the screen's scale (`GetDpiForMonitor`, as the indicator): width 640, text box 44 high, rows 36 high, 9 rows at most, a 24 high note line when there is a note. Font Segoe UI Variable Text, falling back to Segoe UI, 15 px. Top edge at `rcWork.top + 20%` of the work area height. Colors from the indicator: background `0x202020`, text `0xF0F0F0`, dim text `0x909090`, accent `ACCENT`. Exact pixels may be tuned by eye during the build.
- Row layout: icon (16 at 96 dpi, `DrawIconEx`), program (a fixed dim column, cut with …), title (cut with …), `S · W` and ● at the right.
- Open order: snapshot, compute rows, `SetWindowPos` + show, `SetForegroundWindow(popup)`, `SetFocus(edit)`; if `GetForegroundWindow()` is not the popup, `close(false)` and log `finder: Windows refused focus`.
- `WM_ACTIVATE` with `WA_INACTIVE`: `close(false)`.
- Ctrl+W: `PostMessageW(hwnd, WM_CLOSE)`; failure with `ERROR_ACCESS_DENIED` gives the admin note. A 200 ms popup timer drops rows in `closing` whose window is gone (`!IsWindow`) or no longer visible (`!IsWindowVisible`), and stops when `closing` is empty.
- Matching: one `nucleo_matcher::Matcher` (default config) kept for the session; a `Pattern` built with `Pattern::new(query, CaseMatching::Ignore, Normalization::Smart, AtomKind::Fuzzy)` (no fzf operators); each row scored on `"{program} {title}"` with `Pattern::score`. Check the exact calls against the crate's 0.3.1 docs.

`ws::bring(hwnd, device)` steps:
1. `!IsWindow`: `Err(Gone)`.
2. Find where it is now: a hidden list (screen, workspace) or on screen (`MonitorFromWindow`).
3. On the target screen and on screen: focus, restore if minimized, done.
4. Hidden on the target screen: remove from its list, uncloak, focus, save.
5. From another screen: read `GetWindowPlacement`; map its normal rect from the old screen's work area to the target's, in proportion (`map_rect`). The placement's normal rect is in workspace coordinates, so convert to and from screen coordinates with the primary screen's work area offset. Set the placement with the mapped rect, keeping `SW_SHOWMAXIMIZED` if maximized, `SW_SHOWNORMAL` if minimized or normal. If not maximized, read the window rect back and `SetWindowPos` once more to the mapped rect when the app resized itself on the scaling change. A hidden window is moved while still cloaked, then uncloaked. If setting the placement fails: `Err(CantMove)`, and nothing about the window or `ws` state has changed.
6. Remove it from the hidden list it was in (if any), focus, save.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| hook | is it the finder | `settings::get().finder` in the `all` lists, compared with `Combo { mods_held(), vk }` |
| open | screen to open on | `GetCursorPos` + `MonitorFromPoint`, as `ws::switch` |
| open | popup place and size | that screen's `rcWork` and `GetDpiForMonitor` scale, with the sizes above |
| open | window you came from | `GetForegroundWindow()` at open, root owner |
| row | which windows | `ws::windows()` |
| row | title | `ws::title(hwnd)` |
| row | program | `GetWindowThreadProcessId` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW`, file stem; for `ApplicationFrameHost`, the process of its `Windows.UI.Core.CoreWindow` child when one exists |
| row | icon | `SendMessageTimeoutW(WM_GETICON, ICON_SMALL2, then ICON_SMALL, then ICON_BIG; SMTO_ABORTIFHUNG, 50 ms)`, then `GetClassLongPtrW(GCLP_HICONSM, then GCLP_HICON)`, then `LoadIconW(None, IDI_APPLICATION)` |
| row | screen number | present screens (`EnumDisplayMonitors`) sorted by `rcMonitor.left`, then `.top`; 1 based position of the window's device |
| row | workspace number | `Place.workspace + 1`: the hidden list's index, or the screen's `Monitor.active` for windows on screen |
| row | ● | `Place.on_screen` |
| order | recency | the recency map (fed by `WM_FOLLOW` and by every finder pick), the window you came from set to now at open |
| order | tie order for unused windows | `ws::windows()` order |
| match | score | `Pattern::score` on `"{program} {title}"`, `None` = no match |
| select | first pick | row 2 when the query is empty and there are 2 or more rows, else row 1 |
| Shift+Enter | target screen and workspace | the session's `screen` device and that screen's `Monitor.active` |
| Shift+Enter | new rect | `map_rect(old rcWork, new rcWork, normal rect)` |
| Ctrl+W | admin refusal | `PostMessageW` failing with `ERROR_ACCESS_DENIED` |
| notes | text | fixed: "No match", "No windows", "That window is gone", "Can't move this window", "Can't close an admin window" |
| log | lines | `finder: opened on <device>, <n> windows`; `finder: -> "<title>" (<device> workspace <N>)`; `finder: brought "<title>" to <device>`; `finder: closed "<title>"`; `finder: <reason>` for each failure |

**Key invariants**:
- The finder never cloaks or uncloaks anything itself; every change of where a window is goes through `ws` on the main thread, and `ws` still only uncloaks windows it hid.
- Every action looks up where the window is now (restore may have moved it since the snapshot), never trusts the row's place.
- Focus moves to the target before the finder hides, so Windows never picks a window to activate in between.
- The finder and its EDIT are never in `ws` lists (tool window style, which `app_windows` skips) and never in recency (the hook skips des-k's own process).
- Icons from `WM_GETICON` and the class belong to the app and are never destroyed.
- At most one finder session; the hook only posts `WM_FINDER`.
- No `WM_APP` range message is ever sent to the finder window: the main loop matches `WM_APP + 1` whatever the target window is.
- A failed Shift+Enter leaves the window and `ws` state exactly as before.

**Security model**: no new data leaves the machine and nothing new is written except log lines, which already carry window titles. des-k stays a medium integrity process (not admin): it reads titles, program names and icons that any app can read. UIPI keeps it from closing (and perhaps moving) admin windows; AC-9 and AC-10 surface that instead of failing silently. No compliance scope.

**Configuration required**: no environment variables or credentials. One new `settings.toml` key, `[shortcuts] finder`. Your existing file is never rewritten, so it only appears there if you add it; missing means `["Win+F"]`. New dependency `nucleo-matcher = "0.3.1"`; `Cargo.toml` may need the `windows` feature that holds `QueryFullProcessImageNameW` if `Win32_System_Threading` does not already cover it.

**Critical test scenarios** (each maps to an acceptance criterion in ## Requirements):
- Happy path: Notepad hidden on screen 3 workspace 2, mouse on screen 1: Win+F opens on screen 1 with focus in the box; typing "note" puts Notepad first with `3 · 2`; Enter switches screen 3 to workspace 2 with Notepad focused and the indicator there, screens 1 and 2 unchanged. Verifies **AC-1**, **AC-2**, **AC-3**, **AC-5**, **AC-7**.
- Alt+Tab habit: in Chrome, Win+F to Code, Enter; then Win+F, Enter: back in Chrome, from any screen. Turn `follow_activation` off, restart, repeat: same result. Verifies **AC-4**, **AC-8**.
- Fuzzy and words: "vsc" ranks Visual Studio Code first; "code des-k" finds the Code window with des-k in its title; "explorer" lists every File Explorer window; "zzzq" shows "No match". Verifies **AC-3**, **AC-5**.
- Keys and mouse: Tab, Ctrl+N, Down move the same way and stop at the last row; with more than 9 windows the list scrolls; Ctrl+Backspace removes the last word; paste a title; click a row. Verifies **AC-6**.
- Bring across scaling: a Chrome window filling the left half of the 150% laptop screen, Shift+Enter with the mouse on an external screen: it fills the left half there, focused; a maximized window stays maximized; a window hidden on another screen appears without flashing on its old screen. Verifies **AC-9**.
- Admin windows: an admin terminal: Ctrl+W shows "Can't close an admin window" and the row stays; Shift+Enter either moves it or shows "Can't move this window" with the window untouched. Verifies **AC-9**, **AC-10**.
- Close and tray apps: Ctrl+W on a Notepad with no changes: the row disappears, finder stays; on a Notepad with changes: the save prompt takes focus, finder closes; on an app that closes to the tray: the row disappears. Verifies **AC-10**.
- Closing the finder: Esc gives focus back; Win+F again closes it; click on the desktop closes it; Win+Ctrl+Right while open closes it and switches. Verifies **AC-11**.
- Gone window: open the finder, close a listed window with the mouse, then Enter on it: "That window is gone", finder still open. Verifies **AC-12**.
- Look: on the 150% laptop and a 100% screen the popup is sharp and sized alike; Alt+Tab while it is open does not show it. Verifies **AC-13**.
- Config: `cargo test` covers the default file parsing to the defaults, a `finder` combo clashing with `next` turning `finder` off, and `finder = ["Win+3"]` logging that digit 3 is skipped. Verifies **AC-14**.
- Unit tests for the pure helpers, written to fail when the rule they protect changes: `rank` (score first, then recency, then `ws` order; the empty query selects row 2), `screen_numbers` (left to right, then top), `map_rect` (proportional, stays inside the new work area), `delete_word` (Ctrl+Backspace stops at a space or punctuation boundary). Verifies **AC-4**, **AC-5**, **AC-6**, **AC-9**, **AC-3**.

## Build plan

Tracer Bullet: one thin thread first, from Win+F in `settings.toml` to landing on a hidden window, then each layer thickened on top of it.

1. [x] Thin thread end to end: the `finder` setting (default, `DEFAULT_FILE`, `SHORTCUT_KEYS`, `Display`, the clash loop with the send lists) with its tests; `WM_FINDER` in the hook and the main loop; `finder.rs` with the popup, the dark EDIT, `finder::key` for Up/Down/Enter/Esc, the main loop's `TranslateMessage` and key filter, and the close first rule in the other arms; `ws::windows()` and `ws::go_to`; rows showing just title and `S · W` with `screen_numbers`; focus check on open; close on Esc, Win+F, focus lost. Satisfies **AC-1**, **AC-2**, **AC-7**, **AC-8**, **AC-11**, **AC-14**.
2. [x] Finding: add `nucleo-matcher`; program names (Store app child lookup included); the foreground hook installed always and `finder::used`; `rank` with its tests and the second row pick; "No match" and "No windows". Satisfies **AC-3**, **AC-4**, **AC-5**.
3. [x] Full rows and navigation: icons with the timeout chain, ●, the 9 row scroll, Tab/Shift+Tab, Ctrl+N/Ctrl+P, Ctrl+Backspace with `delete_word` and its test, mouse click, the note line and the gone window rule, the final look (dim columns, accent selection, sizes at each scale). Satisfies **AC-3**, **AC-6**, **AC-12**, **AC-13**.
4. [x] Shift+Enter: `ws::bring` with `map_rect` and its test, the placement and second size pass, the hidden move while cloaked, the "Can't move this window" path. Satisfies **AC-9**.
5. [x] Ctrl+W: `WM_CLOSE`, the `closing` timer, the admin note. Satisfies **AC-10**.
6. [ ] Try every critical test scenario on your screens, then run `install.ps1` and confirm `des-k.exe` is running afterward. Satisfies **AC-1** to **AC-14**.

## Consequences

**Positive**:
- Any window is a few letters away, wherever des-k hid it.
- Win+F then Enter flips between your last two windows across every screen and workspace.
- Landing reuses `switch_to`, so it behaves exactly like a switch, indicator and saving included.

**Negative / tradeoffs**:
- Win+F no longer opens Feedback Hub. Setting `finder` to another combo gives it back; there is no off switch.
- A new dependency, `nucleo-matcher`, whose last release is from February 2024.
- The first des-k window that takes focus: if Windows ever refuses des-k the foreground, the finder only logs and closes.
- The main loop now runs `TranslateMessage` and a key filter on every message, and the foreground hook is always installed.
- Recency starts empty after every restart.
- Moving windows across different scaling depends on how each app reacts to the scaling change; some may land a little off the mapped size.
- Store apps whose app is suspended show as `ApplicationFrameHost` with a generic icon.
- The list is a snapshot: a window opened while the finder is open is not listed until the next open.

**Neutral**:
- New module `src/finder.rs` and one new thread message id, `WM_APP + 6`.
- Ctrl+W on a hidden window can make its save prompt appear; with `follow_activation` on, the prompt's screen switches to it (spec 0002).

## Follow-up

- [ ] After the build, `/sync` adds to `AGENTS.md`: `WM_APP + 6` finder (spec 0007) to the thread message ids, `nucleo-matcher` to Key dependencies, `src/finder.rs` and its rule (changes where a window is only through `ws`), the always on foreground hook, and a `Declined:` line for the `smithery.ai/nucleo-matcher` Agent Skill.
- [ ] Mouse wheel scrolling in the list was not asked for; add it if the keys feel slow with many windows.
