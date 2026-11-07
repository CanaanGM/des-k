// What Narrator hears from des-k's own windows (spec 0017 AC-5): a small UI Automation provider,
// the only place des-k implements a COM interface (raw vtable calls stay in cloak.rs). A
// provider holds plain data copied from what its window painted: UIA calls it whenever the
// window's thread pumps messages, also inside a sent message while main holds the ws lock, so a
// provider never borrows a window's state, locks ws, writes settings or sends a message.
//
// Two shapes: `Text`, a window whose whole content is one piece of text (the indicator), and a
// `Tree` of `Node`s the window keeps in step with its paint (the finder's count, rows and note).
use std::sync::{Arc, Mutex};
use windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::ClientToScreen,
    System::{
        Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, SAFEARRAY},
        Ole::{SafeArrayCreateVector, SafeArrayPutElement},
        Variant::{VARIANT, VT_I4, VT_UNKNOWN},
    },
    UI::{
        Accessibility::*,
        WindowsAndMessaging::{CHILDID_SELF, DefWindowProcW, OBJID_CLIENT, WM_GETOBJECT},
    },
};
use windows::core::{BOOL, BSTR, Error, HRESULT, HSTRING, IUnknown, IUnknownImpl, Interface, Result, implement};

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

/// Whether Narrator or another screen reader listens: words cost nothing to make until one does.
pub(crate) fn listening() -> bool {
    unsafe { UiaClientsAreListening() }.as_bool()
}

/// Narrator reads `text` now, without the window taking focus; `queue` lets it finish what it
/// was saying first, otherwise the newest words replace any not yet read. `who` names the
/// window to Narrator. Nothing is made while no screen reader listens.
pub(crate) fn notify(h: HWND, text: &str, who: &str, queue: bool) {
    if !unsafe { UiaClientsAreListening() }.as_bool() {
        return;
    }
    let p: IRawElementProviderSimple = Text { hwnd: h, name: text.to_string() }.into();
    let how = if queue { NotificationProcessing_All } else { NotificationProcessing_MostRecent };
    let shown = unsafe { UiaRaiseNotificationEvent(&p, NotificationKind_Other, how, &BSTR::from(text), &BSTR::from(who)) };
    if let Err(e) = shown {
        eprintln!("narrator: {who}'s words were not sent ({e})");
    }
}

/// WM_GETOBJECT for a window whose whole content is `text`.
pub(crate) fn answer(h: HWND, wp: WPARAM, lp: LPARAM, text: String) -> LRESULT {
    if lp.0 as i32 != UiaRootObjectId {
        return unsafe { DefWindowProcW(h, WM_GETOBJECT, wp, lp) };
    }
    let p: IRawElementProviderSimple = Text { hwnd: h, name: text }.into();
    unsafe { UiaReturnRawElementProvider(h, wp, lp, &p) }
}

/// One piece of a window as it was painted: its role, its name (the painted words), where it
/// sits in client pixels, and for a list item whether it is the chosen one.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Node {
    pub(crate) role: UIA_CONTROLTYPE_ID,
    pub(crate) name: String,
    pub(crate) rect: RECT,
    pub(crate) selected: Option<bool>, // Some: a list item
    pub(crate) kids: Vec<Node>,
}

/// A window's tree: the root is the window itself. The window replaces it after each paint;
/// a provider only ever `try_lock`s it, so a call that comes in while the window is changing
/// it gets "not available" instead of waiting.
pub(crate) type Tree = Arc<Mutex<Node>>;

fn gone() -> Error {
    Error::from(HRESULT(UIA_E_ELEMENTNOTAVAILABLE as i32))
}

/// The node at `path` (child indexes from the root), read under a short try_lock.
fn at<R>(tree: &Tree, path: &[usize], f: impl FnOnce(&Node) -> R) -> Result<R> {
    let root = tree.try_lock().map_err(|_| gone())?;
    let mut n = &*root;
    for &i in path {
        n = n.kids.get(i).ok_or_else(gone)?;
    }
    Ok(f(n))
}

fn make(hwnd: HWND, tree: &Tree, path: Vec<usize>) -> IRawElementProviderFragment {
    if path.is_empty() { Root { hwnd, tree: tree.clone() }.into() } else { Elem { hwnd, tree: tree.clone(), path }.into() }
}

/// The deepest node under a client point, as a path.
fn hit(n: &Node, x: i32, y: i32, path: &mut Vec<usize>) {
    for (i, k) in n.kids.iter().enumerate() {
        let r = k.rect;
        if x >= r.left && x < r.right && y >= r.top && y < r.bottom {
            path.push(i);
            return hit(k, x, y, path);
        }
    }
}

fn property(n: &Node, id: UIA_PROPERTY_ID) -> VARIANT {
    if id == UIA_NamePropertyId {
        VARIANT::from(BSTR::from(n.name.as_str()))
    } else if id == UIA_ControlTypePropertyId {
        VARIANT::from(n.role.0)
    } else if id == UIA_IsKeyboardFocusablePropertyId {
        VARIANT::from(false) // focus stays in the window's own text box
    } else {
        VARIANT::default()
    }
}

/// The window itself, hosted by its HWND (UIA adds the HWND's own properties and children).
#[implement(IRawElementProviderSimple, IRawElementProviderFragment, IRawElementProviderFragmentRoot)]
struct Root {
    hwnd: HWND,
    tree: Tree,
}

impl IRawElementProviderSimple_Impl for Root_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, _: UIA_PATTERN_ID) -> Result<IUnknown> {
        Err(Error::empty())
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        at(&self.tree, &[], |n| property(n, id))
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        unsafe { UiaHostProviderFromHwnd(self.hwnd) }
    }
}

impl IRawElementProviderFragment_Impl for Root_Impl {
    fn Navigate(&self, d: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let kids = at(&self.tree, &[], |n| n.kids.len())?;
        // The HWND's host answers for parent and siblings.
        let to = if kids == 0 {
            return Err(Error::empty());
        } else if d == NavigateDirection_FirstChild {
            0
        } else if d == NavigateDirection_LastChild {
            kids - 1
        } else {
            return Err(Error::empty());
        };
        Ok(make(self.hwnd, &self.tree, vec![to]))
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut()) // the host HWND's
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        Ok(UiaRect::default()) // the host HWND's
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(Root { hwnd: self.hwnd, tree: self.tree.clone() }.into())
    }
}

impl IRawElementProviderFragmentRoot_Impl for Root_Impl {
    fn ElementProviderFromPoint(&self, x: f64, y: f64) -> Result<IRawElementProviderFragment> {
        let mut p = POINT::default();
        unsafe { let _ = ClientToScreen(self.hwnd, &mut p); }
        let mut path = vec![];
        at(&self.tree, &[], |n| hit(n, x as i32 - p.x, y as i32 - p.y, &mut path))?;
        Ok(make(self.hwnd, &self.tree, path))
    }

    fn GetFocus(&self) -> Result<IRawElementProviderFragment> {
        Err(Error::empty()) // focus is in the native text box, which UIA knows itself
    }
}

/// A node below the root, found again by its path on every call.
#[implement(IRawElementProviderSimple, IRawElementProviderFragment, ISelectionItemProvider, ISelectionProvider)]
struct Elem {
    hwnd: HWND,
    tree: Tree,
    path: Vec<usize>,
}

impl Elem_Impl {
    fn parent(&self) -> IRawElementProviderFragment {
        make(self.hwnd, &self.tree, self.path[..self.path.len() - 1].to_vec())
    }
}

impl IRawElementProviderSimple_Impl for Elem_Impl {
    fn ProviderOptions(&self) -> Result<ProviderOptions> {
        Ok(ProviderOptions_ServerSideProvider)
    }

    fn GetPatternProvider(&self, id: UIA_PATTERN_ID) -> Result<IUnknown> {
        let (item, list) = at(&self.tree, &self.path, |n| (n.selected.is_some(), n.kids.iter().any(|k| k.selected.is_some())))?;
        let me: IRawElementProviderSimple = self.to_interface();
        if (id == UIA_SelectionItemPatternId && item) || (id == UIA_SelectionPatternId && list) { me.cast() } else { Err(Error::empty()) }
    }

    fn GetPropertyValue(&self, id: UIA_PROPERTY_ID) -> Result<VARIANT> {
        at(&self.tree, &self.path, |n| property(n, id))
    }

    fn HostRawElementProvider(&self) -> Result<IRawElementProviderSimple> {
        Err(Error::empty())
    }
}

impl IRawElementProviderFragment_Impl for Elem_Impl {
    fn Navigate(&self, d: NavigateDirection) -> Result<IRawElementProviderFragment> {
        let (up, last) = self.path.split_at(self.path.len() - 1);
        let siblings = at(&self.tree, up, |n| n.kids.len())?;
        let kids = at(&self.tree, &self.path, |n| n.kids.len())?;
        let with = |i: usize| [up, &[i]].concat();
        let down = |i: usize| [&self.path[..], &[i]].concat();
        // UIA's constants are not upper case, so they can't be match patterns without a warning.
        let to = if d == NavigateDirection_Parent {
            return Ok(self.parent());
        } else if d == NavigateDirection_NextSibling && last[0] + 1 < siblings {
            with(last[0] + 1)
        } else if d == NavigateDirection_PreviousSibling && last[0] > 0 {
            with(last[0] - 1)
        } else if d == NavigateDirection_FirstChild && kids > 0 {
            down(0)
        } else if d == NavigateDirection_LastChild && kids > 0 {
            down(kids - 1)
        } else {
            return Err(Error::empty());
        };
        Ok(make(self.hwnd, &self.tree, to))
    }

    fn GetRuntimeId(&self) -> Result<*mut SAFEARRAY> {
        // Unique within the window: UiaAppendRuntimeId, then the path (1 based, never 0).
        let ids: Vec<i32> = std::iter::once(UiaAppendRuntimeId as i32).chain(self.path.iter().map(|&i| i as i32 + 1)).collect();
        unsafe {
            let a = SafeArrayCreateVector(VT_I4, 0, ids.len() as u32);
            for (i, v) in ids.iter().enumerate() {
                SafeArrayPutElement(a, &(i as i32), (v as *const i32).cast())?;
            }
            Ok(a)
        }
    }

    fn BoundingRectangle(&self) -> Result<UiaRect> {
        let r = at(&self.tree, &self.path, |n| n.rect)?;
        let mut p = POINT { x: r.left, y: r.top };
        unsafe { let _ = ClientToScreen(self.hwnd, &mut p); }
        Ok(UiaRect { left: p.x as f64, top: p.y as f64, width: (r.right - r.left) as f64, height: (r.bottom - r.top) as f64 })
    }

    fn GetEmbeddedFragmentRoots(&self) -> Result<*mut SAFEARRAY> {
        Ok(std::ptr::null_mut())
    }

    fn SetFocus(&self) -> Result<()> {
        Ok(())
    }

    fn FragmentRoot(&self) -> Result<IRawElementProviderFragmentRoot> {
        Ok(Root { hwnd: self.hwnd, tree: self.tree.clone() }.into())
    }
}

/// The chosen row is the window's to change (its own keys and clicks); UIA can only read it.
impl ISelectionItemProvider_Impl for Elem_Impl {
    fn Select(&self) -> Result<()> {
        Err(Error::from(HRESULT(UIA_E_INVALIDOPERATION as i32)))
    }

    fn AddToSelection(&self) -> Result<()> {
        Err(Error::from(HRESULT(UIA_E_INVALIDOPERATION as i32)))
    }

    fn RemoveFromSelection(&self) -> Result<()> {
        Err(Error::from(HRESULT(UIA_E_INVALIDOPERATION as i32)))
    }

    fn IsSelected(&self) -> Result<BOOL> {
        at(&self.tree, &self.path, |n| BOOL::from(n.selected == Some(true)))
    }

    fn SelectionContainer(&self) -> Result<IRawElementProviderSimple> {
        self.parent().cast()
    }
}

impl ISelectionProvider_Impl for Elem_Impl {
    fn GetSelection(&self) -> Result<*mut SAFEARRAY> {
        let chosen = at(&self.tree, &self.path, |n| n.kids.iter().position(|k| k.selected == Some(true)))?;
        unsafe {
            let a = SafeArrayCreateVector(VT_UNKNOWN, 0, chosen.is_some() as u32);
            if let Some(i) = chosen {
                let p: IUnknown = make(self.hwnd, &self.tree, [&self.path[..], &[i]].concat()).cast()?;
                SafeArrayPutElement(a, &0, p.as_raw())?; // it takes its own reference
            }
            Ok(a)
        }
    }

    fn CanSelectMultiple(&self) -> Result<BOOL> {
        Ok(BOOL(0))
    }

    fn IsSelectionRequired(&self) -> Result<BOOL> {
        Ok(BOOL(0))
    }
}

/// WM_GETOBJECT for a window described by `tree`.
pub(crate) fn answer_tree(h: HWND, wp: WPARAM, lp: LPARAM, tree: &Tree) -> LRESULT {
    if lp.0 as i32 != UiaRootObjectId {
        return unsafe { DefWindowProcW(h, WM_GETOBJECT, wp, lp) };
    }
    let p: IRawElementProviderSimple = Root { hwnd: h, tree: tree.clone() }.into();
    unsafe { UiaReturnRawElementProvider(h, wp, lp, &p) }
}

/// Gives a native control with no label of its own a name Narrator reads (a text box whose
/// placeholder des-k paints itself).
pub(crate) fn name_control(h: HWND, name: &str) {
    let named = unsafe {
        CoCreateInstance::<_, IAccPropServices>(&CLSID_AccPropServices, None, CLSCTX_INPROC_SERVER)
            .and_then(|s| s.SetHwndPropStr(h, OBJID_CLIENT.0 as u32, CHILDID_SELF, PROPID_ACC_NAME, &HSTRING::from(name)))
    };
    if let Err(e) = named {
        eprintln!("narrator: could not name the text box \"{name}\" ({e})");
    }
}
