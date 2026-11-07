# 0018. Install updates from the tray: rationale

## Context

Spec 0015 tells you a newer des-k exists and opens its release page. From there you download the setup, run it, and accept an admin prompt. You find that too tedious for a tool you update often, and the fix for a broken Windows update reaches you only as fast as you go through those steps.

des-k runs as admin (spec 0008). Anything it downloads and runs, runs as admin without a prompt, so the update path decides what code gets the highest rights on your PC. AGENTS.md already forbids anything in `%LOCALAPPDATA%` from naming a program to run, because anything running as you can write there; the same reasoning applies to the environment des-k inherits.

Releases are built by `.github/workflows/build.yml` and carry the setup, a zip, and `SHA256SUMS.txt`. The exe is not code signed. The setup (`des-k.iss`) already hard stops des-k (keeping `state.txt`, so the layout survives), installs, and starts des-k again through its logon task.

The installed des-k runs from that logon task. Tried 2026-11-07: a process started by a scheduled task is inside a job object whose limit flags are 0, so its children cannot break away. A setup des-k started directly would stay in des-k's job, and the setup's own `schtasks /end /tn des-k` would end it partway, leaving des-k stopped and its windows hidden.

## Options considered

### Option 1: Download, verify, run the setup through a one shot task (chosen)

des-k downloads the checksum file and the setup into `<Program Files>\des-k\update\`, checks SHA-256, registers a task `des-k update` that runs the setup silently, and runs it.

**Pros**: no prompt, outside des-k's job, the setup's own stop, install and restart logic is reused unchanged; no new dependency.
**Cons**: a second task to create and clean up; trust rests on GitHub alone.

### Option 2: Start the setup directly from des-k

`CreateProcessW` on the verified setup.

**Pros**: simplest code.
**Cons**: the setup lands in des-k's task job, which refuses breakaway, so `schtasks /end` kills it, and while it lives the des-k task still counts as running, so `schtasks /run` is ignored (`MultipleInstancesPolicy IgnoreNew`). Unworkable without reshaping the setup.

### Option 3: Your own signing key over the checksum file

As Option 1, plus you sign `SHA256SUMS.txt` on your PC with an offline key and des-k carries the public key.

**Pros**: a taken over GitHub account can no longer ship you a setup.
**Cons**: a manual signing step on every release, and a key to keep safe. You chose the simpler trust level for now.

### Option 4: Authenticode code signing

Sign the exe with a certificate (paid, or a monthly signing service) and check the signer.

**Pros**: strongest, and SmartScreen trusts the downloads too.
**Cons**: money and CI setup for a one person tool.

### Option 5: Open the verified setup through explorer, with one UAC prompt

des-k downloads and verifies as in Option 1, then opens the setup through explorer (`IShellDispatch2::ShellExecute`, as the tray's Open log). It runs outside des-k's job as you, and the setup raises its own admin prompt.

**Pros**: no task, no XML, no polling, no restart action; the setup's own error boxes show to you; the prompt is explicit consent for admin code. Raised by the cross check of 2026-11-07.
**Cons**: one prompt and the setup's window on every update, which is part of the tedium you asked to remove. You chose silence (2026-11-07).

## Rationale

You chose checksum level trust: it stops a damaged or swapped download, and against a taken over GitHub account it is no weaker than installing by hand, which trusts the same release page. What the in app path adds, running as admin without a prompt, is covered by the admin only folder and by taking no path from the environment. Option 1 is then the only way that works with the existing setup: the job test rules out Option 2, and reusing the setup keeps the one install path the uninstaller, `install.ps1` and the layout handling already rely on. Signing (Options 3 and 4) stays a follow up that slots in before the run step without changing the rest.

The not installed copies keep opening the page because they cannot write Program Files or skip the prompt, and a test build must never replace your install by accident. The setup restarting des-k after a failure exists because, unlike a manual install, nobody is watching the screen when a silent one fails.
