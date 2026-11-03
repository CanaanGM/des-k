# 0003. Restore layout after reboot: match reopened windows to their saved workspaces

**Date**: 2026-10-26
**Status**: In Progress

## Summary

After a reboot, a crash of Windows, or a sign out, every window gets a new handle, so today des-k drops your saved layout and everything reopens visible. With this change des-k saves a fingerprint of each window (its program, its window class, and its title). After a fresh sign in, it watches for a few minutes as apps reopen on their own, and hides each window it recognizes back into the workspace it came from. When it is not sure, it leaves the window visible, and it never starts programs itself.

## Requirements

**User stories**:
- As the des-k user, I want windows that reopen after a reboot to go back to the workspaces they were on, so that I don't have to sort every screen by hand each morning.
- As the des-k user, I want des-k to leave a window visible when it is unsure, so that nothing I need ends up hidden on the wrong workspace.
- As the des-k user, I want a window I open myself after login to never vanish, so that restoring never fights me.

**Acceptance criteria**:
- **AC-1**: After a reboot, a window that was hidden on workspace N of screen S, and that reopens on its own on screen S within the restore period, is hidden into workspace N. Switching S to N shows it.
- **AC-2**: After a reboot, each present screen comes back on the workspace it showed before (today's behavior, kept).
- **AC-3**: Windows already open when des-k starts are matched exactly like windows that open later in the period.
- **AC-4**: When an app had several saved windows, a reopened window goes to the one entry whose title matches it. A window whose title fits no single entry clearly stays visible, and is checked again when its title changes, until the period ends.
- **AC-5**: A reopened window that best matches a window that was visible before the reboot stays visible, even when the same app also had hidden windows.
- **AC-6**: A window that opens after the restore period ends is never hidden by restore. When the period ends, every unmatched saved entry is removed from `state.txt`.
- **AC-7**: Restarting des-k within the same sign in (crash, `install.ps1` update) never starts a new restore. The layout is adopted by window handle exactly as today.
- **AC-8**: A window whose saved screen is missing, or whose saved workspace no longer exists (count lowered), stays visible. A window that is on a different screen than the saved one stays visible unless it moves to the saved screen before the period ends.
- **AC-9**: If a window had keyboard focus when restore hid it, focus moves to the top visible window on that screen (or the desktop), and the screen does not jump to the hidden window (follow activation, spec 0002, ignores it).
- **AC-10**: `[restore] enabled = false` in `settings.toml` turns restore off. `[restore] minutes` sets the period (1 to 30, default 3). A bad value resets only that key, with a log line (spec 0001).
- **AC-11**: Every window hidden by restore, every window left visible because the match was unclear, and the final counts are written to `des-k.log`. Nothing pops up on screen.
- **AC-12**: If the machine restarts again before the period ends (a second Windows Update restart), the entries not yet matched are still in `state.txt` and get matched after that restart.
- **AC-13**: A shutdown with Fast Startup on, or a sign out and back in, starts matching the same as a full reboot.
- **AC-14**: A `state.txt` written by an older des-k (no fingerprints) loads without error. Its entries just cannot be matched after a reboot.
- **AC-15**: If des-k starts more than `minutes` + 2 minutes after you signed in (you started it by hand later), no restore runs, and the log says why.
- **AC-16**: If des-k restarts during a restore (crash, update), the entries not yet matched are matched again, with a new full period.
- **AC-17**: Signing out, then back in, leaves the saved layout in `state.txt` (des-k is not wiping it on the way out).

## Decision

**Chosen option**: Option 1: Fingerprint every window, match during a short period after a fresh sign in

des-k extends `state.txt` with a fingerprint per window and a logon session id, and for `minutes` after it sees a new logon session, it checks every half second for reopened windows to hide back into their saved workspaces.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Feature design

**Data model** (`state.txt`, one record per line, written by `save()`):

| Line | Fields | Change |
|---|---|---|
| `L` | logon session id (hex), logon time (Unix seconds) | **new**, one per file |
| `M` | device, active workspace | unchanged |
| `W` | device, workspace, hwnd, pid, then TAB app, TAB class, TAB title | **extended**: today's five fields stay space separated; the fingerprint follows, tab separated, title last |
| `V` | device, then TAB app, TAB class, TAB title | **new**: a window visible on a screen's active workspace at save time (device only for reading the log) |

- `app` is the lowercased full exe path. For a Store app (exe is `applicationframehost.exe`) it is the window's app id (AppUserModelID) instead, falling back to the exe path when that read fails. An empty `app` (the process could not be opened) never matches.
- Tabs and line breaks inside a title are replaced by spaces when saving.
- Parsing: split the line at the first TAB. The part before is space split as today (`W` keeps its five fields, so a line with no TAB is an old line). The part after is `splitn(3, '\t')` into app, class, title.
- `save()` writes to `state.tmp` and renames it over `state.txt`, so a crash or power loss mid write never leaves half a file.
- While a restore is running, `save()` also writes every pending entry back (`W` with hwnd `0` and pid `0`, `V` as is), so a second restart keeps them (AC-12, AC-16).
- `restore_all` (the `--quit`, panic, and Ctrl+C paths) saves without any fingerprint work: only `L` and `M` lines, since nothing is hidden any more. No process opening or COM calls on a panic path.

**In memory only** (in `ws::State`, never saved except as above):
- `saved`: every `W` and `V` entry as loaded, never changed. Rule 2 of the matching rule reads this.
- `pending`: the saved entries not yet used, each used at most once.
- `resolved`: windows (hwnd, pid) restore has finished with: hidden, kept visible by a `V` match, or rejected for good. Later ticks skip them.
- `unclear_logged`: windows (hwnd, pid) already logged as unclear, so the log says it once.
- `deadline`: start time plus `minutes`. `None` means no restore is running.
- `restored`: windows restore just hid, with the time, so `follow()` ignores them for 2 seconds.
- A cache of `app` and class per (hwnd, pid), so each tick only reads titles again. A Store window whose app id was not readable yet is not cached, so it is fingerprinted again next tick.
- All of the above are cleared when the restore ends.

**State transitions** (restore, once per des-k start). `ws::load()` decides this first, before it creates the `Shell` (explorer may not be ready at autostart):
- `off` → `running` (new sign in): `[restore] enabled`, the saved `L` id or logon time differs from the current one, and the current logon time is at most `minutes` + 2 minutes before now. The `W`/`V` lines go to `saved` and `pending` instead of the adopt by handle path.
- `off` → `running` (resume): same `L`, and the file still holds `W` lines with hwnd `0` (a restore was cut short). Those lines and the `V` lines become `saved` and `pending`, with a new full period. Lines with a real hwnd are adopted by handle as today.
- New sign in but logon too long ago: stays `off`, logs `restore: signed in N min ago, skipped`, and drops the saved entries (AC-15).
- `running` → `off`: the deadline passes, or `pending` becomes empty. Unmatched entries are dropped, the counts are logged, and the file is saved.
- Same `L` and no hwnd `0` lines, or no `L` line (an older file): stays `off`, and `load()` runs exactly as today.

**Matching rule** (`restore::pick`, a pure function with unit tests). Titles compare case insensitive, after trimming. For one window, the group is the entries in `saved` with the same `app` and class; the candidates are the members of that group still in `pending`.
1. No candidates: no match, skip it.
2. The whole saved group (not just what is left) leads to one placement (all `V`, or all `W` with the same device and workspace): take any candidate.
3. Otherwise exactly one candidate has a title equal to the window's: take it. Two or more equal: unclear.
4. Otherwise exactly one candidate's title contains the window's title or the reverse, with the shorter title at least 4 characters long: take it. Two or more: unclear.
5. Unclear, or nothing fits: the window stays visible, is not added to `resolved`, and is checked again next tick.

A `V` winner: the window stays visible, the entry is used up, and the window goes in `resolved`. A `W` winner: placement rules below.

**Each tick** (`ws::restore_tick`): list candidate windows on every screen, bottom of z order first, skipping `resolved`. Pass 1 applies only rule 3 (exact title) to all of them; pass 2 applies rules 2 and 4 to the windows left. So a guess never takes an entry that an exact title match in the same tick would have taken.

**Placement rules** for a `W` winner:
- The saved device is missing, or the saved workspace is not below the current count: the window stays visible, the entry is used up, the window goes in `resolved` (AC-8).
- The saved workspace is that screen's active one (you switched there during the restore): the window stays visible, the entry is used up, the window goes in `resolved`.
- The window is on another screen right now (`MonitorFromWindow`): nothing is used up; the window is checked again next tick, since apps often open on the main screen and move a moment later.
- Otherwise des-k cloaks the window, puts it at the top of `hidden[ws]`, records it in `restored` and `resolved`, uses up the entry, and saves. If the cloak fails (a brand new window fails `GetViewForHwnd` for a moment), nothing is used up and the next tick tries again.

**Focus**: only when the window restore just hid was `GetForegroundWindow()`, des-k calls `SetForegroundWindow` on the top window of `app_windows_on(mon)`, else on `GetShellWindow()`, as `switch_to` does. Windows may refuse (des-k has had no recent input); then des-k logs `restore: could not move focus` and moves on.

**During a restore, if you switch a screen yourself**: `switch_to` hides every visible window, including reopened windows not yet matched, so those go to the workspace you left (they are cloaked and drop out of the candidates). A pending entry for the workspace you switched to keeps its window visible (placement rules above). Both are accepted.

**Interface surface** (no network API; internal calls and the main loop):

| Piece | Where | Does |
|---|---|---|
| `restore::fingerprint(hwnd)` | new `src/restore.rs` | returns `app`, class, title |
| `restore::logon()` | `src/restore.rs` | returns the current logon session id and logon time |
| `restore::pick(&saved, &pending, &fp, pass)` | `src/restore.rs` | the matching rule above, pure, unit tested |
| `ws::load() -> bool` | `src/ws.rs` | as today, plus it starts or resumes a restore and returns whether one runs (false on every early return) |
| `ws::restore_tick() -> bool` | `src/ws.rs` | one tick as above, a fresh `Shell` each time (a failure means try again next tick), then the deadline check; returns whether the restore is still running |
| restore timer | `src/main.rs` | when `load()` returned true: after the follow hook, the panic hook, the Ctrl+C handler, and `keys::spawn`, call `SetTimer(None, 0, 500, None)` and keep the id it returns (with no window, the id argument is ignored). In the loop, a `WM_TIMER` arm matching `msg.hwnd.is_invalid() && msg.wParam.0 == id` calls `ws::restore_tick()`; on false, `KillTimer(None, id)`. Indicator timers carry a window, so they still go to `DispatchMessageW` |

The candidate filter is the one `app_windows_on` already uses, applied to all screens. The startup sweep (AC-3) is simply the first tick, run once right after the timer is set, so every exit path is in place before restore cloaks anything.

`follow()` checks `restored` for both the window and its root owner (the same pair it already looks up), right after its stale check and before the cooldown.

**Value sourcing**:

| Action | Value | Source |
|---|---|---|
| detect a fresh sign in | current logon session id | `GetTokenInformation(TokenStatistics).AuthenticationId` on the des-k process token |
| detect a fresh sign in | current logon time | `LsaGetLogonSessionData(id).LogonTime` (FILETIME, converted to Unix seconds) |
| detect a fresh sign in | saved id and logon time | the `L` line in `state.txt` |
| skip a late start | minutes since sign in | now minus the current logon time |
| resume a cut restore | pending entries | `W` lines with hwnd `0`, plus `V` lines |
| fingerprint | `app` | `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `QueryFullProcessImageNameW`; for `applicationframehost.exe`, `SHGetPropertyStoreForWindow` + `PKEY_AppUserModel_ID` |
| fingerprint | class, title | `GetClassNameW`, `GetWindowTextW` (the existing `class_name`, `title` helpers) |
| placement | saved device, workspace | the `W` entry |
| placement | screen the window is on now | `MonitorFromWindow` + `device_name` |
| placement | active workspace, count | `ws::State`, `settings::get().workspaces` |
| deadline | period length | `[restore] minutes` in `settings.toml` |
| focus after hiding | next window to focus | top of `app_windows_on(mon)` after the hide, else `GetShellWindow()` (same as `switch_to`) |
| follow suppression | when restore hid it | `restored` map in `ws::State` |
| `V` entries | visible windows | `app_windows_on(mon)` for each present screen at save time (`monitor_named` per device in `s.monitors`) |

**Key invariants**:
- Restore only ever cloaks; it never moves, resizes, or starts anything.
- Each pending entry places at most one window. Each window is placed at most once per restore.
- Nothing is hidden after the deadline.
- A window restore hid is in `hidden[ws]` like any other, so `restore_all`, `--quit`, the panic hook, and Ctrl+C show it again.
- All restore work runs on the main thread from the timer message. No callback touches `ws` state (AGENTS.md threading rule).

**Security model**: one local user, no new privileges. `PROCESS_QUERY_LIMITED_INFORMATION` works on elevated processes from medium integrity; where it fails, the window simply never matches. `state.txt` now holds window titles in plain text under `%LOCALAPPDATA%\des-k`, readable only by your account.

**Configuration required** (`settings.toml`, following spec 0001: added to `Settings`, the defaults, `DEFAULT_FILE`, `TOP_KEYS`, and the `Display` line):
```toml
[restore]
enabled = true   # after a reboot or sign in, hide reopened windows back into their workspaces
minutes = 3      # how long after start to keep matching (1 to 30)
```

**Critical test scenarios**:
- Happy path: Notepad hidden on workspace 3 of the laptop screen, reboot with "restart apps" on: Notepad reopens and vanishes within a second; switching to workspace 3 shows it. Verifies **AC-1**, **AC-2**.
- Early window: an app set to start at login opens before des-k; it is still hidden into its workspace. Verifies **AC-3**.
- Several windows: three Edge windows, two hidden on workspaces 2 and 4, one visible: after reboot each goes back where it was. Verifies **AC-4**, **AC-5**.
- Unclear: two saved Edge windows whose titles changed completely: both stay visible, log says unclear. Verifies **AC-4**, **AC-11**.
- Late window: open Notepad by hand 5 minutes after login: it stays visible. Verifies **AC-6**.
- Restart only: `install.ps1` while windows are hidden: adopted by handle, no matching in the log. Verifies **AC-7**.
- Undocked boot: windows saved on an external screen stay visible when booting without it. Verifies **AC-8**.
- Focus: a reopened window takes focus and is hidden: focus lands on a visible window, the screen stays put. Verifies **AC-9**.
- Settings: `enabled = false`, then `minutes = 0`: off, then a log line and the default 3. Verifies **AC-10**.
- Double restart: reboot again within the period: unmatched entries survive and match after the second boot. Verifies **AC-12**.
- Fast Startup shutdown and sign out: matching runs. Verifies **AC-13**.
- First start after the update with an old `state.txt`: loads, no errors. Verifies **AC-14**.
- Late start: with windows hidden, hard kill des-k (`Stop-Process`), change the id in the `L` line of `state.txt`, and start des-k more than `minutes` + 2 minutes after you signed in: no restore, the log says skipped. Verifies **AC-15**.
- Restart mid restore: kill des-k (`install.ps1`) one minute after login: on start, the remaining entries are matched again. Verifies **AC-16**.
- Sign out with windows hidden, sign back in: `state.txt` still has its `W` lines before des-k starts (check with des-k not yet started, or in the log). Verifies **AC-17**.
- Unit tests for `pick`: a single entry group matches regardless of title; a group that was V plus W never matches on rule 2 even after the V entry is used; equal title wins; two equal titles are unclear; containment wins; containment under 4 characters does not count; two containment hits are unclear; a different class never matches; pass 1 never takes a title less match. Each test says why (a wrong guess hides a window on the wrong workspace). Verify **AC-4**, **AC-5**.

## Build plan

Tracer Bullet: tasks 1 and 2 give a working end to end restore (reboot, a single app window, back on its workspace). The rest thicken it.

0. Check first, before writing code: with the installed des-k hiding a few windows, sign out and back in, and look at `state.txt` before des-k starts again. If its `W` lines are gone, find what wiped it (the console handler or the message loop seeing an end of session) and make that path leave `state.txt` alone. Satisfies **AC-17**.
1. Add `src/restore.rs` with `fingerprint` and `logon`, plus the `windows` features they need. Extend `save()` to write the `L` line, the fingerprint on `W` lines, and the new `V` lines through `state.tmp` and a rename; `restore_all` saves without fingerprints; extend `load()` to parse both old and new `W` formats. No behavior change. Satisfies **AC-14**.
2. Thin restore: in `load()`, before the `Shell` check, detect a new sign in (id or logon time differs, logon recent enough), fill `saved` and `pending`, set the deadline (fixed 3 minutes for now), and return true; add `ws::restore_tick()` with only rule 2 of the matching rule plus the placement rules and `resolved`; add the 500 ms timer in `main` after the hooks, run the first tick then, kill the timer when done, drop unmatched entries and save. Satisfies **AC-1**, **AC-2**, **AC-3**, **AC-6**, **AC-7**, **AC-8**, **AC-13**, **AC-15**.
3. Full `restore::pick` (rules 3 and 4, unclear stays pending, `V` winners) and the two passes per tick, with the unit tests. Satisfies **AC-4**, **AC-5**.
4. Focus after hiding and the `restored` map; `follow()` ignores those windows and their owners for 2 seconds. Satisfies **AC-9**.
5. `save()` writes pending entries during a restore, and `load()` resumes a cut restore from hwnd `0` lines. Satisfies **AC-12**, **AC-16**.
6. `[restore] enabled` and `minutes` in `settings.rs`, with unit tests (the default file still parses to the defaults; a bad `minutes` resets only that key); `load()` respects them. Satisfies **AC-10**.
7. Log lines: each hide, each unclear window (once per window, not every tick), and the end summary. Satisfies **AC-11**.
8. Try every critical test scenario on your screens, then run `install.ps1` and confirm `des-k.exe` is running. Satisfies **AC-1** to **AC-17**.

## Consequences

**Positive**:
- A reboot, a Windows crash, a Windows Update restart, or a sign out no longer flattens your layout, for every app that reopens its own windows.
- Restarting des-k itself works exactly as before; restore is a separate path that only runs after a new sign in.
- The matching rule is a pure function, so `cargo test` now covers the riskiest logic, not just settings.

**Negative / tradeoffs**:
- A reopened window can be visible for up to half a second before it is hidden, and may briefly take focus.
- Fingerprints are only as fresh as the last save (a switch, a display change, or start). A window whose title changed a lot since then may stay visible. A window opened after the last switch has no `V` entry, so after a reboot it can compete with a hidden window of the same app for that app's `W` entry.
- Apps whose exe path holds a version folder (some Electron apps) change `app` after an update, so their windows miss after a reboot that also updated them.
- Apps that don't reopen their windows (no session restore, not in "restart apps") are not restored; des-k never relaunches them.
- During the period, a window you open yourself from an app that had exactly one saved window, or whose saved windows all sat on one workspace, can be taken by that entry.
- `save()` now opens each window's process to read its exe path, on every switch. A few dozen windows cost a few milliseconds; the cache keeps ticks cheap.
- Window titles now sit in `state.txt` in plain text.

**Neutral**:
- `state.txt` grows two line types; old files still load.
- AGENTS.md needs a line for the restore timer and `src/restore.rs` (left to `/sync`).

## Migration plan

**Strategy**: no migration step; the new format is read alongside the old one.
**Phases**:
1. First start of the new des-k reads the old `state.txt` (no `L`, no fingerprints), adopts by handle as today, and writes the new format on its next save. No restore runs that start (no `L` line).
**Rollback**: run `des-k.exe --quit` first (shows every hidden window), then install the older build. Do not let `install.ps1` hard kill the new build into an older one: the old parser skips the new `W` lines, so those windows would stay hidden until a reboot.
**Risks**: the downgrade case above; nothing else, since the old line types are unchanged.

## Follow-up

- [ ] Screen names (`DISPLAY1`, `DISPLAY2`) can be numbered differently after a reboot or a dock change. If saved screens get mixed up, key screens by hardware id instead (affects feature G too). Needs a decision.
- [ ] Relaunch apps that don't come back on their own: added to the scope's Deferred list. Needs a decision.
- [ ] An explorer crash may drop cloaks entirely. Not covered here; worth one test by hand.

## Rationale

Reasoning and options: see [rationale.md](rationale.md).
