//! Telling the map's triggers what the player does in the interface: the
//! commands they listen for, and what's selected while any tests that.
//! Both go as orders, so every copy of the match hears them on one tick.

use bevy::prelude::*;
use tgsim::{Command, Piece};

use crate::intent::Intent;
use crate::{Local, Orders, Screen, Selected, SelectedBuilding, Sim};

pub struct HeardPlugin;

impl Plugin for HeardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Said>()
            .add_systems(OnEnter(Screen::Match), |mut said: ResMut<Said>| {
                said.0 = None
            })
            .add_systems(
                Update,
                (tell_heard, tell_selected).run_if(in_state(Screen::Match)),
            );
    }
}

/// The selection last told to the match, which plays it only later.
#[derive(Resource, Default)]
struct Said(Option<Vec<Piece>>);

/// Tells the triggers of each command asked for that they listen for.
fn tell_heard(
    mut intents: MessageReader<Intent>,
    sim: Res<Sim>,
    local: Res<Local>,
    orders: Option<ResMut<Orders>>,
) {
    let asked: Vec<&Intent> = intents.read().collect();
    let (Some(player), Some(mut orders)) = (local.0, orders) else {
        return;
    };
    let listening = &sim.0.triggers.listening;
    for Intent(asked) in asked {
        let command = runs(asked);
        if listening.contains(&command.to_lowercase()) {
            let heard = Command::Heard {
                command: command.to_string(),
            };
            _ = orders.give(&sim.0, player, heard);
        }
    }
}

/// The command what's asked for runs: a widget, where the interface asks
/// by its name, runs the original's command for it.
fn runs(asked: &str) -> &str {
    RUNS.iter()
        .find(|(widget, _)| asked.eq_ignore_ascii_case(widget))
        .map_or(asked, |&(_, command)| command)
}

/// The widgets the interface asks for by name, with the command each runs.
const RUNS: &[(&str, &str)] = &[("ButtonMessageLog", "MessageLogToggle")];

/// Tells the triggers what's selected whenever it changes, while any
/// trigger tests it.
fn tell_selected(
    selected: Res<Selected>,
    building: Res<SelectedBuilding>,
    sim: Res<Sim>,
    local: Res<Local>,
    orders: Option<ResMut<Orders>>,
    mut said: ResMut<Said>,
) {
    let (Some(player), Some(mut orders)) = (local.0, orders) else {
        return;
    };
    if !sim.0.triggers.watches_selection {
        return;
    }
    let pieces = pieces(&selected.0, building.0);
    if said.0.as_ref() == Some(&pieces) {
        return;
    }
    if orders
        .give(
            &sim.0,
            player,
            Command::Select {
                pieces: pieces.clone(),
            },
        )
        .is_ok()
    {
        said.0 = Some(pieces);
    }
}

/// The selection as the triggers take it.
fn pieces(organizations: &[usize], building: Option<usize>) -> Vec<Piece> {
    let organizations = organizations.iter().map(|&r| Piece::Organization(r));
    organizations.chain(building.map(Piece::Building)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_widget_runs_its_command_and_a_command_itself() {
        assert_eq!(runs("ButtonMessageLog"), "MessageLogToggle");
        assert_eq!(runs("ActorCommand move"), "ActorCommand move");
    }

    #[test]
    fn a_selection_is_its_organizations_or_its_building() {
        assert_eq!(
            pieces(&[2, 0], None),
            [Piece::Organization(2), Piece::Organization(0)]
        );
        assert_eq!(pieces(&[], Some(3)), [Piece::Building(3)]);
    }
}
