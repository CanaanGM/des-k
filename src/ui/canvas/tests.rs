// The Canvas tests: how a name is cut to fit.
use super::*;

// Spec 0023 handoff fitName: a name never ends in a stub of 3 letters or fewer, a cut that
// falls between words stays as fit makes it, and a name with no room is dropped, not "…".
#[test]
fn a_cut_name_never_ends_in_a_stub() {
    let fonts = Fonts::new(1.0);
    let dc = unsafe { GetDC(None) };
    let c = Canvas::new(dc, false, 1.0, LIGHT, &fonts);
    let name = "Research and reading";
    assert_eq!(c.fit(name, F::Small, c.width("Research and r…", F::Small)), "Research and r…");
    assert_eq!(c.fit_name(name, F::Small, c.width("Research and r…", F::Small)), "Research and…");
    assert_eq!(c.fit_name(name, F::Small, c.width("Research and read…", F::Small)), "Research and read…");
    assert_eq!(c.fit_name("Mail and more", F::Small, c.width("Mail and…", F::Small)), "Mail and…");
    assert_eq!(c.fit_name(name, F::Small, 1.0), "");
    assert_eq!(c.fit_name(name, F::Small, 400.0), name);
    unsafe { ReleaseDC(None, dc) };
}
