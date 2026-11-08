# des-k

Virtual desktops **per monitor** for Windows 11.

Windows already has virtual desktops, but switching one switches *every* screen at once.
des-k gives each screen its own set of workspaces instead: you switch the screen under your
mouse, and the other screens stay exactly where they are. Like on macOS or GNOME.

des-k is not a tiling window manager like [komorebi](https://github.com/LGUG2Z/komorebi).
Windows stay floating, just as they normally are. des-k never tiles or rearranges them; it only
hides and shows them.

**There's a nicer version of this guide, with pictures: <https://canaangm.github.io/des-k/>**
## Install (the easy way)

1. Download the newest `des-k-…-setup.exe` from
   [Releases](https://github.com/CanaanGM/des-k/releases/latest) and run it.
2. des-k isn't signed yet, so Windows may say "Windows protected your PC". Click
   **More info**, then **Run anyway**.
3. Say **Yes** to the admin prompt. A des-k icon shows up in the tray.

To update, run the newer setup the same way. Your workspaces are kept.

The release also has a zip with the same `des-k.exe` and `install.ps1`, if you'd rather use a
script: unzip it and run `powershell -ExecutionPolicy Bypass -File install.ps1` in that folder.

## Build it yourself instead

You need Windows 11, [Rust](https://rustup.rs) and Git. Get the code with
`git clone https://github.com/CanaanGM/des-k`, then open a **normal** terminal (not "Run as
administrator") in that folder and run:

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

It builds des-k, asks once for admin (the UAC prompt), copies it to `C:\Program Files\des-k`,
and sets it to start by itself every time you sign in. A small des-k icon shows up in the tray.

To update after pulling new code, just run the same command again. Your workspaces are kept.

> Why admin? When an admin window (like Task Manager) has focus, Windows stops normal programs
> from hearing the keyboard. Running des-k as admin keeps the shortcuts working everywhere.

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
| `Win+Ctrl+U` | **Pin** the focused window: it stays on every workspace of its screen; again to unpin |

Each switch shows a small card for a moment, so you can see which workspace you're on: its number
in its own color, the screen's name when you have two or more, and a bar per workspace. (Prefer a
plain row of dots? Pick the Dots style in Settings, Indicator.) After a pin, every bar lights up:
the window is on all of them. Sending a pinned window to a workspace unpins it.

Forgot a shortcut? Tray → **Shortcut hints** lists every shortcut and gesture as you have them set.

Note: des-k takes over `Win+Tab` (Windows' Task View) and `Win+1` … `9` (Windows' "open the
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

des-k reads your touchpad's three and four finger swipes and taps itself, so they work over
every window, admin ones like Task Manager too. Out of the box:

| Gesture | 4 fingers | 3 fingers |
|---|---|---|
| Swipe left | next workspace | next app |
| Swipe right | previous workspace | previous app |
| Swipe up | overview | overview |
| Swipe down | show desktop | show desktop |
| Tap | notifications | search |

Windows has its own three and four finger gestures, and one swipe must never do two things. So
des-k acts only while Windows' own ones are all set to **Nothing**. The first time des-k starts,
a welcome screen helps you with that. It shows what each gesture does in des-k. **Set up in
Windows Settings** saves your current Windows values and opens Settings at Touchpad for you to set them
to Nothing. **Choose my own** does the same and also opens the settings page at Touchpad, so
you can change what each gesture does. If one is still on, des-k logs which one and does nothing for that
gesture. Open the screen again any time from the settings page: Touchpad, **Touchpad setup**. It shows, for each
kind of gesture, whether Windows still has it on and what des-k runs for it.

**Give gestures back to Windows** (on that same screen) writes your saved values back and stops
des-k reading the touchpad at once. Windows uses them from your next sign in, or set them in
Settings now. **Quit** writes them back too, from your next sign in; start des-k again in the
same sign in and its gestures keep working.

Change what each gesture does on the settings page, under Touchpad (or by hand in
`settings.toml`, the `[touchpad]` part and the `[[gesture]]` blocks). A gesture can also send a
key combo, or run a second action when you do it twice. The top of that section says whether
des-k is reading your gestures right now, or what it is waiting for. Turn **Read touchpad
gestures** off to make des-k ignore the touchpad, no restart needed. Plugging
a touchpad in or out is picked up without a restart. Two finger scroll, zoom and right click
stay exactly as they are.

## Settings

Right click the tray icon → **Settings** to open the settings page. It shows every setting,
says when each change takes effect, and saves each click to
`%LOCALAPPDATA%\des-k\settings.toml` at once. You can also edit that file by hand (the page's
**Open settings.toml** opens it); every line has a comment saying what it does. On the page,
Tab moves between the side list and the settings, Up and Down go row to row, and Left and Right
change a value. Things you can change:

- How many workspaces each screen has (2 to 9, default 4), and whether des-k follows a hidden
  window to its workspace when it comes to the front
- Every shortcut, or turn one off
- The indicator: on or off, card or dots, how long it stays, and where it shows (see below)
- des-k's colors: your accent and a color per workspace (see below)
- Whether the Copilot key acts as `Right Ctrl`
- The overview's window titles (`"hover"`, `"always"` or `"none"`), whether it updates live,
  and how it opens (`"off"` or `"fade"`)
- **Skip animations** (Motion): every des-k fade and move happens at once
- What each three and four finger gesture does, how far a swipe travels before it counts, or
  touchpad reading off altogether

**Colors.** des-k's accent follows your Windows accent color and changes with it. Choose **My
own** to type a color instead (like `2E8B57`); des-k mixes two neighbor shades from it for its
headers and badges. Each workspace can have its own color too, shown on the indicator, the
finder, the overview and the tray menu. **Reset** puts des-k's color back. With Windows' high
contrast on, des-k uses Windows' colors and ignores yours.

**Where the indicator shows.** Under Indicator, click one of the 9 spots on the little screen
picture (corners, edges or center), set how far it stays from the edges, or choose **Custom**
and drag it anywhere. With two or more screens, pick one in the Screen list to give it its own
place; **Same as all screens** undoes that, and **Show it there** shows the real indicator on
that screen.

Workspaces shows a picture of each screen with its windows. Hover × on the last workspace to
see which windows would move before you remove it; they go to the workspace before it, never
away. To record a shortcut, click **+ Add** and press the combo; if another action already
uses it, the page asks before moving it. Indicator's **Play** shows the dots as they are set.

des-k checks the file once a second and applies every change right away, except restore,
which applies from des-k's next start (Quit it from the tray and start **des-k** from the Start
menu, or sign out and back in). If
you change the file by hand while the page is open, a banner says what changed. If you type a
bad value, only that one setting
goes back to its default; the page and the log say which. If the file can't be read at all,
des-k keeps the settings it had and the page turns read only until you fix it.

## Good to know

- Windows on other workspaces disappear from the taskbar and from Alt+Tab, so each screen only
  shows what's on it. (Admin windows like Task Manager are the exception and still show in
  Alt+Tab.)
- If a hidden window pops up by itself (a chat message, say), des-k switches its screen to that
  window's workspace.
- After a restart, des-k puts reopened windows back on the workspaces they were on, for the
  first few minutes after you sign in.
- When you unplug a screen (or close the laptop lid), its windows move to the main display after
  10 seconds (30 just after a wake from sleep): what was hidden on its workspace 2 goes hidden into
  the main display's workspace 2, and what you were looking at stays in view. Plug the screen in
  again, even after a sleep, a reboot or a power cut, and 2 seconds later every one of its windows
  goes back to its workspace, place and size, unless you moved it to another screen yourself. A
  note on the screen and a tray balloon say what moved. **Settings** lists the screens des-k is
  waiting for, with **Forget** for one you will not plug in again. des-k knows a screen by the
  monitor's own model and serial, so another port or dock is still the same screen.
- The tray icon menu has **Shortcut hints**, **Settings**, **Open log**, **Check for updates**
  and **Quit**.
- **Check for updates** asks GitHub which des-k release is newest and answers in a balloon. If a
  newer one is out, click the balloon (or **Install X.Y.Z** in the menu): des-k downloads that
  release's setup, checks it against the release's `SHA256SUMS.txt`, and runs it silently. des-k
  stops, updates and comes back with your layout, then says "des-k updated". If anything fails,
  a balloon says why and des-k carries on. Only the installed des-k does this; a copy you run
  yourself opens the release page instead. des-k checks and installs only when you click.
- des-k keeps the title, app, class, place and size of each window it tracks, and the model and
  serial of each screen, in `state.txt`, all in `%LOCALAPPDATA%\des-k` on this PC only. The log
  names windows by their app (`chrome.exe`), never by their title, so it is safe to share. None
  of it leaves your PC.
  The only thing des-k sends out is the update check you click: a request to github.com that
  carries your address and des-k's version, nothing else.

## Stopping and uninstalling

**Quit** from the tray (or `& "$env:ProgramFiles\des-k\des-k.exe" --quit`) stops des-k and
shows every hidden window, so nothing is ever lost. It also forgets which window was on which
workspace, on purpose. If des-k runs your touchpad gestures, Quit also hands Windows its own
gestures back from your next sign in.

To remove it completely, uninstall **des-k** from **Settings → Apps → Installed apps**. The
uninstaller quits des-k first, so your windows and touchpad gestures come back.

If you installed with `install.ps1` instead, **Quit it first**, then in an **admin** terminal:

```powershell
Unregister-ScheduledTask -TaskName des-k -Confirm:$false
Remove-Item -Recurse "$env:ProgramFiles\des-k"
```

Your settings and log stay in `%LOCALAPPDATA%\des-k`; delete that folder too if you want.

## If something goes wrong

- **A window seems lost**: press `Win+F` and search for it, or open the overview.
- **des-k stopped** (no tray icon) and windows are still hidden: run
  `& "$env:ProgramFiles\des-k\des-k.exe" --quit`. Even with des-k stopped, it shows every window
  des-k hid and gives Windows its touchpad gestures back. Then start des-k from the Start menu
  (**des-k**), which starts it as admin through its logon task.
- **Everything is weird**: Quit from the tray. Every window comes back. Then start des-k again
  from the Start menu.
- The log (tray → **Open log**) says what des-k did and why. Each line has the time and a level:
  `INFO` is something done as intended, `WARN` a hiccup des-k carried on from, `ERROR` something
  you may notice (a window still hidden, shortcuts not working). The first line names the des-k
  version and Windows build. The run before lives on in `des-k.old.log` beside it, so after a
  crash it still says what happened. When the log reaches 5 MB, it starts fresh and the old part
  moves to `des-k.old.log`.
- **Windows Defender says des-k is a trojan** (for example `Trojan:Win32/Bearfoos.A!ml`): the
  `!ml` means a guess by Defender's machine learning, which often flags new unsigned programs.
  des-k is open source and every release lists its SHA-256 in `SHA256SUMS.txt`. Update
  Defender's definitions, then restore des-k from Windows Security, Protection history (or
  install it again).

des-k uses an undocumented part of Windows to hide windows. It works on Windows 11 25H2, but a
big Windows update could change that. If it does, the tray shows a warning and switching hides
nothing until des-k is updated. Tray → **Check for updates** tells you when a fix is out.

## License

Copyright (C) 2026 CanaanGM

des-k is free software: you can redistribute it and/or modify it under the terms of the GNU
General Public License as published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version. It comes with no warranty. See
[LICENSE](LICENSE) for the full text.

The third party agent skills in `.claude/skills/` keep their own authors' licenses.
