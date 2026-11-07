// Low-level keyboard hook, on its own thread so slow cloaking never stalls input.
// - The previous/next shortcuts from settings: swallowed, posted to the main thread as
//   WM_APP (wparam = -1 / +1).
// - The four send shortcuts: posted as WM_SEND (wparam = -1 / +1, lparam = 1 follow, 0 stay).
//   A held send acts once (spec 0004).
// - A digit 1 to 9 (number row or numpad) with a modifier set from settings: WM_JUMP, or
//   WM_SEND_TO (wparam = index, lparam as WM_SEND). The lists above win (spec 0005).
// - The finder shortcut: posted as WM_FINDER, once per press (spec 0007).
// - The overview shortcut: posted as WM_OVERVIEW, once per press (spec 0010).
// - The pin shortcut: posted as WM_PIN, once per press (spec 0014).
// - The hints shortcut: posted as WM_HINTS, once per press (spec 0017).
// - Copilot key (sends LWin, LShift, F23): turned into Right Ctrl, unless settings turn it off.
// - The keys and clicks touchpad gestures type (spec 0011).
// - Recording for the settings page or the hints window: while on, every key is swallowed and
//   none of the above fires; each first key down that is not a modifier goes to the main thread
//   as WM_RECORDED (wparam = vk, lparam = modifier bits) (spec 0012 AC-7, spec 0017 AC-HNT-4).
// - A watch: Windows silently removes a low level hook that answers too slowly, and then
//   every shortcut goes to Windows. Raw input counts the keys too; keys it saw that the hook
//   never did mean the hook is gone, and it is installed again.
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        Input::{KeyboardAndMouse::*, RAWINPUTDEVICE, RIDEV_INPUTSINK, RegisterRawInputDevices},
        WindowsAndMessaging::*,
    },
};
use windows::core::{PCWSTR, w};

use crate::settings::{self, ALT, CTRL, Combo, SHIFT, WIN};

const MAGIC: usize = 0x7064_6B73; // dwExtraInfo on our own injected input ("pdks")
const VK_MASK: u16 = 0xE8; // unassigned vk: makes the shell think Win was combined, so no Start menu
/// Thread message to the main thread: send the focused window one workspace over.
pub const WM_SEND: u32 = WM_APP + 3;
/// Thread message: show workspace wparam (0 based) on the monitor under the mouse.
pub const WM_JUMP: u32 = WM_APP + 4;
/// Thread message: send the focused window to workspace wparam (0 based).
pub const WM_SEND_TO: u32 = WM_APP + 5;
/// Thread message: open or close the window finder.
pub const WM_FINDER: u32 = WM_APP + 6;
/// Thread message: open or close the workspace overview.
pub const WM_OVERVIEW: u32 = WM_APP + 7;
/// Thread message: the settings page's recorder got a key (spec 0012).
pub const WM_RECORDED: u32 = WM_APP + 10;
/// Thread message: pin or unpin the focused window (spec 0014).
pub const WM_PIN: u32 = WM_APP + 12;
/// Thread message: open, move or close the Shortcut hints window (spec 0017).
pub const WM_HINTS: u32 = WM_APP + 14;

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
static RECORDING: AtomicBool = AtomicBool::new(false);
/// The window recording, which main sends WM_RECORDED on to; 0 while none is. Main thread.
static OWNER: AtomicIsize = AtomicIsize::new(0);

/// The settings page or the hints window turns recording on (`owner`, its window) and off.
/// Only one records at a time: whoever starts cancels the other first, and an `off` from a
/// window that no longer owns it changes nothing. Main thread.
pub fn record(owner: HWND, on: bool) {
    let me = owner.0 as isize;
    if on {
        OWNER.store(me, Ordering::Relaxed);
    } else if OWNER.compare_exchange(me, 0, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    RECORDING.store(on, Ordering::Relaxed);
}

/// The window recording now, if any: main hands it WM_RECORDED.
pub fn recording_window() -> Option<HWND> {
    let h = OWNER.load(Ordering::Relaxed);
    (h != 0).then_some(HWND(h as *mut _))
}

#[derive(Default)]
struct State {
    win_down: bool,
    win_before: bool, // was LWin already down before the latest LWin down?
    shift_down: bool,
    shift_before: bool,
    copilot: bool,           // between the Copilot key's F23 down and its trailing LWin up
    copilot_released: bool,  // its F23 up has been seen
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
    static SEND_HELD: Cell<Option<u16>> = const { Cell::new(None) }; // the key of a send, finder or overview combo that is down
    // Keys whose down the recorder swallowed: their repeats and up are swallowed too, even
    // after recording ends, so Windows never sees half a key.
    static SWALLOWED: RefCell<[bool; 256]> = const { RefCell::new([false; 256]) };
    // The watch: the hook, and key events each side saw since the last check.
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static HOOK_CALLS: Cell<u32> = const { Cell::new(0) };
    static RAW_KEYS: Cell<u32> = const { Cell::new(0) };
}

const WATCH_CLASS: PCWSTR = w!("des-k-keys");
const WATCH_TIMER: usize = 1;
const WATCH_MS: u32 = 5000;

fn mod_of(vk: u16) -> u8 {
    match VIRTUAL_KEY(vk) {
        VK_LWIN | VK_RWIN => WIN,
        VK_LCONTROL | VK_RCONTROL | VK_CONTROL => CTRL,
        VK_LMENU | VK_RMENU | VK_MENU => ALT,
        VK_LSHIFT | VK_RSHIFT | VK_SHIFT => SHIFT,
        _ => 0,
    }
}

/// Recording, or a key the recorder still owns: swallow it. The modifiers come from the keys
/// it swallowed (Windows never saw those go down) plus any held from before recording.
fn recorder(vk: u16, up: bool) -> bool {
    let recording = RECORDING.load(Ordering::Relaxed);
    SWALLOWED.with_borrow_mut(|keys| {
        let i = vk as usize & 0xFF;
        if up {
            if keys[i] {
                keys[i] = false;
                return true;
            }
            // Held from before recording: its up goes through, but a lone Win or Alt up would
            // open Start or a menu bar.
            if recording && mod_of(vk) & (WIN | ALT) != 0 {
                send(&[(VK_MASK, false), (VK_MASK, true)]);
            }
            return false;
        }
        if keys[i] {
            return true; // a repeat
        }
        if !recording {
            return false;
        }
        keys[i] = true;
        if mod_of(vk) == 0 {
            let swallowed = keys.iter().enumerate().filter(|(_, down)| **down).fold(0, |m, (k, _)| m | mod_of(k as u16));
            unsafe {
                let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), WM_RECORDED, WPARAM(vk as usize), LPARAM((mods_held() | swallowed) as isize));
            }
        }
        true
    })
}

/// The recorder while it is on or still owns this key, else the shortcuts.
fn route(vk: u16, up: bool) -> bool {
    if RECORDING.load(Ordering::Relaxed) || SWALLOWED.with_borrow(|keys| keys[vk as usize & 0xFF]) {
        return recorder(vk, up);
    }
    shortcut(vk, up)
}

/// One key event; `extra` is MAGIC for des-k's own input, 0 for a combo the hook should see.
fn key(vk: u16, up: bool, extra: usize) -> INPUT {
    let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    // Without it the arrows and friends arrive as their numpad twins.
    let extended = [VK_RCONTROL, VK_LWIN, VK_LEFT, VK_RIGHT, VK_UP, VK_DOWN, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_INSERT, VK_DELETE];
    if extended.iter().any(|e| e.0 == vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: extra },
        },
    }
}

fn send(keys: &[(u16, bool)]) {
    let inputs: Vec<INPUT> = keys.iter().map(|&(vk, up)| key(vk, up, MAGIC)).collect();
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Types VK_MASK, a key no app uses. Windows then counts des-k as the source of the last
/// input and lets it move focus, which after a touchpad gesture it does not (spec 0011).
pub fn mask() {
    send(&[(VK_MASK, false), (VK_MASK, true)]);
}

/// Presses `vks` in order and lets them go in reverse, as des-k's own input, so it goes
/// straight to Windows: Win+S, the media keys (spec 0011).
pub fn tap(vks: &[u16]) {
    let keys: Vec<(u16, bool)> = vks.iter().map(|&vk| (vk, false)).chain(vks.iter().rev().map(|&vk| (vk, true))).collect();
    send(&keys);
}

/// Types a combo as if you pressed it: untagged, so this hook sees it and a combo that is a
/// des-k shortcut runs that shortcut (spec 0011). Win or Alt ones get VK_MASK before the
/// modifiers go up, as the hook does, so neither Start nor a menu bar opens.
pub fn combo(c: Combo) {
    let mods: Vec<u16> = [(WIN, VK_LWIN), (CTRL, VK_LCONTROL), (ALT, VK_LMENU), (SHIFT, VK_LSHIFT)]
        .iter()
        .filter(|(bit, _)| c.mods & bit != 0)
        .map(|(_, vk)| vk.0)
        .collect();
    let mut inputs: Vec<INPUT> = mods.iter().map(|&vk| key(vk, false, 0)).collect();
    inputs.extend([key(c.vk, false, 0), key(c.vk, true, 0)]);
    if c.mods & (WIN | ALT) != 0 {
        inputs.extend([key(VK_MASK, false, MAGIC), key(VK_MASK, true, MAGIC)]);
    }
    inputs.extend(mods.iter().rev().map(|&vk| key(vk, true, 0)));
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// A middle button down and up wherever the pointer is (spec 0011).
pub fn middle_click() {
    let click = |flags| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: flags, dwExtraInfo: MAGIC, ..Default::default() } },
    };
    unsafe { SendInput(&[click(MOUSEEVENTF_MIDDLEDOWN), click(MOUSEEVENTF_MIDDLEUP)], size_of::<INPUT>() as i32) };
}

fn held(vk: VIRTUAL_KEY) -> bool {
    (unsafe { GetAsyncKeyState(vk.0 as i32) }) < 0
}

/// The modifiers held right now, either side. The Copilot key's injected Right Ctrl counts.
fn mods_held() -> u8 {
    let mut m = 0;
    for (on, bit) in [(held(VK_LWIN) || held(VK_RWIN), WIN), (held(VK_CONTROL), CTRL), (held(VK_MENU), ALT), (held(VK_SHIFT), SHIFT)] {
        if on {
            m |= bit;
        }
    }
    m
}

/// The 0 based workspace of a number row or numpad digit 1 to 9. With Shift held, Windows
/// turns numpad digits into navigation keys, so those never get here.
fn digit(vk: u16) -> Option<usize> {
    match vk {
        v if (b'1' as u16..=b'9' as u16).contains(&v) => Some((v - b'1' as u16) as usize),
        v if (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&v) => Some((v - VK_NUMPAD1.0) as usize),
        _ => None,
    }
}

/// What a pressed combo does: (message, wparam, lparam follow), or None for a key that is
/// not des-k's. The lists first, in settings::LISTS order, then the digit sets in order, so a
/// list holding one of a set's digits keeps it (spec 0012 AC-8). Modifiers must match exactly.
fn decide(st: &settings::Settings, combo: Combo) -> Option<(u32, isize, isize)> {
    // (list, message, wparam direction, lparam follow)
    let all: [(&[Combo], u32, isize, isize); 10] = [
        (&st.previous, WM_APP, -1, 0),
        (&st.next, WM_APP, 1, 0),
        (&st.send_previous, WM_SEND, -1, 1),
        (&st.send_next, WM_SEND, 1, 1),
        (&st.send_previous_stay, WM_SEND, -1, 0),
        (&st.send_next_stay, WM_SEND, 1, 0),
        (&st.finder, WM_FINDER, 0, 0),
        (&st.overview, WM_OVERVIEW, 0, 0),
        (&st.pin, WM_PIN, 0, 0),
        (&st.hints, WM_HINTS, 0, 0),
    ];
    let sets = [(st.jump, WM_JUMP, 0), (st.send_to, WM_SEND_TO, 1), (st.send_to_stay, WM_SEND_TO, 0)];
    all.iter().find(|(list, ..)| list.contains(&combo)).map(|&(_, msg, dir, follow)| (msg, dir, follow)).or_else(|| {
        let i = digit(combo.vk)?;
        sets.iter().find(|(set, ..)| *set == Some(combo.mods)).map(|&(_, msg, follow)| (msg, i as isize, follow))
    })
}

/// A switch or send shortcut: act on key down, and swallow its down, repeats and up.
fn shortcut(vk: u16, up: bool) -> bool {
    // A held send acts once, even if a modifier is let go meanwhile: otherwise a held
    // Win+Ctrl+Shift+Left would turn into Win+Ctrl+Left switches.
    match SEND_HELD.get() {
        Some(held) if held == vk => {
            if up {
                SEND_HELD.set(None);
            }
            return true;
        }
        // Any other key down ends it, so a missed key up (say after Win+L) costs one press at most.
        Some(_) if !up => SEND_HELD.set(None),
        _ => {}
    }
    let st = settings::get();
    if digit(vk).is_none() && !settings::LISTS.iter().any(|l| st.list(l).iter().any(|c| c.vk == vk)) {
        return false; // most keys: skip the modifier lookups
    }
    let combo = Combo { mods: mods_held(), vk };
    let Some((msg, wparam, follow)) = decide(st, combo) else {
        return false;
    };
    if !up {
        // Releasing Win alone opens Start, Alt alone opens an app's menu bar.
        if combo.mods & (WIN | ALT) != 0 {
            send(&[(VK_MASK, false), (VK_MASK, true)]);
        }
        // A held finder, overview, pin or hints key would toggle it over and over.
        if matches!(msg, WM_SEND | WM_SEND_TO | WM_FINDER | WM_OVERVIEW | WM_PIN | WM_HINTS) {
            SEND_HELD.set(Some(vk));
        }
        unsafe {
            let _ = PostThreadMessageW(MAIN_THREAD.load(Ordering::Relaxed), msg, WPARAM(wparam as usize), LPARAM(follow));
        }
    }
    true
}

/// true = swallow the event.
fn handle(vk: u16, up: bool) -> bool {
    if !settings::get().copilot_as_right_ctrl {
        return route(vk, up);
    }
    STATE.with_borrow_mut(|s| {
        // Copilot key in progress: its own LWin/LShift repeats and ups must not reach anyone.
        if s.copilot {
            match (vk, up) {
                (0x86, false) => return true, // F23 repeat
                (0x86, true) => {
                    send(&[(VK_RCONTROL.0, true)]);
                    s.copilot_released = true;
                    return true;
                }
                (v, false) if v == VK_LWIN.0 || v == VK_LSHIFT.0 => return true,
                (v, true) if v == VK_LSHIFT.0 && s.copilot_released => return true,
                (v, true) if v == VK_LWIN.0 && s.copilot_released => {
                    s.copilot = false;
                    return true;
                }
                _ => return route(vk, up), // e.g. Copilot as Ctrl, with Win+Left
            }
        }

        match (vk, up) {
            (v, false) if v == VK_LWIN.0 => {
                s.win_before = s.win_down;
                s.win_down = true;
            }
            (v, true) if v == VK_LWIN.0 => s.win_down = false,
            (v, false) if v == VK_LSHIFT.0 => {
                s.shift_before = s.shift_down;
                s.shift_down = true;
            }
            (v, true) if v == VK_LSHIFT.0 => s.shift_down = false,
            (0x86, false) => {
                // The LWin/LShift downs just before this F23 came from the Copilot key.
                // Release them unless they were already physically held.
                let mut seq = vec![(VK_MASK, false), (VK_MASK, true)];
                if !s.shift_before {
                    seq.push((VK_LSHIFT.0, true));
                    s.shift_down = false;
                }
                if !s.win_before {
                    seq.push((VK_LWIN.0, true));
                    s.win_down = false;
                }
                seq.push((VK_RCONTROL.0, false));
                send(&seq);
                // A recorder that swallowed those downs forgets them: the Copilot key counts
                // as Ctrl held (the Right Ctrl just sent), not as Win+Shift (spec 0012 AC-7).
                SWALLOWED.with_borrow_mut(|keys| {
                    if !s.shift_before {
                        keys[VK_LSHIFT.0 as usize] = false;
                    }
                    if !s.win_before {
                        keys[VK_LWIN.0 as usize] = false;
                    }
                });
                s.copilot = true;
                s.copilot_released = false;
                return true;
            }
            _ => {}
        }

        route(vk, up)
    })
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    HOOK_CALLS.set(HOOK_CALLS.get().wrapping_add(1));
    if code == HC_ACTION as i32 {
        let k = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        let up = matches!(wp.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        if k.dwExtraInfo != MAGIC && handle(k.vkCode as u16, up) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// Whether the hook looks removed: since the last check raw input saw a few key events and
/// the hook none. A quiet keyboard says nothing either way.
fn hook_gone(hook_calls: u32, raw_keys: u32) -> bool {
    hook_calls == 0 && raw_keys >= 3
}

fn install() -> windows::core::Result<HHOOK> {
    unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), Some(GetModuleHandleW(None)?.into()), 0) }
}

unsafe extern "system" fn watch(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match m {
        WM_INPUT => RAW_KEYS.set(RAW_KEYS.get().wrapping_add(1)),
        WM_TIMER if wp.0 == WATCH_TIMER => {
            if hook_gone(HOOK_CALLS.take(), RAW_KEYS.take()) {
                if let Some(old) = HOOK.take() {
                    unsafe { let _ = UnhookWindowsHookEx(old); }
                }
                match install() {
                    Ok(new) => {
                        HOOK.set(Some(new));
                        eprintln!("keyboard hook: Windows had removed it (it answered too slowly once), installed again");
                    }
                    Err(e) => eprintln!("keyboard hook: Windows had removed it and it could not be installed again ({e}), trying again in {} s", WATCH_MS / 1000),
                }
            }
            return LRESULT(0);
        }
        _ => {}
    }
    // WM_INPUT too: DefWindowProc frees what raw input allocated.
    unsafe { DefWindowProcW(h, m, wp, lp) }
}

/// The watch's message only window. Without it the hook still works, only unwatched: logged.
fn start_watch() {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let wc = WNDCLASSW { lpfnWndProc: Some(watch), hInstance: module.into(), lpszClassName: WATCH_CLASS, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = match CreateWindowExW(Default::default(), WATCH_CLASS, w!(""), Default::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(module.into()), None) {
            Ok(h) => h,
            Err(e) => return eprintln!("keyboard hook: no watch window, a hook Windows removes stays gone: {e}"),
        };
        // INPUTSINK: keys arrive whatever window has focus. Keyboard: usage page 1, usage 6.
        let rid = RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd };
        if let Err(e) = RegisterRawInputDevices(&[rid], size_of::<RAWINPUTDEVICE>() as u32) {
            return eprintln!("keyboard hook: no raw input, a hook Windows removes stays gone: {e}");
        }
        SetTimer(Some(hwnd), WATCH_TIMER, WATCH_MS, None);
    }
}

pub fn spawn(main_thread: u32) {
    MAIN_THREAD.store(main_thread, Ordering::Relaxed);
    std::thread::spawn(|| unsafe {
        HOOK.set(Some(install().expect("keyboard hook")));
        start_watch();
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn c(mods: u8, vk: u16) -> Combo {
        Combo { mods, vk }
    }

    // The defaults the README promises: a wrong message here moves the wrong thing.
    #[test]
    fn the_default_keys_do_what_the_readme_says() {
        let s = Settings::default();
        assert_eq!(decide(&s, c(WIN | CTRL, VK_LEFT.0)), Some((WM_APP, -1, 0)));
        assert_eq!(decide(&s, c(WIN | CTRL | SHIFT, VK_RIGHT.0)), Some((WM_SEND, 1, 1)));
        assert_eq!(decide(&s, c(WIN | CTRL | ALT, VK_LEFT.0)), Some((WM_SEND, -1, 0)));
        assert_eq!(decide(&s, c(WIN, b'3' as u16)), Some((WM_JUMP, 2, 0)));
        assert_eq!(decide(&s, c(WIN | SHIFT, VK_NUMPAD9.0)), Some((WM_SEND_TO, 8, 1)));
        assert_eq!(decide(&s, c(WIN | CTRL, b'1' as u16)), Some((WM_SEND_TO, 0, 0)));
        assert_eq!(decide(&s, c(WIN | CTRL, b'U' as u16)), Some((WM_PIN, 0, 0)));
    }

    // Everything des-k does not own must reach Windows and the apps: an extra modifier is
    // another shortcut (Win+Ctrl+Shift+Alt+Left), and 0 is no workspace.
    #[test]
    fn other_combos_pass_through() {
        let s = Settings::default();
        assert_eq!(decide(&s, c(WIN | CTRL | SHIFT | ALT, VK_LEFT.0)), None);
        assert_eq!(decide(&s, c(CTRL, VK_LEFT.0)), None);
        assert_eq!(decide(&s, c(WIN, b'0' as u16)), None);
        assert_eq!(decide(&s, c(WIN | ALT, b'1' as u16)), None, "no digit set has Win+Alt");
    }

    // The settings page tells you a list on one of a set's digits keeps that digit (spec 0012 AC-8).
    #[test]
    fn a_list_beats_a_digit_set_on_its_digit() {
        let s = Settings { finder: vec![c(WIN, b'2' as u16)], ..Settings::default() };
        assert_eq!(decide(&s, c(WIN, b'2' as u16)), Some((WM_FINDER, 0, 0)));
        assert_eq!(decide(&s, c(WIN, b'3' as u16)), Some((WM_JUMP, 2, 0)));
    }

    // The hints key has none by default, so no press of Win+Ctrl+H is taken from Windows until
    // you set it; once set, it opens the window (spec 0017 AC-HNT-1).
    #[test]
    fn the_hints_key_fires_only_once_set() {
        assert_eq!(decide(&Settings::default(), c(WIN | CTRL, b'H' as u16)), None);
        let s = Settings { hints: vec![c(WIN | CTRL, b'H' as u16)], ..Settings::default() };
        assert_eq!(decide(&s, c(WIN | CTRL, b'H' as u16)), Some((WM_HINTS, 0, 0)));
    }

    // Two windows record keys: a key pressed for one must never land in the other, and the
    // window that lost recording to the other must not switch it off by stopping late.
    #[test]
    fn only_the_window_recording_can_stop_it() {
        let (page, hints) = (HWND(3 as *mut _), HWND(4 as *mut _));
        record(page, true);
        record(hints, true);
        assert_eq!(recording_window(), Some(hints));
        record(page, false);
        assert_eq!((recording_window(), RECORDING.load(Ordering::Relaxed)), (Some(hints), true));
        record(hints, false);
        assert_eq!((recording_window(), RECORDING.load(Ordering::Relaxed)), (None, false));
    }

    // Typing with the hook gone sends every shortcut to Windows' own desktops; a quiet keyboard
    // must not count as gone, or the hook would be reinstalled for nothing every few seconds.
    #[test]
    fn only_keys_the_hook_never_heard_mean_it_is_gone() {
        assert!(hook_gone(0, 3));
        assert!(!hook_gone(0, 0), "nobody typed");
        assert!(!hook_gone(0, 2), "a key or two can fall between two checks");
        assert!(!hook_gone(1, 40), "the hook heard some");
    }

    // A set turned off ("" in the file) gives its digits back to Windows (Win+1 opens a taskbar app).
    #[test]
    fn a_set_turned_off_matches_nothing() {
        let s = Settings { jump: None, ..Settings::default() };
        assert_eq!(decide(&s, c(WIN, b'1' as u16)), None);
    }
}
