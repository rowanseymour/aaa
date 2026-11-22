//! The keys the match answers to. What the player can do by key is named
//! as the game's hotkey files name it, such as `ActorCommand move`, and
//! [`Bindings`] says which keys give it, and [`send`] sends each as an
//! [`Intent`], so the rest of the interface asks for actions and never
//! for keys. The bindings are the game's own;
//! another set would be read into the same place.

use bevy::prelude::*;
use tgdata::Hotkey;

use crate::intent::Intent;
use crate::ui::Marked;

/// Stepping through the player's builders with nothing to do.
pub const CYCLE_IDLE: &str = "CycleSelection 1";
/// Stepping through the player's organizations that have no HQ.
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
    /// What the key's called, with the keys held with it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The key's name in brackets, as a tooltip's lines put it.
    pub fn bracketed(&self) -> String {
        format!("({})", self.name)
    }

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

/// Where a binding applies, as its hotkey's target says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Scope {
    /// It presses the widget named, wherever that's on screen.
    Widget(String),
    /// It gives its command in any of these places, such as `game` or a
    /// panel's window.
    In(Vec<String>),
}

impl Scope {
    fn of(hotkey: &Hotkey) -> Scope {
        let mut places = hotkey.target.split('|').map(str::to_string);
        match hotkey.command {
            None => Scope::Widget(places.next().unwrap_or_default()),
            Some(_) => Scope::In(places.collect()),
        }
    }

    /// Whether it's heard: with no panel up, in the match's own places
    /// and on its widgets; with `window` up, in that window, over
    /// everything, and on the widgets `passes` lets through.
    fn heard(&self, window: Option<&str>, passes: impl Fn(&str) -> bool) -> bool {
        match (self, window) {
            (Scope::In(places), None) => places.iter().any(|p| MATCH.contains(&p.as_str())),
            (Scope::In(places), Some(window)) => {
                (places.iter()).any(|p| p == ROOT || p.eq_ignore_ascii_case(window))
            }
            (Scope::Widget(_), None) => true,
            (Scope::Widget(widget), Some(_)) => passes(widget),
        }
    }

    /// Whether it gives its command in `place`, such as a panel that's up
    /// beside the match rather than over it.
    fn given_in(&self, place: &str) -> bool {
        matches!(self, Scope::In(places) if places.iter().any(|p| p.eq_ignore_ascii_case(place)))
    }
}

/// The places a match hears commands in while no panel's up.
const MATCH: [&str; 4] = [ROOT, "vworld", "game", "dev"];
/// The place heard over everything.
const ROOT: &str = "root";

/// A widget's name without the slot a repeated one is numbered by, e.g.
/// `ActionButtonsButton` for `ActionButtonsButton_3`.
fn unnumbered(widget: &str) -> &str {
    match widget.rsplit_once('_') {
        Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
        _ => widget,
    }
}

/// Which keys give each action, where, and how each key shows.
#[derive(Resource, Default)]
pub struct Bindings(Vec<(Chord, String, KeyName, Scope)>);

impl Bindings {
    /// The bindings a set of hotkeys makes, leaving out any on keys not
    /// known here, with the keys named as the game's `strings` name
    /// them.
    pub fn new(hotkeys: &[Hotkey], strings: &tgdata::Strings) -> Self {
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
            Some((chord, h.action().to_string(), shown, Scope::of(h)))
        };
        Bindings(hotkeys.iter().filter_map(bound).collect())
    }

    /// Leaves out what presses a widget the screen doesn't have, such as
    /// the main menu's, given the names of those it has.
    pub fn on_screen(mut self, widgets: &std::collections::HashSet<String>) -> Self {
        self.0.retain(|(.., scope)| match scope {
            Scope::Widget(w) => widgets.contains(&unnumbered(w).to_ascii_lowercase()),
            Scope::In(_) => true,
        });
        self
    }

    fn bound<'a>(
        &'a self,
        action: &'a str,
    ) -> impl Iterator<Item = &'a (Chord, String, KeyName, Scope)> {
        self.0.iter().filter(move |(_, a, ..)| a == action)
    }

    /// Whether a key that gives `action` has just been pressed.
    pub fn pressed(&self, keys: &ButtonInput<KeyCode>, action: &str) -> bool {
        self.bound(action).any(|(chord, ..)| chord.pressed(keys))
    }

    /// The actions whose keys have just been pressed, in the order bound,
    /// with where each applies.
    fn just_pressed<'a>(
        &'a self,
        keys: &'a ButtonInput<KeyCode>,
    ) -> impl Iterator<Item = (&'a str, &'a Scope)> {
        (self.0.iter())
            .filter(|(chord, ..)| chord.pressed(keys))
            .map(|(_, action, _, scope)| (action.as_str(), scope))
    }

    /// How the key that gives `action` shows, if one does.
    pub fn key(&self, action: &str) -> Option<&KeyName> {
        let bound = self.0.iter().find(|(_, a, ..)| a == action);
        bound.map(|(_, _, name, _)| name)
    }

    /// `name` with the key that gives `action` marked in it, if one does.
    pub fn marked(&self, action: &str, name: &str) -> Marked {
        match self.key(action) {
            Some(key) => key.mark(name),
            None => Marked::plain(name),
        }
    }
}

/// A button's name, with the key that gives what it asks for marked in
/// it, kept marked as the hotkeys change.
#[derive(Component)]
#[require(Marked)]
pub struct Named {
    action: &'static str,
    name: String,
}

impl Named {
    pub fn new(bindings: &Bindings, action: &'static str, name: String) -> (Named, Marked) {
        let marked = bindings.marked(action, &name);
        (Named { action, name }, marked)
    }
}

/// Marks each [`Named`] button's key afresh once the hotkeys change.
pub fn mark(bindings: Res<Bindings>, mut buttons: Query<(&Named, &mut Marked)>) {
    for (named, mut marked) in &mut buttons {
        marked.set_if_neq(bindings.marked(named.action, &named.name));
    }
}

/// Asks for what each key just pressed gives where it's heard: in the
/// match, or in the display panel that's up.
pub fn send(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    displays: Res<crate::displays::Displays>,
    politics: Option<Res<crate::politics::Politics>>,
    mut intents: MessageWriter<Intent>,
) {
    if keys.get_just_pressed().next().is_none() {
        return;
    }
    let window = displays.window();
    let beside = politics
        .is_some_and(|p| p.open)
        .then_some(crate::politics::WIDGET);
    let heard: Vec<_> = (bindings.just_pressed(&keys))
        .filter(|(_, scope)| {
            scope.heard(window, |w| displays.passes(w))
                || beside.is_some_and(|place| scope.given_in(place))
        })
        .map(|(action, _)| Intent::from(action))
        .collect();
    // Esc closes a panel the hotkey files give no key of its own to, as
    // they do for most.
    let unbound = heard.is_empty() && keys.just_pressed(KeyCode::Escape);
    let close = (window.is_some() && unbound).then(|| Intent::from(crate::displays::CLOSE_ALL));
    intents.write_batch(heard.into_iter().chain(close));
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
    let mut widgets = std::collections::HashSet::new();
    fn named(widget: &tgdata::widgets::Widget, out: &mut std::collections::HashSet<String>) {
        out.insert(widget.name.to_ascii_lowercase());
        // The hotkey files name a widget within a display after the
        // display, as `CompanyFormationButtons` for the company display's
        // `FormationButtons`.
        let display = widget.name.strip_suffix("Display");
        for child in &widget.children {
            if let Some(display) = display {
                out.insert(format!("{display}{}", child.name).to_ascii_lowercase());
            }
            named(child, out);
        }
    }
    game.game_widgets()
        .iter()
        .for_each(|w| named(w, &mut widgets));
    let bindings = Bindings::new(&hotkeys, &game.strings()).on_screen(&widgets);
    commands.insert_resource(bindings);
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

    fn scoped(target: &str, command: Option<&str>) -> Scope {
        Scope::of(&Hotkey {
            chord: "a".into(),
            target: target.into(),
            command: command.map(str::to_string),
        })
    }

    #[test]
    fn keys_are_heard_where_the_files_scope_them() {
        let none = |_: &str| false;
        let command = scoped("game|action|verify", Some("ActorCommand move"));
        assert!(command.heard(None, none));
        assert!(!command.heard(Some("ObjectivesDisplay"), none));
        let close = scoped("ObjectivesDisplay", Some("ObjectivesDisplayClose"));
        assert!(!close.heard(None, none));
        assert!(close.heard(Some("ObjectivesDisplay"), none));
        assert!(!close.heard(Some("TechnologyDisplay"), none));
        let root = scoped("root", Some("screenshot"));
        assert!(root.heard(None, none) && root.heard(Some("ObjectivesDisplay"), none));
        assert!(!scoped("editbox", Some("EditboxCopy")).heard(None, none));
        let button = scoped("ButtonObjectives", None);
        assert!(button.heard(None, none));
        assert!(!button.heard(Some("TechnologyDisplay"), none));
        assert!(button.heard(Some("TechnologyDisplay"), |_| true));
    }

    #[test]
    fn widgets_off_the_screen_lose_their_keys() {
        let press = |target: &str| Hotkey {
            chord: "q".into(),
            target: target.into(),
            command: None,
        };
        let hotkeys = [
            press("MainMenuButtonQuit"),
            press("ActionButtonsButton_0|action"),
            hotkey("m", "ActorCommand move"),
        ];
        let widgets = ["actionbuttonsbutton".to_string()].into();
        let bindings = Bindings::new(&hotkeys, &tgdata::Strings::default()).on_screen(&widgets);
        let actions: Vec<&str> = bindings.0.iter().map(|(_, a, ..)| a.as_str()).collect();
        assert_eq!(actions, ["ActionButtonsButton_0", "ActorCommand move"]);
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
            &tgdata::Strings::default(),
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
        let strings =
            tgdata::Strings::from_iter([("key_delete".to_string(), "Delete".to_string())]);
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
