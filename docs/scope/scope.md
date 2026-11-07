# Scope: des-k

des-k gives Windows 11 per screen virtual desktops like macOS and GNOME: each screen switches its own workspaces, and the other screens stay put. It is a personal tool for one person on a laptop with external screens.

**Build approach:** Tracer Bullet (each feature is built completely, from shortcut to window behavior to saved layout, and works before the next one starts).
**Workflow:** Prototype (nothing runs after `/develop`; you try each feature on your own screens). The project default level of rigor. `/architect` is the recommended first stop for a feature with a real decision, but skippable when you already know the build. Any feature can carry its own tag (e.g. `· Alpha`) to do more or less.

_These are recommendations to keep your build orderly, not requirements. Skip anything that does not fit: if you already know how to build a feature, use `/develop` and skip `/architect`. You decide when a feature is `done`._

## At a glance

| # | Feature | Phase | Status |
|---|---------|-------|--------|
| A | Per screen workspaces | Existing | existing |
| B | Switching by keyboard and swipe | Existing | existing |
| C | Copilot key as Right Ctrl | Existing | existing |
| D | Taskbar follows the workspace | Existing | existing |
| E | Workspace indicator | Existing | existing |
| F | Autostart and install | Existing | done |
| G | Layout survives restarts and undocking | Existing | in-progress |
| 1 | Coding standards and AGENTS.md | Foundation | done |
| 2 | Settings file | Foundation | done |
| 3 | Follow activation | Slice 1 | done |
| 4 | Send window to workspace | Slice 2 | done |
| 5 | Send window to screen | Slice 2 | done |
| 6 | Window finder | Slice 3 | done |
| 7 | Alt+Tab per workspace | Slice 4 | done |
| 8 | Tray icon | Slice 4 | done |
| 9 | Workspace overview | Slice 5 | done |
| 10 | Restore layout after reboot | Slice 4 | in-progress |
| 11 | Jump and send by number | Slice 2 | done |
| 12 | Keys work over admin windows | Slice 4 | in-progress |
| 13 | Touchpad gestures | Slice 6 | in-progress |
| 14 | Settings page | Slice 7 | in-progress |
| 15 | Setup exe installer | Slice 8 | in-progress |
| 16 | Site guide follows the installer | Slice 8 | done |
| 17 | Pin a window to every workspace | Slice 9 | in-progress |
| 18 | Check for updates from the tray | Slice 9 | in-progress |
| 19 | Return windows to their screens | Slice 10 | in-progress |
| 20 | Look B screens | Slice 11 | in-progress |

## Already built

### A. Per screen workspaces · existing
Four workspaces per screen, hidden through the shell's undocumented cloak call (works on admin windows too, no elevation needed). Windows are hidden bottom first so nothing flashes. code in `src/ws.rs`, `src/cloak.rs`

### B. Switching by keyboard and swipe · existing
Win+Ctrl+Left/Right switches the screen under the mouse through a low level keyboard hook on its own thread. The four finger swipe sends the same shortcut (set in Settings, Advanced gestures). code in `src/keys.rs`

### C. Copilot key as Right Ctrl · existing
The Copilot key (LWin, LShift, F23) becomes Right Ctrl, keeping any physically held Win or Shift, with no Start menu pop. code in `src/keys.rs`

### D. Taskbar follows the workspace · existing
Hidden windows lose their taskbar button and get it back when shown. code in `src/cloak.rs`

### E. Workspace indicator · existing
A dots popup in the middle of the switched screen, DPI aware, fades out. A redesign is wanted later (see Deferred). code in `src/indicator.rs`

### F. Autostart and install · done
Release build with no console, one copy at a time, log file, `--quit`, and `install.ps1` that registers start at login. Starting at login confirmed by you (2026-10-30). code in `src/main.rs`, `install.ps1`

### G. Layout survives restarts and undocking · in-progress
Saves each screen's workspace and every hidden window, and adopts them on start. Restart was confirmed working. A screen now counts as gone only after 10 one second checks in a row, so a short drop out (sleep, a DisplayPort relink) keeps its workspaces; neither the short nor the long case has been tried yet. A window that fails to show stays tracked, `--quit` with des-k stopped shows what `state.txt` says it hid, and after an explorer restart des-k hides its windows again (2026-11-03 review). code in `src/ws.rs`

## Foundations

### 1. Coding standards and AGENTS.md · done
Capture how des-k is built (raw COM vtable calls, the main thread vs hook thread split, never un-hide windows des-k did not hide) so later features follow the same rules.
**Done when:** a root `AGENTS.md` describes the modules, the threading rules, and the safety rules the code already follows.
- [x] Capture conventions: `/audit`

### 2. Settings file · done
One file for the shortcuts, the number of workspaces per screen, and indicator timing, so you can change them without rebuilding.
**Done when:** changing the workspace count or a shortcut in the file takes effect after a restart, and a broken file falls back to defaults with a line in the log.
spec [0001](../specs/0001-settings-file/index.md) · code in `src/settings.rs`
- [x] Design it (spec): `/architect settings file`
- [x] Build it: `/develop settings file`
  - [x] Settings file loads and the workspace count works end to end (AC-1 to AC-4, AC-10 to AC-13)
  - [x] Indicator timing and the Copilot switch (AC-8, AC-9)
  - [x] Configurable shortcuts (AC-5 to AC-7)
  - [x] Tried on your screens, then installed (all ACs)

## Slice 1: Follow activation

### 3. Follow activation · done
When a hidden window comes to the front (an app reusing its window, a notification click, Alt+Tab), its screen jumps to that window's workspace, so nothing is ever active but invisible.
**Done when:** opening a `.txt` file while Notepad sits on a hidden workspace switches that screen to it, and the indicator shows the jump. (Photos opens a new window per image on this machine, so it cannot show this.)
spec [0002](../specs/0002-follow-activation/index.md) · code in `src/ws.rs`, `src/main.rs`, `src/settings.rs`
- [x] Design it (spec): `/architect follow activation`
- [x] Build it: `/develop follow activation`
  - [x] Jump on activation end to end: switch by index, foreground hook, lookup, focus, save (AC-1 to AC-6, AC-10, AC-11)
  - [x] Cooldown, bounce guard, and the `follow_activation` setting (AC-7 to AC-9)
  - [x] Tried on your screens, then installed (all ACs)

## Slice 2: Moving windows

### 4. Send window to workspace · done
Move the focused window to the previous or next workspace on its screen with a shortcut, optionally following it there.
**Done when:** the shortcut moves the window, the layout file records the new place, and the follow or stay behavior matches the spec.
spec [0004](../specs/0004-send-window-to-workspace/index.md) · code in `src/ws.rs`, `src/keys.rs`, `src/settings.rs`, `src/indicator.rs`, `src/main.rs`
- [x] Design it (spec): `/architect send window to workspace`
- [x] Build it: `/develop send window to workspace`
  - [x] Follow send end to end: settings, hook, wrap, saved layout (AC-1, AC-3, AC-4, AC-7, AC-8, AC-10, AC-12)
  - [x] Stay send with the focus move and the follow guard (AC-2, AC-9, AC-11, AC-12)
  - [x] Stay repeat, one action per press, accent dot (AC-2, AC-5, AC-6)
  - [x] Tried on your screens, then installed (all ACs)

### 5. Send window to screen · done
Move the focused window to another screen, landing on that screen's current workspace with a sensible size across different scaling.
**Done when:** a window sent between your 150% laptop screen and an external screen keeps a sensible size and position, and maximized windows stay maximized.
spec [0006](../specs/0006-send-window-to-screen.md) · no code: Windows' own Win+Shift+Left/Right does it, des-k keeps that combo free
- [x] Design it (spec): `/architect send window to screen`
- [x] Tried on your screens (2026-10-28)

### 11. Jump and send by number · done
Go straight to workspace N with a shortcut, or send the focused window there, with or without following it.
**Done when:** Win+N jumps, Win+Shift+N and Win+Ctrl+N send, and the quick fix, too high and settings rules match the spec.
spec [0005](../specs/0005-jump-send-by-number/index.md) · code in `src/ws.rs`, `src/keys.rs`, `src/settings.rs`, `src/main.rs`
- [x] Design it (spec): `/architect jump and send by number`
- [x] Build it: `/develop jump and send by number`
  - [x] Jump end to end: settings, digit hook, too high and same workspace (AC-1, AC-2, AC-3, AC-7, AC-8, AC-9, AC-10)
  - [x] Numbered sends on the 0004 path, with the quick fix and guards (AC-3 to AC-10)
  - [x] Tried on your screens, then installed (all ACs)
  - [x] Default keys Win, Win+Shift, Win+Ctrl in the code and tests, then installed (AC-10)

## Slice 3: Window finder

### 6. Window finder · done
A search popup listing every window on every workspace and screen. Type a few letters, press Enter, and you land on it.
**Done when:** the popup opens by shortcut, filters as you type, and Enter switches the right screen to the right workspace and focuses the window.
spec [0007](../specs/0007-window-finder/index.md) · code in `src/finder.rs`, `src/ws.rs`, `src/keys.rs`, `src/settings.rs`, `src/main.rs`, `src/restore.rs`
- [x] Design it (spec): `/architect window finder`
- [x] Build it: `/develop window finder`
  - [x] Open, list and go end to end: setting, Win+F, popup, Enter on hidden and visible windows, closing (AC-1, AC-2, AC-7, AC-8, AC-11, AC-14)
  - [x] Finding and rows: fuzzy ranking, recency, program names, icons, navigation keys, click, notes, final look (AC-3 to AC-6, AC-12, AC-13)
  - [x] Shift+Enter brings a window to you, Ctrl+W closes it (AC-9, AC-10)
  - [x] Tried on your screens, then installed (all ACs)

## Slice 4: Living with it

### 7. Alt+Tab per workspace · done
Alt+Tab only lists windows on the workspaces you can see right now.
**Done when:** hidden windows do not appear in Alt+Tab, and they come back to Alt+Tab when their workspace is shown again or des-k quits.
spec [0009](../specs/0009-alt-tab-per-workspace.md) · no code: explorer already leaves cloaked windows out of Alt+Tab and Task View; hidden admin windows still show, and picking one follows to its workspace
- [x] Design it (spec): `/architect alt tab per workspace`

### 8. Tray icon · done
An icon near the clock showing des-k is running, with Quit and Open log (and Open settings once the settings file exists).
**Done when:** the icon appears at start, Quit shows every window and exits, and the icon survives an explorer restart.
code in `src/tray.rs`, `src/main.rs`
- [x] Build it: `/develop tray icon`
  - [x] Tried on your screens: icon shows, Open settings and Open log work (2026-10-26)
  - [x] Still to try: Quit shows every window and exits, icon comes back after an explorer restart (2026-10-28, as admin)

### 10. Restore layout after reboot · in-progress
After a reboot or a sign out every window gets a new handle; des-k matches reopened windows to their saved fingerprint (program, class, title) and puts them back on their workspaces for the first few minutes after you sign in.
**Done when:** after a reboot, windows that reopen land back on their saved workspaces, a window des-k is unsure about stays visible, and the log says what was restored.
spec [0003](../specs/0003-restore-layout-after-reboot/index.md) · code in `src/restore.rs`, `src/ws.rs`, `src/settings.rs`
- [x] Design it (spec): `/architect restore layout after reboot`
- [ ] Build it: `/develop restore layout after reboot`
  - [x] Check step, then the thin restore: fingerprints, sign in detection, hide on a match (AC-1, AC-2, AC-3, AC-6, AC-7, AC-8, AC-13, AC-14, AC-15, AC-17)
  - [x] Full matching, focus guard, resume after a cut, `[restore]` settings, log lines (AC-4, AC-5, AC-9 to AC-12, AC-16)
  - [ ] Tried on your screens after a reboot, then installed (all ACs)

### 12. Keys work over admin windows · in-progress · from spec 0008
des-k runs as admin from a logon task, so an admin window with focus (Task Manager, an admin terminal) no longer stops its shortcuts, and Win+Ctrl+Right never falls through to Windows' own desktops.
**Done when:** with Task Manager focused, Win+Ctrl+Right switches des-k's workspace, des-k starts as admin at login with no prompt, and the exe lives in Program Files.
spec [0008](../specs/0008-run-des-k-as-admin/index.md) · code in `install.ps1`, `src/main.rs`, `src/tray.rs`, `src/indicator.rs`, `src/settings.rs`
- [x] Design it (spec): `/architect admin windows`
- [ ] Build it: `/develop keys work over admin windows`
  - [x] Admin des-k by hand: start log line, message filters, settings in the data folder, tried from an admin terminal (AC-1, AC-2, AC-5, AC-6, AC-7, AC-9)
  - [x] Install as admin: Program Files, logon task, one UAC prompt, cleanup, running check (AC-3, AC-4, AC-5, AC-10)
  - [ ] Editor opens as you, then tried on your screens after a sign out (AC-8, all ACs)

## Slice 5: Overview

### 9. Workspace overview · done
A per screen view of all its workspaces as live thumbnails, like Task View but for one screen, to pick a workspace or a window.
**Done when:** the overview opens on the screen under the mouse, shows each workspace's windows, and clicking one switches there.
spec [0010](../specs/0010-workspace-overview/index.md) · code in `src/overview.rs`, `src/ws.rs`, `src/keys.rs`, `src/settings.rs`
- [x] Design it (spec): `/architect workspace overview`
- [x] Build it: `/develop workspace overview`
  - [x] Hidden window preview check, then the thin thread: Win+Tab, a popup per screen, tiles, spread, click to go or switch (AC-1, AC-3, AC-4, AC-6, AC-12, AC-13, AC-14)
  - [x] Strip mini windows, near real place spread, cards, labels setting (AC-2, AC-3, AC-4, AC-5)
  - [x] Keys, selection, closing windows (AC-8, AC-9)
  - [x] Drag and drop across screens (AC-7)
  - [x] Live setting and fade animation, with their settings tests (AC-10, AC-11, AC-14)
  - [x] Tried on your screens and installed (all ACs), confirmed by you (2026-11-03)

## Slice 6: Touchpad

### 13. Touchpad gestures · in-progress
des-k reads three and four finger swipes and taps itself through Raw Input and runs the action your `settings.toml` map names, over every window (the Windows shortcut route dies over Task Manager). A welcome screen turns Windows' own gestures off; Quit gives them back.
**Done when:** with Task Manager focused, a four finger swipe switches des-k's workspace, every gesture in the map runs its action, and Quit gives Windows its gestures back.
spec [0011](../specs/0011-touchpad-gestures/index.md) · code in `src/touchpad.rs`, `src/gestures.rs`, `src/settings.rs`, `src/setup.rs`, `src/main.rs`, `src/tray.rs`, `src/welcome.rs`
- [x] Design it (spec): `/architect native touchpad gestures`
- [ ] Build it: `/develop touchpad gestures`
  - [x] Check step on your touchpad, then the thin thread: four finger left and right over Task Manager (AC-1, AC-11, AC-13)
  - [x] The map: `[touchpad]`, `[[gesture]]` rules, twice, every action (AC-2 to AC-7)
  - [x] Setup logic: registry gate, save, write back on Quit and same sign in rewrite, Touchpad setup entry (moved from the tray to the settings page, 2026-11-02) (AC-6, AC-8, AC-9, AC-10)
  - [x] Welcome screen to the approved H2 design: Q1 check first, then the window, four rows, give back results; plus the 2026-11-01 change (what des-k is, the map in each row, Use these gestures or Choose my own) (AC-8, AC-9, AC-11)
  - [ ] Hot plug, README, installed and tried on your screens (AC-12, all ACs)

## Slice 7: Settings page

### 14. Settings page · in-progress
A des-k window where you change every setting by clicking, instead of editing `settings.toml` by hand: workspaces, shortcuts, the indicator, restore, the overview and touchpad gestures. It opens from the tray and from the welcome screen's "Choose my own".
**Done when:** every setting in `settings.toml` can be changed from the page, the tray opens it, "Choose my own" opens it at the gestures, and the page says when each change takes effect.
spec [0012](../specs/0012-settings-page/index.md) · code in `src/settings_page.rs`, `src/settings.rs`, `src/main.rs`, `src/tray.rs`, `src/keys.rs`, `src/welcome.rs`
- [x] Design it (spec): `/architect settings page`
- [ ] Build it: `/develop settings page`
  - [x] Live settings core and a one toggle tracer page, tray opens it (AC-1, AC-3, AC-4, AC-5, AC-14, AC-15, AC-17)
  - [x] Design pass approved (`pdesk-design/settings/`, r4, 2026-11-01)
  - [x] Every plain setting, screen pictures, live workspace resize, banner and touchpad status, keyboard and themes (AC-2, AC-4, AC-6, AC-10, AC-11, AC-14, AC-16, AC-18 to AC-21)
  - [x] Shortcut recorder, chips and clash prompt (AC-7, AC-8, AC-9)
  - [x] Gesture rows and Choose my own opens the page (AC-12, AC-13)
  - [ ] Wrap up: README, installed and tried on your screens (all ACs)

## Slice 8: Distribution

### 15. Setup exe installer · in-progress · from spec 0013
des-k installs from a double clicked `des-k-vX.Y.Z-setup.exe` instead of a PowerShell command, and uninstalls from Settings, Apps, giving every hidden window back first.
**Done when:** the setup installs, updates keeping the layout, and uninstalls on your machine, and a tagged release carries the setup exe and its checksum.
spec [0013](../specs/0013-setup-exe-installer.md) · code in `des-k.iss`, `.github/workflows/build.yml`, `README.md`
- [x] Design it (spec): `/architect setup exe installer`
- [ ] Build it: `/develop setup exe installer`
  - [x] Installer, uninstaller, release job and README built (AC-1 to AC-9)
  - [ ] Tried on your machine: fresh install, update with a layout, uninstall, install again (AC-1, AC-2, AC-5, AC-6)
  - [ ] First tagged release carries the setup exe and its checksum (AC-8)

### 16. Site guide follows the installer · done · from spec 0013
The online guide still tells people to unzip and run `install.ps1`; it should match the README: download the setup exe, get past SmartScreen, uninstall from Settings, Apps.
**Done when:** `site/guide-content.html` and `site/index.html` describe the setup exe and the uninstaller, with the script kept as the alternative.
- [x] Build it: `/develop site guide follows the installer`
spec [0013](../specs/0013-setup-exe-installer.md) · code in `site/guide-content.html`, `site/index.html` (same body, edit both together)

## Slice 9: From the review

### 17. Pin a window to every workspace · in-progress · from spec 0014
Win+Ctrl+U keeps the focused window in view on every workspace of its screen (a call, music, notes); again unpins it.
**Done when:** a pinned window stays through switches both ways, survives a restart, and a send unpins it.
spec [0014](../specs/0014-pin-window.md) · code in `src/ws.rs`, `src/keys.rs`, `src/settings.rs`, `src/indicator.rs`
- [x] Design it (spec): `/architect pin window`
- [ ] Build it: `/develop pin window`
  - [x] Pin and unpin, switches skip pinned windows, the indicator, a send unpins (AC-1, AC-2, AC-4, AC-6, AC-7)
  - [x] `P` lines: kept over a restart, matched after a reboot (AC-5)
  - [ ] Tried on your screens (all ACs)

### 18. Check for updates from the tray · in-progress · from spec 0015
A tray item asks GitHub whether a newer des-k is out and answers in a balloon; a click on it opens the release page. Nothing runs by itself, nothing is downloaded.
**Done when:** on your machine a check says newer (a dev build set to `0.0.1`), newest, and why when offline, and the page opens in your browser.
spec [0015](../specs/0015-check-for-updates/index.md) · code in `src/update.rs`, `src/tray.rs`, `Cargo.toml`
- [x] Design it (spec): `/architect check for updates`
- [ ] Build it: `/develop check for updates`
  - [x] Version, tag and answer rules with unit tests (AC-2, AC-3, AC-4, AC-7)
  - [x] WinHTTP fetch on a worker thread, the posted answer, log lines (AC-1, AC-4, AC-6, AC-8)
  - [x] Tray item, balloons, balloon click, the broken warning kept (AC-1, AC-2, AC-3, AC-6, AC-7, AC-9)
  - [x] AGENTS.md, README, site guide (AC-5, AC-7)
  - [ ] Tried on your machine: newer, newest, offline (all ACs)

## Slice 10: Docking

### 19. Return windows to their screens · in-progress · from spec 0016
Unplugging a screen folds its windows onto the main display by workspace number; plugging it back (after a sleep, a reboot, a crash or a power cut too) sends each window home to its workspace, spot and state, unless you moved it. Screens are known by the monitor's own ID, and the settings page lists the ones des-k is waiting for.
**Done when:** with your three screens, undock, use the laptop, dock again: every window is back where it was; the same after a power cut while undocked; a window you sent while undocked stays put.
spec [0016](../specs/0016-return-windows-to-their-screens/index.md) · code in `src/ws.rs`, `src/restore.rs`, `src/indicator.rs`, `src/tray.rs`, `src/settings_page.rs`
- [x] Design it (spec): `/architect return windows to their screens and workspaces after unplug, replug, sleep or crash`
- [ ] Build it: `/develop return windows to their screens`
  - [x] Monitor IDs and spots: screens keyed by ID, positions saved, old `state.txt` upgraded (AC-1, AC-15, AC-17)
  - [x] Fold and return in one run, with your moves winning, retries and the wake grace (AC-2 to AC-7, AC-11, AC-12, AC-14, AC-16, AC-18)
  - [x] Remembered across restarts, quits, reboots and crashes (AC-8, AC-10)
  - [x] Indicator note, balloon, settings page list with Forget, docs (AC-9, AC-13, AC-17)
  - [ ] Tried on your screens: undock and dock, sleep, power cut while undocked (all ACs)

## Slice 11: Look B

### 20. Look B screens · in-progress · from spec 0017
Every des-k window rebuilt to its approved look B design (first run, Shortcut hints, settings page, indicator, finder, overview, tray menu), one shared drawing layer in `ui.rs`, Narrator support, a `fastest` switch and an overview fade time, released together as 0.4.0.
**Done when:** each screen matches its approved design in light, dark and high contrast on your screens, Narrator reads them, `fastest` makes every fade instant, and the overview logs at most 100 ms on the screens you have.
spec [0017](../specs/0017-look-b-screens/index.md) · code in `src/ui.rs`, `src/uia.rs`, `src/indicator.rs`, `src/finder.rs`, `src/overview.rs`, `src/wallpaper.rs`, `src/tray.rs`, `src/tray_menu.rs`, `src/hints.rs`, `src/settings_page.rs`, `src/welcome.rs`, `src/settings.rs`, `src/keys.rs`, `src/main.rs`, `src/ws.rs`
- [x] Design it (spec): `/architect the new screens`
- [ ] Build it: `/develop look b screens`
  - [ ] Design side first: cross flow review of the seven boards, and the rows for `fastest` and the overview fade time approved (AC-11)
  - [ ] Indicator with the shared layer: tokens, fonts, popup helper, theme check, motion, screen names, the start of the Narrator layer, `fastest` and `[indicator] style` (AC-1 to AC-6, AC-10)
  - [ ] Finder and overview, with `[overview] fade_ms`, the repaint rules and the timing log line (AC-1, AC-4 to AC-8, AC-10)
  - [ ] Tray menu popup and Shortcut hints, with `[shortcuts] hints`, `WM_APP + 14` and version 4 tray messages (AC-1, AC-4, AC-5, AC-10)
  - [ ] Settings page and first run, with `first_run_seen`; then 0.4.0 (AC-1, AC-2, AC-5, AC-9 to AC-12)

## Deferred
Out of scope for the current build pass, kept so the plan stays honest.
- **Indicator redesign**: a nicer popup look, asked for later · needs a decision
- **Dynamic workspaces**: GNOME style, always one empty at the end (you chose a fixed count for now) · needs a decision
- **Per app follow exclusions**: a `settings.toml` list of apps that never trigger follow activation, only if one keeps stealing focus (from spec 0002) · needs a decision
- **Dimmed wallpaper behind the overview**: draw your wallpaper instead of solid dark (from spec 0010) · needs a decision
- **Mouse wheel in the window finder**: scroll the finder's list with the wheel, only if the keys feel slow with many windows (from spec 0007) · needs a decision
- **Relaunch apps after a reboot**: start apps that don't reopen their own windows, so restore has something to match (from spec 0003) · needs a decision
- **Stable screen names**: `DISPLAY1` and `DISPLAY2` can swap after a reboot or a dock change; key saved screens by something stable, only if they get mixed up (from spec 0003) · needs a decision
- **Restart after a crash**: Task Scheduler's restart on failure does not restart a task whose program exits with an error code (tried 2026-11-03), so it would not help; the Start menu entry starts des-k again by hand · needs a decision
- **Overview speed on the 4K screens**: measure the overview again against spec 0017 AC-8 when the three 43 inch screens arrive; if it misses, Direct2D for the overview alone (from spec 0017) · needs a decision
- **Code signing**: sign the setup exe and `des-k.exe` so SmartScreen stops warning (SignPath Foundation, Azure Trusted Signing, Certum open source, plus a winget manifest), put off for now (from spec 0013) · needs a decision

## Legend

**The decision box.** Every feature carries exactly one, the sub task whose label ends with `(spec)`. Skills locate it by that `(spec)` suffix, never by an exact label. Every other box is an execution box and `/architect` never ticks one.

**Feature lifecycle**: the scope updates as a feature moves; each row is what it shows and who sets it:

| State | Set by | The feature shows |
|---|---|---|
| `planned` · needs a decision | `/scope` | one box: `Design it (spec): /architect <feature>` |
| `in-progress` (designed) | **`/architect` at spec capture** | `Design it` ticked; spec linked; `Build it: /develop <feature>` + **2 to 5 milestones** |
| `in-progress` (building) | `/develop` | milestone sub boxes tick one by one; code pointer filled |
| `done` | **you, when you decide it is**; `/sync` reconciles | with the Prototype workflow, the suggested point is right after `/develop`, once you have tried it |

- **Next step** = the first unticked box (always a command or a tracked milestone).
- **needs a decision** = run `/architect` first; otherwise straight to `/develop` (or `/audit` for standards). The tag drops once the spec is captured.
- **Atomic build tasks live in the spec's `## Build plan`, not here**: the scope carries only the milestone rollup.
- **Status** `planned` → `in-progress` → `done`, plus `existing` (built before this workflow) and `dropped` (de-scoped, kept for history).
- **Workflow tier tag** beside a heading (e.g. `· Alpha`) sets that one feature's rigor; no tag inherits the default.
- **Workflow** (header line) is the project default, what runs after `/develop`: **Prototype** = nothing (trust develop's own self check and your eye); **Alpha** = `/check verify`; **Beta** = `/check verify` then `/test`; **GA** = adds a fresh model `/check review` then `/document`.
- **Pointer line** (`spec <n> · code in <path>`): the spec link added by `/architect`, the code path by `/develop`.
