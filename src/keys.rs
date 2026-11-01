// Low-level keyboard hook, on its own thread so slow cloaking never stalls input.
// - The previous/next shortcuts from settings: swallowed, posted to the main thread as
//   WM_APP (wparam = -1 / +1).
// - The four send shortcuts: posted as WM_SEND (wparam = -1 / +1, lparam = 1 follow, 0 stay).
//   A held send acts once (spec 0004).
// - A digit 1 to 9 (number row or numpad) with a modifier set from settings: WM_JUMP, or
//   WM_SEND_TO (wparam = index, lparam as WM_SEND). The lists above win (spec 0005).
// - The finder shortcut: posted as WM_FINDER, once per press (spec 0007).
// - The overview shortcut: posted as WM_OVERVIEW, once per press (spec 0010).
// - Copilot key (sends LWin, LShift, F23): turned into Right Ctrl, unless settings turn it off.
// - The keys and clicks touchpad gestures type (spec 0011).
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::GetModuleHandleW,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

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

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);

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
}

/// One key event; `extra` is MAGIC for pdesk's own input, 0 for a combo the hook should see.
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

/// Types VK_MASK, a key no app uses. Windows then counts pdesk as the source of the last
/// input and lets it move focus, which after a touchpad gesture it does not (spec 0011).
pub fn mask() {
    send(&[(VK_MASK, false), (VK_MASK, true)]);
}

/// Presses `vks` in order and lets them go in reverse, as pdesk's own input, so it goes
/// straight to Windows: Win+S, the media keys (spec 0011).
pub fn tap(vks: &[u16]) {
    let keys: Vec<(u16, bool)> = vks.iter().map(|&vk| (vk, false)).chain(vks.iter().rev().map(|&vk| (vk, true))).collect();
    send(&keys);
}

/// Types a combo as if you pressed it: untagged, so this hook sees it and a combo that is a
/// pdesk shortcut runs that shortcut (spec 0011). Win or Alt ones get VK_MASK before the
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
    // (list, message, wparam direction, lparam follow)
    let all: [(&[Combo], u32, isize, isize); 8] = [
        (&st.previous, WM_APP, -1, 0),
        (&st.next, WM_APP, 1, 0),
        (&st.send_previous, WM_SEND, -1, 1),
        (&st.send_next, WM_SEND, 1, 1),
        (&st.send_previous_stay, WM_SEND, -1, 0),
        (&st.send_next_stay, WM_SEND, 1, 0),
        (&st.finder, WM_FINDER, 0, 0),
        (&st.overview, WM_OVERVIEW, 0, 0),
    ];
    let index = digit(vk);
    if index.is_none() && !all.iter().any(|(list, ..)| list.iter().any(|c| c.vk == vk)) {
        return false; // most keys: skip the modifier lookups
    }
    let combo = Combo { mods: mods_held(), vk };
    // (message, wparam, lparam follow): the lists first, then the digit sets in order.
    let sets = [(st.jump, WM_JUMP, 0), (st.send_to, WM_SEND_TO, 1), (st.send_to_stay, WM_SEND_TO, 0)];
    let found = all.iter().find(|(list, ..)| list.contains(&combo)).map(|&(_, msg, dir, follow)| (msg, dir, follow)).or_else(|| {
        let i = index?;
        sets.iter().find(|(set, ..)| *set == Some(combo.mods)).map(|&(_, msg, follow)| (msg, i as isize, follow))
    });
    let Some((msg, wparam, follow)) = found else {
        return false;
    };
    if !up {
        // Releasing Win alone opens Start, Alt alone opens an app's menu bar.
        if combo.mods & (WIN | ALT) != 0 {
            send(&[(VK_MASK, false), (VK_MASK, true)]);
        }
        // A held finder or overview key would open and close it over and over.
        if matches!(msg, WM_SEND | WM_SEND_TO | WM_FINDER | WM_OVERVIEW) {
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
        return shortcut(vk, up);
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
                _ => return shortcut(vk, up), // e.g. Copilot as Ctrl, with Win+Left
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
                s.copilot = true;
                s.copilot_released = false;
                return true;
            }
            _ => {}
        }

        shortcut(vk, up)
    })
}

unsafe extern "system" fn hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let k = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        let up = matches!(wp.0 as u32, WM_KEYUP | WM_SYSKEYUP);
        if k.dwExtraInfo != MAGIC && handle(k.vkCode as u16, up) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

pub fn spawn(main_thread: u32) {
    MAIN_THREAD.store(main_thread, Ordering::Relaxed);
    std::thread::spawn(|| unsafe {
        let module = GetModuleHandleW(None).expect("module handle");
        let _hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), Some(module.into()), 0).expect("keyboard hook");
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    });
}
