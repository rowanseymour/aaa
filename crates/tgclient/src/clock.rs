//! The keys that pause the match and set how fast it's played, and the
//! button on the minimap's rim that pauses it as its key does: Escape's
//! menu leaves a match hosted elsewhere playing on.

use std::ops::RangeInclusive;

use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};
use bevy::ui_widgets::Button;
use tgsim::Action;

use crate::actions::Blocks;
use crate::displays::{self, Displays, Rim};
use crate::intent::{Asked, Asks, Intent};
use crate::keys::{Bindings, Named};
use crate::ui::{Click, Edge, Hint, Ink, Surface, Theme};
use crate::widgets::Pressable;
use crate::{Loaded, Orders, Screen};

/// What the hotkey files call pausing or unpausing, stepping the speed up
/// or down, and setting it.
pub(crate) const PAUSE: &str = "gamepause";
const SPEED_UP: &str = "SpeedUp";
const SPEED_DOWN: &str = "SpeedDown";
const SPEED: &str = "GameSpeed";

/// Pauses or unpauses the match, or steps or sets its speed, as asked.
pub fn keys(
    mut intents: MessageReader<Intent>,
    mut orders: ResMut<Orders>,
    loaded: Res<Loaded>,
    mut displays: Option<ResMut<Displays>>,
) {
    let asked = Asked::read(&mut intents);
    if asked.is_empty() {
        return;
    }
    let speeds = loaded.game.game_speeds();
    // Pausing goes by the match as it is, not as asked, so that pressing
    // again before the host answers flips a vote back as each press does.
    let (paused, speed) = (orders.paused(), orders.speeding());
    let wanted = wanted(|name| asked.wants(name), paused, speed, &speeds);
    ask(&mut orders, displays.as_deref_mut(), wanted);
}

/// Asks for those of `actions` the match's host would carry out. A pause
/// asked for while a panel's up holds once it closes, as the panel only
/// lets go of a pause it made itself.
pub(crate) fn ask(orders: &mut Orders, mut displays: Option<&mut Displays>, actions: Vec<Action>) {
    for action in actions {
        if !orders.may(&action) {
            continue;
        }
        if action == Action::Pause(true)
            && let Some(displays) = displays.as_deref_mut()
        {
            displays.keep_paused();
        }
        orders.act(action);
    }
}

/// What the keys `wants` names were pressed for ask of a match `paused`
/// or not and played `speed` times as fast, within `speeds`.
fn wanted(
    wants: impl Fn(&str) -> bool,
    paused: bool,
    speed: f32,
    speeds: &RangeInclusive<f32>,
) -> Vec<Action> {
    let now = speed.log2();
    let mut wanted = Vec::new();
    if wants(PAUSE) {
        wanted.push(Action::Pause(!paused));
    }
    for (name, to) in [
        (SPEED_UP, stepped(now, 0.5, speeds)),
        (SPEED_DOWN, stepped(now, -0.5, speeds)),
        (SPEED, 0.0f32.clamp(*speeds.start(), *speeds.end())),
    ] {
        if wants(name) {
            wanted.push(Action::Speed(to.exp2()));
        }
    }
    wanted
}

/// The speed, as a power of two, a step `by` from `now` comes to: from
/// `now` to the nearest half, kept within `speeds`.
fn stepped(now: f32, by: f32, speeds: &RangeInclusive<f32>) -> f32 {
    let nearest = (now * 2.0).round() / 2.0;
    (nearest + by).clamp(*speeds.start(), *speeds.end())
}

/// What either interface's pause button is [`called`](crate::ui::called),
/// so a script works with both.
pub(crate) const PAUSE_BUTTON: &str = "clock.pause";

/// A button, of either interface, that pauses and unpauses the match.
#[derive(Component, Default)]
pub struct Pauses;

/// What the pause buttons show: whether the match is paused, whoever
/// paused it, and whether the player may pause it at all.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct PauseState {
    pub paused: bool,
    pub usable: bool,
}

/// Keeps [`PauseState`] to the match's.
pub fn follow(orders: Res<Orders>, mut state: ResMut<PauseState>) {
    state.set_if_neq(PauseState {
        paused: orders.paused(),
        usable: orders.allowed(&Action::Pause(true)),
    });
}

/// Lets the pause buttons be pressed only by those who may pause, such
/// as not anyone only watching.
pub fn disable_buttons(
    mut commands: Commands,
    state: Res<PauseState>,
    buttons: Query<(Entity, Has<InteractionDisabled>), With<Pauses>>,
) {
    for (id, disabled) in &buttons {
        match (state.usable, disabled) {
            (true, true) => _ = commands.entity(id).remove::<InteractionDisabled>(),
            (false, false) => _ = commands.entity(id).insert(InteractionDisabled),
            _ => {}
        }
    }
}

/// Our interface's pause button, a glyph on the minimap's rim.
#[derive(Component)]
pub struct PauseButton;

/// The glyph on the [`PauseButton`].
#[derive(Component)]
pub struct PauseGlyph;

/// Spawns the pause button.
pub fn spawn_button(
    mut commands: Commands,
    loaded: Res<Loaded>,
    bindings: Res<Bindings>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    mut icons: ResMut<displays::Icons>,
) {
    let strings = loaded.game.strings();
    let said = crate::ui::said(&strings);
    let name = said("control_panel_ButtonPause_ToolTip", "Pause Game");
    let help = said("control_panel_ButtonPause_tooltip_2", "Pauses the game.");
    let glyph = icons.get(displays::icon!("pause"), &mut images, formats.0);
    commands.spawn((
        Blocks,
        Button,
        Click::TOGGLE,
        RelativeCursorPosition::default(),
        Named::new(&bindings, PAUSE, name),
        (Asks::new(PAUSE), Hint(help)),
        crate::ui::called(PAUSE_BUTTON),
        (Pauses, PauseButton),
        Rim::TopLeft.node(),
        Surface::Clear,
        Edge::Border,
        GlobalZIndex(2),
        DespawnOnExit(Screen::Match),
        children![(
            PauseGlyph,
            ImageNode::new(glyph),
            Node {
                width: px(displays::GLYPH_SIZE),
                height: px(displays::GLYPH_SIZE),
                ..default()
            },
            Pickable::IGNORE,
        )],
    ));
}

/// What [`light_button`] reads and sets on the button.
type ButtonLook = (
    Ref<'static, PauseButton>,
    Ref<'static, Hovered>,
    Has<bevy::ui::Pressed>,
    &'static mut Edge,
    &'static mut Surface,
    &'static Children,
);

/// Lights our pause button while the match is paused, its edge while it's
/// hovered and its face while it's held down, and greys it for those who
/// may not pause.
pub fn light_button(
    state: Res<PauseState>,
    theme: Res<Theme>,
    mut buttons: Query<ButtonLook>,
    mut glyphs: Query<&mut ImageNode, With<PauseGlyph>>,
) {
    let PauseState { paused, usable } = *state;
    for (button, hovered, pressed, mut edge, mut surface, children) in &mut buttons {
        surface.set_if_neq(match pressed && usable {
            true => Surface::Hovered,
            false => Surface::Clear,
        });
        let restyle = state.is_changed() || theme.is_changed() || button.is_added();
        if restyle || hovered.is_changed() {
            edge.set_if_neq(Edge::lit(paused, hovered.get() && usable, Edge::Border));
        }
        if !restyle {
            continue;
        }
        let colour = match (paused, usable) {
            (true, _) => theme.edge(Edge::Lit),
            (false, true) => theme.ink(Ink::Help),
            (false, false) => theme.ink(Ink::Muted),
        };
        for &child in children {
            if let Ok(mut glyph) = glyphs.get_mut(child) {
                glyph.color = colour;
            }
        }
    }
}

/// Switches the original interface's pause button on while the match is
/// paused.
pub fn select_original_button(
    state: Res<PauseState>,
    mut buttons: Query<(Ref<Pauses>, &mut Pressable)>,
) {
    for (pauses, mut button) in &mut buttons {
        if state.is_changed() || pauses.is_added() {
            button.selected = state.paused;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_original_pause_button_follows_the_pause_and_who_may_pause() {
        let mut app = App::new();
        app.init_resource::<PauseState>()
            .add_systems(Update, (disable_buttons, select_original_button));
        let button = app.world_mut().spawn((Pauses, Pressable::default())).id();
        let set = |app: &mut App, paused, usable| {
            *app.world_mut().resource_mut::<PauseState>() = PauseState { paused, usable };
            app.update();
            let world = app.world();
            let selected = world.get::<Pressable>(button).unwrap().selected;
            (selected, world.get::<InteractionDisabled>(button).is_some())
        };
        assert_eq!(set(&mut app, true, true), (true, false));
        assert_eq!(set(&mut app, false, false), (false, true));
        assert_eq!(set(&mut app, true, true), (true, false));
    }

    #[test]
    fn each_key_asks_for_its_pause_or_speed() {
        let speeds = -2.0..=2.0;
        let pressed = |key: &'static str| move |name: &str| name == key;
        let asks = |key, paused, speed| wanted(pressed(key), paused, speed, &speeds);
        assert_eq!(asks(PAUSE, false, 1.0), [Action::Pause(true)]);
        // Paused, pressing again asks to unpause, which for anyone but the
        // pauser is a vote, however soon after.
        assert_eq!(asks(PAUSE, true, 1.0), [Action::Pause(false)]);
        assert_eq!(asks(SPEED_UP, false, 1.0), [Action::Speed(2f32.sqrt())]);
        assert_eq!(asks(SPEED_DOWN, false, 2.0), [Action::Speed(2f32.sqrt())]);
        assert_eq!(asks(SPEED, true, 4.0), [Action::Speed(1.0)]);
        assert_eq!(asks("ActorCommand stop", false, 1.0), []);
    }

    #[test]
    fn a_step_goes_from_the_nearest_half_and_stays_within_the_speeds() {
        let speeds = -2.0..=2.0;
        assert_eq!(stepped(0.0, 0.5, &speeds), 0.5);
        assert_eq!(stepped(0.0, -0.5, &speeds), -0.5);
        // 3 times as fast is nearest 2^1.5.
        assert_eq!(stepped(3.0f32.log2(), 0.5, &speeds), 2.0);
        assert_eq!(stepped(2.0, 0.5, &speeds), 2.0);
        assert_eq!(stepped(-2.0, -0.5, &speeds), -2.0);
        // From beyond them, back within.
        assert_eq!(stepped(4.0, -0.5, &speeds), 2.0);
        // Nine speeds from slowest to fastest.
        let mut at = -2.0;
        let mut seen = vec![at];
        while at < 2.0 {
            at = stepped(at, 0.5, &speeds);
            seen.push(at);
        }
        assert_eq!(seen.len(), 9);
    }
}
