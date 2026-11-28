# des-k

[![build](https://github.com/CanaanGM/des-k/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/CanaanGM/des-k/actions/workflows/build.yml)
[![latest release](https://img.shields.io/github/v/release/CanaanGM/des-k)](https://github.com/CanaanGM/des-k/releases/latest)

Virtual desktops **per monitor** for Windows 11.

Windows' own virtual desktops switch every screen at once. des-k gives each screen its own
workspaces: you switch the screen under your mouse, and the other screens stay where they are,
like on macOS or GNOME.

des-k is not a tiling window manager like [komorebi](https://github.com/LGUG2Z/komorebi).
Windows stay floating; des-k only hides and shows them. A switch takes about 40 ms
([how fast is it?](https://canaangm.github.io/des-k/how-it-works.html#how-fast-is-it)).

**The full guide, with pictures: <https://canaangm.github.io/des-k/>**

How it works inside, for developers: <https://canaangm.github.io/des-k/how-it-works.html>, with
[where each part's code is](https://canaangm.github.io/des-k/how-it-works.html#where-is-the-code).

## Install

1. Download the newest `des-k-…-setup.exe` from
   [Releases](https://github.com/CanaanGM/des-k/releases/latest) and run it.
2. Windows may say "Windows protected your PC", since the setup has no Windows code signature
   yet. Click **More info**, then **Run anyway**. To be sure the file is des-k's,
   [check the download](#check-a-download) first.
3. Say **Yes** to the admin prompt. A des-k icon shows up in the tray.

To update, click **Check for updates** in the tray menu, then **Install X.Y.Z**. Your workspaces
are kept.

> Why admin? When an admin window (like Task Manager) has focus, Windows stops normal programs
> from hearing the keyboard. Running des-k as admin keeps the shortcuts working everywhere.

No admin rights, the zip, or building from the code: see
[other ways to install](https://canaangm.github.io/des-k/#how-do-i-install-it).

## Shortcuts

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

Keys act on **the screen under your mouse**, or on the focused window's screen if you set
[Keys act on](https://canaangm.github.io/des-k/#change-your-shortcuts). des-k takes `Win+Tab` and `Win+1` … `9` from
Windows, and you can give them back. Forgot a shortcut? Tray → **Shortcut hints**.

## What it does

- [Finder](https://canaangm.github.io/des-k/#find-any-window): `Win+F` finds any window by its name, its app or its workspace.
- [Overview](https://canaangm.github.io/des-k/#see-everything-at-once): `Win+Tab` shows every workspace of every screen, live.
- [Touchpad](https://canaangm.github.io/des-k/#switch-with-the-touchpad): three and four finger swipes work over every window.
- [Pause](https://canaangm.github.io/des-k/#pause-des-k): every hidden window comes back and Windows gets its keys back.
  Swipes do nothing until you resume.
- [Workspaces per screen](https://canaangm.github.io/des-k/#workspaces-on-each-screen): two on the laptop, six on the big screen.
- [Names](https://canaangm.github.io/des-k/#name-your-workspaces) and [pictures](https://canaangm.github.io/des-k/#a-picture-for-each-workspace) for each
  workspace. Your Windows wallpaper is never changed.
- [Apps in their place](https://canaangm.github.io/des-k/#open-apps-in-their-place): an app's new windows open where you said.
- [Apps left alone](https://canaangm.github.io/des-k/#leave-an-app-alone): des-k never hides, follows or places their windows.
- [A hidden window that wants you](https://canaangm.github.io/des-k/#when-a-hidden-window-wants-you) shows a note on its screen.
- [Screens that come and go](https://canaangm.github.io/des-k/#screens-that-come-and-go): unplug one, and its windows wait on
  the main display until it is back.
- [After a restart](https://canaangm.github.io/des-k/#after-a-restart), reopened windows go back to their workspaces.
- [Settings](https://canaangm.github.io/des-k/#settings) change all of this, every shortcut, the indicator and the colors.

## If something goes wrong

- **A window seems lost**: press `Win+F` and search for it, or open the overview with `Win+Tab`.
- **des-k stopped** (no tray icon) and windows are still hidden: run
  `& "$env:ProgramFiles\des-k\des-k.exe" --quit`, then start **des-k** from the Start menu.

More fixes in the guide: [What do I do when …?](https://canaangm.github.io/des-k/#what-do-i-do-when)

## Your data

des-k keeps its layout, settings and log in `%LOCALAPPDATA%\des-k`, on this PC only. The only
thing it sends out is the update check you click. The guide says
[what des-k keeps](https://canaangm.github.io/des-k/#what-des-k-keeps) and how to clear it.

Also in the guide: [stop or remove des-k](https://canaangm.github.io/des-k/#stop-or-remove-des-k), and
[the Windows builds it was tried on](https://canaangm.github.io/des-k/#which-windows-builds-was-des-k-tried-on).

## Check a download

- **Is the file the one in the release?** Its SHA-256 matches its line in the release's
  `SHA256SUMS.txt`.
- **Does it come from des-k's publisher?** From 0.10.0 on, `SHA256SUMS.txt.minisig` checks out
  with the publisher's key.
- **Is it safe?** No check proves that, but the code is public and CI builds each release from its
  tagged commit.

The guide shows [the steps and the keys](https://canaangm.github.io/des-k/#check-a-download).

## License

Copyright (C) 2026 CanaanGM

des-k is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. It comes with no warranty. See
[LICENSE](LICENSE) for the full text. Want to help? See [CONTRIBUTING.md](CONTRIBUTING.md).
