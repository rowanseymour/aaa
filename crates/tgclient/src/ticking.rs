//! Stepping the sim: the orders carried out each tick, and the ticks
//! held back until the match is shown or a host elsewhere has sent them.

use std::time::Duration;

use bevy::prelude::*;
use tgsim::{State, World};

use crate::orders::Orders;
use crate::{Screen, Selected, SelectedBuilding, Sim, messages};

/// Says which of the orders carried out were refused.
pub(crate) fn log_refused(refused: Vec<(u32, tgsim::Refused)>) {
    for (player, e) in refused {
        info!("player {player}'s order refused: {e:?}");
    }
}

/// Carries out orders as soon as they're given, ticking or not, so that
/// what shows them sees them taken up.
pub(crate) fn carry_out(mut sim: ResMut<Sim>, mut orders: ResMut<Orders>) {
    // Those given this frame as one group go together.
    orders.send_gathered();
    log_refused(orders.carry_out(&mut sim.0));
}

/// The match has been drawn once, so the sim may step: its first tick
/// comes a tick after that frame, however long the frame took.
#[derive(Resource)]
pub(crate) struct Shown;

/// Tells who goes and what's said in a match, from when it starts loading:
/// a game joined to a match hosted elsewhere keeps in touch with its host
/// while the match loads, as it does once the match is played.
pub(crate) struct InTouch;

/// What [`InTouch`] tells, once the match has played what it's given.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Telling;

impl Plugin for InTouch {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (|mut orders: ResMut<Orders>| orders.wait())
                    .run_if(in_state(Screen::Loading))
                    .run_if(resource_exists::<Orders>),
                messages::tell_leaving.run_if(resource_exists::<Orders>),
                messages::relist,
            )
                .chain()
                .in_set(Telling)
                .run_if(in_state(Screen::Loading).or_else(in_state(Screen::Match))),
        );
    }
}

/// Holds the sim's ticks off until the match has been shown once.
pub(crate) struct HoldFirstTick;

impl Plugin for HoldFirstTick {
    fn build(&self, app: &mut App) {
        app.configure_sets(FixedUpdate, Ticks.run_if(resource_exists::<Shown>))
            .add_systems(
                Update,
                show_first
                    .run_if(in_state(Screen::Match))
                    .run_if(not(resource_exists::<Shown>)),
            )
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.remove_resource::<Shown>()
            });
    }
}

/// Lets the sim step from the next frame, its first tick a tick's time
/// from now rather than after whatever time loading left over.
fn show_first(mut commands: Commands, mut fixed: ResMut<Time<Fixed>>) {
    let over = fixed.overstep();
    fixed.discard_overstep(over);
    commands.insert_resource(Shown);
}

/// What advances the sim a tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Ticks;

pub(crate) fn step(
    mut sim: ResMut<Sim>,
    mut orders: ResMut<Orders>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
    mut fixed: ResMut<Time<Fixed>>,
) {
    let played = sim.0.ticks;
    step_world(&mut sim.0, &mut orders, &mut selected.0, &mut chosen.0);
    if sim.0.ticks == played {
        wait_for_tick(&mut fixed);
    }
}

/// Leaves the tick that fell due and wasn't played, its host not having
/// said to, due again on the next frame. Until then what's drawn between
/// the last two ticks stays where the last left it, rather than starting
/// out from the one before again.
fn wait_for_tick(fixed: &mut Time<Fixed>) {
    let over = fixed.overstep();
    fixed.discard_overstep(over);
    // Short of a whole tick, which would be due within this frame.
    fixed.accumulate_overstep(fixed.timestep() - Duration::from_nanos(1));
}

/// Plays a tick of the game after carrying out the orders waiting,
/// dropping destroyed organizations from the selection. A packed building
/// selected on its own that unpacks hands the selection to the building,
/// and a building that packs to what it packed into.
pub(crate) fn step_world(
    sim: &mut World,
    orders: &mut Orders,
    selected: &mut Vec<usize>,
    chosen: &mut Option<usize>,
) {
    log_refused(orders.step(sim));
    if let &[r] = &selected[..]
        && let Some(b) = sim.organizations[r].unpacked_into
    {
        *chosen = Some(b);
    }
    selected.retain(|&r| !sim.destroyed(r));
    if let Some(b) = *chosen
        && let State::Packed(_, r) = sim.buildings[b].state
    {
        *chosen = None;
        *selected = vec![r];
    }
}

#[cfg(test)]
mod tests {
    use tgsim::TICK;

    use super::*;
    use crate::{orders, sound};

    #[test]
    fn a_tick_a_host_elsewhere_has_yet_to_send_stays_due_and_the_last_stays_drawn() {
        let (mut host, orders, sim) = orders::testing::joined(&[1]);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Sim(sim))
            .insert_resource(orders)
            .init_resource::<Selected>()
            .init_resource::<SelectedBuilding>()
            .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
            // Three frames to a tick and a bit.
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_secs_f32(TICK * 0.4),
            ))
            .add_systems(FixedUpdate, step);
        let drawn = |app: &App| {
            let ticks = app.world().resource::<Sim>().0.ticks;
            (
                ticks,
                app.world().resource::<Time<Fixed>>().overstep_fraction(),
            )
        };
        // However long the host takes, what's drawn holds at the end of
        // the last tick played.
        for _ in 0..10 {
            app.update();
        }
        let (ticks, between) = drawn(&app);
        assert!(ticks == 0 && between > 0.99, "{between}");
        // The tick is played on the frame after it comes, and drawn from
        // its start.
        host.turn();
        app.update();
        let (ticks, between) = drawn(&app);
        assert!(ticks == 1 && between > 0.3 && between < 0.5, "{between}");
        // Two that come at once are both played as the next falls due.
        host.turn();
        host.turn();
        app.update();
        assert_eq!(drawn(&app).0, 1);
        app.update();
        assert_eq!(drawn(&app).0, 3);
    }

    #[test]
    fn a_game_joined_elsewhere_hears_who_goes_and_what_is_said_while_it_loads() {
        let (mut host, mut orders, _) = orders::testing::joined(&[1]);
        let (link, _) = orders::testing::let_in(&mut host, &[2]);
        let mut other = tgserver::Client::new(link);
        let clock = tgserver::Clock::stopped();
        host.clock(clock.clone());
        orders.clock(clock.clone());
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .add_plugins(InTouch)
            .insert_state(Screen::Loading)
            .insert_resource(orders)
            .insert_resource(messages::testing::log())
            .insert_resource(sound::Sounds::of(Default::default(), &[]));
        app.update();
        other.say("hurry up".into()).unwrap();
        host.wait();
        drop(other);
        clock.pass(std::time::Duration::from_secs(1));
        host.wait();
        app.update();
        let log = app.world().resource::<messages::MessageLog>();
        assert_eq!(
            messages::testing::texts(log),
            ["Player Player 2 quit the game", "Player 2: hurry up"]
        );
    }

    #[test]
    fn the_sim_waits_for_the_match_to_be_shown() {
        #[derive(Resource, Default)]
        struct Stepped(u32);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .add_plugins(HoldFirstTick)
            .insert_state(Screen::Loading)
            .init_resource::<Stepped>()
            .insert_resource(Time::<Fixed>::from_seconds(TICK.into()))
            // Every frame long enough for a tick.
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                std::time::Duration::from_secs_f32(TICK * 1.5),
            ))
            .add_systems(
                FixedUpdate,
                (|mut s: ResMut<Stepped>| s.0 += 1)
                    .in_set(Ticks)
                    .run_if(in_state(Screen::Match)),
            );
        app.update();
        app.update();
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Match);
        app.update();
        assert_eq!(app.world().resource::<Stepped>().0, 0, "first frame");
        app.update();
        assert!(app.world().resource::<Stepped>().0 > 0, "after it");
    }
}
