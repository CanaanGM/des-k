# pdesk

Virtual desktops **per monitor** for Windows 11.

Windows already has virtual desktops, but switching one switches *every* screen at once.
pdesk gives each screen its own set of workspaces instead: you switch the screen under your
mouse, and the other screens stay exactly where they are. Like on macOS or GNOME.

Windows stay floating, just as they normally are. pdesk never tiles or rearranges them; it only
hides and shows them.

**There's a nicer version of this guide, with pictures: <https://canaangm.github.io/pdesk/>**
## Install (the easy way)

1. Download the newest `pdesk-…-windows-x64.zip` from
   [Releases](https://github.com/CanaanGM/pdesk/releases/latest) and unzip it.
2. Open a terminal in the unzipped folder and run:

   ```powershell
   powershell -ExecutionPolicy Bypass -File install.ps1
   ```

3. Say **Yes** to the admin prompt. A pdesk icon shows up in the tray.

To update, do the same with the newer zip. Your workspaces are kept.

## Build it yourself instead

You need Windows 11, [Rust](https://rustup.rs) and Git. Get the code with
`git clone https://github.com/CanaanGM/pdesk`, then open a **normal** terminal (not "Run as
administrator") in that folder and run:

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

It builds pdesk, asks once for admin (the UAC prompt), copies it to `C:\Program Files\pdesk`,
and sets it to start by itself every time you sign in. A small pdesk icon shows up in the tray.

To update after pulling new code, just run the same command again. Your workspaces are kept.

> Why admin? When an admin window (like Task Manager) has focus, Windows stops normal programs
> from hearing the keyboard. Running pdesk as admin keeps the shortcuts working everywhere.

## Shortcuts

Everything works on **the screen under your mouse**.

| Keys | What it does |
|---|---|
| `Win+Ctrl+Left` / `Right` | Previous / next workspace |
| `Win+1` … `Win+9` | Go straight to workspace N |
| `Win+Ctrl+Shift+Left` / `Right` | Send the focused window to the previous / next workspace, and go with it |
| `Win+Ctrl+Alt+Left` / `Right` | Send it there, but stay where you are |
| `Win+Shift+1` … `9` | Send the focused window to workspace N, and go with it |
| `Win+Ctrl+1` … `9` | Send it to workspace N, but stay |
| `Win+Shift+Left` / `Right` | Move a window to another screen (this one is Windows' own) |
| `Win+F` | **Finder**: search every window on every screen and workspace |
| `Win+Tab` | **Overview**: see every workspace of every screen at once |

Each switch shows a small row of dots for a moment, so you can see which workspace you're on.

Note: pdesk takes over `Win+Tab` (Windows' Task View) and `Win+1` … `9` (Windows' "open the
Nth taskbar app"). You can change any shortcut in the settings.

### Finder (`Win+F`)

Type part of a window's name or its app. The list starts with the windows you used most recently.

- `Enter`: go to that window (its screen switches to the right workspace)
- `Shift+Enter`: bring the window here instead
- `Ctrl+W`: close it
- `Esc`: close the finder

### Overview (`Win+Tab`)

Every screen shows a row of numbered tiles at the top, one per workspace, with live previews
of their windows. Below that, the current workspace's windows are spread out so none overlap.

- Click a window to go to it, or click a tile to switch that screen
- Drag a window onto a tile (on any screen) to move it there
- Arrow keys or `Tab` pick a window, `Enter` goes to it, a digit switches the workspace
- Hover a window and click its **X**, middle click it, or press `Ctrl+W` to close it
- `Esc`, a click on the background, or `Win+Tab` again closes the overview

The overview is the newest part and is still being worked on.

## Touchpad gestures

pdesk reads your touchpad's three and four finger swipes and taps itself, so they work over
every window, admin ones like Task Manager too. Out of the box:

| Gesture | 4 fingers | 3 fingers |
|---|---|---|
| Swipe left | next workspace | next app |
| Swipe right | previous workspace | previous app |
| Swipe up | overview | overview |
| Swipe down | show desktop | show desktop |
| Tap | notifications | search |

Windows has its own three and four finger gestures, and one swipe must never do two things. So
pdesk acts only while Windows' own ones are all set to **Nothing**. The first time pdesk starts,
a welcome screen helps you with that. It shows what each gesture does in pdesk. **Use these
gestures** saves your current Windows values and opens Settings at Touchpad for you to set them
to Nothing. **Choose my own** does the same and also opens `settings.toml`, so you can change
what each gesture does. If one is still on, pdesk logs which one and does nothing for that
gesture. Open the screen again any time from the tray, **Touchpad setup**. It shows, for each
kind of gesture, whether Windows still has it on and what pdesk runs for it.

**Give gestures back to Windows** (on that same screen) writes your saved values back and stops
pdesk reading the touchpad at once. Windows uses them from your next sign in, or set them in
Settings now. **Quit** writes them back too, from your next sign in; start pdesk again in the
same sign in and its gestures keep working.

Change what each gesture does in `settings.toml` (the `[touchpad]` part and the `[[gesture]]`
blocks; the comments there list every action). A gesture can also send a key combo, or run a
second action on `times = 2`. Set `enabled = false` to make pdesk ignore the touchpad. Plugging
a touchpad in or out is picked up without a restart. Two finger scroll, zoom and right click
stay exactly as they are.

## Settings

Right click the tray icon → **Open settings**. The file is
`%LOCALAPPDATA%\pdesk\settings.toml`, and every line has a comment saying what it does. Things
you can change:

- How many workspaces each screen has (2 to 9, default 4)
- Every shortcut, or turn one off
- The dots indicator: on or off, and how long it stays
- Whether the Copilot key acts as `Right Ctrl`
- The overview's window titles (`"hover"`, `"always"` or `"none"`), whether it updates live,
  and how it opens (`"off"` or `"fade"`)
- What each three and four finger gesture does, how far a swipe travels before it counts, or
  touchpad reading off altogether

pdesk reads the file once at start, so after editing it run `install.ps1` again (or sign out
and back in). If you type a bad value, only that one setting goes back to its default; the log
says which.

## Good to know

- Windows on other workspaces disappear from the taskbar and from Alt+Tab, so each screen only
  shows what's on it. (Admin windows like Task Manager are the exception and still show in
  Alt+Tab.)
- If a hidden window pops up by itself (a chat message, say), pdesk switches its screen to that
  window's workspace.
- After a restart, pdesk puts reopened windows back on the workspaces they were on, for the
  first few minutes after you sign in.
- The tray icon menu has **Touchpad setup**, **Open settings**, **Open log** and **Quit**.

## Stopping and uninstalling

**Quit** from the tray (or `& "$env:ProgramFiles\pdesk\pdesk.exe" --quit`) stops pdesk and
shows every hidden window, so nothing is ever lost. It also forgets which window was on which
workspace, on purpose. If pdesk runs your touchpad gestures, Quit also hands Windows its own
gestures back from your next sign in.

To remove it completely, **Quit it first** (so your windows and touchpad gestures come back),
then in an **admin** terminal:

```powershell
Unregister-ScheduledTask -TaskName pdesk -Confirm:$false
Remove-Item -Recurse "$env:ProgramFiles\pdesk"
```

Your settings and log stay in `%LOCALAPPDATA%\pdesk`; delete that folder too if you want.

## If something goes wrong

- **A window seems lost**: press `Win+F` and search for it, or open the overview.
- **Shortcuts stopped working**: check the tray icon is there. If not, run `install.ps1` again.
- **Everything is weird**: Quit from the tray. Every window comes back. Then run `install.ps1`.
- The log (tray → **Open log**) says what pdesk did and why.

pdesk uses an undocumented part of Windows to hide windows. It works on Windows 11 25H2, but a
big Windows update could change that.

## License

Copyright (C) 2026 CanaanGM

pdesk is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. It comes with no warranty. See
[LICENSE](LICENSE) for the full text.

The third party agent skills in `.claude/skills/` keep their own authors' licenses.
