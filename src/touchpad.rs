// Touchpad reader and gesture recognizer (spec 0011), on its own thread like the keyboard
// hook, so frames (about 125 a second) never queue behind a slow switch. It reads every
// precision touchpad's raw reports through Raw Input, follows the fingers, and posts
// WM_GESTURE (wparam = rule index in settings' gestures) to the main thread. It never runs
// an action itself.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use windows::Win32::{
    Devices::HumanInterfaceDevice::*,
    Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::*, WindowsAndMessaging::*},
};
use windows::core::{PCWSTR, w};

use crate::settings::{self, Motion};

/// Thread message to the main thread: run gesture rule wparam.
pub const WM_GESTURE: u32 = WM_APP + 8;
/// Thread message to the main thread: a touchpad came or went, so an open welcome screen refreshes.
pub const WM_TOUCHPADS_CHANGED: u32 = WM_APP + 9;
const CLASS: PCWSTR = w!("pdesk-touchpad");
// A tap: every finger up this soon after the first one landed, having moved no more than this.
const TAP_TIME: Duration = Duration::from_millis(250);
const TAP_DRIFT_MM: f64 = 3.0;
const TWICE_TIMER: usize = 1;

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

/// A gesture that has a twice rule, waiting `twice_ms` for its second go.
#[derive(Clone, Copy)]
struct Pending {
    fingers: usize,
    motion: Motion,
    once: Option<usize>,
}

/// One axis: its logical minimum and how many mm one logical unit is.
#[derive(Clone, Copy)]
struct Axis {
    min: i32,
    mm: f64,
}

/// The reports that share one scan time. Hybrid touchpads spread a frame over a few
/// reports and give the contact count only in the first.
#[derive(Default)]
struct Frame {
    scan: u32,
    want: usize,           // contacts in this frame, from its first report
    read: usize,           // contacts read so far, lifting ones included
    down: Vec<(f64, f64)>, // the fingers touching, in mm
    click: bool,           // the touchpad's button is pressed
}

/// From the first finger down until every finger is up.
#[derive(Default)]
struct Session {
    fingers: usize, // the most down at once so far
    down: usize,    // down in the last frame
    at: (f64, f64), // their mean position in the last frame, mm
    start: (f64, f64),
    fired: bool,
    began: Option<Instant>, // the first finger down
    moved: bool,            // ever further than TAP_DRIFT_MM from the start
    clicked: bool,
}

struct Pad {
    pre: Vec<u8>,      // preparsed data: the device's own description of its reports
    fingers: Vec<u16>, // the link collection of each finger slot in a report
    x: Axis,
    y: Axis,
    frame: Frame,
    session: Session,
}

thread_local! {
    // None: a device whose reports cannot be read, logged once.
    static PADS: RefCell<HashMap<isize, Option<Pad>>> = RefCell::default();
    static BUF: RefCell<Vec<u64>> = RefCell::default(); // u64 keeps RAWINPUT aligned
    static PENDING: Cell<Option<Pending>> = const { Cell::new(None) }; // one for all touchpads
}

fn ok(s: windows::Win32::Foundation::NTSTATUS) -> bool {
    s == HIDP_STATUS_SUCCESS
}

fn value(pp: PHIDP_PREPARSED_DATA, rep: &mut [u8], page: u16, lc: u16, usage: u16) -> Option<u32> {
    let mut v = 0;
    ok(unsafe { HidP_GetUsageValue(HidP_Input, page, Some(lc), usage, &mut v, pp, rep) }).then_some(v)
}

/// mm per logical unit from the unit and exponent in the description, or None if it has none
/// that makes sense. Unit system nibble 1 is centimeters, 3 is inches.
fn mm_per_unit(c: &HIDP_VALUE_CAPS) -> Option<f64> {
    let exp = match (c.UnitsExp & 0xF) as i32 {
        e if e >= 8 => e - 16,
        e => e,
    };
    let k = match c.Units & 0xF {
        1 => 10.0,
        3 => 25.4,
        _ => return None,
    };
    let (logical, physical) = (c.LogicalMax - c.LogicalMin, c.PhysicalMax - c.PhysicalMin);
    (logical > 0 && physical > 0).then(|| physical as f64 * 10f64.powi(exp) * k / logical as f64)
}

/// Reads a touchpad's description and logs one line naming it.
fn pad_for(dev: HANDLE) -> Result<Pad, String> {
    unsafe {
        let mut info = RID_DEVICE_INFO { cbSize: size_of::<RID_DEVICE_INFO>() as u32, ..Default::default() };
        let mut size = info.cbSize;
        GetRawInputDeviceInfoW(Some(dev), RIDI_DEVICEINFO, Some((&mut info as *mut RID_DEVICE_INFO).cast()), &mut size);
        let (vid, pid) = (info.Anonymous.hid.dwVendorId, info.Anonymous.hid.dwProductId);
        let mut size = 0;
        GetRawInputDeviceInfoW(Some(dev), RIDI_PREPARSEDDATA, None, &mut size);
        let mut pre = vec![0u8; size as usize];
        if size == 0 || GetRawInputDeviceInfoW(Some(dev), RIDI_PREPARSEDDATA, Some(pre.as_mut_ptr().cast()), &mut size) == u32::MAX {
            return Err(format!("touchpad {vid:04x}:{pid:04x}: no report description, no gestures from it"));
        }
        let pp = PHIDP_PREPARSED_DATA(pre.as_ptr() as isize);
        let mut caps = HIDP_CAPS::default();
        let mut n = 0;
        if ok(HidP_GetCaps(pp, &mut caps)) {
            n = caps.NumberInputValueCaps;
        }
        let mut vc = vec![HIDP_VALUE_CAPS::default(); n as usize];
        if n == 0 || !ok(HidP_GetValueCaps(HidP_Input, vc.as_mut_ptr(), &mut n, pp)) {
            return Err(format!("touchpad {vid:04x}:{pid:04x}: unreadable report description, no gestures from it"));
        }
        let mut fingers = vec![];
        let (mut x, mut y) = (None, None);
        for c in &vc[..n as usize] {
            let usage = if c.IsRange { c.Anonymous.Range.UsageMin } else { c.Anonymous.NotRange.Usage };
            match (c.UsagePage, usage) {
                (1, 0x30) => {
                    fingers.push(c.LinkCollection);
                    x = Some(c);
                }
                (1, 0x31) => y = Some(c),
                _ => {}
            }
        }
        fingers.sort();
        fingers.dedup();
        let (Some(x), Some(y)) = (x, y) else {
            return Err(format!("touchpad {vid:04x}:{pid:04x}: reports no finger positions, no gestures from it"));
        };
        if x.LogicalMax <= x.LogicalMin || y.LogicalMax <= y.LogicalMin {
            return Err(format!("touchpad {vid:04x}:{pid:04x}: empty position range, no gestures from it"));
        }
        let (w, h) = ((x.LogicalMax - x.LogicalMin) as f64, (y.LogicalMax - y.LogicalMin) as f64);
        let (xmm, ymm) = match (mm_per_unit(x), mm_per_unit(y)) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                println!("touchpad {vid:04x}:{pid:04x}: no usable size in its description, assuming 100 × 60 mm");
                (100.0 / w, 60.0 / h)
            }
        };
        println!("touchpad {vid:04x}:{pid:04x}, {:.0} × {:.0} mm, {} finger slots", w * xmm, h * ymm, fingers.len());
        Ok(Pad {
            pre,
            fingers,
            x: Axis { min: x.LogicalMin, mm: xmm },
            y: Axis { min: y.LogicalMin, mm: ymm },
            frame: Frame::default(),
            session: Session::default(),
        })
    }
}

/// Reads and logs a touchpad the first time it is seen.
fn add(pads: &mut HashMap<isize, Option<Pad>>, dev: HANDLE) {
    pads.entry(dev.0 as isize).or_insert_with(|| pad_for(dev).map_err(|e| eprintln!("{e}")).ok());
}

/// Every precision touchpad there now.
fn touchpads() -> Vec<HANDLE> {
    unsafe {
        let item = size_of::<RAWINPUTDEVICELIST>() as u32;
        let mut n = 0;
        GetRawInputDeviceList(None, &mut n, item);
        let mut list = vec![RAWINPUTDEVICELIST::default(); n as usize];
        if GetRawInputDeviceList(Some(list.as_mut_ptr()), &mut n, item) == u32::MAX {
            n = 0;
        }
        let is_touchpad = |d: &&RAWINPUTDEVICELIST| {
            let mut info = RID_DEVICE_INFO { cbSize: size_of::<RID_DEVICE_INFO>() as u32, ..Default::default() };
            let mut size = info.cbSize;
            GetRawInputDeviceInfoW(Some(d.hDevice), RIDI_DEVICEINFO, Some((&mut info as *mut RID_DEVICE_INFO).cast()), &mut size);
            info.Anonymous.hid.usUsagePage == 0x0D && info.Anonymous.hid.usUsage == 0x05
        };
        list.iter().take(n as usize).filter(|d| d.dwType == RIM_TYPEHID).filter(is_touchpad).map(|d| d.hDevice).collect()
    }
}

/// For the welcome screen: any thread.
pub fn present() -> bool {
    !touchpads().is_empty()
}

/// Every precision touchpad there now, logged one line each.
fn add_all() {
    let list = touchpads();
    if list.is_empty() {
        println!("no precision touchpad found: no touchpad gestures");
    }
    PADS.with_borrow_mut(|pads| {
        for d in list {
            add(pads, d);
        }
    });
}

impl Pad {
    /// One report. Returns the gesture it completes, if any.
    fn report(&mut self, rep: &mut [u8]) -> Option<(usize, Motion)> {
        let pp = PHIDP_PREPARSED_DATA(self.pre.as_ptr() as isize);
        let count = value(pp, rep, 0x0D, 0, 0x54).unwrap_or(0) as usize;
        let scan = value(pp, rep, 0x0D, 0, 0x56).unwrap_or(0);
        let f = &mut self.frame;
        if count > 0 {
            // A new frame; one still half read is dropped.
            f.scan = scan;
            f.want = count;
            f.read = 0;
            f.down.clear();
            f.click = false;
        } else if f.read >= f.want || f.scan != scan {
            return None; // the rest of a frame we never started, or one whose scan time moved on
        }
        // The button page: a press there is a click, which is never a tap.
        let mut buttons = [0u16; 4];
        let mut n = buttons.len() as u32;
        f.click |= ok(unsafe { HidP_GetUsages(HidP_Input, 0x09, Some(0), buttons.as_mut_ptr(), &mut n, pp, rep) }) && n > 0;
        // Parallel reports carry every finger, but only the first `count` slots are live;
        // the rest hold stale contacts.
        let live = self.fingers.len().min(f.want - f.read);
        for &lc in &self.fingers[..live] {
            f.read += 1;
            let mut usages = [0u16; 8];
            let mut n = usages.len() as u32;
            if !ok(unsafe { HidP_GetUsages(HidP_Input, 0x0D, Some(lc), usages.as_mut_ptr(), &mut n, pp, rep) }) {
                continue;
            }
            let on = &usages[..(n as usize).min(usages.len())];
            // Confidence 0 is a palm, as Windows treats it.
            if !(on.contains(&0x42) && on.contains(&0x47)) {
                continue;
            }
            if let (Some(x), Some(y)) = (value(pp, rep, 1, lc, 0x30), value(pp, rep, 1, lc, 0x31)) {
                f.down.push(((x as i32 - self.x.min) as f64 * self.x.mm, (y as i32 - self.y.min) as f64 * self.y.mm));
            }
        }
        if f.read < f.want {
            return None;
        }
        self.session.frame(&f.down, f.click)
    }
}

impl Session {
    /// One complete frame: the fingers touching, in mm, and whether the button is pressed.
    fn frame(&mut self, down: &[(f64, f64)], click: bool) -> Option<(usize, Motion)> {
        let n = down.len();
        self.clicked |= click;
        if n == 0 {
            // Every finger up: a tap if it was short, still, and no click.
            let quick = self.began.is_some_and(|t| t.elapsed() <= TAP_TIME);
            let tap = quick && !self.fired && !self.moved && !self.clicked && (3..=4).contains(&self.fingers);
            let fingers = self.fingers;
            *self = Session::default();
            return tap.then_some((fingers, Motion::Tap));
        }
        if self.down == 0 {
            self.began = Some(Instant::now());
        }
        let at = (down.iter().map(|p| p.0).sum::<f64>() / n as f64, down.iter().map(|p| p.1).sum::<f64>() / n as f64);
        if n > self.fingers {
            // Fingers landing one by one is not motion, and a two finger scroll before is no swipe.
            self.start = at;
        } else if n != self.down {
            // A finger lifting, or one back after dropping out for a frame (lost contact, a palm
            // flag), makes the mean jump; move the start with it to keep the distance so far.
            self.start.0 += at.0 - self.at.0;
            self.start.1 += at.1 - self.at.1;
        }
        self.fingers = self.fingers.max(n);
        self.down = n;
        self.at = at;
        let (dx, dy) = (at.0 - self.start.0, at.1 - self.start.1);
        self.moved |= dx.hypot(dy) > TAP_DRIFT_MM;
        if self.fired || !(3..=4).contains(&self.fingers) {
            return None;
        }
        let (big, small) = (dx.abs().max(dy.abs()), dx.abs().min(dy.abs()));
        if big < settings::get().swipe_mm as f64 || big < 2.0 * small {
            return None;
        }
        self.fired = true; // one swipe per session, however long
        // Touchpad Y grows toward you, so up is Y getting smaller.
        Some((self.fingers, match (dx.abs() > dy.abs(), dx < 0.0, dy < 0.0) {
            (true, true, _) => Motion::Left,
            (true, false, _) => Motion::Right,
            (false, _, true) => Motion::Up,
            (false, _, false) => Motion::Down,
        }))
    }
}

/// Hands rule `rule` to the main thread, or logs that the gesture has none.
fn post(fingers: usize, motion: Motion, rule: Option<usize>) {
    match rule.and_then(|i| Some((i, settings::get().gestures.get(i)?))) {
        Some((i, g)) => {
            println!("gesture {g} → {}", g.action);
            unsafe {
                let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_GESTURE, WPARAM(i), LPARAM(0));
            }
        }
        None => println!("gesture {fingers} {} → no rule", motion.word()),
    }
}

/// A recognized gesture. One with a twice rule waits `twice_ms` for its second go: then only
/// the twice rule runs, else only the once rule (spec 0011 AC-5).
fn recognized(hwnd: HWND, fingers: usize, motion: Motion) {
    // Read now, before any twice wait: while Windows acts on gestures too, each would do two things.
    if let Err(why) = crate::setup::windows_off() {
        return println!("gesture {fingers} {} → skipped, Windows' {why}", motion.word());
    }
    let (once, twice) = settings::rules_for(fingers, motion);
    if let Some(p) = PENDING.take() {
        unsafe { let _ = KillTimer(Some(hwnd), TWICE_TIMER); }
        if (p.fingers, p.motion) == (fingers, motion) {
            return post(fingers, motion, twice);
        }
        post(p.fingers, p.motion, p.once); // a different gesture ends the wait
    }
    match twice {
        None => post(fingers, motion, once),
        Some(_) => {
            PENDING.set(Some(Pending { fingers, motion, once }));
            unsafe { SetTimer(Some(hwnd), TWICE_TIMER, settings::get().twice_ms, None) };
        }
    }
}

fn on_input(hwnd: HWND, h: HRAWINPUT) {
    let header = size_of::<RAWINPUTHEADER>() as u32;
    let mut size = 0;
    unsafe { GetRawInputData(h, RID_INPUT, None, &mut size, header) };
    let gestures = BUF.with_borrow_mut(|buf| {
        // Never smaller than a RAWINPUT, so reading its header stays inside the buffer.
        buf.resize((size as usize).max(size_of::<RAWINPUT>()).div_ceil(8), 0);
        if size == 0 || unsafe { GetRawInputData(h, RID_INPUT, Some(buf.as_mut_ptr().cast()), &mut size, header) } == u32::MAX {
            return vec![];
        }
        let (kind, dev, count, each) = {
            let raw = unsafe { &*(buf.as_ptr() as *const RAWINPUT) };
            let hid = unsafe { &raw.data.hid };
            (raw.header.dwType, raw.header.hDevice, hid.dwCount as usize, hid.dwSizeHid as usize)
        };
        if kind != RIM_TYPEHID.0 {
            return vec![];
        }
        // The reports themselves, never past what GetRawInputData wrote.
        let bytes = unsafe { std::slice::from_raw_parts_mut(buf.as_mut_ptr().cast::<u8>(), buf.len() * 8) };
        let start = std::mem::offset_of!(RAWINPUT, data) + std::mem::offset_of!(RAWHID, bRawData);
        let fits = |r: &&mut [u8]| each > 0 && count.checked_mul(each).is_some_and(|n| n <= r.len());
        let Some(reports) = bytes.get_mut(start..size as usize).filter(fits) else {
            return vec![];
        };
        PADS.with_borrow_mut(|pads| {
            add(pads, dev);
            let Some(Some(pad)) = pads.get_mut(&(dev.0 as isize)) else {
                return vec![];
            };
            reports.chunks_exact_mut(each).take(count).filter_map(|r| pad.report(r)).collect()
        })
    });
    for (fingers, motion) in gestures {
        recognized(hwnd, fingers, motion);
    }
}

fn touchpads_changed() {
    unsafe { let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_TOUCHPADS_CHANGED, WPARAM(0), LPARAM(0)); }
}

unsafe extern "system" fn wnd(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_INPUT => on_input(h, HRAWINPUT(lp.0 as _)),
        // No second go in time: the once rule runs.
        WM_TIMER if wp.0 == TWICE_TIMER => {
            unsafe { let _ = KillTimer(Some(h), TWICE_TIMER); }
            if let Some(p) = PENDING.take() {
                post(p.fingers, p.motion, p.once);
            }
        }
        WM_INPUT_DEVICE_CHANGE if wp.0 as u32 == GIDC_ARRIVAL => {
            PADS.with_borrow_mut(|pads| add(pads, HANDLE(lp.0 as _)));
            touchpads_changed();
        }
        WM_INPUT_DEVICE_CHANGE if wp.0 as u32 == GIDC_REMOVAL => {
            PADS.with_borrow_mut(|pads| pads.remove(&lp.0));
            touchpads_changed();
        }
        _ => {}
    }
    // Always, also for WM_INPUT: it frees the input data, which RIDEV_INPUTSINK would leak otherwise.
    unsafe { DefWindowProcW(h, m, wp, lp) }
}

/// Starts the reader unless `[touchpad] enabled` is false. A failure here costs the
/// gestures only, logged, never pdesk: a panic on this thread would show every hidden window.
pub fn spawn(main_thread: u32) {
    if !settings::get().touchpad_enabled {
        println!("touchpad gestures off ([touchpad] enabled = false)");
        return;
    }
    MAIN_THREAD.store(main_thread, Ordering::Relaxed);
    std::thread::spawn(|| unsafe {
        let Ok(module) = GetModuleHandleW(None) else {
            eprintln!("touchpad: no module handle, no gestures");
            return;
        };
        let wc = WNDCLASSW { lpfnWndProc: Some(wnd), hInstance: module.into(), lpszClassName: CLASS, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(Default::default(), CLASS, w!(""), Default::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(module.into()), None) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("touchpad: no window, no gestures: {e}");
                return;
            }
        };
        add_all();
        // INPUTSINK: reports arrive whatever window has focus. DEVNOTIFY: touchpads coming and going.
        let rid = RAWINPUTDEVICE { usUsagePage: 0x0D, usUsage: 0x05, dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY, hwndTarget: hwnd };
        if let Err(e) = RegisterRawInputDevices(&[rid], size_of::<RAWINPUTDEVICE>() as u32) {
            eprintln!("touchpad: raw input not registered, no gestures: {e}");
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four fingers side by side, their mean at `x` mm.
    fn four(x: f64) -> Vec<(f64, f64)> {
        (0..4).map(|i| (x - 7.5 + 5.0 * i as f64, 30.0)).collect()
    }

    #[test]
    fn a_finger_flickering_mid_swipe_keeps_the_distance_so_far() {
        // A finger near the edge often drops out for a frame (lost contact, or its confidence
        // bit flickers). The swipe must still fire once the hand has moved swipe_mm (15) in all,
        // not 15 more after the finger comes back: else gestures come late or never.
        let mut s = Session::default();
        assert_eq!(s.frame(&four(10.0), false), None);
        assert_eq!(s.frame(&four(20.0), false), None); // 10 mm so far
        assert_eq!(s.frame(&four(21.0)[..3], false), None); // one finger out for a frame
        assert_eq!(s.frame(&four(22.0), false), None); // back; the 2 mm moved while it was out is lost
        assert_eq!(s.frame(&four(28.0), false), Some((4, Motion::Right))); // 16 mm counted (6 after a reset)
    }

    #[test]
    fn fingers_landing_one_by_one_are_not_a_swipe() {
        // The mean jumps as each finger lands; that jump is not motion.
        let mut s = Session::default();
        let hand = four(10.0);
        for n in 1..=4 {
            assert_eq!(s.frame(&hand[..n], false), None);
        }
        assert_eq!(s.frame(&four(20.0), false), None); // 10 mm since the last landed
        assert_eq!(s.frame(&four(26.0), false), Some((4, Motion::Right)));
    }
}
