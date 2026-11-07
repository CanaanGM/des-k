# 0015. Check for updates from the tray: rationale

The decision record behind [index.md](index.md). The build reads index.md; this file says why.

## Context

des-k ships as GitHub releases: `.github/workflows/build.yml` publishes one per `v*` tag, with the setup exe, a zip and checksums, on `https://github.com/CanaanGM/des-k/releases`. Nothing in des-k tells you a newer one exists, so the installed copy can fall behind without you noticing. That matters more now that des-k uses an undocumented part of Windows: a Windows update can break hiding (since 0.3.0 the tray warns when it does), and the fix arrives only as a new release.

des-k runs as admin (spec 0008). AGENTS.md says nothing in `%LOCALAPPDATA%\des-k` may name a program to run, and files the tray opens go through explorer so they run as you. Anything a network answer can influence is input from outside the machine, so it must never decide what program runs or what file is opened with admin rights.

You want no traffic you did not ask for: GitHub should see your address only when you click.

## Options considered

### Option 1: Ask GitHub's release redirect with WinHTTP

`GET https://github.com/CanaanGM/des-k/releases/latest` with redirects turned off. GitHub answers with a redirect whose `Location` header ends in `/releases/tag/vX.Y.Z`; des-k reads the version from it. WinHTTP (the HTTP client built into Windows) is reached through the `windows` crate already in use, adding only the `Win32_Networking_WinHttp` feature.

**Pros**:
- No new crate, no JSON parsing: one header and three numbers.
- WinHTTP checks the TLS certificate (the HTTPS proof that the answer comes from github.com) and, in its automatic proxy mode, uses your Windows proxy settings.
- `releases/latest` already skips drafts and prereleases.

**Cons**:
- Rests on the shape of a web redirect, not a documented API; if GitHub changes it, the check fails (loudly, AC-4) until des-k changes.
- WinHTTP is verbose raw Win32: a session, a connection, a request, handles to close.

### Option 2: Ask the GitHub REST API with WinHTTP

`GET https://api.github.com/repos/CanaanGM/des-k/releases/latest` and read `tag_name` from the JSON answer.

**Pros**:
- A documented, versioned API, less likely to change shape.

**Cons**:
- A JSON answer to parse: either a new dependency (`serde_json`) or a hand written field search, which is fragile in its own way.
- Needs a `User-Agent` header and is rate limited to 60 requests an hour without a token (fine for clicks, but one more failure to explain).

### Option 3: A Rust HTTP crate (for example `ureq` with `rustls`)

A higher level client with its own TLS stack.

**Pros**:
- Shorter, safer looking code than raw WinHTTP.

**Cons**:
- Several new dependencies, a bigger exe and a longer build, against AGENTS.md's four direct dependencies.
- Its own TLS stack and certificate store, apart from Windows', and it does not pick up the system proxy by default.

### Option 4: No network, the menu item opens the releases page

**Check for updates** just opens `https://github.com/CanaanGM/des-k/releases` in your browser; you compare versions by eye.

**Pros**:
- No network code in an admin process at all.

**Cons**:
- You must remember which version you run; the tray never tells you there is nothing new. It is a bookmark, not a check.

## Rationale

The forces are: an admin process, a wish for no surprise traffic, a tool with four dependencies, and a single fact to learn (the newest version number). Option 1 learns that fact with the least code that can go wrong: no JSON, no new crate, and Windows' own TLS and proxy handling, which matters in an admin process where a second TLS stack is one more thing to keep patched. The cost is leaning on a redirect's shape; that is acceptable because a change fails loudly into AC-4's balloon and log line, never into a wrong answer, since anything but `/tag/v<digits>.<digits>.<digits>` is rejected.

Option 2 is the runner up: the documented API is the better long term contract, but it trades the header for a JSON parse that either adds a dependency or a hand rolled search. If the redirect ever changes, moving to Option 2 is a small change inside one function (the fetch), with the rest of the design unchanged.

Option 3 adds more than it saves for one request a click. Option 4 does not answer the question you asked ("is there a newer one?").

Doing the request on a worker thread keeps the rule that the main thread never waits on slow work (AGENTS.md, Threads): a 10 s timeout on the main thread would freeze switching. The answer comes back as a posted window message to the tray window, not a thread message, because a thread message is lost while a menu is open (the review's modal loop finding), and a posted window message is dispatched inside a menu's loop too.

## References

None.
