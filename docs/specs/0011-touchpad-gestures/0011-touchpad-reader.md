# 0011 child: touchpad reader and gesture recognizer

## Summary

A new thread asks Windows for the raw reports of every precision touchpad (the touchpads Windows 11 drives itself, which report each finger's position). It follows the fingers, decides when three or four of them made a swipe or a tap, and tells the main thread which rule to run. It does no slow work, like the keyboard hook thread.

## Design

**Thread** (new file `src/touchpad.rs`, owns the reader and the recognizer):
- `touchpad::spawn(main_thread)` is called in `main` next to `keys::spawn`, after `settings::load()`. It starts a thread that creates a message only window (`HWND_MESSAGE` parent, class `pdesk-touchpad`) and calls `RegisterRawInputDevices` with usage page `0x0D` (digitizer), usage `0x05` (touchpad), flags `RIDEV_INPUTSINK | RIDEV_DEVNOTIFY`, target that window. It then runs its own `GetMessageW` loop.
- It adds no message filter. `WM_INPUT` and `WM_INPUT_DEVICE_CHANGE` come from the system.
- If `[touchpad] enabled` is false at start, the thread is not started (log line).
- A recognized gesture is matched against the map only while `windows_off()` (0011-first-run-setup) is true, read from the registry right then: 13 small reads once per gesture, never per report, before any twice wait. Otherwise it logs `gesture 4 left → skipped, Windows' FourFingerLeft is on` and posts nothing.
- After handling `WM_INPUT`, always call `DefWindowProcW`, which frees the input data (with `RIDEV_INPUTSINK` skipping it leaks).
- On `WM_INPUT_DEVICE_CHANGE`, besides refreshing the device cache, post `WM_TOUCHPADS_CHANGED` (`WM_APP + 9`) to the main thread so an open welcome screen refreshes.
- If a touchpad driver sends no raw reports at all (found by the check step or seen on some machine), pdesk logs it once after the first minute of touchpad use and you stay on the Windows key route ("Give gestures back").

**Reading a report**:
- On `WM_INPUT`: `GetRawInputData(RID_INPUT)` into a buffer reused by the thread; the `RAWHID` part holds `dwCount` reports of `dwSizeHid` bytes each.
- Per device (`hDevice`), cached on first sight and on `WM_INPUT_DEVICE_CHANGE`, dropped on removal: the preparsed data (`GetRawInputDeviceInfoW(RIDI_PREPARSEDDATA)`), `HidP_GetCaps`, and from `HidP_GetValueCaps` the link collection of each finger and the X and Y logical and physical ranges.
- **Millimeters**: `mm = (v - LogicalMin) * (PhysicalMax - PhysicalMin) / (LogicalMax - LogicalMin) * 10^UnitsExp * k`, where the unit's system nibble gives `k` = 10 for centimeters (SI linear) or 25.4 for inches (English linear); `UnitsExp` is the 4 bit signed exponent. If the physical range is missing, zero or inverted, or the unit is neither, assume the pad is 100 × 60 mm and log it once.
- Per report: `HidP_GetUsageValue` for contact count (`0x0D`/`0x54`) and scan time (`0x0D`/`0x56`) on link collection 0; per finger link collection, tip switch (`0x0D`/`0x42`) and confidence (`0x0D`/`0x47`) via `HidP_GetUsages`, contact id (`0x0D`/`0x51`), X (`0x01`/`0x30`), Y (`0x01`/`0x31`); and the button page (`0x09`) for a physical click.
- Both ways touchpads report a frame are handled: all fingers in one report (parallel: read only the first `count` finger collections), or a few fingers per report with the count only in the first (hybrid). A frame is the reports sharing one scan time; it is complete when the fingers read equal the count of its first report. A scan time change before that drops the half frame.
- A finger counts only with tip switch 1 and confidence 1 (confidence 0 is a palm, as Windows treats it).
- A report that fails to parse is skipped; the first failure per device is logged.

**Recognizing** (one touch session = from the first finger down until every finger is up; session state is kept per touchpad, keyed by `hDevice`, and reset when that touchpad is removed):
- Fingers = the most fingers down at once in this session so far. Sessions reaching 5 or more fingers recognize nothing.
- Position = the mean X and Y of the fingers down, in mm. The start point is taken again whenever a finger is added, so fingers landing one by one do not count as motion. When a finger lifts, the position jumps; the start point shifts by that same jump, so the distance traveled so far is kept.
- **Swipe**: with 3 or 4 fingers, once the position has moved at least `swipe_mm` from the start point and the larger axis is at least twice the smaller, the swipe is left, right, up or down along the larger axis. Up means fingers moving away from you, which is Y getting smaller (touchpad Y grows toward you). It fires once per session; nothing more fires until every finger is up.
- **Tap**: 3 or 4 fingers, every finger up within 250 ms of the first one down, the position never more than 3 mm from the start point, and no physical click during the session. Fires at the last finger up.
- **Twice** (one pending gesture shared by all touchpads): on a recognized gesture, the thread asks `settings::rules_for(fingers, motion) -> (Option<usize>, Option<usize>)` for the once and twice rule indexes. No twice rule → post the once rule now (if any). A twice rule → if a pending first of the same gesture exists, cancel its timer and post the twice rule; else remember it as pending and `SetTimer(twice_ms)`. The wait counts from the moment the first gesture fired to the moment the second fires. On the timer, post the once rule (if any) and clear pending. A different gesture arriving while one is pending first posts the pending once rule, then handles the new one.
- Posting: `PostThreadMessageW(main, WM_GESTURE, wparam = rule index, 0)`. No rule → log "gesture <n> <motion> → no rule".

**Main thread**: `touchpad::WM_GESTURE => gestures::run(index)` (0011-gesture-map), alongside the existing arms in `main.rs`.

## Value sourcing

| Value | Source |
|---|---|
| fingers, motion | recognized from the reports, as above |
| swipe distance, twice wait | `[touchpad] swipe_mm`, `twice_ms` (0011-gesture-map) |
| tap time 250 ms, tap drift 3 mm, axis ratio 2 | fixed here |
| touchpad size in mm | the device's X and Y value caps; else 100 × 60 assumed |
| active or not | `[touchpad] enabled` at start, then `windows_off()` per recognized gesture (0011-first-run-setup) |
| rule index | `settings::get().gestures` |

## Invariants

- The reader never runs an action and never touches `ws` state; only the main thread does.
- At most one action per touch session, plus the twice logic across two sessions.
- One finger and two finger input is never acted on.

## Short rationale

Raw Input is the documented way to read a precision touchpad's HID reports (HID: the standard format USB and Bluetooth input devices report in), and `HidP_*` parses them from the device's own description, so no touchpad specific byte layouts are hard coded. An elevated pdesk gets these reports whatever window has focus (the check step confirms it). Its own thread matches the keyboard hook's rule: frames arrive about 125 times a second and must not queue behind a slow switch on the main thread. Firing a swipe at the distance rather than at lift makes it feel as quick as Windows'. Runner up: reading on the main thread, rejected because a busy main thread would make swipes late.
