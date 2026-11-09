# Changelog

What changed in each release, newest first. Specs live in `docs/specs/`. Downloads are on
[Releases](https://github.com/CanaanGM/des-k/releases).

## Unreleased

- The README says how des-k differs from a tiling window manager.

## 0.6.1 (2026-11-07)

- `des-k.exe` has a version card (Properties → Details).

## 0.6.0 (2026-11-07)

- Pick your own accent and workspace colors, and where the indicator shows (spec 0020).
- Log lines carry a time and a level, never a window title, so a log is safe to share
  (spec 0019).

## 0.5.0 (2026-11-07)

- Install updates from the tray: the setup is downloaded, checked against `SHA256SUMS.txt` and
  run silently (spec 0018).

## 0.4.1 (2026-11-07)

- `Win+Tab` works while one of des-k's own windows has focus.

## 0.4.0 (2026-11-07)

- New look for the indicator, finder, overview, tray menu, settings page and first run
  (spec 0017).
- Windows go back to their screen when it is plugged in again (spec 0016).
- Check for updates from the tray (spec 0015).
- Shortcut hints; des-k's own windows are never hidden by a switch.
- An icon for the exe and the setup.

## 0.3.0 (2026-11-03)

- Pin a window so it stays on every workspace (spec 0014).

## 0.2.1 (2026-11-03)

- Renamed from pdesk to des-k; your data folder moves over by itself.
- A setup exe with an uninstaller (spec 0013).
- Touchpad setup moved to the settings page; the welcome screen and settings page fit small
  screens.

## 0.2.0 (2026-11-02)

- Touchpad gestures (spec 0011).
- A settings page with a shortcut recorder; settings apply live (spec 0012).
- A welcome screen on first run.
- Overview: a fade, and closed windows leave at once.
- Licensed under the GPL 3.0 or later.

## 0.1.0 (2026-10-30)

- First release: per monitor workspaces, finder, overview, saved layout.
