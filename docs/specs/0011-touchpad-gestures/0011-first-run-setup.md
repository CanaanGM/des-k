# 0011 child: first run setup and Windows' gestures

## Summary

des-k cannot turn Windows' gestures off live: a registry write does nothing until you sign in again (check step, index.md Build plan step 1). So you set them to Nothing in Settings yourself, and des-k acts only while it reads them all off in the registry. On the first start, a welcome screen says plainly what des-k is, shows what each three and four finger gesture will do, and lets you go with that ("Set up in Windows Settings") or "Choose my own" (the same setup, plus des-k's settings page opens at Touchpad, spec 0012). Either one saves your current Windows gesture values and opens Settings at Touchpad. Quit writes your saved values back, and Windows uses them from your next sign in. The settings page can open the screen any time (Touchpad, Touchpad setup).

## Setup state

- File `%LOCALAPPDATA%\des-k\touchpad.txt`, plain lines `name=value`:
  - the saved values: one line for each of the 13 names below, `<value name>=<number>`, or `<value name>=absent` when the value did not exist. These are your values from the first time you pressed "Set up in Windows Settings" or "Choose my own" while they were not all off; later presses never replace them. "Give gestures back" removes them; press it, then one of those again, to save newer ones.
  - `restored=<logon time>`: written by an exit that writes your values back. The number is this Windows sign in's logon time (`WTSQuerySessionInformationW`, `WTSSessionInfo`, `LogonTime`, in decimal). The next start removes it.
- A missing file means first start, so the welcome screen opens. Skip writes an empty `touchpad.txt`.
- Every line is checked: a saved value's name must be one of the 13 fixed names compiled into des-k, and its number a DWORD from `0` to `0xffff`; `restored` must be a number. A bad line is skipped with a log line. If any of the 13 saved lines is bad or missing, des-k treats the file as holding no saved values (log line) and never writes them back, so it never writes back half of your settings.
- des-k writes `touchpad.txt` to a temporary file in the same folder and renames it over the old one, so a hard kill mid save never loses your saved values.
- There is no `applied` line. Whether Windows' gestures are off is read from the registry each time (below), so des-k never trusts a click it cannot see.

## Windows' gesture values

- Location: `HKCU\Software\Microsoft\Windows\CurrentVersion\PrecisionTouchPad` (not documented by Microsoft). The logon task runs des-k as you (elevated), so `HKCU` is your own. The 13 value names and their "off" numbers are compiled in as a fixed table, from the check step: `ThreeFingerTapEnabled`, `CustomThreeFingerTap`, `ThreeFingerUp`, `ThreeFingerDown`, `ThreeFingerLeft`, `ThreeFingerRight`, `FourFingerTapEnabled`, `CustomFourFingerTap`, `FourFingerUp`, `FourFingerDown`, `FourFingerLeft`, `FourFingerRight` off at `0`; `ThreeFingerSlideEnabled` off at `0xffff`. An absent value counts as on (Windows' default).
- **`windows_off()`**: reads the 13 values; true only when every one is a DWORD at its off number. A read error, or a value of another type, counts as not off (fail closed). It returns the first value that is not off, for the log (the screen shows rows instead, see Welcome screen): `FourFingerLeft is on`, or `FourFingerLeft is missing (Windows may have renamed it)`. Settings writes these the moment you change a gesture, so the read follows what you set.
- **`touchpad_active()`** = `[touchpad] enabled` and `windows_off()`.
- **Registry equals saved values**: for each of the 13 names, a saved number matches a DWORD of that number, a saved `absent` matches a missing value; anything else (another number, another type, a read error) is a mismatch.
- **Set up in Windows Settings** (the welcome button; named "Use des-k's gestures" before 2026-11-01 and "Use these gestures" before 2026-11-03, renamed because it never turns gestures on by itself):
  1. If `touchpad.txt` holds no saved values and `windows_off()` is false, read the 13 values and save them, noting each absent one. A read failure other than "not found" is logged and nothing is saved. If they are already all off, nothing is saved, and the screen says Quit has nothing to give back.
  2. Open Settings at Touchpad (`ms-settings:devices-touchpad`, through explorer like the tray's files). The screen says "Set every three and four finger swipe and tap to Nothing. des-k starts reading as soon as they are all off."
  3. Nothing else: the next recognized gesture reads the registry and finds them off.
- **Choose my own** (the second welcome button): if the card's primary button is shown and enabled ("Set up in Windows Settings" or "Open Touchpad settings"), it first does exactly what that button does. Then it opens des-k's settings page at Touchpad (spec 0012). In every other state it only opens the page. Changes made there, or by hand in `settings.toml`, apply at once (spec 0012), and an open welcome screen refreshes with them; the screen says so above the rows.
- **Write back** runs on Quit (tray), `--quit` (through `WM_CLOSE`), a panic on any thread (the existing hook, which then exits), and the console Ctrl+C handler. Only when saved values exist and `windows_off()` is true (if you changed a gesture in Settings since, des-k leaves your settings alone). In this order:
  1. Read this sign in's logon time. If that fails, write nothing and log it.
  2. Write `restored=<logon time>` to `touchpad.txt` first, so a crash partway leaves a line the next start can act on safely.
  3. Write each saved number back, and delete each value saved as `absent`. A failed write is logged; the rest still run.
  Windows uses them from your next sign in. Sign out and shutdown (`WM_ENDSESSION`) write nothing, since des-k starts again at sign in; writing back there would hand Windows its gestures at every sign in. A hard kill writes nothing; `install.ps1` hard kills des-k on purpose, so an update keeps gestures working.
- **Start**, before the gesture thread starts:
  - With a `restored` line: remove it. Then, if its logon time equals this sign in's and the registry equals the saved values:
    - with `[touchpad] enabled` true, write the off numbers again, and log loudly: "Quit wrote your Windows gestures back earlier in this sign in; setting them off again. If you changed them in Settings since, every gesture now acts twice; press Give gestures back." Windows still has them off live (the write back never took effect), so this makes the registry match again and gestures keep working.
    - with `[touchpad] enabled` false, write nothing and log "Windows' gestures come back at your next sign in; until then three and four finger gestures do nothing".
  - Otherwise (you signed in since, or you changed them yourself) write nothing.
  - If `[touchpad] enabled` is false and `windows_off()` is true, log "touchpad reading is off in settings.toml, but Windows' three and four finger gestures are set to Nothing, so they do nothing; set them back in Settings, Touchpad". Nothing opens by itself.
  - Log one line: Windows' gestures off, or the first value not off, and whether your values are saved.
- **Give gestures back to Windows** (shown in every state while saved values exist; it writes even if you changed gestures in Settings, since you asked): write the saved values back and delete the absent ones. Only if every write succeeded, remove the saved lines and any `restored` line; otherwise keep them, log the failure, and the screen says it failed. Then open Settings at Touchpad; the screen shows the result (see "After Give gestures back" in Welcome screen). des-k stops reading at once, since the registry no longer reads off. After this, no exit writes anything.
- des-k writes only the fixed value names, only DWORDs, only in `HKCU`. Since you can write `HKCU` yourself, a planted `touchpad.txt` gains nothing.
- **Fallback** if build step 4's check shows a sign in does not pick up the written values either: no exit writes anything, `touchpad.txt` keeps no saved values and no `restored` line, "Give gestures back" only opens Settings at Touchpad with "Set them the way you like", and the screen and README say Quit cannot give gestures back. The `windows_off()` gate stays.

## Welcome screen

Approved design (updated 2026-10-31): H2 "checklist", `pdesk-design/welcome/micro-suite-r4.html` (sha256 `064558909c3befab6159fbfead5e8e874e9f4a4e3975d9ead29a3dafb9c81a62`, outside the repo; owner approval in `pdesk-design/decisions.md`). That file is the visual source: colors, sizes, spacing and every state's layout. Its colors are the provisional indicator tokens; if the indicator look changes before this ships, take the new tokens. The one card design this section described before was rejected: it showed only the first gesture still on and hid the rest.

**Change of 2026-11-01** (your words after opening the built screen: "its not intuitive, the welcome screen should 1. tell what the program is 2. allow me to either set my own defaults or go with the defaults it already have"). You chose: plain lines above the tiles, the gesture map shown on the screen, two choices ("Use these gestures" and "Choose my own"), and the screen telling you that edits apply from the next start. These parts are NOT in the approved r4 file. They reuse r4's styles (the intro in r4's body text style, the "des-k does" text in its row text style, "Choose my own" as r4's outline button like Skip), and you approve them on the real screen, not by a new design file. Everything else stays as r4 draws it.

**Change of 2026-11-02** (spec 0012, the settings page). Settings now apply live and des-k has its own settings page, so "Choose my own" opens that page at Touchpad instead of `settings.toml`, and the two screen texts that said edits wait for the next start or a restart now say changes apply at once. Only those texts and that one button's target change; the layout, styles and states stay as they are.

**Change of 2026-11-03** (three independent UX reviews; you chose "scroll, buttons pinned" and asked to fix every finding). The text below already says the new way:
- Fit: the window is never taller than its screen's work area. When it would be, the band stays at the top, the card's buttons move to a strip pinned above the footer, and the part between (intro to card) scrolls with the mouse wheel and Page Up / Page Down, with a thumb like the settings page's. When everything fits it looks as before. This replaces "the part below the screen edge is cut off (Tab and Enter still reach every button)".
- Rows: each chip names Windows ("Windows: still on"), and the line about what des-k runs moved above the rows at body size, so a row never reads as des-k doing nothing.
- Nothing hidden: reading off and the give back states show the four rows and the "Windows: n of 4 off" count too, with a caption saying des-k runs none of them right now.
- The primary button says what it does: "Set up in Windows Settings" (was "Use these gestures"). With reading off it is gone (it could not do anything); with no touchpad it stays disabled under the banner that says why.
- Lines that point at the log say how to open it: "(tray icon › Open log)". The All off line names des-k's settings, not `settings.toml`. The tray's Quit says what happens to gestures (Opening it again).

**Layout**, top to bottom (480 dip wide; as tall as its content, about 950 dip in the tallest state, and nothing shrunk to save room. Changed 2026-11-01: the old 640 dip cap could not hold the new parts. Changed 2026-11-03: capped at the screen's work area, see Window):
- A color band: "Welcome to des-k!", the line "Every screen gets its own desktops.", and the art of three screens.
- What des-k is, in plain lines: "des-k gives each of your screens its own set of workspaces. You switch the screen under your mouse, and your other screens stay where they are. des-k never moves or resizes your windows. It only hides and shows them."
- Three key tiles: Switch `Win Ctrl ← →`, See all `Win Tab`, Find `Win F`. Under them, a small line (added 2026-11-03): "These keys, and Win+1 … 9 with or without Shift or Ctrl, take over Windows' own: its desktops, Task View, Feedback Hub and the taskbar's apps. Change any of them in des-k's settings, under **Shortcuts**."
- The gesture card, headed "Your touchpad gestures" with "Windows: n of 4 off" at its right in every state. The state's banner or line, then above the rows, in body text, "Under each gesture: what des-k runs for it. Choose my own changes them, and changes apply at once." (with reading off: "Under each gesture: what des-k would run. Touchpad reading is off, so it runs none of them."; after a give back: "Under each gesture: what des-k would run. It stopped reading gestures when you gave them back."). Then the rows, the state's small line, and the card's buttons: the primary button (by state) and "Choose my own". (Changed 2026-11-03; the line was under the rows in small text.)
- A footer: the give back button and its line on the left, Skip or Close on the right. The link "Change what each gesture does" is gone; "Choose my own" replaces it.

**Rows.** The card lists four rows. Each row shows what des-k does for it and a chip for Windows' own setting:

| Row | Values (from the 13) |
|---|---|
| 3-finger swipes | `ThreeFingerSlideEnabled`, `ThreeFingerUp`, `ThreeFingerDown`, `ThreeFingerLeft`, `ThreeFingerRight` |
| 3-finger taps | `ThreeFingerTapEnabled`, `CustomThreeFingerTap` |
| 4-finger swipes | `FourFingerUp`, `FourFingerDown`, `FourFingerLeft`, `FourFingerRight` |
| 4-finger taps | `FourFingerTapEnabled`, `CustomFourFingerTap` |

- What des-k does: from `settings::get().gestures`, the rules for that row's finger count. A swipe row shows four entries, `←`, `→`, `↑`, `↓`, two per line; a tap row shows one. Each entry is its once rule's label ("nothing" when no once rule exists), plus " · twice: <label>" when a `times = 2` rule exists. Labels: `previous` "previous workspace", `next` "next workspace", `send_previous` / `send_next` "send window to previous / next", `send_previous_stay` / `send_next_stay` "send window to previous / next, stay", `jump_n` "workspace n", `send_to_n` "send window to n", `send_to_stay_n` "send window to n, stay", `finder` "finder", `overview` "overview", `show_desktop` "show desktop", `app_next` / `app_previous` "next app / previous app", `search` "search", `notifications` "notifications", `middle_click` "middle click", `play_pause` "play or pause", `next_track` / `previous_track` "next track / previous track", `volume_up` / `volume_down` "volume up / down", `mute` "mute", `none` "nothing", a combo its `Combo` text (`Ctrl+Shift+T`). The labels are a fixed table compiled in next to `ACTIONS`; an action missing from it fails a test.
- Chip: "✓ Windows: Nothing" when every value in the row reads off; "Windows: can't read" when any read fails or has another type; otherwise "Windows: still on" (an absent value is on). (Changed 2026-11-03: the chips name Windows.) "n of 4 off" counts the Nothing rows. A row that cannot be read counts as on, like `windows_off()`.
- The grouping comes from the value names and is not yet checked control by control (Q1). Build step 5 checks it first: change each Settings control to Nothing, one at a time, while reading the registry. If any value is not set by exactly one of these four Settings controls, stop and come back to `/architect` before building the rows.
- The screen never shows "X is missing (Windows may have renamed it)"; that wording stays in the log only.

**States** (copy as in the design file; `[Primary]` is the filled button):

| State | Card shows | Buttons |
|---|---|---|
| No precision touchpad | "No precision touchpad found." "Plug one in and this updates by itself. Your keys work as before." The rows. | Set up in Windows Settings, disabled; Choose my own |
| `[touchpad] enabled = false` | "Touchpad reading is off in des-k's settings." If `windows_off()` is also true, a warning: "Windows' three and four finger gestures are set to Nothing, so right now they do nothing. Turn reading back on with Choose my own, or set them back in Windows Settings." The rows, under the reading off caption. (Changed 2026-11-03: the rows used to be hidden here.) | with the warning "Open Touchpad settings"; Choose my own (no primary button: it could do nothing; changed 2026-11-03) |
| Not all off | "In Settings › Touchpad, open Three-finger gestures and Four-finger gestures and set Swipes and Taps to Nothing. Then come back here." The rows. "des-k runs them once all four are off, even over admin windows. After that, quitting des-k gives them back from your next sign in." | [Set up in Windows Settings], or [Open Touchpad settings] once values are saved; Choose my own |
| One value can't be read | "des-k can't read the <row> setting, so it leaves every gesture to Windows. The log has the details (tray icon › Open log)." The rows. The same Quit line. | as Not all off |
| All off | "✓ des-k runs these gestures." "Three and four finger swipes and taps now do what you set in des-k's settings, under Touchpad." The rows. With no saved values: "You set them to Nothing yourself, so Quit has nothing to give back." | Choose my own |

- Footer while saved values exist (every state): "Give gestures back to Windows" and "Gives back what Windows had before des-k saved your values, from your next sign in." On first start without saved values: "You can open this again in des-k's settings, under Touchpad: Touchpad setup." ("des-k's settings", because the card above says "Settings" and means Windows Settings.) Opened from the settings page without saved values: no footer text.
- The state refreshes when the window gets focus back (you return from Settings), when the gesture thread posts `WM_TOUCHPADS_CHANGED`, and after each button.

**After "Give gestures back"** the card shows a banner and the rows relabeled from what the write did, not from a registry read (the registry now holds your values, but Windows keeps its gestures off until you sign in again). The "Windows: n of 4 off" count still shows the registry read (changed 2026-11-03), and there is no Set up button while a failure banner shows.

| Result (`give_back`) | Banner | Row chips |
|---|---|---|
| Every write worked and `touchpad.txt` saved | ⓘ "Your gestures come back at your next sign in." "To have them now, set them in Settings (it's open). des-k has stopped reading gestures." | all "✓ Written back" |
| Some writes failed | ! "Giving back stopped part way." "Some values were written back for your next sign in, some weren't. The log names them (tray icon › Open log). Your saved values are kept, so you can try again. Settings is open if you want gestures now." Button [Try giving back again] | per row: "✓ Written back" (all its values), "Part written" (some), "Not written" (none) |
| Every write failed | ! "Giving back didn't work." "Nothing was written back. The log says why (tray icon › Open log). Your saved values are kept, so you can try again. Settings is open if you want gestures now." Button [Try giving back again] | all "Not written" |
| Every write worked, saving `touchpad.txt` failed | ⓘ the "come back at your next sign in" banner, plus "des-k couldn't clear its copy of your values. The log says why (tray icon › Open log). Press Give gestures back again to clear it." | all "✓ Written back" |

- The result stays in memory until des-k exits, a later give back replaces it, or `windows_off()` reads true again; so closing and reopening the screen from the settings page shows the same banner and rows. After a des-k restart it is gone and the rows read the registry again (this can show "Still on" for values Windows still has off live until you sign in; rare, accepted).
- `give_back` returns this result: `write_all` already names each failed value, and a failed `save` is the last case.

**Controls and keyboard.** Real Windows `BUTTON` controls, owner drawn (des-k paints them; Windows handles focus, Tab, Enter, Space and the name a screen reader says), for the card's primary button, "Choose my own", "Give gestures back to Windows", and Skip or Close. The main loop passes this window's messages through `IsDialogMessageW`.
- Tab order: the card's primary button, "Choose my own", "Give gestures back to Windows", Skip or Close.
- Enter presses the default button: the card button when it is shown and enabled, else Skip or Close.
- Esc and the title bar X act like Skip on first start (write an empty `touchpad.txt`), else Close.
- If a refresh removes the focused control, focus moves to the default button.
- "Choose my own" works as described under Windows' gesture values.

**Window.** A normal top level window with a title bar (`des-k`), centered on the screen under the mouse (its work area, the screen minus the taskbar). Never taller than that work area (checked at open, on `WM_DPICHANGED` for the new screen, and on every refresh; a window that grows past the screen's bottom moves up). When the content is taller, the band stays at the top, the card's buttons sit in a strip pinned above the footer, and the middle (intro, tiles, heading, card) scrolls: mouse wheel 48 dip a notch, Page Up / Page Down a page, a 5 dip thumb at the right edge. No button ever scrolls, so every button stays in reach of the mouse. (Changed 2026-11-03: it used to cut off the part below the screen edge.) In the taskbar while open, created on the main thread (not the placeholder's own thread). Per monitor DPI aware like the rest of des-k: it rescales on `WM_DPICHANGED`. One at most: Touchpad setup focuses an open one. No message filter is added.

**Drawing.** GDI for text (Segoe UI Variable, ClearType) and GDI+ (`Win32_Graphics_GdiPlus`, the flat `Gdip*` calls, started once with `GdiplusStartup`) for the smooth (anti aliased) band gradient, art, tiles, pills and chips. Painted into a back buffer, then copied in one step, so nothing flickers.

**Themes.** Follows Windows' app mode, light or dark (`AppsUseLightTheme`), and high contrast (`SPI_GETHIGHCONTRAST`: system colors only, the band drawn in the window color with a border). It repaints on `WM_SETTINGCHANGE` (`ImmersiveColorSet`) and `WM_SYSCOLORCHANGE`.

## Opening it again

- **Touchpad setup** on des-k's settings page, under Touchpad (spec 0012), opens the welcome screen (`welcome::show(false)`). One screen at most: if it is open, the button focuses it, and restores it first if it is minimized.
- The tray has no Touchpad setup item. It had one (`ID_TOUCHPAD = 4`) until 2026-11-02, when it moved to the settings page so every des-k setting has one home; the tray keeps Settings, Open log and Quit. Since 2026-11-03 Quit says what happens to gestures, read when the menu opens: with Windows' gestures all off and values saved, "Quit (shows every window; gestures return at next sign in)"; all off and nothing saved, "Quit (shows every window; touchpad gestures stop)"; otherwise "Quit (shows every window)".
- Setting Windows' gestures to Nothing after an earlier Skip needs no restart: the gate reads the registry on every recognized gesture.

## Value sourcing

| Value | Source |
|---|---|
| first start | `touchpad.txt` missing |
| Windows' gestures off | `windows_off()`: the 13 registry values, read once per recognized gesture, at start, and when the screen refreshes |
| saved values | `touchpad.txt`, written only by the first "Set up in Windows Settings" or "Choose my own" while not all off; all 13 valid or none |
| same sign in as the write back | `touchpad.txt` `restored` compared with `WTSSessionInfo` `LogonTime` |
| registry equals saved values | the per name rule above |
| value names, off numbers, ranges | compiled table from the check step; every range `0` to `0xffff` |
| touchpad present | the Raw Input device list (`GetRawInputDeviceList`, usage page `0x0D` usage `0x05`) |
| each row's "des-k does" text | `settings::get().gestures` (the map read at start) and the compiled label table in Rows |
| what des-k is (the intro lines) | fixed copy in Layout |
| each row's chip and the "Windows: n of 4 off" count | the same 13 reads as `windows_off()`, grouped by the Rows table |
| "Written back" / "Part written" / "Not written" | `give_back`'s result: `write_all`'s failed names, and whether `save` worked; kept in memory until exit |
| light, dark, high contrast | `AppsUseLightTheme` (HKCU Personalize) and `SPI_GETHIGHCONTRAST`, read at open and on `WM_SETTINGCHANGE` |
| colors, sizes, copy | the approved design file named in Welcome screen; the 2026-11-01 parts use its styles and the copy in this section |

## Short rationale

des-k cannot switch Windows' gestures off live, and a "Done" button would trust a click des-k cannot see; a missed click means every gesture acts twice. Reading the registry that Settings writes makes the gate follow the truth, with no stored flag to drift. Turning off all three and four finger gestures, not only mapped ones, means a map edit never needs setup again. You chose to keep Quit giving gestures back, which works only from the next sign in, so a start in the same sign in sets them off again in the registry, and sign out writes nothing. Saving once, all 13 or none, and never at start keeps your originals safe across a hard kill. Runner up: no exit writes anything (simpler, but Quit and uninstall leave three and four finger gestures dead until you set them in Settings); it stays as the fallback. The welcome screen shows all four gesture rows rather than one line, because the owner judged a screen that hides which gestures are still on a failure, not a style choice. After a give back, the rows say what was written, not what the registry reads, since Windows keeps its gestures off until the next sign in. It is drawn with GDI+ shapes and real owner drawn buttons: smooth pills and chips for the friendlier look the owner asked for, while Windows supplies keyboard focus and screen reader names. Runners up: plain GDI like the indicator (no new dependency, but jagged pills and hand written keyboard handling) and Direct2D (best rendering, far more code for one screen). On 2026-11-01, after seeing the built screen, you found it did not say what des-k is and gave no real choice. So it now says what des-k is, shows the live map next to each Windows row (you see what you agree to before you press anything), and offers "Choose my own" beside "Use these gestures". Both buttons do the same Windows setup, because no des-k gesture can run until Windows' are off. Choose my own just opens the file too. Runner up: a picker of actions right on the screen (no file editing, but new controls, a new design round and a settings writer). Edits still apply from the next start, as spec 0001 reads settings once; a restart button was the runner up (one click, but a new exit and start path to build and check). Updated 2026-11-02: spec 0012 made settings live and added the settings page, which also covers the runner up picker, so Choose my own opens that page and the "next start" lines are gone.
