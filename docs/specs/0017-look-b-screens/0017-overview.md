# 0017. Overview in look B, with a speed limit

## Summary

The overview keeps its behavior (spec 0010) and gets the look B layout:
- a card colored band of workspace tiles, with chips under them;
- spread windows with a soft shadow;
- the shown workspace in the settings page's ring;
- purple only for the chosen window and drop targets;
- a key bar on the selected screen.

It must show on all your screens within 100 ms. Its fade gets its own time setting.

**Design source**: `pdesk-design/restart/overview-r3.html` (r2.1), sha256 `5731599af1fe26b8465dd3e4dc4e7ecd9b07c11da7213bada2442ed61091d3c8`, captures `restart/captures/ovr3-*` (sums `SHA256SUMS-ovr3`). Approved 2026-11-05.

**Replaces**: spec 0010 AC-13 (look). The fixed `FADE_MS` (250) becomes `[overview] fade_ms`.

## Acceptance criteria

- **AC-OVR-1**: Windows' light or dark mode, with system colors in HC. The tile shown on each screen gets the settings page's current workspace ring (text color). Workspace colors appear only on chips. Purple appears only on the chosen window and on drop targets.
- **AC-OVR-2**: Layout as the design:
  - a card colored band with a rule;
  - chips under the tiles instead of numbers;
  - the spread starts 24 dip under the band and ends 64 dip above the screen's bottom (room for the bar);
  - text 14/20 regular (today 16 semibold, `overview.rs:595`);
  - rings 2 px, 3 px off the edge (today `OUTLINE` 3, touching);
  - the close X 28 dip square, radius 6 (today `CLOSE_BTN` 24 round), red when hot (`WM_MOUSEMOVE` and `WM_MOUSELEAVE`).
- **AC-OVR-3**: Spread windows get a floating shadow: a pre blurred nine slice image, made once per DPI and kept, drawn with `AlphaBlend` before DWM draws the previews. With `fastest` on, it is skipped. Tiles show the screen's wallpaper, scaled to tile size, from a cache kept for the process's life:
  - The cache is keyed per monitor by the wallpaper's path, file time and `GetPosition` (`IDesktopWallpaper`).
  - It is checked at open, and the image is decoded again only when the key changed.
  - A solid color, a slideshow, Spotlight, or a failed read (explorer hung or restarting) falls back to `GetBackgroundColor`, or the card color when that fails too.
  - A wallpaper read never blocks the open: if the cache is cold, the tiles first paint the background color, and the wallpaper is painted when it is ready.
- **AC-OVR-4**: Rings, one meaning each:
  - no target ring on the dragged window's own tile or spread;
  - the chosen ring is skipped while a drag moves (today it is always drawn, about `:1252`);
  - the drag source shows at 40% (`DWM_TNP_OPACITY`);
  - the pointed at tile gets a 1 px edge (dotted in HC).
- **AC-OVR-5**: The drag ghost has a 1 px text colored edge, and its shadow comes from DWM on the non layered ghost (else edge only). It is 240 dip wide over the spread and tile size (at least 32) over the band, resized only when it crosses the band's rule.
- **AC-OVR-6**: Labels: Hover also labels the chosen window. Hover and chosen labels run up to the cell width. Pinned windows show a pin glyph.
- **AC-OVR-7**: The key bar shows on the selected screen only and moves with it:
  - Enter Go to, Drag Move (dashed cap), 1 to N Show workspace, Ctrl W Close window, Esc Cancel;
  - Enter and Ctrl W are greyed with an empty spread, and Drag shows only when the screen lists no window;
  - during a drag it reads "Drop on a workspace · Esc Cancel";
  - a note replaces it for `NOTE_MS` on its screen (a drop's note on the screen dropped on), with a grey line beside the words.
- **AC-OVR-8**: Notes:
  - "Can't move this window" / "It stays where it was.";
  - "Can't close an admin window" / "It stays open." only on `ERROR_ACCESS_DENIED` (as `finder.rs:466`), else "Can't close this window" / "It stays open.";
  - new: when a closed window comes back (`closing_tick`), it is selected and the note reads "A window didn't close" / "It may be asking you something. Enter goes to it." (info icon), on the screen where the close was asked.
- **AC-OVR-9**: Fades use `ui::motion_ms([overview] fade_ms)` (index AC-6, AC-7).
- **AC-OVR-10**: Speed (index AC-8): within 100 ms of the key press on the screens you have now, measured as the index says, with the log line on every open. To get there:
  - hover, selection and drag repaint only the changed rectangles (`InvalidateRect` with the old and new bounds, never the whole popup);
  - the shadow image (per DPI) and the wallpaper tiles are cached (AC-OVR-3);
  - fonts come from the shared cache.
- **AC-OVR-11**: Narrator reads each screen's tiles (workspace number, shown or not, window count) and spread windows (title, app, chosen), and the bar's words as they change.

## Build notes

- The private copies of the finder colors that the finder slice left in `overview.rs`, and `TILE_BG` (`:32`), go to `ui` tokens and are deleted.
- `indicator::ACCENT` is deleted in this slice: the overview is its last user.
- `[overview] fade_ms` goes into `Settings`, `OVERVIEW_KEYS`, `DEFAULT_FILE`, write values, the problem title and a test.
- Render tests: one, three and twelve screens, a drag, each note, empty spread, light, dark and HC. Output goes to `target/overview/`. Live previews and the wallpaper are drawn as flat stand in rectangles there, since DWM thumbnails don't render into a bitmap.

## Check on the real machine

- The shadow during the fade.
- Ghost flicker when it crosses the rule.
- The 100 ms limit on the screens you have now, ten opens, cold and warm wallpaper cache.

## Rationale (short)

The overview is the one window big enough to stress GDI+. So it carries the speed limit and the repaint rules, and if it misses after them, it alone moves to Direct2D (a follow up spec).
