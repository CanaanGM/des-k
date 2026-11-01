# 0012. Settings page: rationale

## Context

Every pdesk setting lives in `settings.toml` (spec 0001). Changing one means opening the file, knowing its format, and restarting pdesk, because settings are read once at start. Spec 0011 added about 25 gesture lines, and the welcome screen's "Choose my own" sends you straight into that file. You asked for a page for all settings, not only the touchpad.

Forces that shaped the design:
- pdesk runs as admin. Nothing in `%LOCALAPPDATA%\pdesk` may name a program to run, and anything running as you can write that folder.
- Every comment in the default file explains a setting, and you may have added your own.
- Readers of settings are spread across the hook thread and the main thread, all through `settings::get() -> &'static Settings`.
- The workspace count sizes per monitor state in `ws.rs`. Shrinking it with hidden windows is the one change that can lose windows.
- There's no text editing anywhere in pdesk's custom drawn windows, and building it is a lot of code.
- You prefer designs that show all state rather than hide it, and the welcome screen set a look through a design pass.

## Options considered

### Option 1: Live settings, in place edits, custom drawn page (chosen)
Page edits values in the file with `toml_edit`; pdesk reloads on save and on file change, and swaps settings behind an atomic pointer.
- **Pros**: one source of truth; changes felt at once; comments kept; no reader changes.
- **Cons**: new crate; live workspace shrink is risky; leaked old settings.

### Option 2: Page writes the file, pdesk restarts itself
Save, then pdesk relaunches and adopts its layout.
- **Pros**: no live apply logic; settings stay read once.
- **Cons**: a blink and tray icon churn on every click; a self relaunch from an admin process has to keep the single instance rule and the layout handoff right.

### Option 3: Page writes, applies at next start
- **Pros**: least code.
- **Cons**: you can't feel a change; two rules (page vs restart) to remember.

### Option 4: Standard Win32 controls (edit, combo box, check box)
- **Pros**: text entry and accessibility for free.
- **Cons**: they don't follow pdesk's dark theme or its welcome look; mixing them with custom paint looks broken.

## Rationale

Live apply was your call, and it fits: `get()` returning `&'static` means an atomic swap of a leaked value changes nothing for readers, and the hook thread can read it safely without locks. The runner up, `RwLock<Arc<Settings>>`, would change every caller for no gain at this size.

In place edits keep the file trustworthy as documentation, which matters because the file is still the fallback when the page can't parse it. Rewriting the whole file was simpler but would erase your comments silently.

A 1 s write time check on the main thread beats a directory watcher thread: same result for one file, no new thread, no cross thread state. Runner up: `ReadDirectoryChangesW` on its own thread.

Recording shortcuts, steppers and pickers mean no typed text anywhere, which is what makes a custom drawn page in the welcome style affordable.

Moving windows from removed workspaces onto the new last one (your pick) keeps them hidden and reachable, instead of dumping them on screen. On a failed live parse, keeping the last good settings beats falling back to defaults: a typo mid edit shouldn't suddenly remap every shortcut. At start, spec 0001's defaults rule still applies, since there's nothing older to keep.

Touchpad off keeps the reader thread running and drops reports, rather than unregistering raw input. Starting and stopping the thread live adds failure paths for little gain.

## After the design pass (2026-11-01)

The approved r4 mockup left 14 points open (A1 to A14, from `pdesk-design/settings/reviews/parity-r4.md`). You chose four; the rest follow from the code.

Your picks:
- **A6, digit clash: take one digit.** Runner up: turn the whole set off (what r4 drew). The parser already lets a list take one digit from a set, and a unit test guards it on purpose ("must not cost the set its other eight digits"). Turning nine shortcuts off to place one would undo that. The cost: the skip stops being a logged problem and becomes a shown fact on the digit row, and the r4 clash copy changes.
- **A1, Esc with a modifier: record it.** The file accepts `Escape`, so refusing it would hide a value you can write by hand. Only a bare Esc cancels.
- **A5, indicator Play: keep.** `indicator::show` already exists, so it is small, and hold and fade are things you feel, not read.
- **A12, no undo.** The per screen line before you remove says what moves. An undo would grow the riskiest path (windows moved while hidden). Fits the Prototype tier.

Settled from the code:
- **A2, numpad and other keys: refuse.** The file can only name A to Z, 0 to 9, F1 to F24 and a few named keys (`vk_of`). Saving a numpad digit as a number row digit would make a combo the numpad doesn't fire. Digit sets still match both rows, as today.
- **A3, problems with no row go in a Notes card.** Unknown keys and `gesture rule N` have nowhere else to show; hiding them breaks AC-14.
- **A4, a row with no once rule shows Nothing.** The runtime does nothing for it, so Nothing is the truth; "not in the file" says why.
- **A7, off grid values are valid.** Spec 0001's ranges don't require the step, so the page must not flag them.
- **A8, keep the last parse error.** `watch()` drops `reload`'s error today, so the page can't know. A stored error, cleared by a good read, covers start and live alike.
- **A9, banner only for hand edits.** A change you just made on the page needs no banner; one from the editor does, since the page moved under you.
- **A10, screen names from DisplayConfig.** `ws.rs` keeps only the GDI device name. `DisplayConfigGetDeviceInfo` gives the monitor's own name and whether it is internal; the finder's left then top order keeps "Screen N" the same in both places.
- **A11, windows per workspace from the overview's sources.** `ws::windows()`, `real_rect` and `active()` already feed the overview. A refresh on the 1 s tick while visible keeps the "now" marker right after a switch made by shortcut.
- **A13, fixed art.** The scenes and the small overview teach what a setting means; your live windows add nothing there and the workspace pictures already show them.
- **A14, Not set apart from Nothing.** `recognized()` (`touchpad.rs`) waits for any twice rule, `none` included, so the two behave differently.
