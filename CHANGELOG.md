# Changelog

What changed in each release, newest first. Downloads are on
[Releases](https://github.com/CanaanGM/des-k/releases).

## 0.10.0 (2026-11-24)

- **Updates prove they come from des-k's publisher.** Each release's checksum file is now signed
  with the publisher's own key, on the publisher's own PC. **Install X.Y.Z** in the tray runs an
  update only when that signature is good, was made for that version, and the setup matches it,
  so taking over des-k's GitHub account is no longer enough to run a program on your PC. A
  release that fails the check is refused with a warning and never installed. Updating to 0.10.0
  itself from an older des-k still uses the checksum alone; every update after it is checked.
  [RELEASING.md](RELEASING.md) shows how to check a download by hand.
- **Ready for a Windows that changed.** des-k hides windows through a part of Windows that has no
  public documentation. It now checks that part's fixed id before calling anything in it. On a
  Windows where the id changed, des-k calls nothing there, keeps switching workspaces without
  hiding windows, and the tray tells you why and offers to check for an update.
- **Ending des-k at any moment loses no window.** des-k now writes a window into its layout file
  before it hides it, not after. If des-k is ended halfway through a switch (Task Manager, a
  crash), `des-k --quit` or the next start still finds every window it hid. If it can't write that
  file, on a full disk for example, it hides nothing and says so on screen and in the tray.
  Each release is now tested this way before it ships ([RELEASING.md](RELEASING.md)).
- [SECURITY.md](SECURITY.md) now says what runs as admin, each file des-k reads from your data
  folder, and why no setting can name a program or a path.

## 0.9.0 (2026-11-22)

- **A picture for each workspace.** Put pictures in des-k's wallpapers folder (Settings →
  Workspaces → Wallpaper → **Open folder**) and pick one per workspace, on every screen or on one.
  Switching a screen changes only that screen's picture. Your Windows wallpaper is never changed:
  a workspace with no picture, a quit or a crash simply shows it again. Fill, fit, stretch or
  center; an instant change or a short fade; and **Dim**, which darkens pictures so white icon
  labels stay readable.
- **Slideshows.** A workspace's picture can be a folder, or a few pictures you tick, shown one
  after another. Each workspace can have its own pace, and **Shuffle** picks a random order. A
  slideshow only moves while you can see it, so you come back to the picture you left.
- **Open apps in their place.** In Settings → **Apps**, give an app a workspace (and a screen if
  you like). Its new windows land there the moment they open, hidden if that workspace isn't
  showing, with a short note. Your screens never switch for it. des-k only matches apps; it never
  starts one.
- **Names.** Call a workspace "Mail" or "Code" on every screen, or differently on one screen, and
  rename a screen ("Left"). The names show beside the numbers on the indicator, the overview, the
  finder and the tray menu.
- **Go back.** `Win+Backspace` takes the screen under the mouse back to the workspace it showed
  before, and again returns you. `Win+N` on the workspace you're already on does the same, and a
  gesture can too (`do = "back"`).
- **A hidden window that wants you.** When a window on a workspace you aren't looking at flashes
  for attention, its screen shows a note in your own words ("Slack misses you"), and that
  workspace keeps a dot until you go there. Change the words in Settings → **Attention**.
- **What's new.** After an update, des-k shows once what the release added, with a button that
  opens each setting. It never opens with the first run, and Settings → **What's new** shows it
  again any time.
- A maximized window that goes back to a screen with a different scaling keeps its normal size;
  before, each trip shrank it.
- Store apps (Calculator, say) and Windows 11's Notepad show their real names.
- Starting des-k before the taskbar is up no longer logs an error for the tray icon; it waits for
  the taskbar and says so.

## 0.8.0 (2026-11-17)

- Pick a workspace's color or your accent without typing a hex value: click a tile under
  **Workspaces**, a workspace in the **Overview** picture, or a swatch under **Colors**, then
  choose one of des-k's colors or any color on the wheel. Esc puts the old color back.
- A color you save shows at once in every open des-k window.

## 0.7.1 (2026-11-10)

- A shorter README; the full details live in the [guide](https://canaangm.github.io/des-k/),
  which now covers **Install for me only** and updating from the tray.
- Unplugging a dock no longer squeezes a screen's windows or leaves one behind: des-k waits for
  the screens to settle before it notes where windows are, and before it puts them back.

## 0.7.0 (2026-11-09)

- The README says how des-k differs from a tiling window manager.
- Setup offers **Install for me only** for accounts without admin. That copy runs without admin,
  so its shortcuts stop while an admin window has focus.
- Switching past the last workspace goes to the first, and back, the way sending a window does.

## 0.6.1 (2026-11-07)

- `des-k.exe` has a version card (Properties → Details).

## 0.6.0 (2026-11-07)

- Pick your own accent and workspace colors, and where the indicator shows.
- Log lines carry a time and a level, never a window title, so a log is safe to share.

## 0.5.0 (2026-11-07)

- Install updates from the tray: the setup is downloaded, checked against `SHA256SUMS.txt` and
  run silently.

## 0.4.1 (2026-11-07)

- `Win+Tab` works while one of des-k's own windows has focus.

## 0.4.0 (2026-11-07)

- New look for the indicator, finder, overview, tray menu, settings page and first run
 .
- Windows go back to their screen when it is plugged in again.
- Check for updates from the tray.
- Shortcut hints; des-k's own windows are never hidden by a switch.
- An icon for the exe and the setup.

## 0.3.0 (2026-11-03)

- Pin a window so it stays on every workspace.

## 0.2.1 (2026-11-03)

- Renamed from pdesk to des-k; your data folder moves over by itself.
- A setup exe with an uninstaller.
- Touchpad setup moved to the settings page; the welcome screen and settings page fit small
  screens.

## 0.2.0 (2026-11-02)

- Touchpad gestures.
- A settings page with a shortcut recorder; settings apply live.
- A welcome screen on first run.
- Overview: a fade, and closed windows leave at once.
- Licensed under the GPL 3.0 or later.

## 0.1.0 (2026-10-30)

- First release: per monitor workspaces, finder, overview, saved layout.
