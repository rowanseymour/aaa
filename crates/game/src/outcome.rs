//! Telling the player how the match has come out for their side, the
//! moment it's decided, or that it's ended with its host going.

use aaa_sim::Outcome;
use bevy::prelude::*;

use bevy::ui_widgets::Activate;

use crate::confirm::{self, ask, nothing, offer};
use crate::{Local, Orders, Screen, Sim, menu};

/// What the dialog says.
#[derive(Resource)]
pub struct Words {
    won: String,
    lost: String,
    end: String,
    keep_playing: String,
    /// Not the game's words: there, another machine takes over as host.
    host_gone: String,
}

impl Words {
    pub fn of(strings: &aaa_data::Strings) -> Self {
        let said = crate::ui::said(strings);
        Words {
            won: said("ui_mission_message_won", "Mission Won"),
            lost: said("ui_mission_message_lost", "Mission Lost"),
            end: said("ui_mission_label_end_game", "End Mission"),
            keep_playing: said("ui_mission_label_keep_playing", "Keep Playing"),
            host_gone: "The host has left the game".into(),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&aaa_data::Strings::default())
    }
}

/// What the player has been told, so as to tell it once.
#[derive(Resource, Default)]
struct Told {
    /// The side whose outcome they've been told.
    side: Option<u32>,
    /// Whether they've been told the match's host has gone.
    host_gone: bool,
}

pub struct OutcomePlugin;

impl Plugin for OutcomePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Told>()
            .add_systems(OnEnter(Screen::Match), |mut told: ResMut<Told>| {
                *told = Told::default()
            })
            .add_systems(
                Update,
                (
                    tell.run_if(resource_exists::<Sim>),
                    tell_host_gone.run_if(resource_exists::<Orders>),
                )
                    .run_if(in_state(Screen::Match)),
            );
    }
}

/// Puts the outcome of the player's side up once it's decided. A winner
/// may play on; either may end the mission.
fn tell(
    mut commands: Commands,
    sim: Res<Sim>,
    local: Res<Local>,
    words: Res<Words>,
    mut told: ResMut<Told>,
) {
    let Some(side) = local.0.filter(|&side| told.side != Some(side)) else {
        return;
    };
    let won = match sim.0.outcome(side) {
        Outcome::Playing => return,
        Outcome::Won => true,
        Outcome::Lost => false,
    };
    told.side = Some(side);
    let message = if won { &words.won } else { &words.lost };
    let asked = ask(&mut commands, Screen::Match, message.clone());
    offer(
        &mut commands,
        asked,
        words.end.clone(),
        "outcome.end",
        ask_end,
    );
    if won {
        let keep = words.keep_playing.clone();
        offer(&mut commands, asked, keep, "outcome.keep", nothing);
    }
}

/// Says that the match's host has gone, which leaves the match where it
/// stood: to look over, or to end.
fn tell_host_gone(
    mut commands: Commands,
    orders: Res<Orders>,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    mut told: ResMut<Told>,
) {
    if !orders.gone() || told.host_gone {
        return;
    }
    told.host_gone = true;
    let asked = ask(&mut commands, Screen::Match, words.host_gone.clone());
    let end = words.end.clone();
    offer(&mut commands, asked, end, "outcome.end", ask_end);
    let ok = confirm.ok.clone();
    offer(&mut commands, asked, ok, "outcome.ok", nothing);
}

/// Asks whether to end the mission, which leaves the game, and does if so.
fn ask_end(_: On<Activate>, mut commands: Commands, words: Res<confirm::Words>) {
    let question = words.sure.clone();
    confirm::confirm(&mut commands, &words, Screen::Match, question, menu::quit);
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_map::{Domain, Pathing};
    use aaa_sim::{Sides, World};
    use bevy::state::app::StatesPlugin;

    /// A match of sides 1 and 2, the player as 1.
    fn app() -> App {
        let sides = Sides {
            players: [1, 2].map(crate::tests::player).into(),
            teams: Vec::new(),
        };
        let sim = World::from_regiments(sides, Pathing::uniform(8, Domain::Land), &[]);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, OutcomePlugin))
            .insert_state(Screen::Match)
            .insert_resource(Sim(sim))
            .insert_resource(Local(Some(1)))
            .insert_resource(Words::default())
            .insert_resource(confirm::Words::default());
        app.update();
        app
    }

    /// The words on screen, in alphabetical order.
    fn shown(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        let mut texts = world.query::<&Text>();
        let mut shown: Vec<String> = texts.iter(world).map(|t| t.0.clone()).collect();
        shown.sort();
        shown
    }

    #[test]
    fn nothing_is_said_while_the_side_plays_on() {
        let mut app = app();
        assert!(shown(&mut app).is_empty());
    }

    #[test]
    fn a_winner_is_told_once_and_may_play_on() {
        let mut app = app();
        app.world_mut().resource_mut::<Sim>().0.win(1);
        app.update();
        app.update();
        assert_eq!(
            shown(&mut app),
            ["End Mission", "Keep Playing", "Mission Won"]
        );
    }

    #[test]
    fn a_loser_may_only_end_the_mission() {
        let mut app = app();
        app.world_mut().resource_mut::<Sim>().0.win(2);
        app.update();
        assert_eq!(shown(&mut app), ["End Mission", "Mission Lost"]);
    }

    #[test]
    fn ending_the_mission_asks_first() {
        let mut app = app();
        app.world_mut().resource_mut::<Sim>().0.win(2);
        app.update();
        let world = app.world_mut();
        let mut labels = world.query::<(&Text, &ChildOf)>();
        let end = labels.iter(world).find(|(text, _)| text.0 == "End Mission");
        let entity = end.unwrap().1.parent();
        world.trigger(Activate { entity });
        app.update();
        assert_eq!(shown(&mut app), ["Are you sure?", "Cancel", "Ok"]);
    }
}
