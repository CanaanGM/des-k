// What Narrator hears from des-k's own windows (spec 0017 AC-5): a small UI Automation provider,
// the only place des-k implements a COM interface (raw vtable calls stay in cloak.rs). A
// provider holds plain data copied from what its window painted: UIA calls it whenever the
// window's thread pumps messages, also inside a sent message while main holds the ws lock, so a
// provider never borrows a window's state, locks ws, writes settings or sends a message.
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::Variant::VARIANT,
    UI::{Accessibility::*, WindowsAndMessaging::DefWindowProcW},
};
use windows::core::{BSTR, Error, IUnknown, Result, implement};

/// A window whose whole content is one piece of text, like the indicator's card.
#[implement(IRawElementProviderSimple)]
struct Text {
    hwnd: HWND,
    name: String,
}

impl IRawElementProviderSimple_Impl for Text_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _: UIA_PATTERN_ID) -> Result<IUnknown> {
        Err(Error::empty()) // none: S_OK with no provider
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        Ok(if id == UIA_NamePropertyId {
            VARIANT::from(BSTR::from(self.name.as_str()))
        } else if id == UIA_ControlTypePropertyId {
            VARIANT::from(UIA_TextControlTypeId.0)
        } else if id == UIA_LiveSettingPropertyId {
            VARIANT::from(Polite.0)
        } else if id == UIA_IsKeyboardFocusablePropertyId {
            VARIANT::from(false)
        } else {
            VARIANT::default() // UIA asks the window for the rest
        })
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        unsafe { UiaHostProviderFromHwnd(self.hwnd) }
    }
}

/// Narrator reads `text` now, without the window taking focus. Nothing is made while no
/// screen reader listens.
pub(crate) fn notify(h: HWND, text: &str) {
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    let p: IRawElementProviderSimple = Text { hwnd: h, name: text.to_string() }.into();
    let shown = unsafe { UiaRaiseNotificationEvent(&p, NotificationKind_Other, NotificationProcessing_MostRecent, &BSTR::from(text), &BSTR::from("des-k indicator")) };
    if let Err(e) = shown {
        eprintln!("narrator: the indicator's words were not sent ({e})");
    }
}

/// WM_GETOBJECT for a window whose whole content is `text`.
pub(crate) fn answer(h: HWND, wp: WPARAM, lp: LPARAM, text: String) -> LRESULT {
    if lp.0 as i32 != UiaRootObjectId {
        return unsafe { DefWindowProcW(h, windows::Win32::UI::WindowsAndMessaging::WM_GETOBJECT, wp, lp) };
    }
    let p: IRawElementProviderSimple = Text { hwnd: h, name: text }.into();
    unsafe { UiaReturnRawElementProvider(h, wp, lp, &p) }
}
