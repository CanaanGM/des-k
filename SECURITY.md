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
  downloads only the release's `SHA256SUMS.txt` and setup, from URLs built from version numbers,
  into the admin only `Program Files\des-k\update\`, and runs the setup only if its SHA-256
  matches.
- Nothing in `%LOCALAPPDATA%\des-k` (settings, layout, log) can name a program to run or code to
  load, since anything running as you can write there. des-k never writes through a link there.
- des-k only shows again the windows it hid itself.

A report that breaks one of these is exactly what we want to hear about.

## Known and accepted

A touchpad gesture's key combo in `settings.toml` is typed by the admin des-k, so it reaches
admin windows, and any program running as you can edit that file. Such a program can already
type into your own windows. The **Install for me only** copy runs
without admin on purpose, since its exe sits in a folder you can write to.
