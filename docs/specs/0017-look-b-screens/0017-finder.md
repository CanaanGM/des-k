# 0017. Finder in look B

## Summary

The finder keeps everything it does (spec 0007) and takes the look B surface:
- Windows' light or dark mode;
- a purple chosen row;
- each window's screen name and workspace chip instead of `S · W`;
- matched letters in bold;
- a key line at the bottom.

It is the second slice and the first popup that holds a native text box.

**Design source**: `pdesk-design/restart/finder-r3.html` (r2.1), sha256 `0dda42802a768095cf3e70162b4a4bc9839b630265f1fd02da37d16a4fb9ee59`, captures `restart/captures/fdr3-*` (sums `SHA256SUMS-fdr3`). Approved 2026-11-05.

**Replaces**: spec 0007 AC-13 (look) and the `S · W` and dot part of AC-3. Every other 0007 criterion stands.

## Acceptance criteria

- **AC-FND-1**: The finder is drawn as the design:
  - Windows' light or dark mode;
  - card color, 1 px edge;
  - a 48 dip search box;
  - 720 dip wide;
  - Segoe UI Variable 14/20 for titles and 12/16 for second lines;
  - Fluent icons.

  The EDIT text box and its placeholder are painted for light, dark and high contrast.
- **AC-FND-2**: The chosen row is purple, with the settings side list's bar; hover is the grey track. The finder no longer uses `indicator::ACCENT`.
- **AC-FND-3**: Each row's right side shows the window's screen name and a chip for its workspace:
  - The name comes from `ws::screen_label` (index). A name cut in the middle keeps its " #n" end. The name is hidden when you have one screen.
  - The chip is full when the window is shown and 40% when it is hidden.
  - A pinned window's chip shows a pin, drawn on the current workspace color (needs the `ws` pinned set).
  - A screen that is gone but not yet folded gets a grey outline chip.
- **AC-FND-4**: The app name comes from the exe's FileDescription (cached per exe path for the process's life), else the exe name without `.exe`. Store apps show the hosted app's name. For an `ApplicationFrameHost` window, take its child `Windows.UI.Core.CoreWindow`'s process, then that package's DisplayName (`GetPackageFullName`, then the package's display name), else the window title. A count shows above the rows: "12 windows", or "2 of 12" while filtering. A scroll thumb shows when rows overflow, and the mouse wheel scrolls (spec 0007 follow-up). A row that Ctrl W was sent to reads "Closing…".
- **AC-FND-5**: The key line reads Enter Go to, Shift Enter Bring here, Ctrl W Close window, Esc Cancel. The first three are greyed when no row matches.
- **AC-FND-6**: Matched letters are bold (600), taken from nucleo's `Pattern::indices` and split per column. Columns are drawn as measured runs in two fonts, with a hand drawn … where cut.
- **AC-FND-7**: Notes use grey second lines ("It stays where it was.", "It stays open."), an info icon for "That window is gone", and a second line under "No match". New note: Enter or Shift Enter on a window whose screen is gone but not folded says "Its screen was just unplugged" / "In a moment it moves to Built-in display; try again then." (with the main display's name).
- **AC-FND-8**: The shadow and round corners come from DWM on the non layered popup (`ui::popup`). The finder is never layered and never uses `UpdateLayeredWindow`, which hides the EDIT. If slice 1's check finds that DWM gives no shadow, the fallback the index adds applies here too.
- **AC-FND-9**: Narrator reads the search box (native), the count, each row (title, app, screen, workspace, shown or hidden, pinned) as a list item with selection, and the notes as they appear.

## Build notes

- Rows get 6 px less right padding, so the chip column lines up with the count and Cancel (UI review r2.1).
- The finder's `BG`, `DIM` and `TEXT` move to `ui` tokens. `overview.rs:23` imports them, so this slice copies them into `overview.rs` as private constants until the overview slice.
- Render tests: empty, filtering with bold matches, one screen and three screens, hidden, pinned and gone chips, each note, light, dark and HC. Output goes to `target/finder/`.

## Check on the real machine

- A match inside the cut part of a long title.
- The EDIT caret and selection colors in dark and HC.

## Rationale (short)

The finder shares the indicator's surface, so it reuses slice 1 almost whole. Its one new risk, the native EDIT inside a shadowed popup, decides the shadow fallback order.
