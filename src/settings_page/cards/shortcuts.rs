// The Shortcuts section: the key rows, the Copilot key and the scenes (spec 0012, spec 0024).
use super::*;
use crate::settings_page::recording::Rec;
use crate::settings_page::rows::{Card, MOVE, SEND, Short, drawn_order, entry, has_win, is_digits, keys_of, name_of, said, same_keys, spelled, took, value, win_caps};

const KT_KEYS: f32 = 326.0; // a shortcut row's key column
const KT_SIDE: f32 = 112.0; // and its des-k or Windows column

impl Ctx<'_, '_> {
    // ---- Shortcuts ----

    pub(super) fn shortcuts(&mut self, y: f32) -> f32 {
        let mut h = self.card(y, |c, y| c.scenes(y)) + 12.0;
        h += self.card(y + h, |c, y| {
            let b = c.cx.c.t.b;
            let mut h = c.header(y, "Move around", None, "");
            let (kx, sx) = (X0 + c.cw - 16.0 - KT_SIDE - 16.0 - KT_KEYS, X0 + c.cw - 16.0);
            c.cx.c.text(X0 + 16.0, y + h, 32.0, "Action", F::SmallBold, b.tx2);
            c.cx.c.text(kx, y + h, 32.0, "Key · click to change", F::SmallBold, b.tx2);
            let tail = "Windows' key goes to";
            c.cx.c.text(sx - c.cx.c.width(tail, F::SmallBold), y + h, 32.0, tail, F::SmallBold, b.tx2);
            h += 32.0;
            for sh in MOVE {
                c.sep(y + h);
                h += c.key_row(y + h, sh);
            }
            h
        }) + 12.0;
        h += self.card(y + h, |c, y| {
            let mut h = c.header(y, "Move windows", None, "");
            for (i, sh) in SEND.iter().enumerate() {
                if i > 0 {
                    c.sep(y + h);
                }
                h += c.key_row(y + h, sh);
            }
            h
        }) + 12.0;
        h += self.card(y + h, |c, y| {
            let h = c.header(y, "Copilot key", None, "");
            h + c.copilot(y + h)
        });
        h
    }

    /// The Copilot key's row, in the shortcut table's columns.
    fn copilot(&mut self, y: f32) -> f32 {
        let key = "copilot_as_right_ctrl";
        let b = self.cx.c.t.b;
        let v = value(self.s, key) != 0;
        let mut cap = vec![t(if v { "So Win Ctrl keys work with it. Taken from Windows, where it opens Copilot, or what you set." } else { "Windows keeps it (Copilot, or what you set). Win Ctrl keys need the left Ctrl." })];
        let note = self.bad(key);
        cap.extend(self.default_pieces(key, note.is_some()));
        let lw = self.cw - 32.0 - KT_KEYS - KT_SIDE - 32.0;
        let help: String = cap.iter().filter_map(|p| if let P::T(s) = p { Some(s.as_str()) } else { None }).collect();
        self.cx.say(Ctl::Pick(key), UIA_ComboBoxControlTypeId, "Copilot key as Right Ctrl", &help, self.cx.ro, (None, Some(if v { "des-k" } else { "Windows" }.into())));
        let words = ["des-k", "Windows"];
        let sw = self.cx.seg_w(&words);
        let lh = self.cx.label_h(lw, "Copilot key as Right Ctrl", &cap);
        let top = lh.max(28.0).max(ROW - 16.0);
        let kx = X0 + self.cw - 16.0 - KT_SIDE - 16.0 - KT_KEYS;
        self.cx.c.text(kx, y + 8.0, top, "The Copilot key", F::Small, b.tx2);
        self.cx.seg(X0 + self.cw - 16.0 - sw, y + 8.0 + (top - 28.0) / 2.0, &words, Some(if v { 0 } else { 1 }), false, |i| Ctl::Choice(key, i), Ctl::Pick(key), None);
        self.cx.label(X0 + 16.0, y + 8.0 + (top - lh) / 2.0, lw, "Copilot key as Right Ctrl", &cap);
        let mut h = 8.0 + top;
        if let Some(n) = note {
            h += 6.0 + self.cx.in_note(X0 + 16.0, y + h + 6.0, self.cw - 32.0, &n);
        }
        h + 8.0
    }

    /// The three kinds of shortcut, drawn with the workspace colors, with your keys under them.
    fn scenes(&mut self, y: f32) -> f32 {
        let b = self.cx.c.t.b;
        let s = self.s;
        let colw = (self.cw - 32.0 - 32.0) / 3.0;
        let list = [
            ("Switch", "You go to the next workspace.", 0u8, "shortcuts.previous", "shortcuts.next", "shortcuts.jump"),
            ("Send window", "The window goes, and you go with it.", 1, "shortcuts.send_previous", "shortcuts.send_next", "shortcuts.send_to"),
            ("Send window and stay", "The window goes; you stay.", 2, "shortcuts.send_previous_stay", "shortcuts.send_next_stay", "shortcuts.send_to_stay"),
        ];
        let mut most: f32 = 0.0;
        for (k, (title, cap, kind, prev, next, digits)) in list.into_iter().enumerate() {
            let x = X0 + 16.0 + k as f32 * (colw + 16.0);
            let mut h = 14.0;
            self.cx.c.text(x, y + h, 20.0, title, F::Bold, b.tx);
            h += 20.0;
            h += self.cx.c.para(x, y + h, colw, 16.0, &[(cap, F::Small, b.tx2)]) + 8.0;
            self.scene(x, y + h, kind);
            h += 58.0 + 8.0;
            let (p, n) = (keys_of(s, prev), keys_of(s, next));
            // One ← → pair when both hold the same modifiers.
            let pair = match (p.first(), n.first()) {
                (Some(a), Some(z)) if a.last().map(String::as_str) == Some("Left") && z.last().map(String::as_str) == Some("Right") && a[..a.len() - 1] == z[..z.len() - 1] => {
                    let mut caps = a[..a.len() - 1].to_vec();
                    caps.extend(["Left".to_string(), "Right".to_string()]);
                    Some(vec![caps])
                }
                _ => None,
            };
            let rows: Vec<Vec<String>> = pair.unwrap_or_else(|| p.first().into_iter().chain(n.first()).cloned().collect());
            if rows.is_empty() {
                self.cx.c.text(x, y + h, 22.0, "No key", F::Small, b.tx2);
                h += 26.0;
            }
            for caps in rows {
                self.caps(x, y + h, &caps, false);
                h += 26.0;
            }
            self.cx.c.text(x, y + h, 16.0, "or", F::Small, b.tx2);
            h += 20.0;
            match keys_of(s, digits).first() {
                Some(caps) => {
                    self.caps(x, y + h, caps, false);
                }
                None => self.cx.c.text(x, y + h, 22.0, "No key", F::Small, b.tx2),
            }
            h += 22.0;
            most = most.max(h);
        }
        most + 14.0
    }

    /// A scene: four workspaces, where you are after the press (ring and dot) and where the window is.
    fn scene(&mut self, x: f32, y: f32, kind: u8) {
        let t = self.cx.c.t;
        let b = t.b;
        let (tw, th, gap, x0, y0) = (38.0, 26.0, 8.0, x + 2.0, y + 14.0);
        let tx = |i: usize| x0 + i as f32 * (tw + gap);
        let you = if kind == 2 { 1 } else { 2 };
        for i in 0..4 {
            if t.hc {
                self.cx.c.boxed((tx(i), y0, tw, th), 4.0, if i == you { b.hi } else { b.card }, Some(b.tx));
            } else {
                self.cx.c.fill_rr((tx(i), y0, tw, th), 4.0, op(b.ws[i]));
            }
        }
        self.cx.c.stroke_rr((tx(you) - 2.0, y0 - 2.0, tw + 4.0, th + 4.0), 5.0, 2.0, op(b.tx));
        self.cx.c.circle(tx(you) + tw / 2.0, y + 5.0, 3.5, Some(op(b.tx)), None);
        if kind > 0 {
            self.cx.c.stroke_dash((tx(1) + 9.0, y0 + 6.0, tw - 18.0, th - 12.0), 2.0, 1.0, op(b.on_ws[1]), DashStyleDash);
            self.cx.c.boxed((tx(2) + 9.0, y0 + 6.0, tw - 18.0, th - 12.0), 2.0, ui::LIGHT.b.card, Some(b.on_ws[2]));
        }
        let (a, z, base) = (tx(1) + tw / 2.0, tx(2) + tw / 2.0, y0 + th + 6.0);
        let pts: Vec<(f32, f32)> = (0..=10).map(|j| {
            let u = j as f32 / 10.0;
            (a + (z - a) * u, base + 10.0 * 2.0 * u * (1.0 - u))
        }).collect();
        self.cx.c.lines(&pts, 1.5, op(b.tx2));
        self.cx.c.lines(&[(z - 5.0, base + 1.0), (z, base), (z - 2.0, base + 5.0)], 1.5, op(b.tx2));
    }

    /// Key caps for one key, 22 dip, from `x`. Returns their width.
    fn caps(&self, x: f32, y: f32, caps: &[String], held: bool) -> f32 {
        let mut cx = x;
        for (i, k) in caps.iter().enumerate() {
            if i > 0 {
                cx += 4.0;
            }
            cx += self.cx.c.key_cap(cx, y, 22.0, k, held);
        }
        cx - x
    }

    fn caps_w(&self, caps: &[String]) -> f32 {
        caps.iter().enumerate().map(|(i, k)| {
            let label = match k.as_str() {
                "Left" | "Right" | "Up" | "Down" => ("\u{E72B}", F::IconTiny),
                k => (k, F::Small),
            };
            let tw = self.cx.c.width(label.0, label.1);
            (tw + 2.0 * (22.0 / 2.0 - 5.0) + 2.0).max(22.0) + if i > 0 { 4.0 } else { 0.0 }
        }).sum()
    }

    /// The key field the recorder fills: a ghost cap, or the key it found.
    fn rec_field(&mut self, x: f32, y: f32, held: &[String], ghost: &str) -> f32 {
        let b = self.cx.c.t.b;
        let w = if held.is_empty() { self.cx.c.width(ghost, F::Small) + 12.0 } else { self.caps_w(held) } + 10.0;
        self.cx.field((x, y, w, 32.0), b.card, Some(b.acc));
        if held.is_empty() {
            let gw = self.cx.c.width(ghost, F::Small) + 12.0;
            self.cx.c.stroke_dash((x + 5.0, y + 5.0, gw, 22.0), 4.0, 1.0, op(b.tx2), DashStyleDash);
            self.cx.c.text(x + 11.0, y + 5.0, 22.0, ghost, F::Small, b.tx2);
        } else {
            self.caps(x + 5.0, y + 5.0, held, true);
        }
        w
    }

    /// One shortcut: its keys as fields with × and +, and des-k or Windows (AC-SET-4). One stop
    /// for the row; recording, a clash or a refusal shows its note under it.
    fn key_row(&mut self, y: f32, sh: &'static Short) -> f32 {
        let b = self.cx.c.t.b;
        let s = self.s;
        let key = sh.key;
        if key == "shortcuts.back" {
            self.out.cards.push((Card::Back, y - self.top0));
        }
        let digits = is_digits(key);
        let all = keys_of(s, key);
        let order = drawn_order(s, sh);
        let has = has_win(s, sh);
        let took = if has { None } else { took(s, sh) };
        let rec = self.w.rec;
        let prompt = self.w.prompt.as_ref().filter(|p| p.key == key);
        let recording = matches!(rec, Some(Rec::List(k, _)) | Some(Rec::Digits(k)) if k == key);
        let active = recording || prompt.is_some();
        // The caption.
        let mut cap: Vec<P> = vec![];
        if key == "shortcuts.hints" {
            cap.push(t(if all.is_empty() { "No key by default. Shows every key and gesture, on the screen under your mouse." } else { "Shows every key and gesture, on the screen under your mouse." }));
        }
        if key == "shortcuts.pause" {
            cap.push(t(if all.is_empty() { "No key by default. Shows every hidden window and gives Windows its keys back; press again to resume." } else { "Shows every hidden window and gives Windows its keys back; press again to resume." }));
        }
        if let Some((wk, does)) = sh.win {
            let wk = spelled(&win_caps(wk));
            let line = if has {
                if all.len() > 1 { format!("{wk} is taken from Windows, where it {does}.") } else { format!("Taken from Windows, where it {does}.") }
            } else if let Some(other) = took {
                format!("{}{wk} goes to “{other}” now. Remove it there to get it back.", if all.is_empty() { "No key. " } else { "" })
            } else {
                format!("{}{wk} is Windows' again: it {does}.", if all.is_empty() { "No key. " } else { "Your key. " })
            };
            cap.push(t(line));
        }
        if digits && let Some(m) = s.set(name_of(key)) {
            for (d, list) in s.skips(name_of(key)) {
                cap.push(t(format!(" Skips {d}: “{}” uses {} {d}.", entry(list).1, spelled(&crate::hints::rows::mods_caps(m)))));
            }
        }
        let bad = self.bad(key);
        if !same_keys(s, &DEFAULTS, key) || bad.is_some() {
            cap.push(t(format!(" Default: {}.", said(&DEFAULTS, key))));
            cap.push(P::Link(" Reset".into(), Ctl::Reset(key)));
        }
        let help: String = cap.iter().filter_map(|p| if let P::T(s) = p { Some(s.as_str()) } else { None }).collect::<String>().trim().to_string();
        let keys_said = if all.is_empty() { "no key".to_string() } else { all.iter().map(|k| spelled(k)).collect::<Vec<_>>().join(" or ") };
        self.cx.say(Ctl::KeyRow(key), UIA_GroupControlTypeId, sh.title, &help, self.cx.ro, (None, Some(keys_said)));
        // The note under it.
        let note = if let Some(p) = prompt {
            Some(NoteSpec { tone: Tone::Warn, icon: I_WARN, title: p.title.clone(), lines: vec![t(p.sub.clone())], buttons: vec![(Ctl::Move(key), "Move it here"), (Ctl::Cancel(key), "Cancel")], group: 1 })
        } else if recording {
            Some(match self.w.refused.clone() {
                Some((title, sub)) => NoteSpec { tone: Tone::Warn, icon: I_WARN, title, lines: vec![t(sub)], buttons: vec![(Ctl::StopRec(key), "Cancel")], group: 1 },
                None if digits => NoteSpec { tone: Tone::Rec, icon: I_KEY, title: "Hold the new keys and press a digit".into(), lines: vec![t("Hold Win, Ctrl or Alt (Shift can join them), then press any digit 1 to 9; all nine get the same keys. Esc alone or a click elsewhere cancels.")], buttons: vec![(Ctl::StopRec(key), "Cancel")], group: 1 },
                None => NoteSpec { tone: Tone::Rec, icon: I_KEY, title: "Press the new key".into(), lines: vec![t("Hold Win, Ctrl, Alt or Shift with a key. If Windows uses it, des-k takes it over. Esc alone or a click elsewhere cancels.")], buttons: vec![(Ctl::StopRec(key), "Cancel")], group: 1 },
            })
        } else {
            bad
        };
        let lw = self.cw - 32.0 - KT_KEYS - KT_SIDE - 32.0;
        let lh = self.cx.label_h(lw, sh.title, &cap);
        let kx = X0 + self.cw - 16.0 - KT_SIDE - 16.0 - KT_KEYS;
        // The key column's height: fields wrap after the column's width.
        let wells = |c: &mut Self, y: f32| -> f32 {
            let (mut cx, mut cy) = (kx, y);
            let room = |cx: &mut f32, cy: &mut f32, w: f32| {
                if *cx > kx && *cx + w > kx + KT_KEYS {
                    *cx = kx;
                    *cy += 38.0;
                }
            };
            let replacing = match rec {
                Some(Rec::List(k, Some(i))) if k == key => Some(i),
                Some(Rec::Digits(k)) if k == key && !all.is_empty() => Some(0),
                _ => None,
            };
            let held: Vec<String> = prompt.map(|p| p.held.clone()).unwrap_or_default();
            for &i in &order {
                if replacing == Some(i) {
                    let fw = c.measured(|c| c.rec_field(0.0, 0.0, &held, if digits { "1–9" } else { "key" }));
                    room(&mut cx, &mut cy, fw);
                    cx += c.rec_field(cx, cy, &held, if digits { "1–9" } else { "key" }) + 6.0;
                    continue;
                }
                let caps = &all[i];
                let fw = c.caps_w(caps) + 10.0 + 24.0;
                room(&mut cx, &mut cy, fw);
                let (body, cross) = (Ctl::Key(key, i), Ctl::KeyX(key, i));
                let hov = (c.cx.hovered(body) || c.cx.hovered(cross)) && !c.cx.ro;
                c.cx.field((cx, cy, fw, 32.0), if hov { b.card } else { b.track }, None);
                c.caps(cx + 5.0, cy + 5.0, caps, false);
                let (xx, xy) = (cx + fw - 25.0, cy + 6.0);
                if c.cx.hovered(cross) && !c.cx.ro {
                    c.cx.c.boxed((xx, xy, 20.0, 20.0), 4.0, b.track, Some(b.tx3));
                }
                let iw = c.cx.c.width(I_X, F::IconTiny);
                c.cx.icon(xx + (20.0 - iw) / 2.0, xy, 20.0, I_X, F::IconTiny, if c.cx.ro { b.tx3 } else if c.cx.hovered(cross) { b.tx } else { b.tx2 });
                c.cx.hit(cross, (xx - 2.0, cy, 24.0, 32.0), 1);
                c.cx.hit(body, (cx, cy, fw - 26.0, 32.0), 1);
                cx += fw + 6.0;
            }
            let adding = match rec {
                Some(Rec::List(k, None)) if k == key => true,
                Some(Rec::Digits(k)) if k == key && all.is_empty() => true,
                _ => prompt.is_some(),
            };
            if adding {
                let ghost = if digits { "1–9" } else { "key" };
                let fw = c.measured(|c| c.rec_field(0.0, 0.0, &held, ghost));
                room(&mut cx, &mut cy, fw);
                c.rec_field(cx, cy, &held, ghost);
            } else if all.is_empty() {
                let fw = c.cx.btn_w("Set key", Some(I_ADD));
                room(&mut cx, &mut cy, fw);
                c.cx.button(Ctl::AddKey(key), cx, cy, "Set key", Some(I_ADD), false, 1);
            } else if !digits && !active {
                room(&mut cx, &mut cy, 32.0);
                let off = c.cx.ro;
                let fill = if c.cx.hovered(Ctl::AddKey(key)) && !off { mix(b.card, b.tx, 0.05) } else { b.card };
                c.cx.field((cx, cy, 32.0, 32.0), fill, None);
                let iw = c.cx.c.width(I_ADD, F::IconSmall);
                c.cx.icon(cx + (32.0 - iw) / 2.0, cy, 32.0, I_ADD, F::IconSmall, if off { b.tx3 } else { b.tx });
                c.cx.hit(Ctl::AddKey(key), (cx, cy, 32.0, 32.0), 1);
            }
            cy + 32.0 - y
        };
        let kh = self.measured(|c| wells(c, 0.0));
        let top = lh.max(kh).max(ROW - 16.0);
        let body_h = 8.0 + top + note.as_ref().map_or(0.0, |n| 6.0 + self.measured(|c| c.cx.in_note(0.0, 0.0, c.cw - 32.0, n))) + 8.0;
        if active {
            let tint = if self.cx.c.t.hc { b.card } else { mix(b.card, b.acc_a, 0.06) };
            self.cx.c.fill_rr((X0 + 1.0, y + 0.5, self.cw - 2.0, body_h - 1.0), 0.0, op(tint));
        }
        wells(self, y + 8.0 + (top - kh) / 2.0);
        // des-k or Windows: greyed while another action holds the key.
        if sh.win.is_some() {
            let words = ["des-k", "Windows"];
            let sw = self.cx.seg_w(&words);
            let sel = if took.is_some() { None } else { Some(if has { 0 } else { 1 }) };
            self.cx.seg(X0 + self.cw - 16.0 - sw, y + 8.0 + (top - 28.0) / 2.0, &words, sel, took.is_some(), |i| Ctl::WinSide(key, i == 0), Ctl::KeyRow(key), None);
        }
        self.cx.label(X0 + 16.0, y + 8.0 + (top - lh) / 2.0, lw, sh.title, &cap);
        if self.cx.focused(Ctl::KeyRow(key)) {
            self.cx.c.stroke_rr((X0 + 2.0, y + 2.0, self.cw - 4.0, 8.0 + top + 8.0 - 4.0), 6.0, 2.0, op(b.tx));
        }
        let mut h = 8.0 + top;
        if let Some(n) = &note {
            h += 6.0 + self.cx.in_note(X0 + 16.0, y + h + 6.0, self.cw - 32.0, n);
        }
        h + 8.0
    }
}
