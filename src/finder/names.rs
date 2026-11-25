// The names and icons the finder shows: an app's name from its exe's description or its Store
// package, read once per exe, and each window's icon, asked for once.
use super::*;

thread_local! {
    // Window -> its icon, asked for once (see `icon`). Borrowed handles, never destroyed.
    static ICONS: RefCell<HashMap<isize, HICON>> = RefCell::default();
    // Exe path -> the app's name, read once while des-k runs (AC-FND-4).
    static NAMES: RefCell<HashMap<String, Option<String>>> = RefCell::default();
}

fn stem(exe: &str) -> String {
    Path::new(exe).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned())
}

/// The app's name (AC-FND-4): its exe's file description, else the exe name without .exe. A
/// Store app's frame names the hosted app's package, else shows the window title. Read once per
/// exe while des-k runs.
pub(crate) fn program(hwnd: HWND) -> String {
    let (name, frame) = app(hwnd);
    name.unwrap_or_else(|| if frame { ws::title(hwnd) } else { String::new() })
}

/// `program` without the title: for words des-k shows unasked, where a title could carry a
/// message's text (spec 0022 AC-9). None when no name can be read.
pub(crate) fn app_name(hwnd: HWND) -> Option<String> {
    app(hwnd).0.filter(|n| !n.trim().is_empty())
}

/// The app's name if one can be read, and whether `hwnd` is a Store app's frame.
fn app(hwnd: HWND) -> (Option<String>, bool) {
    let exe = |h: HWND| {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        restore::exe_path(pid).map(|p| (p, pid))
    };
    let Some((path, _)) = exe(hwnd) else { return (None, false) };
    if stem(&path).eq_ignore_ascii_case("ApplicationFrameHost") {
        // The hosted app lives in a CoreWindow child while it runs (not while suspended).
        let core = unsafe { FindWindowExW(Some(hwnd), None, w!("Windows.UI.Core.CoreWindow"), None) }.ok().and_then(exe);
        return (core.and_then(|(path, pid)| named(&path, || package_name(pid))), true);
    }
    (named(&path, || Some(describe(&path).unwrap_or_else(|| stem(&path)))), false)
}

fn named(exe: &str, find: impl FnOnce() -> Option<String>) -> Option<String> {
    if let Some(n) = NAMES.with_borrow(|m| m.get(exe).cloned()) {
        return n;
    }
    let n = find();
    NAMES.with_borrow_mut(|m| m.insert(exe.to_string(), n.clone()));
    n
}

/// The FileDescription in an exe's version info: in its own language first, then US English.
fn describe(exe: &str) -> Option<String> {
    let path = HSTRING::from(exe);
    unsafe {
        let size = GetFileVersionInfoSizeW(&path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(&path, None, size, data.as_mut_ptr().cast()).ok()?;
        let query = |sub: &str| {
            let (mut p, mut len) = (std::ptr::null_mut(), 0u32);
            VerQueryValueW(data.as_ptr().cast(), &HSTRING::from(sub), &mut p, &mut len).as_bool().then_some((p as *const u16, len as usize))
        };
        let mut langs: Vec<(u16, u16)> = match query(r"\VarFileInfo\Translation") {
            Some((p, bytes)) => std::slice::from_raw_parts(p, bytes / 2).as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect(),
            None => vec![],
        };
        langs.push((0x0409, 0x04B0));
        langs.into_iter().find_map(|(lang, cp)| {
            let (p, chars) = query(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\FileDescription"))?;
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, chars));
            let s = no_exe(s.trim_end_matches('\0').trim());
            (!s.is_empty()).then(|| s.to_string())
        })
    }
}

/// A description that is only a file name (Windows 11's Notepad says `Notepad.exe`) without its `.exe`.
fn no_exe(s: &str) -> &str {
    match s.len().checked_sub(4).and_then(|i| s.get(i..).map(|e| (i, e))) {
        Some((i, e)) if e.eq_ignore_ascii_case(".exe") => &s[..i],
        _ => s,
    }
}

/// A Store app's name, as its package gives it.
fn package_name(pid: u32) -> Option<String> {
    let full = unsafe {
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 256];
        let mut len = buf.len() as u32;
        let e = GetPackageFullName(proc, &mut len, Some(PWSTR(buf.as_mut_ptr())));
        let _ = CloseHandle(proc);
        if e != ERROR_SUCCESS {
            return None;
        }
        String::from_utf16_lossy(&buf[..(len as usize).saturating_sub(1)])
    };
    display_name(&full)
}

/// A Store app's name from its app id (`family!app`), for when its window can't say: a hidden
/// Store app is suspended and has no CoreWindow to ask (spec 0027 AC-13).
pub(crate) fn store_name(app_id: &str) -> Option<String> {
    let (family, _) = app_id.split_once('!')?;
    named(app_id, || {
        let family = HSTRING::from(family);
        let (mut count, mut len) = (0u32, 0u32);
        // The first call only sizes the lists.
        let _ = unsafe { FindPackagesByPackageFamily(&family, PACKAGE_FILTER_HEAD, &mut count, None, &mut len, None, None) };
        if count == 0 {
            return None;
        }
        let mut names = vec![PWSTR::null(); count as usize];
        let mut buf = vec![0u16; len as usize];
        let e = unsafe { FindPackagesByPackageFamily(&family, PACKAGE_FILTER_HEAD, &mut count, Some(names.as_mut_ptr()), &mut len, Some(PWSTR(buf.as_mut_ptr())), None) };
        if e != ERROR_SUCCESS {
            return None;
        }
        let full = unsafe { names.first()?.to_string() }.ok()?;
        display_name(&full)
    })
}

/// A package's display name, by its full name.
fn display_name(full: &str) -> Option<String> {
    // An empty user means the user des-k runs as.
    let name = PackageManager::new().and_then(|m| m.FindPackageByUserSecurityIdPackageFullName(&HSTRING::new(), &HSTRING::from(full))).and_then(|p| p.DisplayName());
    match name {
        Ok(n) if !n.is_empty() => Some(n.to_string()),
        Ok(_) => None,
        Err(e) => {
            warn!("finder: no name for the package {full}: {e}");
            None
        }
    }
}

/// A window's icon, asked for once while des-k runs: WM_GETICON waits on the app (a busy one
/// can take the whole ICON_WAIT_MS), and the finder and the overview want every window's
/// icon each time they open. The app's or the class's icon: borrowed, never destroyed.
pub(crate) fn icon(hwnd: HWND) -> HICON {
    if let Some(i) = ICONS.with_borrow(|c| c.get(&(hwnd.0 as isize)).copied()) {
        return i;
    }
    let i = ask_icon(hwnd);
    ICONS.with_borrow_mut(|c| {
        // A window new to the cache is the time to forget the ones that closed.
        c.retain(|&h, _| unsafe { IsWindow(Some(HWND(h as *mut _))) }.as_bool());
        c.insert(hwnd.0 as isize, i);
    });
    i
}

fn ask_icon(hwnd: HWND) -> HICON {
    for kind in [ICON_SMALL2, ICON_SMALL, ICON_BIG] {
        let mut out = 0usize;
        let ok = unsafe { SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(kind as usize), LPARAM(0), SMTO_ABORTIFHUNG, ICON_WAIT_MS, Some(&mut out)) };
        if ok.0 == 0 {
            break; // hung or slow: don't wait on it twice more
        }
        if out != 0 {
            return HICON(out as *mut _);
        }
    }
    for index in [GCLP_HICONSM, GCLP_HICON] {
        let h = unsafe { GetClassLongPtrW(hwnd, index) };
        if h != 0 {
            return HICON(h as *mut _);
        }
    }
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}


#[cfg(test)]
mod tests;
