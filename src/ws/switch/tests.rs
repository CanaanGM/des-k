// Tests for switching: steps and jumps, the screen an action hits, follow's verdicts, and the
// hide and focus plan.
use super::*;
use crate::ws::tests::{with_hidden, hwnd};

// A window that fails to show is never dropped: it may still be cloaked, and only a list
// lets a later switch, quit or panic show it. A closed one goes. Shown windows keep their
// top first order, so focus lands on the old top one.
#[test]
fn a_window_that_fails_to_show_stays_tracked() {
    let (shown, stuck) = show_all(vec![1, 2, 3, 4], |raw| match raw {
        2 => Some(false),
        3 => None,
        _ => Some(true),
    });
    assert_eq!(shown, vec![1, 4]);
    assert_eq!(stuck, vec![2]);
}

// Win+Ctrl+arrows cycle: past the last workspace you land on the first, and before the first
// on the last, so either key reaches every workspace.
#[test]
fn a_step_wraps_both_ways() {
    assert_eq!(step_target(3, 1, 4), 0);
    assert_eq!(step_target(0, -1, 4), 3);
    assert_eq!(step_target(1, 1, 4), 2);
    assert_eq!(step_target(2, -1, 4), 1);
}

// Spec 0038 AC-5 to AC-8: which screen an action hits must be predictable. Swipes and the
// mouse mode never look past the mouse (so the mouse mode is today's des-k); the focus mode
// takes the focused window's screen, and with nothing focused the screen des-k last acted on,
// so a run of presses through empty workspaces stays on one screen.
#[test]
fn the_screen_rule_follows_the_source_and_the_setting() {
    use settings::ActOn::{Focus, Mouse};
    let never = || -> Option<&str> { panic!("the mouse mode and swipes read only the mouse") };
    let rule = |from, act_on, focus: Option<&'static str>, last: Option<&'static str>| pick(from, act_on, || focus, || last, || "mouse");
    assert_eq!(pick(Source::Keys, Mouse, never, never, || "mouse"), "mouse");
    assert_eq!(pick(Source::Pointer, Focus, never, never, || "mouse"), "mouse");
    assert_eq!(pick(Source::Pointer, Mouse, never, never, || "mouse"), "mouse");
    assert_eq!(rule(Source::Keys, Focus, Some("B"), Some("C")), "B", "focus beats the last screen");
    assert_eq!(rule(Source::Keys, Focus, None, Some("C")), "C", "nothing focused: the last screen");
    assert_eq!(rule(Source::Keys, Focus, None, None), "mouse", "no last screen present: the mouse");
}

// With one workspace a step lands where it started, so switch only shows the indicator and
// hides nothing.
#[test]
fn a_step_with_one_workspace_stays_put() {
    assert_eq!(step_target(0, 1, 1), 0);
    assert_eq!(step_target(0, -1, 1), 0);
}

fn shown_on(active: usize, last: Option<usize>) -> Monitor {
    let mut m = Monitor::with(4);
    m.active = active;
    m.last_switch = last.map(|ws| (Instant::now(), ws));
    m
}

// A digit past your workspace count must not switch anywhere: the indicator says there is no
// such workspace, whatever jump_back says.
#[test]
fn a_jump_past_the_count_is_missing() {
    let m = shown_on(1, Some(0));
    assert_eq!(jump_target(&m, 4, 4, true), Jump::Missing);
    assert_eq!(jump_target(&m, 8, 4, false), Jump::Missing);
}

// Pressing the shown workspace's digit is a back key only when you turned jump_back on and
// there is a last workspace; otherwise it just shows where you are (spec 0024 AC-8).
#[test]
fn a_jump_to_the_shown_workspace_goes_back_only_with_jump_back_on() {
    let m = shown_on(2, Some(0));
    assert_eq!(jump_target(&m, 2, 4, true), Jump::Back(0));
    assert_eq!(jump_target(&m, 2, 4, false), Jump::Stay);
    assert_eq!(jump_target(&shown_on(2, None), 2, 4, true), Jump::Stay);
}

// Any other digit goes to its workspace, even with a back target waiting: jump_back changes
// only the shown workspace's digit.
#[test]
fn a_jump_elsewhere_goes_there() {
    let m = shown_on(2, Some(0));
    assert_eq!(jump_target(&m, 3, 4, true), Jump::Go(3));
    assert_eq!(jump_target(&m, 0, 4, false), Jump::Go(0));
}

const WIN: isize = 100;

const OWNER: isize = 200;

/// A state where window 100 (owner 200) is hidden on workspace 3 of screen DEL-1, which shows 1.
fn hidden_window() -> State {
    let mut s = State::default();
    s.monitors.insert("DEL-1".into(), with_hidden(0, &[&[], &[], &[WIN], &[]]));
    s
}

fn verdict(s: &State, cloak: u32, found: bool, now: Instant) -> Follow {
    follow_verdict(s, [WIN, OWNER], "DEL-1", 2, cloak, found, now)
}

fn ruled(from: &str) -> Ruled {
    Ruled { at: Instant::now(), recloaks: 0, from: from.into() }
}

// The plain case: des-k hid it (cloak 2), nothing recent, its screen is here. Focus reaching it
// must bring its workspace, or the keyboard types into a window you cannot see (spec 0002).
#[test]
fn a_window_des_k_hid_is_followed() {
    assert_eq!(verdict(&hidden_window(), 2, true, Instant::now()), Follow::Go);
}

// A window an [apps] rule hid takes focus right away or much later: following it would undo the
// rule, so focus leaves it on the screen it opened on. Any cloak counts, since a new Chrome window
// reads 3; once shown (cloak 0) the rule no longer holds it (spec 0027 AC-5).
#[test]
fn a_window_a_rule_hid_is_not_followed() {
    let mut s = hidden_window();
    s.ruled.insert(OWNER, ruled("DEL-2"));
    let now = Instant::now();
    assert_eq!(verdict(&s, 3, true, now), Follow::RuleHid { from: "DEL-2".into() });
    assert_eq!(verdict(&s, 2, true, now), Follow::RuleHid { from: "DEL-2".into() });
    assert_eq!(verdict(&s, 0, true, now), Follow::NotOurs);
}

// Only cloak 2 is des-k's. Another cloak is Windows' own (a suspended app, another virtual
// desktop): switching for it would show a workspace for nothing.
#[test]
fn a_window_des_k_did_not_cloak_is_not_followed() {
    let s = hidden_window();
    for cloak in [0, 1, 4] {
        assert_eq!(verdict(&s, cloak, true, Instant::now()), Follow::NotOurs, "cloak {cloak}");
    }
}

// A reopened window often takes focus just before restore hides it; that focus is not you, so for
// 2 s it is ignored, and from 2 s on it counts again (spec 0003).
#[test]
fn a_window_restore_hid_is_ignored_for_two_seconds() {
    let mut s = hidden_window();
    let t = Instant::now();
    s.restored.insert(OWNER, t);
    assert_eq!(verdict(&s, 2, true, t + Duration::from_millis(1999)), Follow::JustRestored);
    assert_eq!(verdict(&s, 2, true, t + RESTORED_IGNORE), Follow::Go);
}

// Windows can hand focus back to the window a stay send just hid; following it would bring you
// to the workspace you just sent it away to. Only that window, and only for 1 s (spec 0004).
#[test]
fn a_window_just_sent_is_ignored_for_a_second() {
    let mut s = hidden_window();
    let t = Instant::now();
    s.sent = Some((WIN, t));
    assert_eq!(verdict(&s, 2, true, t + Duration::from_millis(999)), Follow::JustSent);
    assert_eq!(verdict(&s, 2, true, t + Duration::from_secs(1)), Follow::Go);
    s.sent = Some((300, t));
    assert_eq!(verdict(&s, 2, true, t), Follow::Go);
}

// A screen unplugged a moment ago has nothing to switch; the follow is skipped and logged.
#[test]
fn a_window_on_a_missing_screen_is_not_followed() {
    assert_eq!(verdict(&hidden_window(), 2, false, Instant::now()), Follow::NoScreen);
}

// Windows can hand focus to a window mid cloak; going straight back to the workspace you just
// left would undo your switch. Only that workspace, only for 1 s.
#[test]
fn the_workspace_just_left_is_not_followed_back_for_a_second() {
    let mut s = hidden_window();
    let t = Instant::now();
    s.monitors.get_mut("DEL-1").unwrap().last_switch = Some((t, 2));
    assert_eq!(verdict(&s, 2, true, t + Duration::from_millis(999)), Follow::JustLeft);
    assert_eq!(verdict(&s, 2, true, t + Duration::from_secs(1)), Follow::Go);
    s.monitors.get_mut("DEL-1").unwrap().last_switch = Some((t, 1));
    assert_eq!(verdict(&s, 2, true, t), Follow::Go);
}

// Two windows on two workspaces passing focus back and forth would switch the screen in a loop;
// after a follow the screen waits 1 s before it follows again.
#[test]
fn a_screen_cools_down_for_a_second_after_a_follow() {
    let mut s = hidden_window();
    let t = Instant::now();
    s.monitors.get_mut("DEL-1").unwrap().last_follow = Some(t);
    assert_eq!(verdict(&s, 2, true, t + Duration::from_millis(999)), Follow::Cooling);
    assert_eq!(verdict(&s, 2, true, t + Duration::from_secs(1)), Follow::Go);
}

// When several reasons hold, the earlier one wins, so the log names it and a rule's window still
// gets focus moved off it.
#[test]
fn the_first_reason_that_holds_wins() {
    let mut s = hidden_window();
    let t = Instant::now();
    s.restored.insert(WIN, t);
    s.sent = Some((WIN, t));
    s.monitors.get_mut("DEL-1").unwrap().last_follow = Some(t);
    assert_eq!(verdict(&s, 2, false, t), Follow::JustRestored);
    s.ruled.insert(WIN, ruled("DEL-1"));
    assert_eq!(verdict(&s, 2, false, t), Follow::RuleHid { from: "DEL-1".into() });
}

// A switch hides what is on the screen, top first, except the window a follow send brings along
// and pinned windows, which stay in view on every workspace (spec 0014).
#[test]
fn a_switch_hides_all_but_the_kept_and_pinned_windows() {
    let pinned = HashSet::from([3]);
    let hides = to_hide(false, || vec![hwnd(1), hwnd(2), hwnd(3), hwnd(4)], Some(hwnd(2)), &pinned);
    assert_eq!(hides, vec![hwnd(1), hwnd(4)]);
}

// On a Windows layout des-k does not know, hiding would fail halfway: a switch hides nothing and
// does not even read the windows (spec 0030 AC-5).
#[test]
fn a_switch_on_an_unknown_layout_hides_nothing() {
    let hides = to_hide(true, || panic!("windows read on an unknown layout"), None, &HashSet::new());
    assert!(hides.is_empty());
}

// Leaving an empty workspace leaves focus on the desktop, which is nobody's focus to keep: no
// window, the shell window, or explorer's desktop windows.
#[test]
fn the_desktop_is_no_window_the_shell_or_explorer() {
    let shell = hwnd(10);
    assert!(is_desktop(HWND::default(), shell, ""));
    assert!(is_desktop(shell, shell, "Progman"));
    assert!(is_desktop(hwnd(11), shell, "Progman"));
    assert!(is_desktop(hwnd(12), shell, "WorkerW"));
    assert!(!is_desktop(hwnd(13), shell, "Chrome_WidgetWin_1"));
}

// A follow asked for one window: it gets focus, brought to the front only when something else
// took it meanwhile.
#[test]
fn a_followed_window_keeps_or_gets_focus() {
    assert_eq!(focus_after(Some(hwnd(1)), hwnd(1), true, false, Some(hwnd(2))), FocusAfter::Leave);
    assert_eq!(focus_after(Some(hwnd(1)), hwnd(5), false, false, Some(hwnd(2))), FocusAfter::Bring(hwnd(1)));
}

// Keyboard focus must never stay in a window the switch just hid, or on the desktop of a
// workspace you left: the first window shown takes it, or the desktop when none was shown.
#[test]
fn focus_leaves_a_hidden_window_or_the_desktop() {
    assert_eq!(focus_after(None, hwnd(9), true, false, Some(hwnd(2))), FocusAfter::Next(hwnd(2)));
    assert_eq!(focus_after(None, hwnd(9), false, true, Some(hwnd(2))), FocusAfter::Next(hwnd(2)));
    assert_eq!(focus_after(None, hwnd(9), true, false, None), FocusAfter::Desktop);
}

// Focus on another screen's window is yours: a switch on this screen must not steal it.
#[test]
fn focus_elsewhere_stays_put() {
    assert_eq!(focus_after(None, hwnd(9), false, false, Some(hwnd(2))), FocusAfter::Leave);
}

// A hide can cloak the window and then fail its taskbar step: it is shown again and comes off
// its list, since nothing would track it hidden.
#[test]
fn a_half_failed_hide_is_undone_and_unlisted() {
    let cloak = std::cell::Cell::new(2);
    assert!(failed_hide(|| cloak.get(), || cloak.set(0)));
}

// A window that may still be cloaked is never dropped (AGENTS.md): when the undo fails too, it
// stays listed, so a later switch, quit or panic tries to show it again.
#[test]
fn a_window_still_cloaked_after_a_failed_hide_stays_listed() {
    let undone = std::cell::Cell::new(false);
    assert!(!failed_hide(|| 2, || undone.set(true)));
    assert!(undone.get());
}

// A hide that never cloaked has nothing to undo; the window is in view, so it leaves the list.
#[test]
fn a_hide_that_never_cloaked_needs_no_undo() {
    assert!(failed_hide(|| 0, || panic!("nothing to undo")));
}

// Spec 0040 AC-4: the laptop with two and the big screen with six step and jump over their own
// counts. Next from the laptop's 2 wraps to 1 while the big screen goes on to 3, and a jump to 5
// on the laptop is past its count: only the note, which names the laptop's 2.
#[test]
fn steps_and_jumps_use_each_screens_own_count() {
    let set = settings::Settings { count_by_screen: [("internal".to_string(), 2)].into(), shared_count: 6, ..Default::default() };
    let (l, b) = (set.count_for("internal"), set.count_for("DEL-1"));
    assert_eq!((step_target(1, 1, l), step_target(1, 1, b)), (0, 2));
    assert_eq!((step_target(0, -1, l), step_target(0, -1, b)), (1, 5));
    let m = crate::ws::tests::with_hidden(0, &[&[], &[]]);
    assert_eq!(jump_target(&m, 4, l, true), Jump::Missing);
    let m = crate::ws::tests::with_hidden(0, &[&[], &[], &[], &[], &[], &[]]);
    assert_eq!(jump_target(&m, 4, b, true), Jump::Go(4));
}
