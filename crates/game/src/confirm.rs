//! A question asked over everything else and answered Ok or Cancel, as the
//! game asks before doing what can't be undone, such as quitting.

use std::collections::HashMap;

use bevy::ecs::observer::IntoEntityObserver;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::Screen;
use crate::ui::{self, Font, layer};

/// What the dialog's buttons say, and the game's question.
#[derive(Resource)]
pub struct Words {
    ok: String,
    cancel: String,
    /// The game's question before what can't be undone.
    pub sure: String,
}

impl Words {
    pub fn of(strings: &HashMap<String, String>) -> Self {
        let said = crate::ui::said(strings);
        Words {
            ok: said("button_ok", "Ok"),
            cancel: said("button_cancel", "Cancel"),
            sure: said("important_command_confirm", "Are you sure?"),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&HashMap::new())
    }
}

/// The scrim a question is asked on, which goes when it's answered.
#[derive(Component)]
pub(crate) struct Question;

pub struct ConfirmPlugin;

impl Plugin for ConfirmPlugin {
    fn build(&self, app: &mut App) {
        // Ahead of everything else that reads Escape, so the press that
        // cancels a question doesn't also close what it was asked over; but
        // not under the console, whose Escape it is.
        app.add_systems(
            PreUpdate,
            cancel.after(InputSystems).run_if(crate::console::closed),
        );
    }
}

/// Asks `question` on `screen`, running `ok` if it's answered Ok; either
/// answer, Escape, or leaving the screen takes the question down.
pub fn confirm<M>(
    commands: &mut Commands,
    words: &Words,
    screen: Screen,
    question: impl Into<String>,
    ok: impl IntoEntityObserver<M>,
) {
    let root = commands
        .spawn((Question, DespawnOnExit(screen), ui::scrim(layer::QUESTION)))
        .id();
    // Already gone if Escape or the other answer took it down this frame.
    let dismiss = move |_: On<Activate>, mut commands: Commands| {
        commands.entity(root).try_despawn();
    };
    let dialog = commands
        .spawn((
            ChildOf(root),
            ui::panel(
                ui::Surface::Panel,
                Node {
                    width: px(480),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(px(32)),
                    row_gap: px(32),
                    ..default()
                },
            ),
        ))
        .id();
    commands.spawn((ui::text(question, Font::Large), ChildOf(dialog)));
    let row = commands
        .spawn((
            ChildOf(dialog),
            Node {
                width: percent(100),
                column_gap: px(32),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((answer(words.ok.clone()), ChildOf(row)))
        .observe(ok)
        .observe(dismiss);
    commands
        .spawn((answer(words.cancel.clone()), ChildOf(row)))
        .observe(dismiss);
}

/// A [`ui::button`] sharing its row equally with the other answer.
fn answer(words: String) -> impl Bundle {
    let node = Node {
        flex_grow: 1.0,
        flex_basis: px(0),
        ..ui::padded()
    };
    ui::shaped_button(words, Font::Large, node)
}

/// Takes the questions down on Escape, as Cancel would.
fn cancel(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    questions: Query<Entity, With<Question>>,
    mut commands: Commands,
) {
    if questions.is_empty() || !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    keys.clear_just_pressed(KeyCode::Escape);
    questions
        .iter()
        .for_each(|q| commands.entity(q).try_despawn());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct Answered(bool);

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ConfirmPlugin))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::console::Console>()
            .init_resource::<Answered>();
        app.update();
        let world = app.world_mut();
        let words = Words::default();
        let mut commands = world.commands();
        confirm(
            &mut commands,
            &words,
            Screen::MainMenu,
            "Sure?",
            |_: On<Activate>, mut a: ResMut<Answered>| {
                a.0 = true;
            },
        );
        world.flush();
        app
    }

    fn press(app: &mut App, words: &str) {
        let world = app.world_mut();
        let mut labels = world.query::<(&Text, &ChildOf)>();
        let found = labels.iter(world).find(|(text, _)| text.0 == words);
        let entity = found.expect("no such button").1.parent();
        world.trigger(Activate { entity });
        app.update();
    }

    fn asking(app: &mut App) -> bool {
        let world = app.world_mut();
        world.query::<&Question>().iter(world).count() > 0
    }

    fn answered(app: &App) -> bool {
        app.world().resource::<Answered>().0
    }

    #[test]
    fn ok_does_it_and_takes_the_question_down() {
        let mut app = app();
        assert!(asking(&mut app));
        press(&mut app, "Ok");
        assert!(answered(&app) && !asking(&mut app));
    }

    #[test]
    fn cancel_takes_the_question_down_without_doing_it() {
        let mut app = app();
        press(&mut app, "Cancel");
        assert!(!answered(&app) && !asking(&mut app));
    }

    #[test]
    fn escape_cancels_and_is_used_up() {
        let mut app = app();
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::Escape);
        app.update();
        assert!(!answered(&app) && !asking(&mut app));
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(!keys.just_pressed(KeyCode::Escape));
    }
}
