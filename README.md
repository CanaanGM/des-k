# des-k

Virtual desktops **per monitor** for Windows 11.

Windows' own virtual desktops switch every screen at once. des-k gives each screen its own
workspaces: you switch the screen under your mouse, and the other screens stay where they are,
like on macOS or GNOME.

des-k is not a tiling window manager like [komorebi](https://github.com/LGUG2Z/komorebi).
Windows stay floating; des-k only hides and shows them.

**The full guide, with pictures: <https://canaangm.github.io/des-k/>**

How it works inside, for developers: <https://canaangm.github.io/des-k/how-it-works.html>

## Install

1. Download the newest `des-k-…-setup.exe` from
   [Releases](https://github.com/CanaanGM/des-k/releases/latest) and run it.
2. des-k isn't signed yet, so Windows may say "Windows protected your PC". Click
   **More info**, then **Run anyway**.
3. Say **Yes** to the admin prompt. A des-k icon shows up in the tray.

To update, click **Check for updates** in the tray menu, then **Install X.Y.Z**. des-k downloads
the setup, checks it against the release's `SHA256SUMS.txt` and runs it. Your workspaces are kept.

> Why admin? When an admin window (like Task Manager) has focus, Windows stops normal programs
> from hearing the keyboard. Running des-k as admin keeps the shortcuts working everywhere.

<details>
<summary><b>Other ways to install</b>: without admin, from the zip, or from source</summary>

- **Without admin** (a work laptop, say): choose **Install for me only** when Setup asks. des-k
  goes to `%LOCALAPPDATA%\Programs\des-k`. Without admin, its shortcuts stop while an admin
  window has focus, it can't move or close admin windows, and **Install X.Y.Z** opens the release
  page instead (run the newer setup yourself).
- **From the zip**: the release also has a zip with the same `des-k.exe` and `install.ps1`. Unzip
  it and run `powershell -ExecutionPolicy Bypass -File install.ps1` in that folder.
- **From source**: you need [Rust](https://rustup.rs) and Git. Run
  `git clone https://github.com/CanaanGM/des-k`, open a **normal** terminal (not "Run as
  administrator") in that folder and run the same `install.ps1` command. It builds des-k, asks
  once for admin, copies it to `C:\Program Files\des-k` and starts it every time you sign in. Run
  it again after pulling new code to update.

</details>

## Shortcuts

Everything works on **the screen under your mouse**. To have the keys act on the focused window's
screen instead, set Settings → Workspaces → **Keys act on**; swipes still use the screen under
the pointer.

| Keys | What it does |
|---|---|
| `Win+Ctrl+Left` / `Right` | Previous / next workspace |
| `Win+1` … `Win+9` | Go straight to workspace N (on the one you're on: go back) |
| `Win+Backspace` | Go back to the workspace this screen showed before; again to return |
| `Win+Ctrl+Shift+Left` / `Right` | Send the focused window to the previous / next workspace, and go with it |
| `Win+Ctrl+Alt+Left` / `Right` | Send it there, but stay where you are |
| `Win+Shift+1` … `9` | Send the focused window to workspace N, and go with it |
| `Win+Ctrl+1` … `9` | Send it to workspace N, but stay |
| `Win+Shift+Left` / `Right` | Move a window to another screen (this one is Windows' own) |
| `Win+F` | **Finder**: search every window on every screen and workspace |
| `Win+Tab` | **Overview**: see every workspace of every screen at once |
| `Win+Ctrl+U` | **Pin** the focused window: it stays on every workspace of its screen; again to unpin |

des-k takes over `Win+Tab` (Windows' Task View) and `Win+1` … `9` (Windows' "open the Nth
taskbar app"). You can change any shortcut in Settings. Forgot one? Tray → **Shortcut hints**.

## What else it does

- **Finder** (`Win+F`): type part of a window's name or app. `Enter` goes to it, `Shift+Enter`
  brings it here, `Ctrl+W` closes it.
- **Overview** (`Win+Tab`): live previews of every workspace. Click a window to go to it, or drag
  it onto another workspace's tile to move it there.
- **Touchpad**: three and four finger swipes and taps work over every window, admin ones too.
  Four fingers left and right switch workspaces. Windows' own gestures must be set to
  **Nothing**; the welcome screen helps you do that.
- **Wallpapers**: each workspace can have its own picture, or a slideshow, on every screen or on
  one. Your Windows wallpaper is never changed; a workspace without a picture shows it.
- **Apps in their place**: give an app a workspace (and a screen) in Settings → **Apps**, and its
  new windows open there, without switching your screens.
- **Apps des-k leaves alone**: for an app that breaks when hidden or keeps grabbing focus, pick
  **Leave alone** in its row in Settings → **Apps**, or in `settings.toml`:

  ```toml
  [apps]
  "teams.exe" = "alone"   # an exe file name, or a Store app id
  ```

  des-k then never hides, follows, places or restores its windows, so they stay in view on every
  workspace. The finder and the overview leave them out; reach them with Alt+Tab or the taskbar.
- **Names**: call a workspace "Mail" or "Code", and rename a screen. The names show beside the
  numbers everywhere.
- **A hidden window that wants you** (a new Slack message, say) shows a note on its screen, and
  its workspace keeps a dot until you go there.
- **Settings** (tray → **Settings**): the number of workspaces (2 to 9), names, wallpapers, app
  rules, every shortcut, the indicator, colors and gestures. Changes apply at once.
- **Workspaces per screen**: each screen can have its own number, say two on the laptop and six
  on the big screen. Each screen's row in Settings has its own + and ×, or in `settings.toml`:

  ```toml
  workspaces = 4        # every screen without its own count

  [workspaces_per_screen]
  "internal" = 2        # the laptop panel
  ```

  A screen without a line has `workspaces`. Lowering a screen's count moves the windows of the
  removed workspaces to its new last one.
- **Pause** (tray → **Pause des-k**, or a key you set in Settings): every hidden window comes back
  and Windows gets its own shortcuts back, for a game or to see Windows on its own. **Resume
  des-k** puts each window back on its workspace. Swipes do nothing until you resume, and the
  tray icon turns gray. A restart while paused starts paused.
- **Screens**: unplug one and its windows move to the main display. Plug it back in and they go
  back to their workspace, place and size.
- Windows on other workspaces leave the taskbar and Alt+Tab (Store apps like Calculator keep
  their taskbar button; Windows ignores des-k there). If a hidden window pops up by
  itself, des-k switches to its workspace. After a restart, reopened windows go back to their
  workspaces.

The [guide](https://canaangm.github.io/des-k/) explains each of these in full.

## Your data

des-k keeps its layout, settings and log in `%LOCALAPPDATA%\des-k`, on this PC only. The log
names windows by their app (`chrome.exe`), never by their title or folder, so it is safe to share.
The only thing des-k sends out is the update check you click: a request to github.com that
carries your address and des-k's version, nothing else.

The layout lives in `state.txt`, so des-k can put your windows back after a restart or a reboot.
It holds:

- each screen: its model and serial number, its area, and which workspace it shows
- each window des-k tracks: its app's file name (`code.exe`, never its folder), its window class,
  its place and size, its window handle and process id, and its title, only while **Restore** is
  on in settings (restore uses the title to tell two windows of one app apart)

While des-k runs, every tracked window is in there. **Quit** leaves only your screens and the
windows waiting for an unplugged screen. To clear it, Quit from the tray, then delete
`state.txt`. Deleting it while des-k runs does nothing: des-k writes it again at the next change.

## Stop or uninstall

**Quit** from the tray stops des-k and shows every hidden window, so nothing is lost. It also
forgets which window was on which workspace, and gives Windows its touchpad gestures back from
your next sign in.

To remove des-k, uninstall it from **Settings → Apps → Installed apps**. If you installed with
`install.ps1`, Quit it first, then run this in an **admin** terminal:

```powershell
Unregister-ScheduledTask -TaskName des-k -Confirm:$false
Remove-Item -Recurse "$env:ProgramFiles\des-k"
```

Your settings and log stay in `%LOCALAPPDATA%\des-k`; delete that folder too if you want.

## If something goes wrong

- **A window seems lost**: press `Win+F` and search for it, or open the overview.
- **Shortcuts do nothing while a remote control app has focus** (TeamViewer, AnyDesk, Remote
  Desktop): the app takes the keys before des-k hears them, and may send them to the other PC.
  Click another window first, and they work again.
- **des-k stopped** (no tray icon) and windows are still hidden: run
  `& "$env:ProgramFiles\des-k\des-k.exe" --quit`. It shows every window des-k hid. Then start
  **des-k** from the Start menu.
- **Defender calls des-k a trojan** (like `Trojan:Win32/Bearfoos.A!ml`): `!ml` is a machine
  learning guess, common for new unsigned programs. Check the file against the release's
  `SHA256SUMS.txt`, update Defender, then restore des-k from Windows Security → Protection history.
- **To see why**: tray → **Open log**. `des-k.old.log` beside it holds the run before.

## Windows builds it was tried on

des-k hides windows through an undocumented part of Windows, so it matters which Windows it ran
on. This list names each Windows build des-k was tried on, and each release adds its own.

| Windows 11 | Build | Releases tried on it |
|---|---|---|
| 25H2 | 26200 | 0.1.0 to 0.9.0 |

Another Windows 11 build may work too. A big Windows update could change that undocumented part.
If it does, the tray shows a warning and switching hides nothing until des-k is updated.

## License

Copyright (C) 2026 CanaanGM

des-k is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. It comes with no warranty. See
[LICENSE](LICENSE) for the full text.
