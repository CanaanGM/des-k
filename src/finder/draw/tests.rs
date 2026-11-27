// The drawing tests: how the place column cuts a workspace name and a screen.
use super::*;
use crate::ui::{LIGHT, fonts::Fonts};

// Spec 0023 AC-8: the place column cuts the screen first, keeping a twin's " #n", drops it
// before the name is cut, and never cuts " · unplugged"; one screen shows the name alone.
#[test]
fn a_place_keeps_the_name_whole_while_it_fits_alone() {
    let fonts = Fonts::new(1.0);
    let dc = unsafe { GetDC(None) };
    let c = Canvas::new(dc, false, 1.0, LIGHT, &fonts);
    let fits = |(a, b): &(String, String)| c.width(a, F::Small) + c.width(b, F::Small) <= SCREEN_W;
    let place = |name, screen, unplugged| named_place(&c, name, screen, unplugged, SCREEN_W);
    assert_eq!(place("Mail", Some("DELL U2720Q"), false), ("Mail".into(), " · DELL U2720Q".into()));
    assert_eq!(place("Mail", None, false), ("Mail".into(), String::new()));
    let twin = place("Mail", Some("LG ULTRAGEAR 27GR95QE OLED Gaming Monitor #2"), false);
    assert!(twin.0 == "Mail" && twin.1.starts_with(" · LG ULTRA") && twin.1.ends_with("… #2") && fits(&twin), "{twin:?}");
    let dropped = place("Reading list", Some("Built-in display"), true);
    assert_eq!(dropped, ("Reading list".into(), " · unplugged".into()));
    let cut = place("Reading list and papers to file", Some("Built-in display"), true);
    assert!(cut.0.starts_with("Reading list") && cut.0.ends_with('…') && cut.1 == " · unplugged" && fits(&cut), "{cut:?}");
    let alone = place("Quarterly planning and budget 26 and more", None, false);
    assert!(alone.0.ends_with('…') && alone.1.is_empty() && fits(&alone), "{alone:?}");
    unsafe { ReleaseDC(None, dc) };
}
