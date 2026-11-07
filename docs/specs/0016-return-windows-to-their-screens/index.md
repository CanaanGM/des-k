# 0016. Return windows to their screens and workspaces after unplug, replug, sleep or crash

**Date**: 2026-11-04
**Status**: In Progress
**Amended by**: [0017](../0017-look-b-screens/0017-indicator.md): the indicator's screen notes (AC-IND-4).

## Summary

When you unplug a screen, des-k now folds its windows onto the main display: what was hidden on its workspace 2 goes hidden into the main display's workspace 2, and what you were looking at stays in view. When that screen comes back, every one of its windows goes back by itself, to the same workspace, position, size and maximized state, unless you moved it yourself in the meantime. des-k knows a screen by the monitor's own ID (maker, model, serial), not by Windows' `DISPLAY2` name, and remembers an unplugged screen with no time limit, through restarts, reboots, crashes and power cuts. You see it happen on the indicator and in a tray balloon, and the settings page lists the screens it is waiting for, each with a Forget button.

## Requirements

**User stories**:
- As the des-k user, I want my three screen setup to come back by itself when I dock again, so that I never sort windows by hand after a trip, a sleep, or a power cut.
- As the des-k user, I want the windows of a missing screen to keep their groups on the laptop, so that unplugging does not dump everything onto one workspace.
- As the des-k user, I want a window I moved on purpose while undocked to stay where I put it, so that docking never undoes my work.
- As the des-k user, I want to see which screens des-k is still waiting for, so that nothing it remembers is hidden from me.

**Acceptance criteria**:
- **AC-1**: des-k knows each screen by the monitor's own ID: its maker, product code and serial from the EDID (the monitor's built in description), or `internal` for the laptop panel (a target whose connection is `INTERNAL`, `DISPLAYPORT_EMBEDDED` or `UDI_EMBEDDED`, the same list `display_names` uses; closing the lid unplugs `internal`). The same monitor on another port, another dock, or after a reboot is the same screen. Two present monitors with the same ID (no serial) are told apart by their left to right order in Display settings (`#1`, `#2`); if you swap such twins in Display settings they swap in des-k too. The log lists every screen's ID, Windows name and friendly name at start and after each display change.
- **AC-2**: When a screen has been gone for 10 checks in a row (today's rule, unchanged), each window des-k hid on its workspace N goes hidden into workspace N of the main display (the one Windows marks as main), placed at the same relative spot on that screen. When N is the workspace the main display shows, those windows come into view there. The gone screen is remembered, not dropped.
- **AC-3**: The windows that were in view on the gone screen (its shown workspace) stay in view wherever Windows put them. des-k does not hide or move them on unplug, but from the first check the screen is missing it remembers that they belong to that screen and workspace (their away record starts then, not at the fold).
- **AC-4**: A screen back within those 10 checks (sleep, a DisplayPort relink) keeps its workspaces as today. Once it has stayed present for 2 checks (the AC-5 wait), each window whose away record points to it and that is now on another screen goes back to its spot.
- **AC-5**: When a remembered screen is plugged in again and has stayed present for 2 checks in a row (about 2 s), each of its away windows that is still open and that you did not move (AC-6) goes back to that screen: to the same workspace, the same position and size (or the same relative spot and size when that screen's work area changed size), maximized if it was maximized, minimized if it was minimized. The screen shows the workspace it showed before the unplug; windows of that workspace are in view, the others are hidden.
- **AC-6**: An away window you move yourself stays where you put it when its screen returns, and the log says so. "Moved yourself" is: any send (shortcut, gesture, overview drop), the finder's or overview's move to another screen, Win+Shift+Left/Right or a mouse drag that puts it on another screen, and pin or unpin. A screen change counts only when the window went from one present screen to another present screen; a window Windows moves off a missing screen, or onto its own home screen, is never "moved by you". Moving or resizing it on the screen it is on does not count; it still goes back to its old spot.
- **AC-7**: A pinned window on a screen you unplug stays pinned (in view on every workspace) on the main display, and goes back pinned to its own screen. Pinning or unpinning a window while it is away is a move of yours (AC-6): it stays where it is, with the pin you gave it.
- **AC-8**: An unplugged screen and its away windows are remembered with no time limit: through a des-k restart (install, update, hard kill), a tray Quit and start, a sign out, a reboot, a crash and a power cut, until the screen returns or you Forget it. Closed away windows are noticed at each 1 s check (the window is gone, or its handle now belongs to another process). A remembered screen whose away windows have all closed or been moved by you is forgotten at that check, with a log line. Upgrading from an older des-k while undocked loses the missing screens' old lines once (AC-15).
- **AC-9**: The settings page lists every remembered screen that is not plugged in: its friendly name and how many windows are waiting for it, with a **Forget** button. Forget drops that screen and its away windows from memory; the windows stay where they are, as ordinary windows of the screens and workspaces they are on now.
- **AC-10**: After a new sign in (reboot, crash, power cut, sign out) with a saved screen missing, a reopened window that restore (spec 0003) matches to that screen becomes an away window: placed at its spot's relative place on the main display (`map_rect`, as a fold does) and hidden into the main display's workspace N (in view if N is shown there), or left in view if it was in view or pinned. Saved windows whose app never reopens within the restore period are dropped at its end, as today. This replaces spec 0003 AC-8's "stays visible" for a missing screen. When the screen appears, even hours later, it goes back by AC-5.
- **AC-11**: When several screens go or come back in the same check (a dock with two screens), each is handled on its own and together they make one indicator note per screen and one balloon. If the main display itself goes away, its windows and the away windows on it fold onto the new main display; an away window keeps its first home screen.
- **AC-12**: A window that does not move back (its app is frozen, or it refuses the move) is tried again each second for 10 s, then left where it is. A window whose app is not answering (`hung`) is skipped without a move call, so des-k never waits on a frozen app, and the skip counts as a try. If its screen goes away again during the tries, the tries stop and the window stays away. It keeps its away record so the next return of that screen tries again, and the log and the balloon count it.
- **AC-13**: On a fold (AC-2) the indicator shows on the main display with a line such as "DELL U2720Q unplugged: 9 windows moved here", and on a return (AC-5) on the returned screen with "DELL U2720Q is back: 9 windows returned" (plus ", 1 could not be moved" when AC-12 applies). A tray balloon says the same, one line per screen when several change in one check. The name is the remembered friendly name, or "Screen N" when Windows gives none; "1 window", "2 windows". A fold or return that moves no window shows nothing (log only). The note replaces whatever the indicator shows and stays 3 s; for a fold whose main display is new (the old one left), it shows on the new one. A balloon never replaces the "des-k cannot hide windows" warning; then the note goes only to the indicator and the log.
- **AC-14**: A return never takes keyboard focus, never switches any other screen, and never triggers a follow jump (spec 0002).
- **AC-15**: A `state.txt` written by an older des-k (`DISPLAYn` names) loads without error: the name of each present screen becomes that screen's ID, and lines for screens not present are dropped, as today. That loss happens once, only if you upgrade while undocked.
- **AC-16**: Every screen has the same workspace count (spec 0012), so workspace N always exists on the main display. Lowering the count while a screen is away sends its away windows of removed workspaces to the new last workspace, the same rule as spec 0012 AC-6, and their home workspace becomes that one.
- **AC-17**: Every fold, return, kept window (AC-6), stuck window (AC-12), forgotten screen and screen ID is written to `des-k.log`.
- **AC-18**: For 30 s after the PC wakes from sleep, a screen must be missing for 30 checks, not 10, before it folds, since external screens can take a while to wake. Without this a slow screen would fold and then return a moment later.

## Decision

**Chosen option**: Option 1: Fold by workspace number onto the main display, remember each away window's home, and send it back by itself.

Each screen is keyed by its monitor ID; each window that leaves an unplugged screen carries an away record (home screen, home workspace, spot, pinned), saved in `state.txt` as an `A` line, and a returning screen takes its away windows back once it has stayed present for 2 s.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Feature design

**Words used here**: *home screen* is the screen a window belongs to. An *away window* is open on another screen because its home screen is unplugged. A *spot* is a window's position, size and state (normal, maximized, minimized). *Fold* is moving an unplugged screen's windows onto the main display. *Return* is moving them back. *Claim* is you moving an away window yourself, which ends its away record.

**Data model sketch** (`state.txt`, one line per item, fields split by spaces, the fingerprint after the first tab as today):

| Line | Fields | Notes |
|---|---|---|
| `L` | logon id, logon time | unchanged |
| `M` | screen id, active workspace, work area `left top right bottom`, TAB friendly name | Was `M <device> <active>`. Written for present and remembered screens. The work area is the last one seen, for relative spots (AC-5). |
| `W` | screen id, workspace, hwnd, pid, spot, TAB app TAB class TAB title | Screen id replaces the device name; spot added. |
| `P` | screen id, hwnd, pid, spot, TAB fingerprint | Same change. |
| `V` | screen id, spot, TAB fingerprint | Same change. |
| `A` (new) | home screen id, home workspace, hwnd, pid, spot, pinned `0`/`1`, TAB fingerprint | One per away window. hwnd `0` while a restore entry waits for its window, as `W` does. |

A *spot* is five fields: `x y w h state`, with `state` one of `n` (normal), `x` (maximized), `m` (minimized), in screen coordinates (pixels on the whole desktop). For a normal window (not `IsIconic`, not `IsZoomed`) `x y w h` is its window rect (`GetWindowRect`, so a snapped half keeps its snapped size); for a maximized or minimized one it is the restore rect from `GetWindowPlacement`, moved from workspace to screen coordinates as `move_to` does (a minimized window's own rect reads as -32000 and is never used). Cloaked windows are read the same way: cloaking does not move them.

A *screen id* is one token with no spaces or tabs: `internal`, or `MMM-PPPP-SERIAL` (maker letters, product code in hex, serial), with `#1`, `#2` added by left to right order when two present screens would share it, or the monitor's device instance id when the EDID cannot be read. Characters outside `A-Z a-z 0-9 - _ . #` become `_`.

In memory (`ws::State`):
- `monitors: HashMap<String, Monitor>` keyed by screen id (was the device name). `Monitor` gains `device: Option<String>` (the `DISPLAYn` name while present, `None` while remembered), `area: RECT` (last work area) and `name: String` (friendly name).
- `away: HashMap<isize, Away>` keyed by hwnd. `Away { home: String, ws: usize, spot: Spot, pinned: bool, pid: u32, tries: u32 }`. Created for a screen's windows in view at its first missing check (AC-3), and for its hidden windows at the fold (AC-2).
- `spots: HashMap<isize, (String, Spot)>`: each tracked window's last screen id and spot, refreshed each 1 s check (see invariants).
- `back: HashMap<String, u32>`: screens with away windows that are present again, and for how many 1 s checks in a row (the mirror of `gone`; display change calls do not count).
- `woke: Option<Instant>`: when the PC last woke (`WM_POWERBROADCAST` with `PBT_APMRESUMEAUTOMATIC` to the indicator window), for AC-18.
- `notes: Vec<Note>`: indicator notes and balloon lines made under the `ws` lock, shown by `check_screens` after it lets the lock go.

Relationships: a screen has many windows; an away record points to exactly one remembered screen; a window has at most one away record. Forget removes a screen and every away record pointing to it. A claim removes one away record.

**State transitions** (a screen):

```
present ──gone 1 check──▶ missing ──gone 10 checks──▶ remembered (folded)
   ▲                         │                              │
   └──back before 10 checks──┘ (AC-4: spots put back)       │
   ▲                                                        │
   └──────present 2 checks: return (AC-5)───────────────────┘
remembered ──Forget, or no away windows left──▶ forgotten
```

An away window: `away ──screen returns──▶ home` (moved back) · `away ──you move it──▶ claimed` (record dropped) · `away ──closed──▶ dropped at next save` · `away ──move fails 10 tries──▶ away` (kept for the next return).

**API surface** (functions; all run on the main thread under the `ws` lock, none public outside the crate unless marked):

| Function | Kind | Inputs | Outputs | Called by | Key failures |
|---|---|---|---|---|---|
| `ws::screen_ids()` | new | none | present screens as (device, id, friendly name, work area) | `check_screens`, `load`, `screen_info` | display config fails: fall back to the device name, log |
| `restore::edid_id(device_path)` | new, in `restore.rs` beside the fingerprint code | the target's `monitorDevicePath` | `Option<String>` maker, product, serial | `screen_ids` | registry key or EDID missing: `None`, use the instance id |
| `ws::check_screens(tick)` | changed | as today | folds (AC-2), returns (AC-5), spot refresh, claim check by screen change | display change, 1 s timer | per window failures logged and skipped |
| `ws::fold(s, id)` | new | a screen gone 10 checks | away records, windows moved to the main display | `check_screens` | uncloak or move fails: kept, retried next check |
| `ws::give_back(s, id)` | new | a screen back 2 checks | windows moved home | `check_screens` | move fails: AC-12 retries |
| `ws::claim(s, hwnd, why)` | new | a window you moved | its away record dropped, a log line | `send`, `place`, `bring`, `pin`, `check_screens` | none |
| `pub ws::remembered()` | new | none | `Vec<(id, name, waiting)>` for the settings page | `settings_page` | none |
| `pub ws::forget(id)` | new | a screen id | the screen and its away records dropped, saved | settings page Forget | unknown id: log, nothing |
| `indicator::show_note(mon, active, count, text)` | new | screen, dots, one line | the indicator with a line under the dots, 3 s, replacing what it shows | `check_screens` after the lock is released, from `notes` | none |
| `tray::screens_changed(lines)` | new, pub | one line per screen | a balloon, unless the broken warning shows | `check_screens` after the lock is released | balloon refused: log |

**Value sourcing**:

| Action | Value produced / displayed | Source |
|---|---|---|
| any | a screen's id | `QueryDisplayConfig` target: `outputTechnology` `INTERNAL`, `DISPLAYPORT_EMBEDDED` or `UDI_EMBEDDED` → `internal`; else `monitorDevicePath` (a monitor interface path) → its device instance through SetupAPI (`SetupDiGetClassDevs` for `GUID_DEVINTERFACE_MONITOR` with `DIGCF_PRESENT`, `SetupDiOpenDeviceInterface`, then `SetupDiOpenDevRegKey` on that device, `EDID` value) → maker (bytes 8 to 9), product (10 to 11), serial (descriptor tag `0xFF` text, else bytes 12 to 15 when not zero). `DIGCF_PRESENT` keeps a stale EDID of a monitor no longer attached out. On a clash, `#1`, `#2` by the screens' left edge |
| any | a screen's friendly name | `display_names()` (today's code) |
| fold | which screen takes the windows | the present screen with `MONITORINFOF_PRIMARY` |
| fold | a window's home workspace | its index in the gone screen's `hidden` lists; for a window in view, the gone screen's `active`; for a pinned one, `active` with `pinned` set |
| fold | which windows were in view on the gone screen | `spots`: every window whose last screen id is the gone screen and is not in its `hidden` lists |
| fold | where a folded hidden window goes | `map_rect(gone screen's area, main display's work area, its spot)` |
| return | a window's spot | its away record's `spot`, frozen when the screen went missing (invariant 2) |
| return | where on the returned screen | the spot as is when the screen's work area equals the saved `area`; else `map_rect(saved area, new area, spot)` |
| return | the workspace the screen shows | the remembered `Monitor.active` |
| return | still the same window | `IsWindow` and the saved pid both match |
| claim by screen change | that you moved it | an away window's current screen differs from its `spots` screen, both screens were present at the last check and are present now, and the current screen is not its home. des-k updates `spots` right after each of its own moves, so its moves never look like yours |
| fold timing | how many checks count as gone | `GONE_CHECKS` (10), or 30 while `woke` is under 30 s ago (AC-18) |
| adopt after restart | still the same window | hwnd and pid match, and the app and class of the `A` line's fingerprint match the window now |
| settings page | windows waiting | count of away records whose home is that screen |
| indicator, balloon | counts, names | the fold or return just done |
| after a new sign in | an away window's spot and home | the `A`, `W`, `V` or `P` line's spot and screen id, matched by restore (spec 0003) |
| state.txt upgrade | the id for a `DISPLAYn` name | the present screen with that device name, at load |

**Key invariants**:
1. Only windows des-k tracks are ever moved or hidden, and only windows des-k hid are ever uncloaked (AGENTS rule). A fold moves hidden windows while they stay cloaked, so nothing flashes.
2. Spots freeze when a screen goes missing: a check never refreshes the spot of a window whose last screen id is not present now. So the spot an away window carries is its last spot on its home screen, from at most 1 s before the unplug, sleep included (no check runs while the PC sleeps).
3. A window has at most one away record, and its home is the first screen it left; folding a screen that holds away windows of another screen keeps their home.
4. Every exit path still shows every hidden window, folded ones included (they are in the main display's lists). A Quit keeps `M` and `A` lines (AC-8); `--quit` rescue keeps `A` lines and drops `V` and `P` lines as today.
5. Nothing in `state.txt` names a program to run: the `A` line's app path is matched, never started, the same as the fingerprint today.
6. A return moves windows with `SWP_NOACTIVATE` and `SetWindowPlacement` without activation (`SW_SHOWNOACTIVATE`, `SW_SHOWMINNOACTIVE` for a minimized one), so focus and follow (spec 0002) are untouched. A window going to a workspace the returned screen does not show is cloaked first, then moved, so it never flashes; windows are hidden bottom of z order first and shown top first. After each move the rect is set once more when it drifted, as `move_to` does, since an app on a screen with another scale resizes itself.
7. Each check runs in one order: read present screens; claims (AC-6); closed windows; fold and return; then refresh `spots`. Notes and balloons are made under the lock and shown after it is released, so no tray or indicator call runs under the `ws` lock.
8. No move call goes to a window whose app is not answering (`hung`), so a frozen app can never stall the main thread; that applies to fold, return and restore placement alike.
9. Any window the check finds on a present screen belongs to that screen for switching, sending, the finder and the overview, as today; an away record only says where it goes later. No lookup ever needs a `Monitor` for a screen that is not present.

**Security model**: no new surface. The admin des-k only reads the display configuration and the monitor's EDID from the registry (read only). `state.txt` gains window positions and monitor IDs, which name no program and load no code. A screen id is cleaned to a safe character set before it is written, so a crafted EDID cannot break a line.

**Configuration required**: none. No new setting: the 10 check gone rule and the 2 check return rule are constants beside `GONE_CHECKS` (`BACK_CHECKS = 2`, `RETURN_TRIES = 10`).

**Critical test scenarios**:
- Happy path: three screens set up, unplug both externals, wait 10 s, use the laptop, plug them back: every window is back on its screen, workspace, spot and state, and each screen shows the workspace it showed. Verifies AC-2, AC-3, AC-5.
- Your move wins: while unplugged, send one folded window to laptop workspace 3 and drag another within the laptop: on replug the first stays on workspace 3, the second goes home to its old spot. Verifies AC-6.
- Power cut: unplug, pull the power, boot with only the laptop, let apps reopen, plug the screens in an hour later: the matched windows go home. Verifies AC-8, AC-10.
- Short drop: sleep with the screens on, wake: nothing folds, windows Windows moved during the drop are back on their spots. Verifies AC-4.
- Another port: plug the same monitor into a different dock port: it is the same screen and its windows return. Verifies AC-1.
- Failure: a frozen app's window does not move: 10 tries, then left with its record kept, "1 could not be moved" in the balloon, and des-k keeps switching the whole time. Verifies AC-12.
- Not your move: unplug and wait: nothing is reported as "stays, you moved it", although Windows moved every window off that screen. Verifies AC-6.
- Slow wake: sleep docked, wake, the externals take 15 s to light up: nothing folds. Verifies AC-18.
- Permission: no new access rule; a window des-k did not hide is never uncloaked by a fold or return. Verifies AC-2 (invariant 1).

## Build plan

Tracer Bullet: the first slice is the whole unplug and replug loop inside one run, end to end, then it gains memory across restarts, then reboots, then what you see.

1. **Screen ids**: `restore::edid_id`, `ws::screen_ids`, key `State.monitors` by id with `device`, `area`, `name`; `M`, `W`, `P`, `V` lines carry the id; load upgrades `DISPLAYn` names; log every screen's id. Unit tests: EDID bytes to id (maker letters, numeric and text serials, no serial), id cleaning, the clash `#UID` rule, the upgrade of an old file. Satisfies **AC-1**, **AC-15**, **AC-17**.
2. **Spots**: `Spot`, `spots` refreshed each 1 s check with the freeze rule; spots on `W`, `P`, `V` lines. Unit test: a window whose screen is missing keeps its spot. Satisfies **AC-5** (input), **AC-10** (input).
3. **Fold and return in one run** (the tracer bullet): `fold` replaces today's "show them all" in `check_screens`; away records; `back` counting and `give_back` with `map_rect` and placement (reusing `move_to`'s maximized handling, with the window rect for normal windows); pinned handling; AC-4's put back on a short drop; retries; the wake grace from `WM_POWERBROADCAST`; the `hung` skip; the check order of invariant 7. Unit tests: fold by number into the main display's lists, the shown workspace case, the return target when the area changed, the retry counter, the 30 check count after a wake. Satisfies **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-7**, **AC-11**, **AC-12**, **AC-14**, **AC-16**, **AC-18**.
4. **Claims**: `claim` from `send`, `place`, `bring`, `pin`, and the screen change check in `check_screens`. Unit tests: a moved away window is not returned, a window moved on its own screen is, a window Windows moved off a missing screen or onto its home is not a claim. Satisfies **AC-6**.
5. **Memory across restarts**: `A` lines saved and adopted by hwnd, pid, app and class; remembered `M` lines kept at load; Quit and `--quit` rescue keep them; a remembered screen with no away windows is forgotten. Unit tests: save and load round trip, `rescued` keeps `A` lines. Satisfies **AC-8**.
6. **After a new sign in**: restore entries carry spot and screen id; a match for a missing remembered screen becomes an away window instead of staying visible; `A` lines become restore entries. Satisfies **AC-10**.
7. **What you see**: `indicator::show_note`, `tray::screens_changed` (one balloon per check, never over the broken warning), the settings page's "Screens not plugged in" list with Forget (`ws::remembered`, `ws::forget`), and the `cargo test render` states for that list. Satisfies **AC-9**, **AC-13**.
8. **Docs**: README and site guide (what happens when you unplug), `AGENTS.md` rules (screen ids, the `A` line, the freeze rule) via `/sync`, spec 0003 gets a pointer that AC-8 changed. Satisfies **AC-17** (documented), all ACs (owner facing description).

## Consequences

**Positive**:
- Docking, sleeping, and power cuts stop costing you a manual re sort; the layout you built per screen survives.
- Screens are stable across ports and docks, which also fixes the deferred "Stable screen names" item for restore after reboot.
- Nothing des-k remembers is hidden: the settings page shows what it waits for.

**Negative / tradeoffs**:
- `state.txt` changes format (screen ids, spots, `A` lines). An older des-k reading a newer file drops lines it does not understand; going back a version loses the layout once.
- A fold hides windows you were not looking at into the laptop's workspaces, so with 3 screens the laptop's workspaces get crowded until you dock again.
- A remembered screen you never plug in again keeps its away records until you Forget it or its windows close.
- The EDID read adds a registry look up per screen on every display change; cheap, but it is new code that can fail on odd hardware (the instance id fallback covers it).
- Snap groups (Windows' own grouping of snapped windows) are not restored; a snapped window gets back its exact rect, which looks the same but is not a snap group.

**Neutral**:
- Windows' own "Remember window locations based on monitor connection" can stay on or off; des-k places windows after it, so des-k's spot wins.
- Spec 0003 AC-8's missing screen rule changes (AC-10 here); restore's matching itself is unchanged.

## Follow-up

- [ ] After the build, `/sync` adds the screen id, `A` line and freeze rules to `AGENTS.md` and moves "Stable screen names" out of the scope's Deferred list.
- [x] Spec 0003: add a one line pointer that AC-8's missing screen case is now governed by spec 0016 AC-10.

## Rationale

Reasoning and options: see [rationale.md](rationale.md).
