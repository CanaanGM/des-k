# 0011. Touchpad gestures: pdesk reads the touchpad itself and replaces Windows' three and four finger gestures

**Date**: 2026-10-31
**Updated**: 2026-11-02, [spec 0012](../0012-settings-page/index.md) makes settings live and adds the settings page: Choose my own opens the page at Touchpad, and the welcome screen no longer says edits wait for the next start.
**Status**: In Progress

## Summary

Today a four finger swipe reaches pdesk as keys that Windows types for it, and Windows will not type into an admin window, so the swipe dies over Task Manager. pdesk will read the touchpad itself through Raw Input (Windows hands pdesk the raw finger positions), spot each three and four finger swipe and tap, and run the action your `settings.toml` map names for it, over every window. A welcome screen on first start helps you set Windows' own three and four finger gestures to Nothing, and pdesk acts only while it reads them all off, so the two never fight. Quit gives your old settings back from your next sign in.

## Structure

- [0011-touchpad-reader.md](0011-touchpad-reader.md): the gesture thread, Raw Input, reading precision touchpad reports, and recognizing swipes, taps and "twice". Supports the decision to read the touchpad natively.
- [0011-gesture-map.md](0011-gesture-map.md): the `[touchpad]` settings and the `[[gesture]]` rules, every action a gesture can run, and how each runs. Supports the decision that you, not Windows, choose what each gesture does.
- [0011-first-run-setup.md](0011-first-run-setup.md): the welcome screen, turning Windows' gestures off and back on, and the tray item. Supports the decision that pdesk owns the gesture setup.

## Cross child contract

- **Reader → map**: the reader recognizes a gesture as `(fingers 3|4, motion left|right|up|down|tap)` and asks the map for the rules of that gesture; it never knows actions. It posts `WM_GESTURE` (`WM_APP + 8`, a thread message to the main thread) with `wparam` = the index of the matched rule in `settings::get().gestures`. The main thread runs that rule's action.
- **Setup → reader**: the reader acts only while `touchpad_active()` is true: `[touchpad] enabled` is true AND `windows_off()` (all 13 Windows gesture values read off in the registry, once per recognized gesture). Otherwise it posts nothing, so a skipped or half done setup never causes a double action.
- **Setup → exit paths**: Quit, `--quit`, a panic on any thread, and the console Ctrl+C handler write your saved Windows gesture values back (only while saved values exist and `windows_off()`), beside showing every hidden window (the existing AGENTS.md rule). Sign out and shutdown (`WM_ENDSESSION`) do not.
- New thread messages to the main thread: `WM_GESTURE` (`WM_APP + 8`) and `WM_TOUCHPADS_CHANGED` (`WM_APP + 9`).
- The target screen of every screen based action is the screen under the mouse, as with the keys.

## Requirements

**User stories**:
- As the pdesk user, I want my touchpad gestures to work over admin windows like Task Manager, as my keys already do.
- As the pdesk user, I want to choose what every three and four finger swipe and tap does, including a gesture done twice, instead of Windows' fixed choices.
- As the pdesk user, I want pdesk to set this up for me on first start and give Windows its gestures back when pdesk is not running.

**Acceptance criteria** ("gesture" means a three or four finger swipe left, right, up or down, or a three or four finger tap; "the map" means the `[[gesture]]` rules, or the default map when none are written):
- **AC-1**: With Windows' three and four finger gestures set to Nothing, a four finger swipe left switches the screen under the mouse to the next workspace and right to the previous one, with Task Manager focused exactly as with Notepad focused. One swipe moves one workspace, however long the swipe.
- **AC-2**: The default map (used when `settings.toml` has no `[[gesture]]` rule): 4 left `next`, 4 right `previous`, 4 up `overview`, 4 down `show_desktop`, 3 left `app_next`, 3 right `app_previous`, 3 up `overview`, 3 down `show_desktop`, 3 tap `search`, 4 tap `notifications`.
- **AC-3**: Each `[[gesture]]` rule names `fingers` (3 or 4), exactly one of `swipe` (`"left"`, `"right"`, `"up"`, `"down"`) or `tap = true`, optional `times` (1 or 2, default 1), and `do` (an action name from 0011-gesture-map, or a key combo written like the `[shortcuts]` combos). Writing any rule replaces the whole default map.
- **AC-4**: A bad rule (unknown action, bad combo, fingers not 3 or 4, both or neither of swipe and tap, times not 1 or 2, an unknown field) is skipped with one log line naming it; the other rules still work. A rule repeating an earlier rule's fingers, motion and times is skipped with a log line; the first wins.
- **AC-5**: A gesture with no `times = 2` rule acts the moment it is recognized. A gesture with a `times = 2` rule waits `twice_ms`: if the same gesture comes again within that time, only the twice action runs; otherwise only the once action runs (nothing, if there is no once rule).
- **AC-6**: `[touchpad]` keys: `enabled` (default `true`; `false` reads nothing), `swipe_mm` (default 15, 5 to 50: how far the fingers travel before a swipe counts), `twice_ms` (default 300, 100 to 1000). A bad value resets just that key with a log line (spec 0001).
- **AC-7**: Every action in 0011-gesture-map runs as its description says, over admin windows too. A key combo is typed as if you pressed it, so a combo that is a pdesk shortcut runs that shortcut, and over an admin window the combo reaches that window (an accepted risk, see Consequences).
- **AC-8**: On the first start with no `touchpad.txt`, a welcome screen opens. It says in plain words what pdesk is and shows what each three and four finger gesture will do (the live map). "Use these gestures" saves your current Windows three and four finger gesture values (the first time only) and opens Settings at Touchpad, asking you to set them all to Nothing; "Choose my own" does the same and also opens pdesk's settings page at Touchpad (spec 0012), and the screen says changes apply at once. pdesk acts on a gesture only while it reads all of them off in the registry, so a skipped or half done setup never acts twice; with one still on, the gesture logs which one and does nothing. "Skip" closes it. Either way it does not open on its own again.
- **AC-9**: The tray menu has "Touchpad setup", which opens the welcome screen any time. It shows, for each of three finger swipes, three finger taps, four finger swipes and four finger taps, whether Windows still runs it, it is set to Nothing, or pdesk cannot read it, and whether pdesk reads them. While your values are saved, it offers "Give gestures back to Windows": it writes them back (Windows uses them from your next sign in), forgets them, stops reading at once, and opens Settings at Touchpad so you can have them now. The screen then says, for each of the four, whether it was written back; if any write failed it keeps your values and offers to try again.
- **AC-10**: Quit (tray or `--quit`), a panic on any thread, or Ctrl+C on a dev console writes your saved values back while Windows' gestures read off (a value that did not exist is deleted again); Windows uses them from your next sign in. Sign out and shutdown write nothing. A start in the same sign in after such an exit, with `enabled` true and the registry still holding exactly what the exit wrote, sets the off values again, so gestures keep working. A hard kill writes nothing. After "Give gestures back", no exit writes anything. With `enabled` false while Windows' gestures read off, start logs that you have no three and four finger gestures and how to get them back, and the setup screen says the same. If build step 4's check shows a sign in does not pick up written values, no exit writes anything (fallback in 0011-first-run-setup).
- **AC-11**: Two finger scroll, zoom and right click, and one finger pointing and tapping, keep working exactly as before. With no precision touchpad, the log says so, the welcome screen says so and disables its gesture button, and nothing else changes.
- **AC-12**: Plugging in or removing a touchpad (a USB or Bluetooth one) is picked up without a restart.
- **AC-13**: The log gets one line per recognized gesture and the action it ran (or "no rule"), one line at start naming each touchpad found, and one line per setup change. Settings changes apply live (spec 0012).

## Decision

**Chosen option**: Option 2: pdesk reads the touchpad through Raw Input, recognizes gestures itself, and owns Windows' gesture setting through a welcome screen.

Build the three children in the order of the Build plan; the reader with the default map already fixes the Task Manager bug.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Critical test scenarios

- Happy path: Windows' gestures set to Nothing, Task Manager focused, four finger swipe left then right: next, then previous, on the screen under the mouse, verifies **AC-1**.
- Twice: a `times = 2` rule on 4 left; one swipe acts after about `twice_ms`, two quick swipes run only the twice action, 4 right (no twice rule) stays instant, verifies **AC-5**.
- Bad map: a rule with `fingers = 5` and one with `do = "explode"` are logged and skipped, the rest work, verifies **AC-4**.
- Exit: Quit from the tray and start pdesk again without signing out: gestures still work through pdesk. Quit, sign out and in: Windows' own three finger swipe works again and pdesk logs a skipped gesture, verifies **AC-10**.
- No double: setup skipped, Windows' custom shortcut still set: one swipe switches once (only the typed keys act). All set to Nothing but one: pdesk acts on none and logs that one, verifies **AC-8** and the cross child gate.
- Combo: `do = "Win+Ctrl+Right"` behaves like `next`, verifies **AC-7**.

## Build plan

Tracer Bullet: the first slice runs end to end, from touchpad to a switched workspace over Task Manager, before the map grows and the screen gets designed.

1. **Check step (no product code kept)**, the facts the rest rests on, each logged and noted in this spec's Follow-up when done:
   - a dev build registered for touchpad Raw Input as admin gets reports while Task Manager has focus;
   - it gets them while Windows' four finger swipe is set to Nothing and while it is set to a custom shortcut;
   - you change the three and four finger gestures to Nothing in Settings while a script exports `HKCU\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad` before and after; the diff names the exact values setup must write (not documented by Microsoft);
   - writing those values back from a script takes effect without a sign out, trying a plain write, then a `WM_SETTINGCHANGE` "PrecisionTouchPad" broadcast (pass: the next swipe follows the new value). If none passes, setup reads the registry instead of writing it (0011-first-run-setup);
   - giving focus to a window from a gesture (the `app_next` path) works while another app has focus.
   If the dev build gets no raw reports at all, stop: the decision falls back to Option 1 (the key route plus a README note) and comes back to `/architect`. Satisfies **AC-1**, **AC-8**.
2. **Thin thread**: the gesture thread, reader and recognizer (0011-touchpad-reader), four finger left and right only, posting `WM_GESTURE` with the default rules hard wired, the main thread running `next` and `previous`; gated on a temporary `[touchpad] enabled`. Tried over Task Manager. Satisfies **AC-1**, **AC-11**, **AC-13**. **Done 2026-10-31**: built, and tried over Task Manager and an admin terminal (one switch per swipe, focus lands, other swipes log "no rule"); `[touchpad] enabled` defaults to false until step 4 adds the `windows_off()` gate.
3. **The map**: `[touchpad]` and `[[gesture]]` parsing with tests, the full default map, three fingers, taps, `times = 2`, every action in 0011-gesture-map, the default settings text. Satisfies **AC-2** to **AC-7**. **Done 2026-10-31**: built, parsing tests and clippy clean; the actions and the twice wait not yet tried with real swipes.
4. **Setup without the screen**:
   - First a check (no product code kept): with Settings at Nothing, a script writes the check step's "before" values back (deleting the absent ones); sign out and in. Pass: Windows' own three finger swipe works again. Also confirm a change in Settings shows in the registry within a second. If the sign in does not pick the values up, build the fallback in 0011-first-run-setup (no exit writes anything) and note it in Follow-up.
   - Then: `touchpad.txt` (saved values, `restored`), `windows_off()` and the per gesture gate, "Use pdesk's gestures" (save, open Settings), the write back on Quit, `--quit`, panic and Ctrl+C, the same sign in rewrite at start, the `enabled` false warning, "Give gestures back", `[touchpad] enabled` defaulting to `true` (AC-6, safe now that the gate needs Windows' gestures off), and the tray item opening a plain placeholder that shows the state and the two actions. Satisfies **AC-6**, **AC-8** (logic), **AC-9** (logic), **AC-10**. **Done 2026-10-31** (`src/setup.rs`, exit hooks in `src/main.rs`): the sign in check passed, so the write back was built, not the fallback. Built, tests and clippy clean; not yet tried with a real Quit and restart.
5. **The welcome screen**, built to the approved H2 design (0011-first-run-setup, Welcome screen):
   - First the Q1 check: set each of the four Settings controls to Nothing, one at a time, while a read only script logs the 13 registry values. If a value is not set by exactly one control, stop and come back to `/architect`. **Done 2026-11-01**: each value moved only with its own row (swipes direction by direction, each tap control as a pair), so the grouping holds.
   - The window on the main thread (replacing the placeholder box and its thread), GDI+ started once, owner drawn buttons through `IsDialogMessageW`, the four rows from the 13 reads, every state in the States table, light, dark and high contrast. **Built 2026-11-01** (`src/welcome.rs`): every state drawn offscreen (`cargo test render -- --ignored`) in light, dark and high contrast at 100% and 150% and compared with the design file; not yet opened on a real screen.
   - `give_back` returns its result (written, part, none, save failed) and the screen keeps it until exit. **Built 2026-11-01**, with tests.
   - **Reopened 2026-11-01**, after you opened the installed screen: add the plain lines saying what pdesk is, each row's "pdesk does" text from the live map with its compiled label table (a test fails if an action has no label), "Use these gestures" plus "Choose my own" in place of the "Change what each gesture does" link, and the "applies from next start" line (0011-first-run-setup, Change of 2026-11-01). The screen grows to its content (about 850 dip at most, no 640 dip cap) and opens at the top of a screen too small for it. Then update the README's touchpad section to the new button names. **Built 2026-11-01**, with the label test; README updated, installed.
   - **Reopened 2026-11-02** (spec 0012): Choose my own opens the settings page at Touchpad (built in spec 0012 step 6); the line under the rows and the `enabled = false` texts change to the 0011-first-run-setup wording of 2026-11-02 (`src/welcome.rs`), and the render test images are made again. **Built 2026-11-02**, render images made again, installed.
   - Tried at 100% and 150%, in light, dark and high contrast, by keyboard only (Tab, Enter, Esc) and with the mouse, and you approve the new parts on the real screen. Satisfies **AC-8**, **AC-9**, **AC-11**.
6. **Hot plug, README and install**: `WM_INPUT_DEVICE_CHANGE`, README (touchpad section, uninstall steps start with Quit), `install.ps1` run, tried on your screens. Satisfies **AC-12**, all ACs. **Built 2026-11-01**: hot plug was already in `src/touchpad.rs` (device cache add and drop, `WM_TOUCHPADS_CHANGED` refreshes the screen); README touchpad section, tray line and uninstall steps written; installed clean (67 tests, clippy clean), log shows the first start and the touchpad found. Not yet tried on your screens.

## Consequences

**Positive**:
- Gestures work over every window, admin ones included, as your keys do.
- Every three and four finger gesture is yours to map, with twice gestures Windows cannot do.
- One source of gestures: no double switch from Windows and pdesk both acting.

**Negative / tradeoffs**:
- **Accepted risk (your call, 2026-10-31)**: a key combo in `settings.toml` is typed by an admin pdesk, so it reaches admin windows. Anything running as you can rewrite `settings.toml`, so a planted rule could, for example, paste and run a command in an admin terminal on your next gesture. This bends the AGENTS.md rule about `%LOCALAPPDATA%\pdesk` (a combo is not a program, but it carries admin reach). Keep combos few; prefer action names.
- pdesk reads, and on Quit writes, undocumented Windows registry values. A Windows update can rename them; pdesk then reads Windows' gestures as on and acts on none (the log names the value) until the names are found again (the check step's method).
- Quit gives Windows' gestures back only from your next sign in. Until then, and after a hard kill, three and four finger gestures do nothing while pdesk is not running.
- You set Windows' gestures to Nothing yourself in Settings; pdesk cannot do it live.
- If you change a gesture in Settings between a Quit and a start in the same sign in, and the registry ends up exactly at your saved values, that start writes them off in the registry while Windows has them on, and each gesture acts twice until you press "Give gestures back" or change one in Settings. Every such start logs a loud line saying so.
- A gesture with a twice rule acts `twice_ms` late.
- Gesture recognition is pdesk's own code; edge cases Windows handled (palm, a fifth finger, a pause mid swipe) are now pdesk's to get right.
- Changing any gesture in Settings while pdesk reads stops pdesk reading, and Quit then writes nothing, so your own change stays.

**Neutral**:
- New `windows` crate features: `Win32_UI_Input` (Raw Input) and `Win32_Devices_HumanInterfaceDevice` (`HidP_*`, reading touchpad reports); `Win32_System_Registry` (setup); `Win32_System_RemoteDesktop` (`WTSQuerySessionInformationW`, the sign in's logon time); `Win32_Graphics_GdiPlus` (the welcome screen's smooth shapes).
- New thread message ids `WM_APP + 8` (`WM_GESTURE`) and `WM_APP + 9` (`WM_TOUCHPADS_CHANGED`); AGENTS.md's list of ids needs them (`/sync`).
- The scope's Deferred line "Native touchpad gestures" becomes a feature (`/scope`).

## Migration plan

**Strategy**: feature gated. pdesk acts on no gesture until Windows' three and four finger gestures all read off; until then the Windows custom shortcut route works as today.
**Phases**:
1. Build steps 1 to 3 behind `[touchpad] enabled`, with Windows' gestures set to Nothing by hand for trying.
2. Build steps 4 to 6: the welcome screen appears once on your next start after install.
**Rollback**: "Give gestures back to Windows" (or Quit), sign out and in, then install the older build. After a hard kill, set them in Settings, Touchpad.
**Risks**: a sign in that does not pick up written values (build step 4's check; fallback in 0011-first-run-setup); a touchpad whose reports `HidP_*` cannot read (it logs and recognizes nothing while Windows' gestures are off, so press "Give gestures back").

## Follow-up

- [x] Check step results (2026-10-31, ELAN touchpad 04f3:32ea, 114 × 69 mm, parallel reports, 5 finger slots, Windows 11 build 26200):
  - Raw Input over Task Manager as admin: **yes**, every swipe. Reports arrive with Windows' gestures on, set to a custom shortcut, and set to Nothing: **yes**.
  - Registry "off" values (Settings set to Nothing): `ThreeFingerTapEnabled`, `CustomThreeFingerTap`, `ThreeFingerUp`, `ThreeFingerDown`, `ThreeFingerLeft`, `ThreeFingerRight`, `FourFingerTapEnabled`, `CustomFourFingerTap`, `FourFingerUp`, `FourFingerDown`, `FourFingerLeft`, `FourFingerRight` all `0`; `ThreeFingerSlideEnabled` `0xffff`. Before, every three finger value and the four finger tap values were absent; `FourFingerSlideEnabled` and the `*KeyParams` values do not change.
  - A write takes effect live: **no**, neither a plain write nor the `WM_SETTINGCHANGE` "PrecisionTouchPad" broadcast; a web search found no reliable live method. So setup reads the registry and you set the gestures in Settings; whether Quit can give them back from the next sign in is build step 4's check (0011-first-run-setup, updated 2026-10-31).
  - Focus from a gesture: `SetForegroundWindow` refused 6 of 6; after typing `VK_MASK` down and up (with `MAGIC`) first, it worked 6 of 6. Gesture actions that focus a window type that key just before.
- [x] Build step 4 check: does a sign in pick up written gesture values? **Yes (2026-10-31)**: with Settings at Nothing, a script wrote the before values back; after a sign out and in, Settings showed them again and a three finger swipe up worked. So the AC-10 write back is built, not the fallback. Not checked: how fast a change in Settings shows in the registry.
- [ ] `/sync`: AGENTS.md rule for the accepted combo risk, `WM_APP + 8` and `+ 9`, the gesture thread, and "Quit, `--quit`, panic and Ctrl+C also write Windows' saved gestures back".
- [x] Enrolled on the scope as feature 13 "Touchpad gestures", replacing the Deferred line (2026-10-31).
- [x] `/ui-design-explorer`: the welcome screen. H2 checklist approved at r4 (2026-10-31, `pdesk-design/welcome/micro-suite-r4.html`); 0011-first-run-setup updated to it.
- [ ] The 2026-11-01 welcome screen parts (intro lines, the "pdesk does" text, Choose my own) are not in the approved r4 design file; you approve them on the real screen. Record that in `pdesk-design/decisions.md` once you do.
- [x] Optional later: a "Restart pdesk" button or a settings reload, so gesture edits apply without signing out (the runner up you passed on 2026-11-01). Done by spec 0012 (live settings, 2026-11-02).
- [ ] The welcome screen colors are the provisional indicator tokens (indicator look not approved yet); rework if they change.
- [ ] Narrator reads the buttons but not the painted rows and chips. Fine for one person's tool; revisit if pdesk gets other users.
- [ ] Spec 0010 AC-1 and the README touchpad section describe the Windows custom shortcut route; update them once this ships.

## Rationale

Reasoning and options: see [rationale.md](rationale.md).
