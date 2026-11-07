# 0019. Log lines with a time, a level and no window titles

**Date**: 2026-11-07
**Status**: In Progress

## Summary

Every line in `des-k.log` gets the local time with its UTC offset and a level (INFO, WARN or ERROR), written by three small macros in a new `src/log.rs`. Each log starts with a line naming the des-k version, the Windows build and whether des-k runs as admin. Window titles leave the log (the app's exe name takes their place), so a friend can send you their log without leaking email subjects or file names. A repeating line collapses into one, the log rolls over at 5 MB, and a lint stops `println!` from coming back.

## Context

des-k is about to reach friends, and when something breaks for them, their log is the only evidence you get. Today it has about 270 `println!` and `eprintln!` lines across 20 files, written from the main thread, the keyboard and touchpad threads, and the update and install workers. In a release build, `main.rs` points stdout and stderr at `des-k.log`; the run before survives as `des-k.old.log`.

The lines carry an area word (`install:`, `tray:`) but no time and no level. On 2026-11-07, matching des-k's log to Task Scheduler's history meant guessing which line happened when, and a skipped window reads the same as a dead keyboard hook. Nothing says which version or Windows build wrote the log.

Some lines name windows by their title (the finder's `"{title}" is gone`, the overview's missing previews, `ws` lines). A title can be an email subject, a document name or a private chat, which is fine on your machine and not fine in a log a friend sends you.

des-k runs for weeks. A failure in the 1 s timer that logs each time would add 86,400 lines a day with no limit.

## Options considered

### Option 1: Own small macros, one migration pass, a clippy lint

A new `src/log.rs` holds `info!`, `warn!` and `error!` plus a tiny writer: the stamp, the level, the repeat collapse and the 5 MB roll. Every existing line moves over in one change, and `clippy.toml` bans `println!`/`eprintln!` so CI fails on a new one.

**Pros**: no new crate, about 150 lines you can read in one sitting; fits the plain Win32 style; one consistent log from day one; the lint enforces it automatically.
**Cons**: our own code to maintain (stamp format, roll); a large but mechanical diff touching every file.

### Option 2: The `log` crate with our own small logger

The standard Rust facade gives the same macro names; we implement its `Log` trait with the same writer.

**Pros**: the common Rust convention; any library that logs through `log` lands in our file.
**Cons**: one more dependency for no present need (no dependency des-k uses logs through it); the writer is the same work as Option 1 anyway.

### Option 3: `tracing` with its subscriber and file appender

Structured events with spans, filters and a rolling file writer.

**Pros**: rich structure, filtering and rotation ready made.
**Cons**: three crates and a concept (spans) a single process tray app never uses; its appender rotates by time, not by the size limit wanted here; more to read when something goes wrong.

### Option 4: Own macros for new code only

Same macros as Option 1, but old lines stay until someone touches them.

**Pros**: tiny first change.
**Cons**: a mixed log, half stamped, for months; the lint can't be switched on until the last old line goes; titles keep leaking meanwhile.

## Decision

**Chosen option**: Option 1: own small macros, one migration pass, a clippy lint.

des-k logs only through `info!`, `warn!` and `error!` from `src/log.rs`, every line stamped `YYYY-MM-DD HH:MM:SS.mmm +HH:MM LEVEL message`, with windows named by exe, never by title.

**Implementation skills**: `ferris-windows` (`mcapricorns/ferris-skills`, `.claude/skills/ferris-windows/`) · `rust-best-practices` (`apollographql/skills`, `.claude/skills/rust-best-practices/`)

## Rationale

The forces are a log that must answer "when, how bad, which version" for a friend's machine, a codebase that deliberately avoids dependencies it can do without, and an existing log path (`ws::create_fresh`, the std handle swap) that already works. Option 1 meets all three with the least new surface; the `log` crate (Option 2) only pays off once a dependency logs through it, and none does. `tracing` (Option 3) solves problems des-k doesn't have and still lacks the size based roll. Migrating everything at once (rather than Option 4) is what makes the lint possible, and the lint is what keeps the standard from rotting. The diff is large but mechanical: `println!` to `info!`, `eprintln!` to `warn!`, then a short hand pass for `error!` and titles.

Local time with the offset was your call over plain local time: it costs six characters per line and makes a log from a friend in another timezone unambiguous next to your own clock.

## Standard definition

**Canonical pattern**:
```rust
// any thread, any file
info!("install: setup downloaded, {n} bytes, its SHA-256 matches");
warn!("finder: can't close {}, it runs as admin", ws::who(hwnd));
error!("keys: the keyboard hook is gone and could not be installed again: {e}");
```
writes
```
2026-11-07 17:05:33.123 +03:00 INFO  install: setup downloaded, 2980575 bytes, its SHA-256 matches
2026-11-07 17:05:34.002 +03:00 WARN  finder: can't close Taskmgr.exe, it runs as admin
2026-11-07 17:05:35.410 +03:00 ERROR keys: the keyboard hook is gone and could not be installed again: ...
```

**Line format**: `YYYY-MM-DD HH:MM:SS.mmm +HH:MM` (local time, then its UTC offset), one space, the level padded to 5 (`INFO `, `WARN `, `ERROR`), one space, the message. The message keeps today's `area: what happened` wording. One line per call; a message never contains a newline (the macro replaces any with ` / `).

**Levels** (the one rule for choosing):
- `INFO`: something happened as intended (start, a switch, an install step, settings applied).
- `WARN`: something failed and des-k carried on with nothing you'd notice broken (a window skipped, a retry, a bad setting reset to its default, no network).
- `ERROR`: a promise you can see is broken until something changes: a hidden window may stay hidden, shortcuts or gestures stop working, the tray icon is missing, the layout or settings can't be saved, a panic.

**Header**: the first line of every log, and of a rolled one:
`INFO  des-k 0.5.0 started: Windows 25H2 build 26200.7019, admin yes, installed copy`
A `--quit` with nothing running appends instead: `INFO  des-k 0.5.0 --quit with nothing running: ...` (same fields). After a roll, a second line says `log rolled over at 5 MB, the earlier part is in des-k.old.log`.

**Windows, never titles**: a window in a log line is `ws::who(hwnd)`: its process's exe file name (`msedge.exe`), or `window 0x1A2B3C` when the process can't be read. A path under `%LOCALAPPDATA%\des-k` is logged by file name only (`settings.toml`), which keeps the account name out.

**Repeats**: a line with the same level and message as the line before is not written; a count goes up instead. The count is written as `(the line above repeated N more times)`, at the same level, when a different line arrives, before a roll, and on every exit path (`--quit`, the panic hook, Ctrl+C, the tray's Quit) through `log::flush()`. Only the line right before counts: two lines taking turns are both written in full.

**Size**: the writer counts bytes written. Past 5 MB (5,242,880 bytes) it flushes the repeat count, renames `des-k.log` to `des-k.old.log` (replacing it), opens a fresh `des-k.log` through `ws::create_fresh`, points stdout and stderr at it with `SetStdHandle` (so the default panic message and anything else on stderr follow), and writes the header again.

**Where lines go**: with no console (release at login), to the log file as today; with a console (`cargo run`), to stderr in the same format.

**Threads and panics**: one `Mutex` guards the writer; a macro formats the whole line before taking the lock and writes it with one call, so lines from different threads never interleave. The panic hook uses `try_lock`; if the writer is held (a panic inside the writer, or on a thread holding it), it formats the line itself and writes it straight to the std error handle, then carries on with today's rescue. The panic line is `ERROR panic: <message> at <file>:<line>`.

**Value sourcing**:
| Action | Value written | Source |
|---|---|---|
| any line | local date and time, ms | `GetLocalTime` |
| any line | UTC offset | `GetLocalTime` minus `GetSystemTime` (called right after), rounded to 15 minutes |
| any line | level | the macro called (`info!`, `warn!`, `error!`) |
| header | des-k version | `env!("CARGO_PKG_VERSION")` |
| header | Windows release and build | registry `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion`: `DisplayVersion`, `CurrentBuild`, `UBR` (`ProductName` still says Windows 10 on 11, so it is not used) |
| header | admin yes or no | `crate::elevated()` |
| header | installed copy or not | the path half of `install::can_install` (exe is `<Program Files>\des-k\des-k.exe`), split out as `install::installed()` |
| header | started or `--quit` | which of the two calls opened the log (`log_to_file_without_console(append)` today) |
| window lines | exe name | `GetWindowThreadProcessId`, then `restore::exe_path(pid)`, file name part |
| repeat line | N | the writer's count since the last written line |
| roll | 5 MB limit | constant in `src/log.rs` |

**Replaces**:
- `println!` and `eprintln!` anywhere in `src/` outside tests (about 270 lines).
- Window titles in log lines (`ws::title(..)`, `{title}` in the finder and overview, and the like): `ws::who(hwnd)` instead.
- The full data folder path in log lines.
- AGENTS.md's rule wording "Per window failures are logged with `eprintln!`" (becomes `warn!`).

**Enforcement**: `clippy.toml` at the repo root:
```toml
disallowed-macros = [
  { path = "std::println", reason = "log with info!/warn!/error! from src/log.rs (spec 0019)" },
  { path = "std::eprintln", reason = "log with info!/warn!/error! from src/log.rs (spec 0019)" },
  { path = "std::print", reason = "spec 0019" },
  { path = "std::eprint", reason = "spec 0019" },
]
```
CI already runs `cargo clippy --all-targets --locked -- -D warnings`, so a new `println!` fails the build. Titles have no lint; the review convention is "a window in a log line goes through `ws::who`".

**Rollout**: one migration change. `println!` becomes `info!`, `eprintln!` becomes `warn!`, then one hand pass raises the lines that meet the `ERROR` rule and swaps titles for `ws::who`. The lint goes in the same change.

**Exceptions**:
- Tests may print (the render tests, `live_release`): their modules carry `#[allow(clippy::disallowed_macros)]`.
- `src/log.rs` writes with `write!`/`WriteFile`, not the banned macros.
- None otherwise. Lines meant for a person at a terminal (`des-k is already running`) become `info!` too; in a release build they reach the log anyway.

**Critical test scenarios**:
- The stamp: a fixed `SYSTEMTIME` pair (local 17:05:33.123, UTC 14:05:33.120) formats as `2026-11-07 17:05:33.123 +03:00`; a negative offset (`-04:30`) and midnight rollover between the two calls round correctly.
- Collapse: the same line three times writes it once, then `(the line above repeated 2 more times)` before the next different line; A, B, A, B writes four lines.
- Roll: with the limit set low in a test, the writer renames and starts fresh with the header; the count pending at the roll is written before it.
- A message with a newline comes out as one line.
- By hand: a release run's `des-k.log` starts with the header, and the finder closing a window logs its exe name, not its title.

## Consequences

**Positive**:
- A friend's log answers when, how bad, which version and which Windows build, without a follow up question.
- Logs are safe to share: no titles, no account name from paths.
- A runaway repeat can't fill the disk, and the lint keeps the format from drifting.

**Negative / tradeoffs**:
- A diff across all 20 files at once; it can conflict with other work in flight, so it should land on its own.
- You lose window titles when debugging your own machine; the exe name says which app, not which window of it.
- A repeat count is written only when something else happens or des-k exits; a hard kill loses the final count (the first occurrence is still there).
- A roll replaces `des-k.old.log`, so after a 5 MB run the previous run's log is gone.
- Lines get 37 characters longer.

**Neutral**:
- `install::installed()` splits out of `can_install` (same check, now shared).
- No new crate and no new Cargo feature: the time, registry and console APIs are already enabled.

## Follow-up

- [ ] `/sync` after the build: AGENTS.md's "Per window failures are logged with `eprintln!`" rule becomes `warn!`, plus a line on the levels rule, `ws::who` and the lint.
- [ ] Optional: enroll this as a scope feature with `/scope` if you want it tracked next to the others; as a standalone standard it needs no scope row.
