// Restore after a reboot or a new sign in (docs/specs/0003-restore-layout-after-reboot/index.md):
// window fingerprints, the current logon session, and the rule that picks which saved
// entry a reopened window belongs to. A wrong pick hides a window on the wrong
// workspace, so every doubt leaves the window visible.
use windows::Win32::{
    Devices::{DeviceAndDriverInstallation::*, Display::GUID_DEVINTERFACE_MONITOR},
    Foundation::{CloseHandle, HANDLE, HWND, LUID},
    Security::{
        Authentication::Identity::{LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA},
        GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenStatistics,
    },
    Storage::EnhancedStorage::PKEY_AppUserModel_ID,
    System::{
        Com::{CoTaskMemFree, StructuredStorage::{PropVariantClear, PropVariantToStringAlloc}},
        Registry::{KEY_READ, RegCloseKey, RegQueryValueExW},
        Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    },
    UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
};
use windows::core::{HSTRING, PCWSTR, PWSTR, w};

use crate::ws::Spot;

/// What identifies a window across a reboot, where its handle and pid are new.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Fp {
    pub app: String,
    pub class: String,
    pub title: String,
}

/// A saved window: `ws` is `Some` for a hidden one (`W` line), `None` for a visible one (`V`),
/// or a pinned one (`P`, `pin` set: in view on every workspace, spec 0014). An away one (`A`,
/// spec 0016) has `away` set, `dev` its home screen and `ws` its home workspace. `dev` is a
/// screen id; `spot` is where it was, None in a line from an older des-k.
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    pub dev: String,
    pub ws: Option<usize>,
    pub pin: bool,
    pub away: bool,
    pub spot: Option<Spot>,
    pub fp: Fp,
}

#[derive(PartialEq, Debug)]
pub enum Pick {
    Take(usize),
    /// Saved entries of this app and class are left, but none clearly fits.
    Unclear,
    /// Nothing of this app and class is left to match.
    NoMatch,
}

/// Tabs and line breaks would break the `state.txt` line format.
pub fn clean(text: &str) -> String {
    text.replace(['\t', '\r', '\n'], " ")
}

/// The window's program: the lowercased exe path, or the app id for a Store app (whose
/// exe is always applicationframehost.exe). Empty when the process cannot be opened.
pub fn app(hwnd: HWND, pid: u32) -> String {
    let Some(exe) = exe_path(pid) else { return String::new() };
    let exe = clean(&exe.to_lowercase());
    if exe.ends_with("\\applicationframehost.exe") {
        return app_id(hwnd).unwrap_or(exe);
    }
    exe
}

/// The full exe path of a process, as Windows spells it; None when it cannot be opened.
pub fn exe_path(pid: u32) -> Option<String> {
    let proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.is_ok();
    unsafe { let _ = CloseHandle(proc); }
    ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
}

fn app_id(hwnd: HWND) -> Option<String> {
    unsafe {
        let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd).ok()?;
        let mut v = store.GetValue(&PKEY_AppUserModel_ID).ok()?;
        let s = PropVariantToStringAlloc(&v).ok();
        let _ = PropVariantClear(&mut v);
        let s = s?;
        let id = s.to_string().ok();
        CoTaskMemFree(Some(s.0 as _));
        id.filter(|id| !id.is_empty()).map(|id| clean(&id.to_lowercase()))
    }
}

/// The current logon session id and its logon time in Unix seconds. A new id means a new
/// sign in (reboot, Fast Startup boot, or sign out and back in).
pub fn logon() -> Option<(u64, i64)> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
        let mut stats = TOKEN_STATISTICS::default();
        let mut len = 0;
        let ok = GetTokenInformation(token, TokenStatistics, Some(&mut stats as *mut _ as _), size_of::<TOKEN_STATISTICS>() as u32, &mut len).is_ok();
        let _ = CloseHandle(token);
        if !ok {
            return None;
        }
        let id: LUID = stats.AuthenticationId;
        let mut data: *mut SECURITY_LOGON_SESSION_DATA = std::ptr::null_mut();
        if LsaGetLogonSessionData(&id, &mut data).is_err() || data.is_null() {
            return None;
        }
        let filetime = (*data).LogonTime;
        let _ = LsaFreeReturnBuffer(data as _);
        // FILETIME counts 100 ns steps since 1601.
        let unix = filetime / 10_000_000 - 11_644_473_600;
        Some((((id.HighPart as u32 as u64) << 32) | id.LowPart as u64, unix))
    }
}

/// The matching rule from spec 0003. `pending` holds the indexes into `saved` not used
/// yet. Pass 1 only takes exact titles, so in pass 2 a guess can never take an entry
/// that another window's exact title would have taken in the same tick.
pub fn pick(saved: &[Entry], pending: &[usize], fp: &Fp, pass: u8) -> Pick {
    if fp.app.is_empty() {
        return Pick::NoMatch;
    }
    let same = |e: &Entry| e.fp.app == fp.app && e.fp.class == fp.class;
    let candidates: Vec<usize> = pending.iter().copied().filter(|&i| same(&saved[i])).collect();
    let Some(&first) = candidates.first() else { return Pick::NoMatch };

    let norm = |t: &str| t.trim().to_lowercase();
    let want = norm(&fp.title);
    let equal: Vec<usize> = candidates.iter().copied().filter(|&i| norm(&saved[i].fp.title) == want).collect();
    if pass == 1 {
        return match equal.as_slice() {
            [i] => Pick::Take(*i),
            _ => Pick::Unclear,
        };
    }

    // The whole group, used entries too: once a visible entry of the app is used up, its
    // hidden siblings must still need a title to decide. Pinned is a place of its own.
    let mut places = saved.iter().filter(|e| same(e)).map(|e| (e.pin, e.ws.map(|ws| (e.dev.as_str(), ws))));
    let first_place = places.next().expect("candidates come from this group");
    if places.all(|p| p == first_place) {
        return Pick::Take(first);
    }
    match equal.as_slice() {
        [i] => return Pick::Take(*i),
        [_, _, ..] => return Pick::Unclear,
        [] => {}
    }
    let contains = |i: &usize| {
        let t = norm(&saved[*i].fp.title);
        let (short, long) = if t.chars().count() < want.chars().count() { (&t, &want) } else { (&want, &t) };
        short.chars().count() >= 4 && long.contains(short.as_str())
    };
    match candidates.iter().filter(|i| contains(i)).collect::<Vec<_>>().as_slice() {
        [i] => Pick::Take(**i),
        _ => Pick::Unclear,
    }
}

/// A monitor's own id (spec 0016 AC-1), from `path`, a display config target's
/// `monitorDevicePath`: maker letters, product code and serial from the EDID in the monitor's
/// registry key, or the monitor's device instance id when there is no readable EDID. None when
/// the monitor is not present or SetupAPI cannot open it.
pub fn edid_id(path: &str) -> Option<String> {
    let dev = MonitorDev::open(path)?;
    dev.edid().and_then(|e| id_from_edid(&e)).or_else(|| dev.instance())
}

/// `MMM-PPPP-SERIAL` from EDID bytes, or `MMM-PPPP` when the monitor gives no serial.
fn id_from_edid(e: &[u8]) -> Option<String> {
    if e.len() < 128 {
        return None;
    }
    let m = u16::from_be_bytes([e[8], e[9]]);
    let letter = |v: u16| char::from(b'@' + (v & 0x1F) as u8);
    let maker: String = [letter(m >> 10), letter(m >> 5), letter(m)].into_iter().collect();
    let product = u16::from_le_bytes([e[10], e[11]]);
    // The text serial of a display descriptor (tag 0xFF) is the one printed on the label;
    // many makers leave the number at 0 or the same on every unit.
    let text = (0..4)
        .map(|i| &e[54 + i * 18..72 + i * 18])
        .find(|d| d[..3] == [0, 0, 0] && d[3] == 0xFF)
        .map(|d| d[5..].iter().take_while(|&&c| c != 0x0A).map(|&c| c as char).collect::<String>().trim().to_string())
        .filter(|s| !s.is_empty());
    let number = u32::from_le_bytes([e[12], e[13], e[14], e[15]]);
    Some(match text.or((number != 0).then(|| number.to_string())) {
        Some(serial) => format!("{maker}-{product:04X}-{serial}"),
        None => format!("{maker}-{product:04X}"),
    })
}

/// One present monitor in a SetupAPI device list, freed on drop.
struct MonitorDev {
    set: HDEVINFO,
    data: SP_DEVINFO_DATA,
}

impl Drop for MonitorDev {
    fn drop(&mut self) {
        unsafe { let _ = SetupDiDestroyDeviceInfoList(self.set); }
    }
}

impl MonitorDev {
    fn open(path: &str) -> Option<Self> {
        unsafe {
            // DIGCF_PRESENT: a monitor no longer attached keeps its old EDID in the registry.
            let set = SetupDiGetClassDevsW(Some(&GUID_DEVINTERFACE_MONITOR), PCWSTR::null(), None, DIGCF_PRESENT | DIGCF_DEVICEINTERFACE).ok()?;
            let mut dev = MonitorDev { set, data: SP_DEVINFO_DATA { cbSize: size_of::<SP_DEVINFO_DATA>() as u32, ..Default::default() } };
            let mut iface = SP_DEVICE_INTERFACE_DATA { cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32, ..Default::default() };
            SetupDiOpenDeviceInterfaceW(set, &HSTRING::from(path), 0, Some(&mut iface)).ok()?;
            let mut need = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &iface, None, 0, Some(&mut need), None);
            // u64s keep the buffer aligned for the detail struct.
            let mut buf = vec![0u64; (need as usize).div_ceil(8).max(1)];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            SetupDiGetDeviceInterfaceDetailW(set, &iface, Some(detail), need, None, Some(&mut dev.data)).ok()?;
            Some(dev)
        }
    }

    fn edid(&self) -> Option<Vec<u8>> {
        unsafe {
            let key = SetupDiOpenDevRegKey(self.set, &self.data, DICS_FLAG_GLOBAL.0, 0, DIREG_DEV, KEY_READ.0).ok()?;
            let mut buf = vec![0u8; 512];
            let mut len = buf.len() as u32;
            let read = RegQueryValueExW(key, w!("EDID"), None, None, Some(buf.as_mut_ptr()), Some(&mut len));
            let _ = RegCloseKey(key);
            read.ok().ok()?;
            buf.truncate(len as usize);
            Some(buf)
        }
    }

    fn instance(&self) -> Option<String> {
        let mut buf = [0u16; 512];
        unsafe { SetupDiGetDeviceInstanceIdW(self.set, &self.data, Some(&mut buf), None).ok()? };
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(app: &str, class: &str, title: &str) -> Fp {
        Fp { app: app.into(), class: class.into(), title: title.into() }
    }
    fn hidden(dev: &str, ws: usize, f: Fp) -> Entry {
        Entry { dev: dev.into(), ws: Some(ws), pin: false, away: false, spot: None, fp: f }
    }
    fn visible(f: Fp) -> Entry {
        Entry { dev: "DISPLAY1".into(), ws: None, pin: false, away: false, spot: None, fp: f }
    }
    const EDGE: &str = r"c:\edge\msedge.exe";
    const WIN: &str = "Chrome_WidgetWin_1";

    // An app with one saved window: its title often changes across a reboot (a new tab,
    // an unsaved marker), and there is no other place it could belong.
    #[test]
    fn a_single_entry_group_matches_any_title() {
        let saved = [hidden("DISPLAY1", 2, fp(EDGE, WIN, "Inbox - Outlook"))];
        assert_eq!(pick(&saved, &[0], &fp(EDGE, WIN, "New tab"), 2), Pick::Take(0));
    }

    // Rule 2 looks at the whole saved group: after the visible entry is used, the one
    // left is hidden, but a second Edge window could still be the one that was visible.
    #[test]
    fn a_visible_plus_hidden_group_never_matches_on_rule_2() {
        let saved = [visible(fp(EDGE, WIN, "Docs")), hidden("DISPLAY1", 1, fp(EDGE, WIN, "Mail"))];
        assert_eq!(pick(&saved, &[1], &fp(EDGE, WIN, "Something else"), 2), Pick::Unclear);
    }

    // A guess between a pinned and a plain window of one app would pin the wrong one (spec 0014).
    #[test]
    fn a_pinned_plus_visible_group_never_matches_on_rule_2() {
        let pinned = Entry { pin: true, ..visible(fp(EDGE, WIN, "Music")) };
        let saved = [pinned, visible(fp(EDGE, WIN, "Docs"))];
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, "New tab"), 2), Pick::Unclear);
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, "music"), 2), Pick::Take(0));
    }

    #[test]
    fn an_equal_title_wins() {
        let saved = [hidden("DISPLAY1", 1, fp(EDGE, WIN, "Mail")), hidden("DISPLAY1", 3, fp(EDGE, WIN, "Docs"))];
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, " docs "), 2), Pick::Take(1));
    }

    // Two windows with the same title on different workspaces: guessing would put one wrong.
    #[test]
    fn two_equal_titles_are_unclear() {
        let saved = [hidden("DISPLAY1", 1, fp(EDGE, WIN, "Mail")), hidden("DISPLAY1", 3, fp(EDGE, WIN, "Mail"))];
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, "Mail"), 2), Pick::Unclear);
    }

    // Apps add or drop a suffix across a restart ("Notes" vs "Notes - Notepad").
    #[test]
    fn containment_wins() {
        let saved = [hidden("DISPLAY1", 1, fp(EDGE, WIN, "Mail")), hidden("DISPLAY1", 3, fp(EDGE, WIN, "Project docs"))];
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, "Project docs - Edge"), 2), Pick::Take(1));
    }

    // A three letter title like "Mai" appears inside too many others to mean anything.
    #[test]
    fn containment_under_4_characters_does_not_count() {
        let saved = [hidden("DISPLAY1", 1, fp(EDGE, WIN, "Mai")), hidden("DISPLAY1", 3, fp(EDGE, WIN, "Docs"))];
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, "Mail inbox"), 2), Pick::Unclear);
    }

    #[test]
    fn two_containment_hits_are_unclear() {
        let saved = [hidden("DISPLAY1", 1, fp(EDGE, WIN, "Report draft")), hidden("DISPLAY1", 3, fp(EDGE, WIN, "Report final"))];
        assert_eq!(pick(&saved, &[0, 1], &fp(EDGE, WIN, "Report"), 2), Pick::Unclear);
    }

    // Same program, different kind of window (a main window vs a tool palette).
    #[test]
    fn a_different_class_never_matches() {
        let saved = [hidden("DISPLAY1", 1, fp(EDGE, WIN, "Mail"))];
        assert_eq!(pick(&saved, &[0], &fp(EDGE, "Other", "Mail"), 2), Pick::NoMatch);
    }

    // Pass 1 runs before any guess so a guess cannot steal an entry another window owns by title.
    #[test]
    fn pass_1_never_takes_a_title_less_match() {
        let saved = [hidden("DISPLAY1", 2, fp(EDGE, WIN, "Inbox"))];
        assert_eq!(pick(&saved, &[0], &fp(EDGE, WIN, "New tab"), 1), Pick::Unclear);
        assert_eq!(pick(&saved, &[0], &fp(EDGE, WIN, "inbox"), 1), Pick::Take(0));
    }

    // A process des-k could not open has no app; matching on class and title alone is a guess.
    #[test]
    fn an_empty_app_never_matches() {
        let saved = [hidden("DISPLAY1", 2, fp("", WIN, "Inbox"))];
        assert_eq!(pick(&saved, &[0], &fp("", WIN, "Inbox"), 2), Pick::NoMatch);
    }

    fn edid(maker: [u8; 2], product: [u8; 2], number: u32, text: Option<&str>) -> Vec<u8> {
        let mut e = vec![0u8; 128];
        e[8..10].copy_from_slice(&maker);
        e[10..12].copy_from_slice(&product);
        e[12..16].copy_from_slice(&number.to_le_bytes());
        if let Some(t) = text {
            e[72..77].copy_from_slice(&[0, 0, 0, 0xFF, 0]);
            let mut d = t.as_bytes().to_vec();
            d.push(0x0A);
            d.resize(13, b' ');
            e[77..90].copy_from_slice(&d);
        }
        e
    }
    const DEL: [u8; 2] = [0x10, 0xAC]; // "DEL", as Dell writes it

    // The same monitor must get the same id on any port and after a reboot, and the label's
    // serial is what tells two of one model apart (spec 0016 AC-1).
    #[test]
    fn a_text_serial_names_the_monitor() {
        assert_eq!(id_from_edid(&edid(DEL, [0x23, 0x41], 12345, Some("ABC123"))).as_deref(), Some("DEL-4123-ABC123"));
    }

    #[test]
    fn the_number_serves_when_there_is_no_text_serial() {
        assert_eq!(id_from_edid(&edid(DEL, [0x23, 0x41], 808464432, None)).as_deref(), Some("DEL-4123-808464432"));
    }

    // Without any serial the id is maker and model only: twins are told apart by ws's #1, #2.
    #[test]
    fn no_serial_gives_maker_and_product_only() {
        assert_eq!(id_from_edid(&edid(DEL, [0x23, 0x41], 0, None)).as_deref(), Some("DEL-4123"));
    }

    // A short or missing EDID must fall back to the instance id, never an id made of zeros.
    #[test]
    fn a_short_edid_gives_no_id() {
        assert_eq!(id_from_edid(&[0u8; 64]), None);
    }
}
