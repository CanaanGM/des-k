# 0018. Install updates from the tray

**Date**: 2026-11-07
**Status**: In Progress
**Amends**: [0015](../0015-check-for-updates/index.md): replaces its AC-7 ("downloads no file, runs no program") for the installed admin copy only. Every other 0015 criterion stands, AC-5 (network only when you click) included.

## Summary

When Check for updates finds a newer des-k, one click on the balloon (or on the menu's **Install X.Y.Z**) now downloads that release's setup, checks it against the release's checksum file, and runs it silently. The setup stops des-k, installs, and starts it again, and des-k then says "des-k updated". This replaces opening a web page, downloading and running the setup by hand. Only the installed copy running as admin does this; any other copy keeps opening the page. The setup runs through its own one shot scheduled task (a task des-k registers just for this run), because a process des-k starts directly would be killed together with des-k (tried 2026-11-07).

## Requirements

**User stories**:
- As the des-k user, I want one click to update des-k, so that I do not have to open a page, download and run a setup.
- As the des-k user, I want des-k to run only the setup from my own release, unchanged, so that the update cannot be used to run something else as admin.

**Acceptance criteria**:
- **AC-1**: When a check finds a newer version and this des-k can install (AC-2), the balloon titled "des-k update" says "des-k X.Y.Z is out (you have A.B.C). Click here to install it." and the menu's update item reads **Install X.Y.Z** with the line "Downloads and installs it". The menu keeps that item after the balloon is gone. Clicking either starts the install.
- **AC-2**: des-k can install only when it runs as admin and its own exe is `<Program Files>\des-k\des-k.exe`. Any other copy (cargo run, an admin `target\debug` run) keeps spec 0015's behavior: the balloon and the item open the release page.
- **AC-3**: While the download runs, the update item reads "Downloading des-k X.Y.Z…" and is grayed. des-k keeps switching, following and drawing, and a second check or install cannot start (it is ignored with a log line).
- **AC-4**: des-k downloads only `SHA256SUMS.txt` and `des-k-vX.Y.Z-setup.exe` from `https://github.com/CanaanGM/des-k/releases/download/vX.Y.Z/`, URLs it builds from the three numbers. The setup is saved under `<Program Files>\des-k\update\`, and runs only if its SHA-256 equals the one line in `SHA256SUMS.txt` that names exactly that file.
- **AC-5**: When anything fails before the setup runs (no connection, a step taking longer than about 10 s, a certificate error, a proxy login, an HTTP error, a missing setup or checksum file, no matching checksum line, a file over the size limit, a checksum mismatch, the update folder unusable, the update task not registered or not started), a warning balloon titled "Could not install des-k X.Y.Z" says why in plain words and ends "Click here to open its page."; clicking it opens the release page. Every file this install wrote (partial, rejected or verified setup, task XML) and the task are deleted, nothing runs, des-k carries on, the menu offers **Install X.Y.Z** again, and the log has the detail.
- **AC-6**: After a match, des-k registers and runs the one shot task `des-k update`, which runs the setup as you with admin rights, with no prompt and no window (`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`, log in `<Program Files>\des-k\update\setup.log`). The update item reads "Installing des-k X.Y.Z…", grayed. The setup stops des-k the way it does today, installs, and starts it again. Hidden windows come back into the layout the next des-k adopts from `state.txt`. The task's second action starts des-k again after the setup on every path, even a setup that was killed, and does nothing when des-k already runs. If the setup has not started 60 s after the task run, that is the `Task` failure of AC-5.
- **AC-7**: When the new des-k starts and finds the setup it ran for its own version, it shows "des-k updated" / "You now have des-k X.Y.Z." (info), once: the next start does not show it again.
- **AC-8**: When the setup fails after stopping des-k, the setup still starts des-k again (`des-k.iss`), and that des-k, finding a setup newer than itself, shows the warning "des-k was not updated" / "The update to X.Y.Z did not install. Click here to open the setup's log."; clicking opens `setup.log` through explorer, or the release page when there is no `setup.log`. When the setup ends without stopping des-k, the running des-k shows the same warning. When the setup still runs 10 min after it started and des-k is still alive, des-k shows "des-k was not updated" / "The setup is taking unusually long. des-k carries on." and frees the item. Whether des-k was updated is decided by comparing versions only, never the setup's exit code.
- **AC-9**: While the "hiding windows does not work" warning shows, no update or install balloon replaces it: answers go to the log and the menu item, as in spec 0015 AC-9. Accepted: at start the broken check runs on main's 1 s timer, after the outcome balloon, so the broken warning may replace that balloon a second later.
- **AC-10**: After the setup ends, on success or failure, des-k deletes the setup exe, the task XML and the `des-k update` task. Before each install it clears whatever an earlier run left (files, and a still running or leftover task). `setup.log` stays until the next install replaces it. The uninstaller removes the update folder and the task.
- **AC-11**: Every install writes log lines: started (version), each download (status, size), the checksum result, the task registered and started, the outcome at the next start, and every failure with its detail.

## Decision

**Chosen option**: Option 1: Download, verify, and run the setup through a one shot task.

The installed admin des-k downloads the release's checksum file and setup over HTTPS into an admin only folder, checks the SHA-256, and runs the setup silently through its own scheduled task. The setup does the rest exactly as when you run it by hand.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`): WinHTTP downloads, BCrypt hashing, known folder paths, the task XML and process creation. `memory-safety-patterns` (`wshobson/agents`, `.claude/skills/memory-safety-patterns/`): RAII guards for WinHTTP, BCrypt and file handles on every exit path.

## Rationale

Why this option, and the ones not chosen: [rationale.md](rationale.md).

## Feature design

**Data model sketch**:
No setting and no file in `%LOCALAPPDATA%`. The only state on disk is the admin only update folder.

| Value | Type | Where | Meaning |
|---|---|---|---|
| `<Program Files>\des-k\update\` | folder | disk | Created by des-k (inherits Program Files' admin only rights). Holds at most: `des-k-vX.Y.Z-setup.exe.part` (while downloading), `des-k-vX.Y.Z-setup.exe` (verified), `des-k-vX.Y.Z-setup.exe.done` (renamed once its outcome balloon showed, so it shows once; renaming works while the file is still mapped), `des-k-update.xml` (the task definition), `setup.log` (the setup's). |
| `des-k update` | scheduled task | Task Scheduler | One shot, no trigger, two actions in order: the verified setup, then `schtasks /run /tn des-k`. Registered per install, deleted after the setup ends (AC-10). |
| `RUNNING` | `static AtomicBool` in `src/update.rs` | both threads | Now shared: a check **or** an install is in flight (AC-3). Same guard rules as spec 0015, except an install keeps it set through the wait for the setup: `Started` does not clear it, the final post does (`Failed(..)`, or none at all when the setup stops des-k). |
| `Phase` | `static AtomicU64` in `src/install.rs` | both threads | Kind (0 `Idle`, 1 `Downloading`, 2 `Installing`) in the top 16 bits, the version's three parts in the low 48, so kind and version change together. The menu reads it. |
| `InstallFail` | enum | `src/install.rs` | `Net(Fail)` (spec 0015's transport failures, reused), `Missing` (HTTP 404 for the setup or the checksum file), `NoLine` (no well formed line for the setup's name), `TooBig`, `Mismatch`, `Folder` (cannot create, clear or write the update folder, or it is a link), `Task` (could not register or run the task, or the setup did not start within 60 s), `SetupStopped` (the setup ended and des-k is still running), `Slow` (the setup still runs after 10 min and des-k is still running), `Crashed`. |
| `Outcome` | enum | `src/install.rs` | At start: `Updated(Version)`, `NotUpdated(Version)`, `Nothing`. |
| tray `BALLOON` | `RefCell<Option<Click>>` in `src/tray.rs` | main thread | Replaces `BALLOON_URL`: `Click::Open(String)` (a page or `setup.log`) or `Click::Install(Version)`. |
| tray `ANSWER` | existing | main thread | Now also kept for `Newer` when a balloon was shown, so the menu offers **Install X.Y.Z** later (AC-1). |

**State transitions**:
idle → (click a newer balloon or **Install X.Y.Z**) → downloading → (checksum matches) → installing (task started) → des-k stopped by the setup → new des-k starts → outcome balloon → cleanup.
downloading → (any failure) → idle with the "Could not install" balloon (AC-5).
installing → (setup ends, des-k still alive) → idle with "des-k was not updated" (AC-8).

**API surface** (functions, messages and files; no HTTP server):
| Surface | Kind | Key inputs | Key outputs | Who | Key errors |
|---|---|---|---|---|---|
| `install::can_install() -> bool` | fn | none | elevated and own exe is `<Program Files>\des-k\des-k.exe` | tray (balloon text, menu, click) | any error reading either: `false` with a log line |
| `install::start(tray, v)` | fn, main thread | tray hwnd, version from `ANSWER` | sets `RUNNING`, `Phase::Downloading(v)`, spawns the worker | balloon click, menu item | already running: log line only (AC-3) |
| install worker | thread | hwnd as `isize`, `v` | first clears leftovers (deletes old `.part`, exe, `.done` and XML in `update\`, ends and deletes an old `des-k update`; any of it failing → `Folder`); posts `Started` when the setup is running, then at most one final `Failed(InstallFail)` | `start` | a guard like 0015's: on every failed exit deletes this install's files and the task, clears `RUNNING`, resets `Phase`, posts `Failed(Crashed)` if no final post was made, no `unwrap` |
| `download(path, cap) -> Result<Vec<u8> or file, InstallFail>` | fn, worker | URL path built from `v` | bytes (checksum file) or a file streamed to `.part` while hashing | worker | 404 → `Missing`; over `cap` → `TooBig`; other errors via 0015's `fail` |
| `sums_line(text, name) -> Result<[u8; 32], InstallFail>` | pure fn | checksum text, exact file name | the expected hash | worker | `NoLine` |
| `sha256` | BCrypt, incremental | bytes as they arrive | 32 bytes | `download` | BCrypt error → `Crashed` with the code in the log |
| `task_xml(user, setup, log) -> String` | pure fn | account, paths | plain ASCII task XML | worker | none |
| run the task | worker | the XML file | `schtasks /create /tn "des-k update" /xml <file> /f`, then `/run /tn "des-k update"` | worker | nonzero exit → `Task` |
| wait for the setup | worker, after `schtasks /run` | the setup exe path | every 500 ms, open the exe with no sharing (`CreateFileW`, share mode 0, then close): a sharing violation means the setup runs. First violation → post `Started`. Not seen within 60 s → `Failed(Task)`. After `Started`, the open succeeding again while des-k still runs → `Failed(SetupStopped)`; still locked after 10 min → `Failed(Slow)` | worker | never deletes to probe, so a setup that has not opened its exe yet is never mistaken for one that ended |
| `WM_INSTALL` = `WM_APP + 15` | window message to the tray window | kind and fail code in `wParam`, version in `lParam` (as `update::pack`) | balloon, menu label | worker | none |
| `install::outcome() -> Outcome` | fn, main thread at start, after `tray::init` | the update folder's file names (`.exe` only, never `.done`) | balloon (AC-7, AC-8) or nothing; then renames that exe to `.done` | `main` | folder unreadable: `Nothing` with a log line; rename failing: log line |
| `install::cleanup()` | short thread at start when the folder holds a `.part`, exe, `.done` or XML, or the `des-k update` task exists | none | deletes `.part` at once; every 1 s up to 2 min tries to delete the exe or `.done` (fails while the setup still runs), then deletes the XML and the task | `main` | gives up with a log line; the next start tries again |
| `des-k.iss` `DeinitializeSetup` | Inno code | `Stopped` flag set in `PrepareToInstall` | `schtasks /run /tn des-k` when des-k was stopped and is not running | Setup | none (AC-8) |
| `des-k.iss` uninstall | Inno | none | removes `{app}\update`, deletes task `des-k update` | Uninstall | none (AC-10) |

**Value sourcing**:
| Action | Value produced / displayed | Source |
|---|---|---|
| can_install | Program Files folder | `SHGetKnownFolderPath(FOLDERID_ProgramFiles)`, never the `ProgramFiles` environment variable (anything running as you can set the environment des-k inherits) |
| can_install | own exe, elevated | `std::env::current_exe()` compared case insensitively with `<Program Files>\des-k\des-k.exe`; `main::elevated()` |
| download | URLs | `/CanaanGM/des-k/releases/download/v{a}.{b}.{c}/SHA256SUMS.txt` and `/des-k-v{a}.{b}.{c}-setup.exe` on `github.com`, from the version in `ANSWER`, never from response text |
| download | redirects | followed (GitHub sends the file from `release-assets.githubusercontent.com`); WinHTTP's default policy, which refuses HTTPS to HTTP. TLS checks never turned off |
| download | proxy, user agent, timeouts | as spec 0015's `fetch`: automatic proxy, `des-k/<version>`, 10 s for each of resolve, connect, send and each receive |
| download | size limits | checksum file 64 KiB, setup 64 MiB; a larger `Content-Length` fails at once, and reading stops past the limit (`TooBig`) |
| verify | expected hash | the line of `SHA256SUMS.txt` that is exactly 64 hex digits (either case), two spaces, then exactly `des-k-vX.Y.Z-setup.exe`, line ends `\n` or `\r\n`, an optional UTF-8 BOM ignored; none or more than one → `NoLine` (format from `build.yml`) |
| verify | actual hash | BCrypt SHA-256 computed while the `.part` file is written; on a match the `.part` is renamed to the final name, otherwise deleted |
| task | account | des-k's own token user as a SID string (`GetTokenInformation(TokenUser)`, `ConvertSidToStringSidW`), so no account name is pasted into XML |
| task | definition | written to `update\des-k-update.xml`, plain ASCII, no `<?xml?>` line (AGENTS.md gotcha). `<Principal>`: `UserId` the SID, `LogonType` `InteractiveToken`, `RunLevel` `HighestAvailable`. No triggers. `<Settings>` written out in full: `DisallowStartIfOnBatteries` false, `StopIfGoingOnBatteries` false, `ExecutionTimeLimit` `PT0S` (a limit would also stop the second action, leaving des-k down), `MultipleInstancesPolicy` `IgnoreNew`, `Priority` 4. `<Actions>`: first `Exec` command the verified setup path, arguments `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /LOG="<update>\setup.log"`; second `Exec` the full `schtasks.exe` path, arguments `/run /tn des-k` |
| task | `schtasks.exe` path | `GetSystemDirectoryW` + `\schtasks.exe`, never `%SystemRoot%` from the environment |
| outcome | setup version | the one file in the update folder named `des-k-v{X.Y.Z}-setup.exe` (`parse_version` on the middle; the worker's clearing leaves at most one) |
| outcome | which balloon | setup version equal to ours → AC-7; newer → AC-8; older → nothing; `BROKEN` set → log only (AC-9) |
| balloons | text | AC-1, AC-5, AC-7, AC-8 above; the AC-5 reason words: "No connection to GitHub.", "GitHub did not answer in time.", "The secure connection to GitHub failed.", "Your proxy asks for a login.", "GitHub answered with HTTP n.", "This release has no setup or checksum file.", "The checksum file does not list the setup.", "The setup is larger than expected.", "The download does not match its checksum.", "The update folder could not be used.", "Windows did not start the setup.", "The install stopped with an error (the log has it)."; `SetupStopped` and `Slow` use the AC-8 wording |
| balloons | AC-8 click | `setup.log` if it exists, else the release page |
| menu | update item | `Check` gains `Install(Version)` ("Install X.Y.Z", line "Downloads and installs it"), `Downloading(Version)` and `Installing(Version)` (grayed), built in `tray::menu` from `Phase` first, then `ANSWER` and `can_install`. After any `Failed(..)` `ANSWER` stays `Newer`, so the item offers Install again |

**Key invariants**:
- Nothing installs without your click on a balloon or menu item that a check you started produced. No timer, no start check (spec 0015 AC-5 stands).
- Only a file whose SHA-256 matches its release's checksum line ever runs, and only from the admin only update folder. A link at `update\` is refused (`Folder`), never followed.
- Every path des-k itself runs or writes (Program Files, `schtasks.exe`, the update folder, the setup) comes from Windows APIs or constants, never from the environment, `%LOCALAPPDATA%`, a setting, or response text. This covers des-k's own paths only: the setup inherits your environment, as a manual install does, and its bare `taskkill.exe`, `schtasks.exe` and `whoami.exe` still resolve to System32 before `PATH`.
- The setup is never a child of des-k: it runs from the `des-k update` task, outside the des-k task's job.
- The worker touches no `ws` state, no window and no tray icon; it only logs, writes the update folder, runs `schtasks`, and posts `WM_INSTALL`.
- `WM_APP + 15` is new and unique (the main loop matches ids alone; `WM_APP + 14` is already `keys::WM_HINTS`).
- Before the setup runs, des-k is untouched. After it, des-k is started again by the setup (`CurStepChanged` on success, `DeinitializeSetup` on failure) and then by the task's second action, which also covers a setup that was killed. Not covered, and accepted: a setup that hangs forever holds the task, so des-k stays down until you start it from the Start menu.

**Security model**:
One user, admin process. Stopped: a network attacker (TLS plus the checksum), a program running as you swapping the file (the folder is admin only), and environment tricks (no path from the environment). A program running as you can post `WM_TRAY` (let in for the tray icon), so it could fake a balloon click: that only starts the install of the verified release a check you started already found, the same as your click. Accepted: someone who controls the GitHub account can publish a matching setup and checksum. That is no worse than installing by hand today, and the fix is signing (Follow-up). Inno Setup extracts into its own temp folder, the same as a manual install.

**Configuration required**:
- Cargo: add `Win32_Security_Cryptography` (BCrypt), `Win32_Security_Authorization` (`ConvertSidToStringSidW`), and `Win32_UI_Shell` and `Win32_Storage_FileSystem` if not on yet (`SHGetKnownFolderPath`, `CreateFileW`). No settings, no environment variables, no credentials.

**Critical test scenarios**:
- Happy path: set `Cargo.toml` to `0.0.1` (never committed), build release, `iscc /DAppVersion=0.0.1 des-k.iss`, install it, check for updates, click the balloon: the item shows Downloading then Installing, des-k goes and comes back as the newest release, layout kept. Verifies **AC-1**, **AC-3**, **AC-4**, **AC-6** (and **AC-7** once the newest release includes this feature)
- Not the installed copy: `cargo run` with version `0.0.1`, check: the balloon opens the page. Verifies **AC-2**
- Download failure: Wi-Fi off right after clicking the balloon: "Could not install des-k X.Y.Z" with "No connection to GitHub.", clicking opens the page, nothing ran, `update\` holds no `.part`. A mismatch is covered by the unit test on the verify step below. Verifies **AC-5**
- Setup failure: make the install fail (lock `des-k.exe` open with another admin process during the install): des-k comes back as the old version and shows "des-k was not updated"; clicking opens `setup.log`. The version comparison is the source of truth here, not the setup's exit code. Verifies **AC-8**
- Killed setup: end the setup process from an admin Task Manager after des-k stopped: the task's second action starts des-k again, and it shows "des-k was not updated". Verifies **AC-6**, **AC-8**
- Cleanup: after an update, `update\` holds only `setup.log` and `schtasks /query /tn "des-k update"` finds nothing. Verifies **AC-10**
- Unit tests:
  - `sums_line` finds the exact name in a real `SHA256SUMS.txt` (three lines, CRLF), and gives `NoLine` for a missing name, a name with a suffix (`…setup.exe.bak`), 63 or 65 hex digits, one space, and two matching lines. Verifies **AC-4**, **AC-5**
  - `sha256(b"abc")` is `ba7816bf…f20015ad` (the standard test vector). Verifies **AC-4**
  - the download paths are built from the numbers only (`Version(0,4,2)` → `/CanaanGM/des-k/releases/download/v0.4.2/des-k-v0.4.2-setup.exe`). Verifies **AC-4**
  - `task_xml` is ASCII only, has no `<?xml`, quotes the log path, names the setup's full path, has exactly two `Exec` actions in order, and `ExecutionTimeLimit` `PT0S`. Verifies **AC-6**
  - the outcome rule: equal → `Updated`, newer → `NotUpdated`, older or none → `Nothing`; a `.done` file is never read as an outcome. Verifies **AC-7**, **AC-8**
  - `pack`/`unpack` round trip for every `InstallFail` and `Started`, and the `Phase` packing for each kind with 65535 parts. Verifies **AC-3**, **AC-5**, **AC-6**

## Build plan

Tracer Bullet: the first slice is the whole thread from the balloon click to the restarted des-k, then the failure paths.

1. [x] `src/install.rs` pure parts with their unit tests: paths from a version, `sums_line`, `task_xml`, the outcome rule, `InstallFail`, pack and unpack, satisfies **AC-4**, **AC-5**, **AC-6**, **AC-7**, **AC-8**
2. [x] The thread end to end: `can_install`, `start`, the worker (clear leftovers, download both files with WinHTTP and BCrypt hashing into `update\`, verify, write the XML, register and run the task, the no sharing open probe), `WM_INSTALL`, the tray's `Click::Install` balloon, the menu's `Install`/`Downloading`/`Installing` items, `ANSWER` kept for `Newer`, satisfies **AC-1**, **AC-2**, **AC-3**, **AC-4**, **AC-6**
3. [x] Start up: `outcome` with the AC-7 and AC-8 balloons (log only while broken), `cleanup` of the exe, task and XML, satisfies **AC-7**, **AC-8**, **AC-9**, **AC-10**
4. [x] Failure paths: every `InstallFail` to its balloon with the page click, `.part` deleted, size limits, the link check, the "setup ended, des-k alive" wait, log lines throughout, satisfies **AC-5**, **AC-8**, **AC-11**
5. [x] `des-k.iss`: `Stopped` plus `DeinitializeSetup` restart, `[UninstallDelete]` for `{app}\update`, delete task `des-k update` on uninstall, satisfies **AC-8**, **AC-10**
6. [x] Cargo features; AGENTS.md (the network rule now allows this install, `WM_APP + 15`, the job gotcha) via `/sync`; README and site guide (what Install does), satisfies **AC-4**, **AC-6**
7. [ ] Try it on your machine: the 0.0.1 install to the newest release, the not installed copy, Wi-Fi off, a failed setup, satisfies **AC-1** to **AC-11**

## Migration plan

**Strategy**: no data migration; a staged rollout by release.
**Phases**:
1. The release with this feature (0.4.2 or later) is installed once by hand, or from a 0.0.1 test install as above. Copies up to 0.4.1 can only open the page.
2. From then on, every later release installs from the tray.
**Rollback**: install an older setup by hand; nothing on disk needs undoing besides the update folder, which the next install or the uninstaller clears.
**Risks**: a broken install path ships in the release that adds it, and you notice only at the next release; the 0.0.1 test before tagging covers it.

## Consequences

**Positive**:
- Updating is two clicks (check, then the balloon), no browser, no Downloads folder, no admin prompt.
- A broken Windows update fix reaches you as fast as you can click.

**Negative / tradeoffs**:
- An admin process now downloads and runs a program. The checksum shares GitHub as its root of trust, so a taken over GitHub account could ship you a bad setup without a prompt.
- More moving parts at the riskiest moment: a second scheduled task, a folder in Program Files, the setup restarting des-k after a failure.
- The first full test of AC-7 needs a later release than the one that adds the feature.

**Neutral**:
- SmartScreen never shows, since nothing is downloaded through the browser.
- The setup's existing hard stop is reused, so the layout survives an update exactly as it does a manual install.

## Follow-up

- [ ] If you later want the GitHub account out of the trust chain, sign releases (your own offline key over `SHA256SUMS.txt`, or Authenticode) and have des-k check the signature before running.
- [x] `/sync` after the build: AGENTS.md's network rule (spec 0015) now allows this install for the installed admin copy, the new message id `WM_APP + 15`, the missing `WM_APP + 14` (`WM_HINTS`, spec 0017) in its id list, and the gotcha that a task's processes run in a job that refuses breakaway.
- [x] Spec 0015's update item text ("Update to X.Y.Z", "Opens its page on github.com") stays for copies that cannot install; spec 0017's menu tests gain the new `Check` states.
