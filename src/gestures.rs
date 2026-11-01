// What a touchpad gesture does (spec 0011, 0011-gesture-map). The touchpad thread finds the
// rule in settings' [[gesture]] map, the main thread runs its action here.
use crate::settings::{self, Action};
use crate::{finder, keys, overview, ws};

/// Main thread only. `rule` indexes `settings::get().gestures`.
pub fn run(rule: usize) {
    let Some(g) = settings::get().gestures.get(rule) else {
        return;
    };
    // The last input was the touchpad's, so Windows would refuse the focus change an action
    // makes; pdesk's own harmless key makes pdesk the source of it (spec 0011 check step).
    keys::mask();
    // pdesk's own moves do exactly what their keys do, the finder and overview included (spec 0010 AC-12).
    match g.action {
        Action::Switch(dir) => {
            close();
            ws::switch(dir);
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
