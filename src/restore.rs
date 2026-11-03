// Restore after a reboot or a new sign in (docs/specs/0003-restore-layout-after-reboot/index.md):
// window fingerprints, the current logon session, and the rule that picks which saved
// entry a reopened window belongs to. A wrong pick hides a window on the wrong
// workspace, so every doubt leaves the window visible.
use windows::Win32::{
    Foundation::{CloseHandle, HANDLE, HWND, LUID},
    Security::{
        Authentication::Identity::{LsaFreeReturnBuffer, LsaGetLogonSessionData, SECURITY_LOGON_SESSION_DATA},
        GetTokenInformation, TOKEN_QUERY, TOKEN_STATISTICS, TokenStatistics,
    },
    Storage::EnhancedStorage::PKEY_AppUserModel_ID,
    System::{
        Com::{CoTaskMemFree, StructuredStorage::{PropVariantClear, PropVariantToStringAlloc}},
        Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
    },
    UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow},
};
use windows::core::PWSTR;

/// What identifies a window across a reboot, where its handle and pid are new.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Fp {
    pub app: String,
    pub class: String,
    pub title: String,
}

/// A saved window: `ws` is `Some` for a hidden one (`W` line), `None` for a visible one (`V`),
/// or a pinned one (`P`, `pin` set: in view on every workspace, spec 0014).
#[derive(Clone, PartialEq, Debug)]
pub struct Entry {
    pub dev: String,
    pub ws: Option<usize>,
    pub pin: bool,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(app: &str, class: &str, title: &str) -> Fp {
        Fp { app: app.into(), class: class.into(), title: title.into() }
    }
    fn hidden(dev: &str, ws: usize, f: Fp) -> Entry {
        Entry { dev: dev.into(), ws: Some(ws), pin: false, fp: f }
    }
    fn visible(f: Fp) -> Entry {
        Entry { dev: "DISPLAY1".into(), ws: None, pin: false, fp: f }
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
}
