//! Asking the person playing what they go by: on first looking for games on
//! the network, and from Change Name in a game's lobby, which has the host
//! rename them too.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::fields::{Entered, Field, Focused, field};
use crate::lan::Staged;
use crate::{Nickname, NicknameSet, Screen, confirm};

/// What the prompts say, as the game's data has it.
#[derive(Resource)]
pub struct Words {
    first: String,
    change: String,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let said = crate::ui::said(strings);
        Words {
            first: said("ui_lobby_prompt_lan_nickname", "Enter a nickname"),
            change: said("staging_name_change_prompt", "Change Name"),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
    }
}

/// The box a name is typed in, whether it's for a game's lobby, and what
/// takes its question down.
#[derive(Component)]
struct NameBox {
    lobby: bool,
    question: Entity,
}

/// Asks on the LAN screen for a nickname until one is set, filled in with
/// the one gone by until now. Typing isn't stopped at a name's length:
/// what's entered is cut to it after.
pub(crate) fn ask_first(
    mut commands: Commands,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    nickname: Res<Nickname>,
    set: Res<NicknameSet>,
) {
    if !set.0 {
        let question = words.first.clone();
        ask(
            &mut commands,
            &confirm,
            Screen::Lan,
            question,
            &nickname.0,
            None,
        );
    }
}

/// Asks in a game's lobby for the name to go by there, and from then on.
pub(crate) fn ask_change(
    _: On<Activate>,
    mut commands: Commands,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    nickname: Res<Nickname>,
) {
    let question = words.change.clone();
    let longest = Some(tgserver::NAME_LONGEST);
    ask(
        &mut commands,
        &confirm,
        Screen::Lobby,
        question,
        &nickname.0,
        longest,
    );
}

fn ask(
    commands: &mut Commands,
    confirm: &confirm::Words,
    screen: Screen,
    question: String,
    nickname: &str,
    longest: Option<usize>,
) {
    let asked = confirm::ask(commands, screen, question);
    let lobby = screen == Screen::Lobby;
    let typed = field(nickname, longest.unwrap_or(usize::MAX), percent(100));
    let id = commands
        .spawn((
            ChildOf(asked.dialog()),
            typed,
            NameBox {
                lobby,
                question: asked.root(),
            },
            crate::ui::called("naming.name"),
        ))
        .observe(enter)
        .id();
    commands.insert_resource(Focused(Some(id)));
    confirm::offer(commands, asked, confirm.ok.clone(), "naming.ok", ok);
    confirm::offer(
        commands,
        asked,
        confirm.cancel(),
        "naming.cancel",
        confirm::nothing,
    );
}

/// What going by a name changes: the nickname, kept with the options, and
/// in a lobby the name the host has for the person.
#[derive(SystemParam)]
struct Renaming<'w, 's> {
    boxes: Query<'w, 's, (&'static Field, &'static NameBox)>,
    nickname: ResMut<'w, Nickname>,
    set: ResMut<'w, NicknameSet>,
    staged: Option<ResMut<'w, Staged>>,
}

impl Renaming<'_, '_> {
    /// Goes by the name what's typed in the box `of` makes, if it makes
    /// one; else the old name stands.
    fn take(&mut self, of: Entity) {
        let Ok((field, name_box)) = self.boxes.get(of) else {
            return;
        };
        let Some(name) = tgserver::name(&field.text) else {
            return;
        };
        if name_box.lobby
            && let Some(staged) = &mut self.staged
        {
            let counting = staged.entry.staging().countdown.is_some();
            // Sent all the same, so the host says why it's refused.
            let _ = staged.entry.choose(tgserver::Choice::Name(name.clone()));
            if counting {
                return;
            }
        }
        self.nickname.set_if_neq(Nickname(name));
        self.set.set_if_neq(NicknameSet(true));
    }
}

fn ok(_: On<Activate>, boxes: Query<Entity, With<NameBox>>, mut renaming: Renaming) {
    if let Some(of) = boxes.iter().next() {
        renaming.take(of);
    }
}

/// Enter in the box answers Ok.
fn enter(entered: On<Entered>, mut commands: Commands, mut renaming: Renaming) {
    renaming.take(entered.entity);
    if let Ok((_, name_box)) = renaming.boxes.get(entered.entity) {
        commands.entity(name_box.question).try_despawn();
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::state::app::StatesPlugin;

    use super::*;

    fn app(set: bool) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            bevy::input::InputPlugin,
            crate::fields::FieldsPlugin,
        ))
        .insert_state(Screen::Lan)
        .init_resource::<Words>()
        .init_resource::<confirm::Words>()
        .insert_resource(Nickname("Player".into()))
        .insert_resource(NicknameSet(set));
        app.update();
        app.world_mut().run_system_once(ask_first).unwrap();
        app.update();
        app
    }

    /// Types `text` in the box and presses Enter, if the box is up.
    fn enter(app: &mut App, text: &str) -> bool {
        let world = app.world_mut();
        let mut boxes = world.query_filtered::<(Entity, &mut Field), With<NameBox>>();
        let Some((entity, mut field)) = boxes.iter_mut(world).next() else {
            return false;
        };
        field.text = text.into();
        world.trigger(Entered { entity });
        app.update();
        true
    }

    fn named(app: &App) -> (&str, bool) {
        let world = app.world();
        (
            &world.resource::<Nickname>().0,
            world.resource::<NicknameSet>().0,
        )
    }

    #[test]
    fn the_first_nickname_is_asked_for_until_one_is_set() {
        let mut first = app(false);
        // Nothing but spaces is no name: the old one stands, still unset.
        assert!(enter(&mut first, "   "));
        assert_eq!(named(&first), ("Player", false));
        first.world_mut().run_system_once(ask_first).unwrap();
        first.update();
        // Cut to a name's length after it's entered, then trimmed.
        assert!(enter(&mut first, "  A name far too long to keep"));
        assert_eq!(named(&first), ("A name far too l", true));
        // Once set, never again.
        assert!(!enter(&mut app(true), "Bob"));
    }
}
