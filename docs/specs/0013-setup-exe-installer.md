# 0013. Setup exe installer: install and uninstall des-k with a double click, built with Inno Setup

**Date**: 2026-11-02
**Status**: In Progress

## Summary

des-k now ships as `des-k-vX.Y.Z-setup.exe`, a normal Windows installer you double click, instead of a zip you unzip and install by typing a PowerShell command. It does exactly what `install.ps1` does (spec 0008: Program Files, the logon task that starts des-k as admin, keep your layout), and it adds an uninstaller in Settings, Apps that gives your hidden windows back first. It is built with Inno Setup (a free tool that turns one script file into an installer) in the existing release job. The exe is not signed yet, so Windows shows a SmartScreen warning until that is decided separately.

## Context

Spec 0008 made des-k run as admin from a scheduled task (a Windows job that starts a program at sign in) with the exe in `C:\Program Files\des-k`. Installing it meant downloading a zip, opening a terminal in it, and typing `powershell -ExecutionPolicy Bypass -File install.ps1`. That is fine for you, building from source, but not for anyone else: it asks people to run a script with the safety switch turned off, and there was no uninstaller, only two commands to type in an admin terminal.

Forces that shape the answer:
- The install must keep every rule spec 0008 set: one UAC prompt (the "allow this app to make changes" dialog), a hard stop of the running des-k so `state.txt` survives and the layout is adopted, the exact task settings (battery, no time limit, normal priority), the task belongs to the account that started the install (never one typed into an over the shoulder admin prompt), and a loud error if des-k is not running at the end.
- Uninstalling must not strand windows: des-k hides windows, and only its Quit path shows them all again and hands back the touchpad gestures (spec 0011).
- One person's tool: no team to run build infrastructure, no budget for paid tooling.
- Releases already build on a GitHub `windows-latest` runner from a pushed tag.

## Requirements

**User stories**:
- As someone installing des-k, I want to double click one file and say Yes once, so that I never have to open a terminal or run a script with its safety switch off.
- As the des-k user, I want to uninstall des-k from Settings, Apps like any other program, so that removing it is safe and leaves no window hidden.
- As the des-k user, I want a newer setup to update the installed des-k and keep my layout, so that updating costs me nothing.

**Acceptance criteria**:
- **AC-1**: Double clicking `des-k-vX.Y.Z-setup.exe` from an admin account shows exactly one UAC prompt and no terminal window. Afterwards des-k runs as admin from `C:\Program Files\des-k\des-k.exe`, and the `des-k` task has the settings spec 0008 AC-3 requires (start on battery, keep running on battery, no time limit, priority 4, ignore a second start, run level Highest, logon trigger for that account).
- **AC-2**: Running the setup while des-k runs with windows hidden on several workspaces keeps the layout: the running copy is hard stopped (never quit gently, never closed by Restart Manager), and the new copy adopts `state.txt`. This holds when updating an older setup install and when installing over an `install.ps1` install.
- **AC-3**: If the admin prompt is answered by a different account, setup stops before changing anything and its message names the account that answered.
- **AC-4**: If the running des-k cannot be stopped within 5 s, setup stops before changing anything and says so. If the task cannot be registered, or des-k is not running 5 s after setup starts it, setup shows an error saying hidden windows stay hidden until des-k runs and to run the setup again.
- **AC-5**: After install, the old `%LOCALAPPDATA%\des-k\des-k.exe` and the `des-k` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` are gone (spec 0008 AC-5).
- **AC-6**: des-k appears in Settings, Apps. Uninstalling it runs `des-k --quit` first, so every hidden window shows and the touchpad gestures go back to Windows, then deletes the `des-k` task and `C:\Program Files\des-k`. `%LOCALAPPDATA%\des-k` (settings, log, layout) stays.
- **AC-7**: If des-k has not quit 10 s after `--quit`, uninstall stops, removes nothing, and tells you to quit it from the tray and try again.
- **AC-8**: A pushed `vX.Y.Z` tag publishes `des-k-vX.Y.Z-setup.exe` next to the zip, with its SHA256 in `SHA256SUMS.txt`. If the installer fails to build, the release job fails and nothing is published.
- **AC-9**: The README's install steps use the setup exe and explain how to get past the SmartScreen warning ("More info", then "Run anyway"); its uninstall steps point to Settings, Apps, with the manual commands kept for `install.ps1` installs.

## Options considered

### Option 1: Keep the zip and `install.ps1`

Fix in place: keep the script and only improve the instructions.

**Pros**:
- Already built and tried; no new tool.
- A script run from a terminal mostly avoids SmartScreen, since `install.ps1` unblocks the exe.

**Cons**:
- People still type a PowerShell command with `-ExecutionPolicy Bypass`, which is the thing to get rid of.
- No uninstaller and no entry in Settings, Apps.

### Option 2: Inno Setup installer

One `.iss` script compiled by Inno Setup into a single setup exe, with a generated uninstaller and an Apps entry. Its `[Code]` section (Pascal Script) holds the stop, task, and check logic.

**Pros**:
- Free, mature, one text file; builds on the existing Windows runner.
- Uninstaller, Apps entry, and update in place come for free.
- `ExecAsOriginalUser` can check the account that started setup, so the spec 0008 account rule survives elevation.

**Cons**:
- A new build tool, and a second copy of the install logic next to `install.ps1`.
- An unsigned exe triggers SmartScreen on download.
- Pascal Script is a niche language.

### Option 3: WiX (MSI package)

A Windows Installer package, the format companies deploy through tools like Intune.

**Pros**:
- The standard for managed deployment; transactional install and rollback.

**Cons**:
- Much heavier to write; a logon task with these settings needs custom actions (code the MSI runs).
- No managed deployment need exists for a one person tool.

### Option 4: MSIX package

The modern Store style package.

**Pros**:
- Clean install and uninstall, automatic updates through the Store.

**Cons**:
- Its startup tasks cannot run as admin, which undoes spec 0008. Ruled out.

Also ruled out briefly: NSIS (same shape as Inno Setup, harder scripting) and des-k installing itself (`des-k.exe --install`), which would mean writing the Apps entry and the uninstaller by hand in Rust.

## Decision

**Chosen option**: Option 2: Inno Setup installer.

des-k ships as an Inno Setup setup exe that repeats `install.ps1`'s admin steps and adds an uninstaller; the zip and `install.ps1` stay for building from source and for people who prefer a script.

## Rationale

The goal was a normal double click install with an uninstaller, without PowerShell in front of the user. Inno Setup gives exactly that from one script, costs nothing, and builds on the `windows-latest` runner the release already uses, which fits a one person tool with no build infrastructure. MSIX would break the admin requirement from spec 0008, and WiX buys managed deployment features nobody needs at the cost of custom actions for the task. Keeping `install.ps1` costs a second copy of the logic, but the source build path (build as you, then elevate) still needs it, and the shared task definition is small enough to keep in step by hand (see Key invariants).

## Feature design

**Installer flow** (`des-k.iss`):
1. `InitializeSetup` (already elevated): read the account with `whoami`, then run `whoami | findstr /i /x` as the original user through `ExecAsOriginalUser`. A mismatch means another account answered the prompt: stop, nothing changed (AC-3).
2. `PrepareToInstall`: `taskkill /f /im des-k.exe`, then `schtasks /end /tn des-k` (kill first, since ending the task might close des-k gently). Wait up to 5 s for it to be gone, else stop with a message (AC-2, AC-4).
3. Files: copy `target\release\des-k.exe` to `{autopf}\des-k`; delete `{localappdata}\des-k\des-k.exe` and the HKCU Run value (AC-5).
4. `ssPostInstall`: write the task XML to `{tmp}`, `schtasks /create /xml /f`, `schtasks /run`, then wait up to 5 s for des-k to run, else show the error (AC-1, AC-4).
5. `InitializeUninstall`: if des-k runs, `des-k --quit` and wait up to 10 s; still running, stop and remove nothing (AC-6, AC-7).
6. `usUninstall`: `schtasks /delete /tn des-k /f`; Inno removes the files and the Apps entry (AC-6).

**Setup settings that matter**: `PrivilegesRequired=admin`; `CloseApplications=no` (Restart Manager would close des-k gently, showing every hidden window); `DisableDirPage=yes` (the README, AGENTS.md and the task expect `C:\Program Files\des-k`); x64 only; output `target\setup\des-k-v<version>-setup.exe`.

**Release job** (`.github/workflows/build.yml`): after the zip, `choco install innosetup`, then `ISCC /DAppVersion=<tag without v> des-k.iss`; fail on a nonzero exit; copy the setup exe into `dist`; hash every file in `dist` into `SHA256SUMS.txt` (AC-8).

**Value sourcing**:
| Action | Value | Source |
|---|---|---|
| Register task | Account the task runs as | `whoami` in the elevated setup, confirmed equal by the original user's own `whoami` (token based, no file passes the name) |
| Register task | Task settings | Copied from `install.ps1` and the current registered task; spec 0008 *Scheduled task definition* |
| Register task | Exe path | `{app}\des-k.exe`, where `{app}` is `{autopf}\des-k` (64 bit Program Files) |
| Build installer | Version | `/DAppVersion` on the `ISCC` command line: the tag without `v` in CI (the tag already must match `Cargo.toml`), typed by hand locally. Missing version fails the compile (`#error`) |
| Build installer | Exe to package | `target\release\des-k.exe` from `cargo build --release` |
| Install, uninstall | Is des-k running | `tasklist /fi "imagename eq des-k.exe"` piped to `findstr`, exit 0 when found |
| Update in place | Which install to replace | Fixed `AppId` `{246A26B4-9BC0-4230-95AC-DE1E0852CB1A}` |

**Key invariants**:
- The `AppId` never changes. A new one would make a second Apps entry instead of an update.
- Install never quits des-k gently; uninstall always quits it before removing anything.
- The task XML in `des-k.iss` and the task in `install.ps1` stay identical. Change both together.
- Exactly one way starts des-k at login: the task (spec 0008).
- The task XML is plain ASCII with no `<?xml?>` line: `schtasks` rejects UTF-8 task files (tried 2026-11-02).

**Security model**: setup and uninstall run as admin, and so does what they register. They read no file a normal program could write to decide what to run: the account comes from two token reads, the exe path is fixed, and the task XML is written into setup's own temp folder. The release publishes SHA256 sums, so a downloaded setup can be checked. The exe is unsigned, so Windows cannot vouch for who made it (see Follow-up).

**Critical test scenarios**:
- Fresh install: on an account with no des-k, double click the setup; expect one prompt, the tray icon, and the task settings from AC-1 (`schtasks /query /tn des-k /xml`). Verifies **AC-1**, **AC-5**.
- Update with a layout: with windows hidden on several workspaces, run the setup again; expect the same layout afterwards. Repeat over an `install.ps1` install. Verifies **AC-2**.
- Uninstall: uninstall from Settings, Apps; expect every window back, no `des-k` task, no `C:\Program Files\des-k`, `%LOCALAPPDATA%\des-k` still there. Verifies **AC-6**.
- Failure paths, by hand where practical: an over the shoulder prompt (a standard account plus admin credentials) stops at AC-3; a des-k that ignores `--quit` stops the uninstall at AC-7. Verifies **AC-3**, **AC-7**, and the messages of **AC-4**.
- Release: push a tag; expect the setup exe and its sum in the release. Verifies **AC-8**.

## Build plan

Tracer Bullet: the whole path was built end to end on 2026-11-02; what is left is trying it on your machine and in CI.

1. Write `des-k.iss` with the flow above, compile it with no warnings, check the account check and `schtasks /xml` by hand without admin. Satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-5**, **AC-6**, **AC-7** (built).
2. Release job builds, hashes and publishes the setup exe. Satisfies **AC-8** (built, not yet run in CI).
3. README install and uninstall steps. Satisfies **AC-9** (built).
4. Try it on your screens: fresh install, update with a layout, uninstall, install again. Satisfies **AC-1**, **AC-2**, **AC-5**, **AC-6**.
5. Push the next tag and check the release has the setup exe and its sum. Satisfies **AC-8**.

## Consequences

**Positive**:
- Installing is a double click and one Yes; no terminal, no execution policy switch.
- A real uninstaller that gives hidden windows and touchpad gestures back before removing anything.
- Updates replace the installed copy and keep the layout.

**Negative / tradeoffs**:
- The unsigned exe triggers SmartScreen ("Windows protected your PC") on download, which the zip and script mostly avoided. The warning stays until signing is decided.
- The install logic now lives twice (`des-k.iss` and `install.ps1`), kept in step by hand.
- The release depends on Inno Setup, installed unpinned through choco on every release run.
- A Windows account name with letters outside ASCII, or `&`, would break the task XML.

**Neutral**:
- The zip and `install.ps1` still ship; existing `install.ps1` installs update by running the setup over them.
- Local builds need Inno Setup installed (`winget install JRSoftware.InnoSetup`).

## Follow-up

- [ ] Decide code signing to remove the SmartScreen warning (SignPath Foundation, Azure Trusted Signing, Certum open source, plus a winget manifest); its own spec.
- [ ] Pin the Inno Setup version in the release job once a release has run with it.
- [ ] Update the site guide (`site/guide-content.html`, `site/index.html`) to the setup exe and the uninstaller.
- [ ] `AGENTS.md` Commands should list the `iscc` build line (owned by `/sync`).
