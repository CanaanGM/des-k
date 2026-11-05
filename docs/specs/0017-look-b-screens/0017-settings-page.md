# 0017. Settings page in look B

## Summary

The settings page keeps what it does (spec 0012: every setting, saved at once, applied live, comments kept) and takes the approved look B layout:
- a wider window you can resize;
- workspace tiles as the control;
- keys as fields;
- a des-k or Windows choice for each taken key;
- a Touchpad section.

It also gets three new rows: the indicator style, `fastest`, and the overview's fade time.

**Design source**: `pdesk-design/restart/settings-r3.html` (r2.1), sha256 `04c0b5c4cf9a722c2c7e1eafbc1086db5fd4d11d7108a60779bb3155bfc35892`, captures `restart/captures/sr3-*` (sums `SHA256SUMS-sr3`). Approved 2026-11-04. Also:
- the indicator approval's amendments (Style row, preview, captions);
- the rows for `fastest` and `[overview] fade_ms` from the short design pass (index build plan task 0), once you approve it.

**Replaces**: spec 0012's look, AC-1's size, AC-10's bars, AC-13 (replaced by the Touchpad section) and AC-2's counts. Every other 0012 criterion (saving, clash, recording, workspace removal) stands.

## Acceptance criteria

- **AC-SET-1** (amends 0012 AC-1): 1040 dip wide and resizable. It opens on the screen under the mouse.
- **AC-SET-2**: The names follow the first run: Shortcuts, "Send window to 1–9 and stay", the key combo wording. A Welcome entry reopens the first run.
- **AC-SET-3** (amends 0012 AC-10): Numbers use steppers only, with no bars. The workspace count is set through the tiles (KDE style), and a removal note shows what moves where, per screen, unplugged screens included (their windows per workspace). Each value's Reset sits in its caption line. A Reset of a list goes through the clash check.
- **AC-SET-4**: Keys show as fields with × and +. A key Windows also uses offers a des-k or Windows choice. When Windows' own key is drawn first, it maps back to the file's order. The hints key has its row. The Copilot key sits under Shortcuts (`home()`).
- **AC-SET-5**: Each side list item shows its count of problems. Problems have plain titles made from each log line (amends spec 0001).
- **AC-SET-6** (replaces 0012 AC-13): a Touchpad section with Give back and its results (spec 0011). Its Open Windows Settings button calls `setup::use_gestures`. Gesture menu and overview value names keep the first run's words (Never, None and Fade).
- **AC-SET-7**: The Indicator section has:
  - a Style row (Card, Dots), greyed while the indicator is off;
  - a preview that draws the chosen style for the page's screen, through `indicator::draw_preview`;
  - the hold caption "With a line under it, at least 1.5 s.";
  - the section line "and when a screen comes or goes".

  `[indicator]` now counts 4 keys.
- **AC-SET-8**: Rows for `fastest` and `[overview] fade_ms`, drawn to the approved design pass. The fade time is greyed while `animation = "off"` or `fastest` is on.
- **AC-SET-9**: Field edges use the darker edge you chose. The chosen segment uses the ring. Focus is a 2 dip text colored ring, 2 dip out.
- **AC-SET-10**: Narrator reads every control: its name, value, state, and its caption as the description.

## Check on the real machine

- Mouse users find the × on the last tile.
- Resizing at 150%.

## Rationale (short)

The page is the biggest window after the first run and shares its pickers and key fields. It comes fifth so those parts settle on the popups first, and so the design pass for the new rows can land before it.
