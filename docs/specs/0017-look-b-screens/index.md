# 0017. Look B screens: every des-k window rebuilt to the approved look B designs

**Date**: 2026-11-05
**Status**: In Progress

## Summary

des-k's seven windows (first run, Shortcut hints, settings page, indicator, finder, overview, tray menu) were redesigned in look B and each design was approved by you, bound to its file's sha256. This spec is how they get built: one shared drawing layer in `src/ui.rs` (colors, fonts, icons, the rounded shadowed popup, a screen reader layer), then one screen at a time, smallest first, each replacing its old drawing. Drawing stays GDI+ and GDI, with a speed limit on the overview (100 ms, checked on the screens you have now and again when the 4K ones arrive) and Direct2D as the fallback for the overview alone. Two new things come with it: a `fastest` switch that makes every fade and move instant, and a time setting for the overview's fade. Everything ships together as 0.4.0.

## Structure

| Child spec | What it is | Replaces |
|---|---|---|
| [0017-indicator.md](0017-indicator.md) | The card indicator, plus today's dots restyled as a second style. First slice, builds the shared layer. | the dots look in specs 0004, 0014, 0016 |
| [0017-finder.md](0017-finder.md) | The finder in look B: screen names, workspace chips, key line, matched letters bold. | spec 0007 AC-13, parts of AC-3 |
| [0017-overview.md](0017-overview.md) | The overview on a card band with chips, shadowed windows, a key bar, and the speed limit. | spec 0010 AC-13 |
| [0017-tray-menu.md](0017-tray-menu.md) | des-k's own tray popup with a screen map, replacing Windows' menu. | `TrackPopupMenu` in `src/tray.rs`, spec 0015 AC-9 |
| [0017-shortcut-hints.md](0017-shortcut-hints.md) | The new Shortcut hints window and its key. | new |
| [0017-settings-page.md](0017-settings-page.md) | The settings page in look B, plus rows for `fastest` and the overview fade time. | spec 0012 look and AC-10, AC-13 |
| [0017-first-run.md](0017-first-run.md) | The new first run (every setting, touchpad step), shown once to upgraders too. | the welcome screen of spec 0011 (0011-first-run-setup) |

Each child is built to its approved design file. That file and its captures are the visual source of truth: a builder measures from them and never invents a look. Where a child and an older spec disagree, the child wins, and the older spec carries a one line pointer here.

## Requirements

**User stories**:
- As the des-k user, I want every des-k window to look like one app in the approved look B, so des-k feels finished and calm.
- As a user who is more sensitive to motion and delay than most, I want to shorten or turn off every animation, so nothing makes me wait or feel queasy.
- As a user with many large screens, I want the overview to open at once on all of them.
- As a screen reader user, I want Narrator to read des-k's windows.

**Acceptance criteria** (shared by every child; each child adds its own):
- **AC-1**: Each screen matches its approved design (file and sha256 in its child spec) in light, dark and high contrast, at 100% and 150% scaling. `cargo test render -- --ignored` draws each child's listed states to `target/<screen>/` PNGs, which you compare by eye with the design's captures.
- **AC-2**: The theme follows Windows live. Light or dark comes from `AppsUseLightTheme` (windows) and `SystemUsesLightTheme` (indicator and tray popup, which sit with the taskbar). High contrast uses the system colors. An open window repaints within 1 s of a theme or contrast change. That 1 s holds even though the admin des-k may never get the theme broadcast from the non admin Settings app.
- **AC-3**: Text is Segoe UI Variable (Text for 12 to 14 px, Display above), weights 400 and 600 only. "Bold" anywhere in these specs means 600, and `ui.rs` `FACES` drops its 700 face. Icons are Segoe Fluent Icons. If either font is missing, des-k falls back to Segoe UI and Segoe MDL2 Assets (today's `ui::variable()` check) and writes one log line.
- **AC-4**: Every popup (indicator, finder, overview ghost, tray menu) gets rounded corners and its shadow from Windows (DWM `DWMWCP_ROUND`); slice 1 checks this by hand. Every new or rebuilt window is sharp at every screen's scaling and follows a move between screens with different scaling (`WM_DPICHANGED`). Each is one of des-k's own windows: `ws::app_windows` leaves it out, so no switch hides it and the finder and overview never list it. The popups and the hints window never show in Alt+Tab or the taskbar. The settings page and the first run keep their taskbar button, as today.
- **AC-5**: Narrator reads every rebuilt screen through one shared UI Automation layer (UI Automation is Windows' screen reader interface): each control has a name, a role, its state (on, off, greyed, selected) and focus changes. The tray menu cannot ship without it; every other screen gets it in its own slice. Each slice is checked with Narrator by hand.
- **AC-6**: `fastest = true` in `settings.toml` makes every des-k fade and move instant: the indicator's fade, the overview's fade in and out, the first run's grow. The overview also skips its painted window shadows. Windows' own show and hide animation is turned off on the hints window, the settings page and the first run (`DWMWA_TRANSITIONS_FORCEDISABLED`); the popups never have it. Windows' own popup shadows and rounded corners stay, since they cost nothing. The time settings keep their values in the file, so turning `fastest` off brings them back. Holding times (`[indicator] hold_ms`) are not motion and are not changed. It applies live, within 2 s of a change.
- **AC-7**: `[overview] fade_ms` (0 to 1000, default 250) is the length of the overview's fade in and out when `animation = "fade"`; 0 means instant. A bad value resets to 250 with a log line (spec 0001).
- **AC-8**: On the screens you have now, the overview shows on every screen within 100 ms of the key press. The clock starts at the key message's own time (`GetMessageTime` of the `WM_APP + 7` message), so time spent waiting in the queue counts. It stops when the last popup's first full paint ends. A fade in is not counted, and neither is DWM's composition after the paint. Each open writes `overview: shown on N screens in M ms` to the log. Over 100 ms, the line ends `(over the 100 ms limit)` so it is easy to find. The three 43 inch 4K screens are not here yet; a Follow-up measures again on them.
- **AC-9**: The first run shows at start when `first_run_seen` is missing or false, and des-k writes `first_run_seen = true` the moment it opens. A fresh install's file says `first_run_seen = false`; an upgrader's file lacks the key, so both see it once. It is read only at start: changing it while des-k runs does nothing until the next start. When `settings.toml` is not valid TOML, the write fails and the first run opens at every start until the file is fixed. This is accepted, and des-k logs it once per start.
- **AC-10**: Each screen's old drawing code is removed in the slice that replaces it. The only old look kept is the indicator's dots, restyled, as `[indicator] style = "dots"`.
- **AC-11**: The settings page and the first run's More panel show rows for `fastest` and `[overview] fade_ms`, drawn to a short design pass you approve before the settings page slice starts.
- **AC-12**: The version becomes 0.4.0 and a release is made only when all seven children are built and checked. Until then main may hold a mix of old and new screens.

## Decision

**Chosen option**: one umbrella of seven children on a shared GDI+ and GDI drawing layer, built smallest screen first, each replacing its old drawing, released together as 0.4.0.

Drawing stays on `ui.rs`'s Canvas (GDI+ shapes, GDI text). The overview has a written speed limit (AC-8); if it misses it after the repaint rules below, the overview alone moves to Direct2D behind the same Canvas calls.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Rationale

Reasoning and options: see [rationale.md](rationale.md).

## Shared design

### The shared layer in `src/ui.rs`

`ui.rs` already has `Theme` and its tokens, `theme()`, `variable()`, `Fonts` with `FACES`, and `font(face, px, weight)`, used by the welcome screen, the settings page, the finder and the overview. The shared layer **extends these**; it does not add a second set. Renames go in the slice that makes them, with every caller changed in the same commit.

- **Tokens.** `Theme` grows into one table of look B colors for light, dark and high contrast:
  - the card;
  - the 1 px edge (the darker field edge you chose);
  - three greys for text (text, second line, greyed);
  - hover grey;
  - the one purple, #8E3FD9;
  - the nine workspace colors (the first run's tuned set: same hues, one brightness, workspace 2 #DDB43C) and the text on them.

  In high contrast, everything maps to system colors (`GrayText` for greyed text). The nine workspace colors collapse there too: chips, the indicator's number block and the tray map use the system colors the design's HC captures show (each design file has HC frames). The tokens table names that mapping per workspace use. The light and dark values come from the approved first run file (`pdesk-design/restart/first-run-r11.html`, `:root` tokens) and are the same in every child.
  - `indicator::ACCENT` moves here in slice 1 and is deleted with its last user (the overview).
  - The finder's `BG`, `DIM` and `TEXT` are imported by `overview.rs:23`. So when the finder slice moves them to tokens, it copies them into `overview.rs` as private constants. They are deleted in the overview slice, along with `TILE_BG` (`:32`).
- **Fonts and icons.** The existing `font(face, px, weight)` and `Fonts` cache grow a per DPI key, plus `icon(codepoint, size)` from Segoe Fluent Icons, with the AC-3 fallback. `FACES` drops the 700 entry; its users move to 600.
- **Popup helper.** `ui::popup(...)` makes a `WS_POPUP` with `WS_EX_TOOLWINDOW`. It sets `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND` and `DWMWA_TRANSITIONS_FORCEDISABLED`, and takes Windows' popup shadow (`CS_DROPSHADOW` is not used). Windows can't report whether it drew the shadow, so there is no fallback chain. Slice 1 checks the shadow by hand on your screens. If it fails there, the spec gets a fallback before slice 2. The indicator passes a flag to stay layered (`WS_EX_LAYERED`, see its child); every other popup is non layered.
- **Theme.** `ui::theme()` reads `AppsUseLightTheme`, `SystemUsesLightTheme`, high contrast and `Personalize\EnableTransparency`, and caches the result. Main's existing 1 s watch timer reads them again and compares. On a change, it updates the cache and repaints every open des-k window. That path alone meets AC-2, because the admin des-k may never receive the theme broadcast from the non admin Settings app (UIPI), and spec 0008's message filters are not widened. Each window's own `WM_SETTINGCHANGE` and `WM_SYSCOLORCHANGE` handling (`settings_page.rs:2354`, `welcome.rs:953` today) stays as a faster path when it does arrive.
- **Motion.** `ui::motion_ms(base)` returns 0 when `fastest` is on, else `base`. Every fade and move reads its time through it: the indicator's `fade_ms`, the overview's `fade_ms`, the first run's grow (150 ms, no key). Only two other places read `fastest`: the overview's shadow painting and the `DWMWA_TRANSITIONS_FORCEDISABLED` switch on the hints window, the settings page and the first run (set again when `fastest` changes).
- **Screen names.** A new `ws::screen_label(id)` returns the friendly name (`Monitor.name`). When two present screens share a name, it adds " #1" and " #2" left to right. The indicator, finder, tray menu and Narrator names all use it; no window builds a screen name itself.
- **Screen reader layer.** A small UI Automation provider in a new `src/uia.rs`, using the `windows` crate's COM `implement`. Each des-k window answers `WM_GETOBJECT` with a root fragment. Its children are plain data the window keeps in step with what it painted: role, name, second line, state, bounds, focus. The window raises focus and property events when they change, only when `UiaClientsAreListening()`.
  - Threading: UIA calls the provider on the window's thread whenever that thread pumps messages, which includes inside sent messages while main holds the `ws` lock. So provider methods only read the window's own state through `ui::get` (`try_borrow`), returning an empty answer when it is busy. They never lock `ws`, never write settings, and never send messages.
  - On `WM_DESTROY`, the window calls `UiaReturnRawElementProvider(hwnd, 0, 0, None)` and `UiaDisconnectProvider`.
  - Narrator runs with UIAccess, which reaches des-k's admin windows, so the message filters (spec 0008) do not change. Slice 1 checks this by hand.
  - This is the only file that implements COM interfaces; raw vtable calls stay in `src/cloak.rs` only.

### Data model: `settings.toml`

`state.txt` and `touchpad.txt` do not change. New keys, each with its slice:

| Where | Key | Type and range | Default | Rule | Slice |
|---|---|---|---|---|---|
| top | `fastest` | bool | `false` | AC-6 | indicator |
| top | `first_run_seen` | bool | `false` in a fresh file; missing means false | AC-9, read at start only | first run |
| `[shortcuts]` | `hints` | list of combos, like `finder` | `[]` (no key) | same parser and clash rules as every list; `[]` is off | tray menu + hints |
| `[indicator]` | `style` | `"card"` or `"dots"` | `"card"` | bad value resets to `"card"` with a log line | indicator |
| `[overview]` | `fade_ms` | 0 to 1000 | `250` | AC-7 | overview |

Each key goes into all of these together:
- `Settings` and its `Default`;
- `DEFAULT_FILE` with a comment (the parse equals default test must still pass);
- the `*_KEYS` list;
- the parser (`int`, `choice()`, bool);
- `settings::write` values;
- the problem title (spec 0001);
- one parse test.

`settings::write` already inserts a key the file lacks (`set_value`'s `None` branch). `first_run_seen` relies on this on every upgrade, so it gets its own test, run on a 0.3.0 file. The test checks three things: the key lands in the root table, above the first `[table]` header; the comment above that header stays above it; and the new key carries no comment (accepted).

### Value sourcing

| Action | Value | Source |
|---|---|---|
| Any paint | colors | `ui` tokens for the current `ui::theme()` |
| Any paint | sizes in px | dip times the window's monitor DPI (`GetDpiForWindow`) |
| Any fade or move | its length | `ui::motion_ms` of the setting (`[indicator] fade_ms`, `[overview] fade_ms`) or the fixed 150 ms grow |
| Overview log line | N screens, M ms | count of popups shown; from `GetMessageTime` of the `WM_APP + 7` message to the end of the last popup's first full `WM_PAINT` (AC-8) |
| First run at start | show or not | `settings::get().first_run_seen` at start, replacing today's `setup::start()` first start return |
| Screen names | per screen | `ws::screen_label(id)` (friendly name, " #n" for twins) |
| Workspace numbers, colors | per screen | `ws::active`, workspace index into the token table (HC mapping per the design's HC frames) |
| Theme | light, dark, HC, transparency | `ui::theme()` cache, refreshed by main's 1 s timer |
| Narrator names | per control | the same strings the window paints |

### Key invariants

- No screen draws a color that isn't in the token table.
- No fade or move bypasses `ui::motion_ms`.
- des-k's own windows are never hidden, listed or followed.
- The main loop still ends only on `WM_CLOSE` for the indicator or the tray (AGENTS.md). The new tray popup and hints window are not that tray window.
- New thread message id `WM_APP + 14` (hints) is unique; the loop matches ids alone.

### Security model

Unchanged from spec 0008: des-k runs as admin, and the message filters still let in only `WM_CLOSE` (indicator) and `WM_TRAY` plus `TaskbarCreated` (tray). No new key names a program or a path. `first_run_seen` and `fastest` are booleans, harmless if rewritten by anything running as you.

### Critical test scenarios

- Happy path: each child's states render to PNG and match its captures in all three themes. Verifies **AC-1**.
- Theme: switch Windows from dark to light with the finder and the settings page open; both repaint within 1 s. Verifies **AC-2**.
- Fastest: set `fastest = true` by hand; the next switch shows the indicator with no fade, and the overview with `animation = "fade"` opens with no fade and no painted shadows. Turn it off; the fades come back with their times. Verifies **AC-6**, **AC-7**.
- Speed: open the overview ten times on the screens you have now; every log line is at most 100 ms. Verifies **AC-8**.
- Upgrade: start with a 0.3.0 `settings.toml` (no `first_run_seen`) and an existing `touchpad.txt`; the first run opens, the file gains `first_run_seen = true` with its comments kept, and the next start shows nothing. Verifies **AC-9**.
- Narrator: with Narrator on, Tab through the tray menu; it reads each item's name, second line and greyed state. Verifies **AC-5**.

## Build plan

Tracer Bullet: each slice is one screen built completely, from settings key to drawing to Narrator, and works on your screens before the next one starts.

0. Design side, no code: a cross flow review of the seven approved boards together (same tokens, same words, same key wording). Then the short design pass that adds the `fastest` and overview fade time rows to the settings page and the first run's More panel, for your approval. The rows' approval must land before task 5. Satisfies **AC-11**.
1. Indicator slice (see [0017-indicator.md](0017-indicator.md)). It also builds `ui` tokens, fonts and icons, the popup helper, `ui::theme()` with live repaint, `ui::motion_ms`, the `fastest` and `[indicator] style` keys, the first part of `src/uia.rs` (a UIA notification for each card), and the render test harness for popups. Satisfies **AC-1** to **AC-6**, **AC-10**.
2. Finder slice (see [0017-finder.md](0017-finder.md)). Satisfies **AC-1**, **AC-4**, **AC-5**, **AC-10**.
3. Overview slice (see [0017-overview.md](0017-overview.md)), with `[overview] fade_ms`, repaint rules and the timing log line; measured on the screens you have now before the slice is called done. Satisfies **AC-1**, **AC-4** to **AC-8**, **AC-10**.
4. Tray menu and Shortcut hints slice (see [0017-tray-menu.md](0017-tray-menu.md), [0017-shortcut-hints.md](0017-shortcut-hints.md)), with the `[shortcuts] hints` key and `WM_APP + 14`. Satisfies **AC-1**, **AC-4**, **AC-5**, **AC-10**.
5. Settings page slice (see [0017-settings-page.md](0017-settings-page.md)), after task 0's approval. Satisfies **AC-1**, **AC-2**, **AC-5**, **AC-10**, **AC-11**.
6. First run slice (see [0017-first-run.md](0017-first-run.md)), with `first_run_seen`. Satisfies **AC-1**, **AC-5**, **AC-9**, **AC-10**, **AC-11**.
7. Release: a pass over all seven together on your screens (three themes, mixed scaling, many screens), version 0.4.0 in `Cargo.toml`, setup exe. Satisfies **AC-12**.

## Migration plan

**Strategy**: strangler, screen by screen. Each slice swaps one window's drawing on main; nothing is released between slices (AC-12).
**Phases**: tasks 1 to 6 above, each a working commit.
**Rollback**: revert the slice's commits; the other screens keep working, since each depends only on the shared layer that slice 1 added.
**Risks**:
- The tray slice's move to `NOTIFYICON_VERSION_4` changes how every tray message reads, balloon clicks included.
- The overview may miss 100 ms on GDI+, which brings in the Direct2D fallback. On today's screens it may pass and later miss on the 4K ones.
- UIAccess reaching admin des-k is assumed, and checked in slice 1.
- DWM's popup shadow is assumed, and checked in slice 1.

## Consequences

**Positive**:
- One look across all of des-k, from one token table.
- Narrator can read des-k.
- You can make every animation instant.

**Negative / tradeoffs**:
- Seven slices of drawing work before any release.
- Main holds mixed looks in between.
- The UIA layer adds a COM implementation to keep correct in every screen.
- GDI text on the fading indicator needs care (draw on the card color, fade the whole window).
- The overview's shadows are a stretched pre blurred image, slightly less exact than a real blur.

**Neutral**:
- Specs 0004, 0007, 0010, 0011, 0012, 0014, 0015 and 0016 each get a one line pointer to the child that replaces their look criteria.
- The site guide's screenshots (`site/guide-content.html`) go stale until they are retaken.

## Follow-up

- [ ] When the three 43 inch 4K screens arrive, measure the overview again against AC-8 (ten opens, the log lines).
- [ ] Retake the site guide's screenshots after the release (`site/guide-content.html`).
- [ ] If the overview misses AC-8 after the repaint rules, write a child spec for Direct2D in the overview alone (`/architect`).
- [ ] Each child's "check on the real machine" items (listed in each child) are done by hand before its slice is called done.
