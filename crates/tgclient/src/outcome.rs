//! Telling the player how the match has come out for their side, the
//! moment it's decided, or that it's ended with its host going.

use bevy::prelude::*;
use tgsim::Outcome;

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
    kicked: String,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let said = crate::ui::said(strings);
        Words {
            won: said("ui_mission_message_won", "Mission Won"),
            lost: said("ui_mission_message_lost", "Mission Lost"),
            end: said("ui_mission_label_end_game", "End Mission"),
            keep_playing: said("ui_mission_label_keep_playing", "Keep Playing"),
            host_gone: "The host has left the game".into(),
            kicked: said(
                "networking_disconnect_kicked",
                "You were kicked from the game",
            ),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
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
            .add_systems(OnEnter(Screen::Loading), |mut told: ResMut<Told>| {
                *told = Told::default()
            })
            .add_systems(
                Update,
                (
                    tell.run_if(resource_exists::<Sim>)
                        .run_if(in_state(Screen::Match)),
                    // A host elsewhere can be lost, or kick the game out,
                    // while its copy of the match is still being set up.
                    tell_host_gone
                        .run_if(resource_exists::<Orders>)
                        .run_if(in_state(Screen::Loading).or_else(in_state(Screen::Match))),
                ),
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
/// stood: to look over, or to end. Or that the game was kicked out of the
/// match, which leaves the game once that's taken in.
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
    let ok = confirm.ok.clone();
    if orders.kicked() {
        let asked = ask(&mut commands, Screen::Match, words.kicked.clone());
        offer(&mut commands, asked, ok, "outcome.ok", menu::quit);
        return;
    }
    let asked = ask(&mut commands, Screen::Match, words.host_gone.clone());
    let end = words.end.clone();
    offer(&mut commands, asked, end, "outcome.end", ask_end);
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
    use bevy::state::app::StatesPlugin;
    use tgmap::{Domain, Pathing};
    use tgsim::{Sides, World};

    /// A match of sides 1 and 2, the player as 1.
    fn app() -> App {
        let sides = Sides {
            names: Default::default(),
            computers: Default::default(),
            players: [1, 2].map(crate::tests::player).into(),
            teams: Vec::new(),
        };
        let sim = World::from_organizations(sides, Pathing::uniform(8, Domain::Land), &[]);
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
    fn a_game_kicked_out_is_told_so_and_may_only_leave() {
        let mut app = app();
        let (mut host, mut orders, mut world) = crate::orders::testing::joined(&[1]);
        // Kept, as those who leave take their votes with them.
        let _others = [2, 3].map(|p| {
            let (link, _) = crate::orders::testing::let_in(&mut host, &[p]);
            let mut other = tgserver::Client::new(link);
            other.kick(orders.me(), true).unwrap();
            other
        });
        host.turn();
        for _ in 0..2 {
            orders.step(&mut world);
        }
        app.insert_resource(orders);
        app.update();
        assert_eq!(shown(&mut app), ["Ok", "You were kicked from the game"]);
    }

    #[test]
    fn a_game_kicked_out_while_its_match_loads_is_told_so_then() {
        let mut app = app();
        app.world_mut().remove_resource::<Sim>();
        let mut screen = app.world_mut().resource_mut::<NextState<Screen>>();
        screen.set(Screen::Loading);
        app.update();
        let (mut host, mut orders, _) = crate::orders::testing::joined(&[1]);
        let _others = [2, 3].map(|p| {
            let (link, _) = crate::orders::testing::let_in(&mut host, &[p]);
            let mut other = tgserver::Client::new(link);
            other.kick(orders.me(), true).unwrap();
            other
        });
        host.wait();
        for _ in 0..2 {
            orders.wait();
        }
        app.insert_resource(orders);
        app.update();
        assert_eq!(shown(&mut app), ["Ok", "You were kicked from the game"]);
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
