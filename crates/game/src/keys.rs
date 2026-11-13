//! The keys the match answers to. What the player can do by key is named
//! as the game's hotkey files name it, such as `ActorCommand move`, and
//! [`Bindings`] says which keys give it, so the rest of the interface asks
//! for actions and never for keys. The bindings are the game's own;
//! another set would be read into the same place.

use std::collections::HashMap;

use aaa_data::Hotkey;
use bevy::prelude::*;

use crate::ui::Marked;

/// Stepping through the player's regiments that have no HQ.
pub const CYCLE_DETACHED: &str = "CycleSelection 2";
/// Showing or hiding the supply border.
pub const SUPPLY_BORDER: &str = "ButtonSupplyBorder";

/// A key, and the keys held with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: KeyCode,
}

impl Chord {
    /// The chord a hotkey file writes as, say, `ctrl-s`; `None` for what
    /// isn't a key press, such as the mouse wheel, or a key not known
    /// here.
    pub fn parse(chord: &str) -> Option<Chord> {
        let mut out = Chord {
            ctrl: false,
            alt: false,
            shift: false,
            key: KeyCode::Escape,
        };
        let mut rest = chord;
        while let Some((held, after)) = rest.split_once('-') {
            match held {
                "ctrl" => out.ctrl = true,
                "alt" => out.alt = true,
                "shift" => out.shift = true,
                _ => break,
            }
            rest = after;
        }
        out.key = key(rest)?;
        Some(out)
    }

    /// Whether it's just been pressed, with exactly the keys it's held
    /// with down: not with the system's own key, whose shortcuts aren't
    /// the game's.
    fn pressed(&self, keys: &ButtonInput<KeyCode>) -> bool {
        let held = |codes: [KeyCode; 2]| keys.any_pressed(codes);
        keys.just_pressed(self.key)
            && held([KeyCode::ControlLeft, KeyCode::ControlRight]) == self.ctrl
            && held([KeyCode::AltLeft, KeyCode::AltRight]) == self.alt
            && held([KeyCode::ShiftLeft, KeyCode::ShiftRight]) == self.shift
            && !held([KeyCode::SuperLeft, KeyCode::SuperRight])
    }

    /// The character it types, where it's a key pressed alone that
    /// types one.
    fn types(&self) -> Option<char> {
        let alone = !self.ctrl && !self.alt && !self.shift;
        let place = |keys: &[KeyCode]| keys.iter().position(|&k| k == self.key);
        let letter = place(&LETTERS).map(|at| (b'A' + at as u8) as char);
        let digit = place(&DIGITS).map(|at| (b'0' + at as u8) as char);
        letter.or(digit).filter(|_| alone)
    }
}

/// How a key shows in the name of what it gives.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KeyName {
    /// The character it types, for marking in the name.
    types: Option<char>,
    /// What it's called, with the keys held with it, for where the name
    /// has no such character.
    name: String,
}

impl KeyName {
    /// `name` with the key marked: its character where the name has it,
    /// as it's written before in either case, else the key's own name in
    /// brackets ahead of it.
    pub fn mark(&self, name: &str) -> Marked {
        let exact = self.types.and_then(|c| name.find(c));
        let either = || name.find(|c: char| self.types.is_some_and(|k| c.eq_ignore_ascii_case(&k)));
        match exact.or_else(either) {
            Some(at) => Marked {
                before: name[..at].to_string(),
                key: name[at..=at].to_string(),
                after: name[at + 1..].to_string(),
            },
            None => Marked {
                before: "(".to_string(),
                key: self.name.clone(),
                after: format!("){name}"),
            },
        }
    }
}

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

const FUNCTIONS: [KeyCode; 12] = [
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
];

/// The key a hotkey file names, lower case.
fn key(name: &str) -> Option<KeyCode> {
    if let [c] = name.as_bytes() {
        match c {
            b'a'..=b'z' => return Some(LETTERS[(c - b'a') as usize]),
            b'0'..=b'9' => return Some(DIGITS[(c - b'0') as usize]),
            _ => {}
        }
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()) {
        return FUNCTIONS.get(n.checked_sub(1)?).copied();
    }
    Some(match name {
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "esc" => KeyCode::Escape,
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Space,
        "backspace" => KeyCode::Backspace,
        "up" => KeyCode::ArrowUp,
        "down" => KeyCode::ArrowDown,
        "left" => KeyCode::ArrowLeft,
        "right" => KeyCode::ArrowRight,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "pause" => KeyCode::Pause,
        "minus" => KeyCode::Minus,
        "plus" => KeyCode::Equal,
        "backslash" => KeyCode::Backslash,
        "grave" => KeyCode::Backquote,
        "semicolon" => KeyCode::Semicolon,
        "comma" => KeyCode::Comma,
        "period" => KeyCode::Period,
        "slash" => KeyCode::Slash,
        "quote" => KeyCode::Quote,
        "[" => KeyCode::BracketLeft,
        "]" => KeyCode::BracketRight,
        _ => return None,
    })
}

/// Which keys give each action, and how each key shows.
#[derive(Resource, Default)]
pub struct Bindings(Vec<(Chord, String, KeyName)>);

impl Bindings {
    /// The bindings a set of hotkeys makes, leaving out any on keys not
    /// known here, with the keys named as the game's `strings` name
    /// them.
    pub fn new(hotkeys: &[Hotkey], strings: &HashMap<String, String>) -> Self {
        let separator = strings.get("hotkey_modifier_separator");
        let separator = separator.map_or("-", String::as_str);
        let bound = |h: &Hotkey| {
            let chord = Chord::parse(&h.chord)?;
            let held = [
                (chord.ctrl, "Ctrl"),
                (chord.alt, "Alt"),
                (chord.shift, "Shift"),
            ];
            let key = h.chord.rsplit('-').next().unwrap_or(&h.chord);
            let key = strings
                .get(&format!("key_{key}"))
                .map_or(key, String::as_str);
            let mut name = String::new();
            for (_, held) in held.iter().filter(|(down, _)| *down) {
                name += held;
                name += separator;
            }
            name += key;
            let shown = KeyName {
                types: chord.types(),
                name,
            };
            Some((chord, h.action().to_string(), shown))
        };
        Bindings(hotkeys.iter().filter_map(bound).collect())
    }

    fn bound<'a>(&'a self, action: &'a str) -> impl Iterator<Item = &'a (Chord, String, KeyName)> {
        self.0.iter().filter(move |(_, a, _)| a == action)
    }

    /// Whether a key that gives `action` has just been pressed.
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, action: &str) -> bool {
        self.bound(action).any(|(chord, ..)| chord.pressed(keys))
    }

    /// How the key that gives `action` shows, if one does.
    pub fn key(&self, action: &str) -> Option<&KeyName> {
        let bound = self.0.iter().find(|(_, a, _)| a == action);
        bound.map(|(.., name)| name)
    }

    /// `name` with the key that gives `action` marked in it, if one does.
    pub fn marked(&self, action: &str, name: &str) -> Marked {
        match self.key(action) {
            Some(key) => key.mark(name),
            None => Marked::plain(name),
        }
    }
}

/// Reads the hotkeys chosen in the options, and how the game marks them.
pub fn load(
    mut commands: Commands,
    loaded: Res<crate::Loaded>,
    mut theme: ResMut<crate::ui::Theme>,
    options: Res<crate::options::Options>,
) {
    let game = &loaded.game;
    theme.key = (game.highlight_colour()).map(|[r, g, b]| Color::srgb(r, g, b));
    let hotkeys = game.hotkeys(options.hotkeys());
    commands.insert_resource(Bindings::new(&hotkeys, &game.strings()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(chord: &str, command: &str) -> Hotkey {
        Hotkey {
            chord: chord.into(),
            target: "game".into(),
            command: Some(command.into()),
        }
    }

    #[test]
    fn chords_read_as_the_files_write_them() {
        let chord = |c| Chord::parse(c).map(|c| (c.ctrl, c.alt, c.shift, c.key));
        assert_eq!(chord("c"), Some((false, false, false, KeyCode::KeyC)));
        assert_eq!(chord("ctrl-s"), Some((true, false, false, KeyCode::KeyS)));
        assert_eq!(
            chord("ctrl-alt-shift-1"),
            Some((true, true, true, KeyCode::Digit1))
        );
        assert_eq!(chord("f5"), Some((false, false, false, KeyCode::F5)));
        assert_eq!(chord("delete").map(|c| c.3), Some(KeyCode::Delete));
        assert_eq!(chord("shift-minus").map(|c| c.3), Some(KeyCode::Minus));
        assert_eq!(chord("semicolon").map(|c| c.3), Some(KeyCode::Semicolon));
        assert_eq!(chord("mousewheel"), None);
        assert_eq!(chord("+left"), None);
        assert_eq!(chord("numpad-minus"), None);
    }

    #[test]
    fn actions_answer_to_their_keys_and_what_is_held_with_them() {
        let bindings = Bindings::new(
            &[
                hotkey("s", "ActorCommand stop"),
                hotkey("ctrl-s", SUPPLY_BORDER),
                hotkey("mousewheel", "deltazoom"),
            ],
            &HashMap::new(),
        );
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyS);
        assert!(bindings.pressed(&keys, "ActorCommand stop"));
        assert!(!bindings.pressed(&keys, SUPPLY_BORDER));
        assert!(!bindings.pressed(&keys, "deltazoom"));
        // Held with Ctrl, it's the other.
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyS);
        assert!(!bindings.pressed(&keys, "ActorCommand stop"));
        assert!(bindings.pressed(&keys, SUPPLY_BORDER));
        // Not while it stays down.
        keys.clear();
        assert!(!bindings.pressed(&keys, SUPPLY_BORDER));
        // Nor with the system's key, as for its shortcuts.
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::SuperLeft);
        keys.press(KeyCode::KeyS);
        assert!(!bindings.pressed(&keys, "ActorCommand stop"));
    }

    #[test]
    fn names_mark_their_keys() {
        let strings = HashMap::from([("key_delete".to_string(), "Delete".to_string())]);
        let bindings = Bindings::new(
            &[
                hotkey("d", "deploy"),
                hotkey("t", "infantry"),
                hotkey("delete", "detach"),
                hotkey("ctrl-s", SUPPLY_BORDER),
                hotkey("x", "explore"),
            ],
            &strings,
        );
        let marked = |action: &str, name: &str| {
            let m = bindings.key(action).unwrap().mark(name);
            format!("{}[{}]{}", m.before, m.key, m.after)
        };
        // The first place the character stands, as written before in
        // either case.
        assert_eq!(marked("deploy", "Deploy"), "[D]eploy");
        assert_eq!(marked("deploy", "Redeploy Dogs"), "Redeploy [D]ogs");
        assert_eq!(
            marked("infantry", "Infantry Division HQ"),
            "Infan[t]ry Division HQ"
        );
        // A key that types nothing, one held with another, and one the
        // name lacks go in front.
        assert_eq!(
            marked("detach", "Detach from HQ"),
            "([Delete])Detach from HQ"
        );
        assert_eq!(
            marked(SUPPLY_BORDER, "Toggle Supply Border"),
            "([Ctrl-s])Toggle Supply Border"
        );
        assert_eq!(marked("explore", "Scout"), "([x])Scout");
        assert!(bindings.key("stop").is_none());
    }
}
