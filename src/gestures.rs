// What a touchpad gesture does (spec 0011, 0011-gesture-map). The touchpad thread recognizes
// a gesture, the main thread looks up its rule in settings' [[gesture]] map and runs it here.
use crate::settings::{self, Action, Motion};
use crate::{finder, keys, overview, ws};

const MOTIONS: [Motion; 5] = [Motion::Left, Motion::Right, Motion::Up, Motion::Down, Motion::Tap];

/// A gesture as one WM_GESTURE wparam: fingers, motion and times, a byte each.
pub fn pack(fingers: u8, motion: Motion, times: u8) -> usize {
    let m = MOTIONS.iter().position(|&x| x == motion).expect("every motion is listed");
    fingers as usize | m << 8 | (times as usize) << 16
}

fn unpack(w: usize) -> Option<(u8, Motion, u8)> {
    Some((w as u8, *MOTIONS.get((w >> 8) as u8 as usize)?, (w >> 16) as u8))
}

/// Main thread only. `gesture` is from `pack`; its rule is looked up now, in the settings in force.
pub fn run(gesture: usize) {
    let Some((fingers, motion, times)) = unpack(gesture) else { return };
    let Some(action) = settings::get().rule(fingers, motion, times) else {
        return println!("gesture {fingers} {}: its rule went with a settings change", motion.word());
    };
    // The last input was the touchpad's, so Windows would refuse the focus change an action
    // makes; des-k's own harmless key makes des-k the source of it (spec 0011 check step).
    keys::mask();
    // des-k's own moves do exactly what their keys do, the finder and overview included (spec 0010 AC-12).
    match action {
        Action::Switch(dir) => {
            close();
            ws::switch(dir);
            crate::welcome::tried(crate::welcome::Try::Switch);
        }
        Action::Send(dir, follow) => {
            close();
            ws::send(ws::To::Step(dir), follow);
        }
        Action::Jump(i) => {
            close();
            ws::jump(i);
        }
        Action::SendTo(i, follow) => {
            close();
            ws::send(ws::To::Index(i), follow);
        }
        Action::Finder => {
            overview::close(true);
            finder::toggle();
        }
        Action::Overview if overview::is_open() => {}
        Action::Overview => {
            finder::close(true);
            overview::toggle();
            crate::welcome::tried(crate::welcome::Try::Overview);
        }
        // With the finder or overview open, it only closes them.
        Action::ShowDesktop if finder::is_open() || overview::is_open() => close(),
        Action::ShowDesktop => ws::show_desktop(),
        Action::AppNext => {
            close();
            ws::app_next();
        }
        Action::AppPrevious => {
            close();
            ws::app_previous();
        }
        Action::Pin => {
            close();
            ws::pin();
        }
        Action::Keys(vks) => keys::tap(vks),
        Action::MiddleClick => keys::middle_click(),
        Action::Combo(c) => keys::combo(c),
        Action::Nothing => {}
    }
}

/// The finder and the overview close first, focus back, as before any switch.
fn close() {
    finder::close(true);
    overview::close(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    // The touchpad thread and the main thread must agree on every gesture a rule can name,
    // or a swipe runs another swipe's rule.
    #[test]
    fn every_gesture_survives_the_trip_to_the_main_thread() {
        for (fingers, motion) in settings::GESTURE_ROWS {
            for times in [1, 2] {
                assert_eq!(unpack(pack(fingers, motion, times)), Some((fingers, motion, times)));
            }
        }
    }
}
