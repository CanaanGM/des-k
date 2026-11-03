# 0015. Check for updates from the tray

**Date**: 2026-11-03
**Status**: Proposed

## Summary

The tray menu gets a **Check for updates** item. When you click it, des-k asks GitHub which release is newest and answers in a tray balloon: a newer version (click it to open the release page in your browser), you have the newest, or why the check failed. des-k never checks by itself, downloads nothing, and runs nothing. You install a new version yourself, as today. Raised by the five reviewer report of 2026-11-03 ("no update check"). You chose how it behaves (manual only, a balloon, nothing run); how it asks GitHub is the recommendation below.

## Context

des-k ships as GitHub releases: `.github/workflows/build.yml` publishes one per `v*` tag, with the setup exe, a zip and checksums, on `https://github.com/CanaanGM/des-k/releases`. Nothing in des-k tells you a newer one exists, so the installed copy can fall behind without you noticing. That matters more now that des-k uses an undocumented part of Windows: a Windows update can break hiding (since 0.3.0 the tray warns when it does), and the fix arrives only as a new release.

des-k runs as admin (spec 0008). AGENTS.md says nothing in `%LOCALAPPDATA%\des-k` may name a program to run, and files the tray opens go through explorer so they run as you. Anything a network answer can influence is input from outside the machine, so it must never decide what program runs or what file is opened with admin rights.

You want no traffic you did not ask for: GitHub should see your address only when you click.

## Requirements

**User stories**:
- As the des-k user, I want to find out from the tray whether a newer des-k exists, so that I can update when a fix ships.
- As the des-k user, I want des-k to stay off the network unless I ask, so that nothing leaves my machine on its own.

**Acceptance criteria**:
- **AC-1**: The tray menu has a **Check for updates** item, after **Open log** and above the separator before **Quit**. Clicking it starts one check and the menu closes at once; while the check runs the item reads "Checking for updates…" and is grayed, and des-k keeps switching, following and drawing.
- **AC-2**: When GitHub's newest release is newer than the running des-k, a tray balloon says "des-k X.Y.Z is out (you have A.B.C). Click here to open its page." Clicking the balloon opens `https://github.com/CanaanGM/des-k/releases/tag/vX.Y.Z` in your default browser, running as you, not as admin.
- **AC-3**: When the newest release is the same as or older than the running des-k, the balloon says "You have the newest des-k (A.B.C)." Clicking it does nothing.
- **AC-4**: When the check fails (no connection, a step taking longer than about 10 s, a certificate error, a proxy asking for a login, GitHub answering with anything but a redirect to a release tag, no release published, the page moved, a tag that is not `vX.Y.Z`, or this des-k's own version unreadable), a warning balloon titled "Could not check for updates" says why in plain words, and the log has the full detail. des-k carries on normally.
- **AC-5**: des-k makes no network request except while a check you started runs: none at start, none on a timer, none from the settings page.
- **AC-6**: A second check cannot start while one runs (the item is grayed, and a second call is ignored with a log line), and a check that fails in any way, a crash in its thread included, always ends and frees the item again.
- **AC-7**: des-k downloads no file and runs no program because of a check. The only thing it opens is a URL it builds itself from the three numbers of the version, through explorer (`IShellDispatch2::ShellExecute`, as the tray's Open log does).
- **AC-8**: Every check writes log lines: started, the answer (newest version and yours, or the failure with its detail).
- **AC-9**: While the "hiding windows does not work" warning (0.3.0) is showing, a check never replaces it: the answer goes to the log and onto the menu item ("Update to X.Y.Z (open page)" when newer, which opens the page like the balloon would).

## Options considered

### Option 1: Ask GitHub's release redirect with WinHTTP

`GET https://github.com/CanaanGM/des-k/releases/latest` with redirects turned off. GitHub answers with a redirect whose `Location` header ends in `/releases/tag/vX.Y.Z`; des-k reads the version from it. WinHTTP (the HTTP client built into Windows) is reached through the `windows` crate already in use, adding only the `Win32_Networking_WinHttp` feature.

**Pros**:
- No new crate, no JSON parsing: one header and three numbers.
- WinHTTP checks the TLS certificate (the HTTPS proof that the answer comes from github.com) and, in its automatic proxy mode, uses your Windows proxy settings.
- `releases/latest` already skips drafts and prereleases.

**Cons**:
- Rests on the shape of a web redirect, not a documented API; if GitHub changes it, the check fails (loudly, AC-4) until des-k changes.
- WinHTTP is verbose raw Win32: a session, a connection, a request, handles to close.

### Option 2: Ask the GitHub REST API with WinHTTP

`GET https://api.github.com/repos/CanaanGM/des-k/releases/latest` and read `tag_name` from the JSON answer.

**Pros**:
- A documented, versioned API, less likely to change shape.

**Cons**:
- A JSON answer to parse: either a new dependency (`serde_json`) or a hand written field search, which is fragile in its own way.
- Needs a `User-Agent` header and is rate limited to 60 requests an hour without a token (fine for clicks, but one more failure to explain).

### Option 3: A Rust HTTP crate (for example `ureq` with `rustls`)

A higher level client with its own TLS stack.

**Pros**:
- Shorter, safer looking code than raw WinHTTP.

**Cons**:
- Several new dependencies, a bigger exe and a longer build, against AGENTS.md's four direct dependencies.
- Its own TLS stack and certificate store, apart from Windows', and it does not pick up the system proxy by default.

### Option 4: No network, the menu item opens the releases page

**Check for updates** just opens `https://github.com/CanaanGM/des-k/releases` in your browser; you compare versions by eye.

**Pros**:
- No network code in an admin process at all.

**Cons**:
- You must remember which version you run; the tray never tells you there is nothing new. It is a bookmark, not a check.

## Decision

**Chosen option**: Option 1: Ask GitHub's release redirect with WinHTTP.

The tray's **Check for updates** runs one WinHTTP request on a worker thread to `https://github.com/CanaanGM/des-k/releases/latest` with redirects off, reads the version from the redirect's `Location`, compares it with the running version, and answers in a tray balloon whose click opens a release page URL des-k builds itself.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`): WinHTTP handles and their cleanup, the window message across threads.

## Rationale

The forces are: an admin process, a wish for no surprise traffic, a tool with four dependencies, and a single fact to learn (the newest version number). Option 1 learns that fact with the least code that can go wrong: no JSON, no new crate, and Windows' own TLS and proxy handling, which matters in an admin process where a second TLS stack is one more thing to keep patched. The cost is leaning on a redirect's shape; that is acceptable because a change fails loudly into AC-4's balloon and log line, never into a wrong answer, since anything but `/tag/v<digits>.<digits>.<digits>` is rejected.

Option 2 is the runner up: the documented API is the better long term contract, but it trades the header for a JSON parse that either adds a dependency or a hand rolled search. If the redirect ever changes, moving to Option 2 is a small change inside one function (the fetch), with the rest of the design unchanged.

Option 3 adds more than it saves for one request a click. Option 4 does not answer the question you asked ("is there a newer one?").

Doing the request on a worker thread keeps the rule that the main thread never waits on slow work (AGENTS.md, Threads): a 10 s timeout on the main thread would freeze switching. The answer comes back as a posted window message to the tray window, not a thread message, because a thread message is lost while a menu is open (the review's modal loop finding), and a posted window message is dispatched inside a menu's loop too.

## Feature design

**Data model sketch**:
No file and no setting. Nothing is shared between threads but one flag: the answer travels inside the posted message.
| Value | Type | Where | Meaning |
|---|---|---|---|
| `RUNNING` | `static AtomicBool` in `src/update.rs` | both threads | A check is in flight (AC-6). Set by `check` on the main thread; cleared by the worker's guard just before it posts, on every path. |
| `Version` | `(u16, u16, u16)` | `src/update.rs` | Compared as a tuple. Each part 0 to 65535; more digits or a bigger number is a bad tag. |
| `Answer` | enum | `src/update.rs` | `Newer(Version)`, `Newest(Version)` (GitHub's newest, not newer than ours), `Failed(Fail)`. |
| `Fail` | enum | `src/update.rs` | `NoConnection` (name not resolved, cannot connect), `Timeout`, `Certificate` (any secure channel or certificate error), `ProxyLogin` (HTTP 407), `Status(u16)` (any status but 301 or 302), `NoRelease` (`Location` is the releases list, nothing published), `Moved` (`Location` is not on `github.com/CanaanGM/des-k`), `BadTag` (a tag that is not `vX.Y.Z`), `OurVersion` (the running version does not parse), `Crashed` (the worker panicked). |
| `pack(Answer) -> (WPARAM, LPARAM)` / `unpack` | pure fns | `src/update.rs` | `wParam` = kind (0 newer, 1 newest, 2 failed) plus the `Fail` code; `lParam` = major, minor, patch as 3 × 16 bits (for `Status`, the status code). |
| tray `BALLOON_URL` | `RefCell<Option<String>>` in `src/tray.rs` | main thread | What a click on the current balloon opens; set by every balloon, `None` for all but "newer". |
| tray `UPDATE_LABEL` | `RefCell<String>` in `src/tray.rs` | main thread | The menu item's text: "Check for updates", "Checking for updates…" (grayed), or after an answer that was not shown as a balloon (AC-9), "Update to X.Y.Z (open page)". |

The bad tag's text and every Windows error code go to the log from the worker (AC-8); the balloon gets only the plain words for the `Fail` code, so nothing beyond numbers crosses threads.

**State transitions**:
idle → (click) → checking (menu item grayed "Checking for updates…") → (worker posts `WM_UPDATE`) → idle, with one balloon, or with the answer on the menu item while the broken warning shows (AC-9). A click while checking is impossible from the menu (grayed) and ignored by `check` anyway (AC-6).

**API surface** (functions and messages, no HTTP server):
| Surface | Kind | Key inputs | Key outputs | Who | Key errors |
|---|---|---|---|---|---|
| Tray menu `ID_UPDATE` (4), after **Open log**, above the separator before **Quit** | menu command | click | `update::check(tray_hwnd)` | you | none |
| `update::check(hwnd)` | fn, main thread | the tray hwnd, passed to the worker as `isize` (an `HWND` is not `Send`) | sets `RUNNING`, spawns the worker | tray | already running: log line only (AC-6) |
| worker | thread | hwnd as `isize` | posts `WM_UPDATE` with `pack(answer)` | `check` | a guard (a value whose `Drop` runs on every exit, panics included) clears `RUNNING` and posts `Failed(Crashed)` if nothing was posted; no `unwrap` in the worker, since a panic runs des-k's hook, which ends the process; if `PostMessageW` fails, the flag is cleared anyway and the log says so |
| `fetch() -> Result<(u16, Option<String>), Fail>` | fn, worker | none | HTTP status and `Location` | worker | WinHTTP errors mapped to `Fail` (below) |
| `parse_location(&str) -> Result<Version, Fail>` | pure fn | `Location` text | version | worker | `NoRelease`, `Moved`, `BadTag` |
| `parse_version(&str) -> Option<Version>` | pure fn | `X.Y.Z` | version | `parse_location`, our version | anything else → `None` |
| `answer(newest, ours) -> Answer` | pure fn | two versions | `Newer` / `Newest` | worker | none |
| `WM_UPDATE` = `WM_APP + 13` | window message posted to the tray window | `pack(answer)` | balloon or menu label | worker | none |
| `tray::balloon(title, text, warn, url)` | fn, main thread | as named | `NIM_MODIFY` with `NIF_INFO`, `NIIF_WARNING` when `warn` else `NIIF_INFO`; sets `BALLOON_URL` | `WM_UPDATE` handler, `warn_if_broken` | `Shell_NotifyIconW` failure: logged |
| balloon click: `WM_TRAY` with `lParam` = `NIN_BALLOONUSERCLICK` | tray callback | none | `tray::open` of `BALLOON_URL` if `Some` | you | open failure: logged |

`NIN_BALLOONUSERCLICK` is `WM_USER + 5`; define it if the `windows` crate does not export it. The tray stays on the legacy callback (no `NOTIFYICON_VERSION_4`), where `lParam` is the notification itself; switching versions would change how `menu()` reads clicks. `tray::open` already takes any string explorer can open (`Path::new(url)`), so it needs no change beyond its log wording.

**Value sourcing**:
| Action | Value produced / displayed | Source |
|---|---|---|
| fetch | the URL asked | constant `https://github.com/CanaanGM/des-k/releases/latest` in `src/update.rs`, never a setting (AGENTS.md: settings never carry a program or a path) |
| fetch | method, redirects | `GET`, body never read; `WinHttpSetOption(WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_NEVER)` |
| fetch | `User-Agent` | `des-k/` + `env!("CARGO_PKG_VERSION")` |
| fetch | proxy | `WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY` (Windows 8.1 and later: the system's and your own proxy settings, including a manual one, plus automatic detection) |
| fetch | timeouts | `WinHttpSetTimeouts` 10 s each for resolve, connect, send and receive (AC-4) |
| fetch | status, `Location` | `WinHttpQueryHeaders` with `WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER`, then `WINHTTP_QUERY_LOCATION` |
| fetch | its `Fail` | `ERROR_WINHTTP_NAME_NOT_RESOLVED`, `ERROR_WINHTTP_CANNOT_CONNECT` → `NoConnection`; `ERROR_WINHTTP_TIMEOUT` → `Timeout`; `ERROR_WINHTTP_SECURE_*` → `Certificate`; status 407 → `ProxyLogin`; any status but 301/302 → `Status(n)`; 301/302 without `Location` → `Status(n)`; any other WinHTTP error → `NoConnection` with the code in the log |
| answer | newest version | `parse_location`: the text must start, case sensitive, with exactly `https://github.com/CanaanGM/des-k/releases/tag/v` and the rest must be `parse_version`, to the end (no slash, query or suffix). Exactly `https://github.com/CanaanGM/des-k/releases` → `NoRelease`; any other start → `Moved`; the right start with a bad rest → `BadTag` |
| answer | your version | `parse_version(env!("CARGO_PKG_VERSION"))`, once per check; `None` → `Failed(OurVersion)`, never a panic. The release job checks it equals the tag (`build.yml`, "Tag matches Cargo.toml") |
| balloon | title and text | from `Answer` and your version: |
|  | newer | title "des-k update", text "des-k X.Y.Z is out (you have A.B.C). Click here to open its page.", info icon, URL set |
|  | newest | title "des-k is up to date", text "You have the newest des-k (A.B.C).", info icon |
|  | failed | title "Could not check for updates", text the `Fail` in plain words: "No connection to GitHub.", "GitHub did not answer in time.", "The secure connection to GitHub failed.", "Your proxy asks for a login.", "GitHub answered with HTTP n.", "No des-k release is published yet.", "The des-k page on GitHub has moved.", "The newest release has an unexpected name (the log has it).", "This des-k's own version could not be read.", "The check stopped with an error (the log has it).", then " des-k carries on as before."; warning icon |
| balloon click | the URL opened | built as `https://github.com/CanaanGM/des-k/releases/tag/v{a}.{b}.{c}` from the three parsed numbers, never the `Location` text itself (AC-7) |
| menu item | its label | `UPDATE_LABEL` (data model above) |
| `WM_UPDATE` handler | whether the broken warning counts as showing (AC-9) | the tray's existing `BROKEN` flag, set once by `warn_if_broken` and kept for the run (its tooltip stays too) |

**Key invariants**:
- Network only inside a check you clicked (AC-5). No timer, no start up check, no setting turns one on.
- Nothing downloaded, nothing run; the only thing opened is a des-k built URL with three numbers in it, through explorer (AC-7).
- The main thread never waits on the network. The worker touches no `ws` state, no window and no tray icon; it only logs and posts one `WM_UPDATE`.
- `RUNNING` is always cleared, on every worker exit, panics included (AC-6).
- `WM_APP + 13` is new and unique: the main loop matches message ids alone, so it must not equal any thread message id (AGENTS.md).
- The "hiding broken" warning balloon is never replaced by an update balloon (AC-9).

**Security model**:
One user, admin process. What comes from the network is a status code and one header. Only three 16 bit numbers parsed from that header, or a failure code, ever leave the worker; they become text in a balloon and a URL on github.com that des-k builds. TLS certificate checking is WinHTTP's default and is never turned off. No message filter is added: `WM_UPDATE` comes from des-k's own thread, and UIPI keeps normal programs from posting it to the admin tray window (only `WM_TRAY` and `TaskbarCreated` are let in). Even a forged one could only show a balloon naming a github.com release page.

**Configuration required**:
- Cargo: add the `Win32_Networking_WinHttp` feature to the `windows` dependency. No settings, no environment variables, no credentials.

**Critical test scenarios**:
- Happy path: a dev build with `Cargo.toml` set to `0.0.1` (dev copy, not admin, the installed one stopped), click Check for updates; the item grays while it runs; the balloon names the newer version; clicking it opens its page in your browser as you. Verifies **AC-1**, **AC-2**, **AC-6**, **AC-7**
- Newest: on the current release, the balloon says you have the newest. Verifies **AC-3**
- Failure case: Wi-Fi off, the warning balloon says "No connection to GitHub." and switching keeps working meanwhile. Verifies **AC-4**, **AC-1**
- Broken warning up: with the hiding broken warning showing, a check puts its answer on the menu item and in the log, and the warning stays. Verifies **AC-9** (by hand only if hiding is ever broken; otherwise review the code path)
- No traffic: start des-k and leave it; the log shows no update lines. Verifies **AC-5**, **AC-8**
- Unit tests:
  - `parse_location` accepts `https://github.com/CanaanGM/des-k/releases/tag/v0.3.1` and `…/tag/v10.0.65535`; gives `BadTag` for `…/tag/v0.3`, `…/tag/0.3.1`, `…/tag/v0.3.1-rc1`, `…/tag/v0.3.1/`, `…/tag/v0.3.1?x`, `…/tag/v65536.0.0`; `NoRelease` for `…/releases`; `Moved` for `https://github.com/someone/des-k/releases/tag/v1.0.0` and `https://GitHub.com/CanaanGM/des-k/releases/tag/v1.0.0`. Verifies **AC-4**, **AC-7**
  - `answer` says newer only when the tuple is greater (0.10.0 is newer than 0.9.9; 0.3.0 against 0.3.0 is newest). Verifies **AC-2**, **AC-3**
  - `unpack(pack(a)) == a` for every `Answer` and every `Fail`, including `Status(407)` and 65535 parts. Verifies **AC-2**, **AC-3**, **AC-4**
  - `parse_version(env!("CARGO_PKG_VERSION"))` is `Some`, so a release can never ship a version the check cannot read. Verifies **AC-3**

## Build plan

Tracer Bullet: the first task is the whole thread from click to balloon, then the edges.

1. `src/update.rs`: `Version`, `Answer`, `Fail`, `parse_version`, `parse_location`, `answer`, `pack` and `unpack`, with their unit tests, so the rules are pinned before any network code, satisfies **AC-2**, **AC-3**, **AC-4**, **AC-7**
2. `fetch` with WinHTTP (session with automatic proxy and the user agent, 10 s timeouts, redirect policy never, `GET`, status and `Location`, every handle closed on every path, errors mapped to `Fail`), `check`, the worker with its guard, `RUNNING`, and the posted `WM_UPDATE`; log lines for start, result and every failure detail, satisfies **AC-1**, **AC-4**, **AC-6**, **AC-8**
3. Tray: the menu item with `UPDATE_LABEL` (grayed while running), `tray::balloon` shared with `warn_if_broken`, `BALLOON_URL`, the `WM_UPDATE` handler (balloon, or the menu label while the broken warning shows), and `NIN_BALLOONUSERCLICK` opening the URL through `tray::open`, satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-6**, **AC-7**, **AC-9**
4. Cargo feature, AGENTS.md (`WM_APP + 13`, the network rule), README and the site guide (tray menu, what a check sends and to whom), satisfies **AC-5**, **AC-7**
5. Try it on your machine: newer (a dev build set to `0.0.1`), newest, offline, satisfies **AC-1** to **AC-8**

## Consequences

**Positive**:
- You learn about a fix with one click, including the one a Windows update may force.
- No traffic you did not ask for, and no new dependency.

**Negative / tradeoffs**:
- You only hear about a release when you think to look; a broken Windows update shows its own tray warning, but a quiet improvement goes unnoticed.
- Rests on GitHub's `releases/latest` redirect; if its shape changes, every check fails (loudly) until des-k moves to the API (Option 2).
- An admin process now has network code; small, but it is new surface to keep correct.

**Neutral**:
- A balloon is a Windows notification: with Do not disturb on, Windows may hold it in the notification center instead of showing it.
- Prereleases and drafts are never offered; publishing one never reaches a check.

## Follow-up

- [ ] If you later want an automatic check, add it as an opt in setting (off by default), as discussed on 2026-11-03; it reuses `update::check` on a daily timer.
- [ ] If the redirect ever changes, switch `fetch` to the REST API (Option 2).
