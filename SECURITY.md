# Security

des-k runs as admin, hears every key through a keyboard hook, and can download and run its own
setup. A flaw in any of that matters, so please report it privately.

## Reporting a problem

Use **Report a vulnerability** on the repo's
[Security tab](https://github.com/CanaanGM/des-k/security/advisories/new). Please don't open a
public issue for it. Say what an attacker needs (an account on the PC, a program running as you,
a network position), what they get, and the steps to see it. des-k's log is safe to attach: it
names windows by their app, never their title.

des-k is one person's project, so there is no fixed reply time, but a security report comes
before anything else.

## Supported versions

Only the newest release gets fixes. Update from the tray (**Check for updates**) or from
[Releases](https://github.com/CanaanGM/des-k/releases/latest).

## What des-k promises

- Network only when you click **Check for updates** or **Install X.Y.Z** in the tray. It
  downloads only the release's `SHA256SUMS.txt`, its signature and the setup, from URLs built
  from version numbers, into the admin only `Program Files\des-k\update\`, and runs the setup
  only if the publisher's key signed `SHA256SUMS.txt` for that version and the setup's SHA-256
  matches its line ([Check a download](README.md#check-a-download) shows how to check one yourself).
- Nothing in `%LOCALAPPDATA%\des-k` (settings, layout, log) can name a program to run or code to
  load, since anything running as you can write there. des-k never writes through a link there.
- des-k only shows again the windows it hid itself.

A report that breaks one of these is exactly what we want to hear about.

## What runs as admin

- **des-k itself**, from `Program Files\des-k\des-k.exe`. The scheduled task `des-k` starts it
  at sign in with your highest rights, so its shortcuts and window moves still work while an
  admin window (Task Manager, an admin terminal) has focus. Only admins can change that folder.
- **The setup**, when you click **Install X.Y.Z**: des-k runs it through its own one shot task
  `des-k update`, after the checks above.
- Nothing else. des-k opens files and folders for you (the settings file, the log, the
  wallpapers folder) through Explorer, so the editor runs as you, not as admin.
- The **Install for me only** copy is not admin at all: its exe sits in a folder you can write
  to, so its task runs with your normal rights.

## What the admin des-k reads from your data folder

Everything in `%LOCALAPPDATA%\des-k` can be changed by any program running as you, so des-k
treats each file as input to check, never as instructions:

| File | What des-k takes from it |
|---|---|
| `settings.toml` | Numbers, true or false, colors, key combos and action names, names and the attention text (only ever drawn on screen), app rules, and wallpaper picks. |
| `state.txt` | Which windows des-k tracks, by window handle, process id, program file name (never its folder), window class, place and screen, plus the title only while Restore is on, to recognize them after a restart or a reboot. A window counts only if it still has the same process and is still hidden. The README says how to clear it. |
| `touchpad.txt` | Your own 13 Windows touchpad gesture numbers (each 0 to 65535), written back to your own registry key when des-k quits. Any other name or number is skipped. |
| `wallpapers\` | Pictures to draw behind your desktop icons, read with Windows' own image decoder (GDI+). |

des-k writes `des-k.log` there too, but never reads it back.

Who can read the folder: des-k sets no access rules of its own, so the folder takes whatever your
profile folder passes down. On a normal profile that is you, SYSTEM and Administrators. If you have
given another local account read access to your `AppData` (a sandbox account for an AI coding
tool, for example), it can read `state.txt`, the log and your settings too, though not change them.
`icacls "%LOCALAPPDATA%\des-k"` shows who can.

No setting can name a program or a path:

- An **app rule** names an exe file name (`slack.exe`) or a Store app id, and des-k only
  compares it with running windows. It never opens or runs it. A key with `\`, `/` or `:` in
  it is refused.
- A **wallpaper pick** is a picture or a subfolder inside `wallpapers\`. Anything with `\`,
  `/`, `:` or `..` in it is refused, so a pick can never reach outside that folder.
- A **gesture** runs one of des-k's own actions or types a key combo. Anything else is refused.

A refused value is logged by its key and skipped, and the rest of the file still loads. A unit
test in `src/settings.rs` (`no_setting_names_a_program_or_a_path`) gives every one of these a
program and a path, and fails if any is kept, or if any warning in the log repeats the path.

## Known and accepted

A touchpad gesture's key combo in `settings.toml` is typed by the admin des-k, so it reaches
admin windows, and any program running as you can edit that file. Such a program can already
type into your own windows. The **Install for me only** copy runs
without admin on purpose, since its exe sits in a folder you can write to.

The pictures in `wallpapers\` are decoded inside the admin des-k, and any program running as
you can put a picture there. A picture crafted to break Windows' image decoder would then run
as admin. des-k leaves the decoding to Windows, which patches that decoder through Windows
Update.
