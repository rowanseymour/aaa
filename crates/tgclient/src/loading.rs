//! Loading a match: reading its map and setting up the sim while the
//! loading screen shows how far it's got, joining a host elsewhere, and
//! waiting for everyone to be ready.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use tgdata::GameData;
use tgmap::Recruits;
use tgserver::protocol::Welcome;
use tgsim::World;

use crate::orders::Orders;
use crate::{
    Data, Leaders, Loaded, Local, Nickname, Raising, Screen, Side, Sim, control_groups, effects,
    lan, lobby, orders, sunlight, ui,
};

/// The map to play, which [`load`] sets up off the main thread.
#[derive(Resource)]
pub(crate) struct Pending {
    pub(crate) setup: tgserver::Setup,
    /// What the loading screen calls it.
    pub(crate) title: String,
    /// What the loading screen says of it.
    pub(crate) description: String,
    /// The player the person plays, or none to watch, when launched from
    /// the lobby.
    pub(crate) person: Option<u32>,
    /// Whether it was launched from the lobby, which failing to load it goes
    /// back to and which waits for the person to continue once it's loaded,
    /// rather than given on the command line, which it exits or starts.
    pub(crate) from_menu: bool,
    /// The screen a failed load goes back to; none to exit.
    pub(crate) back: Option<Screen>,
    /// The link to its host, what the host let the game in with, and
    /// where the host is, where the match is between machines.
    pub(crate) joined: Option<(orders::Line, Welcome, HostedAt)>,
}

/// Where the host of a match between machines is.
pub(crate) enum HostedAt {
    /// Elsewhere, at this address.
    Elsewhere(String),
    /// Here, in the game, to be given its copy of the match once it's set
    /// up.
    Here(Box<tgserver::Host<orders::Line>>),
}

/// The loading screen, which stays over the match while it's set up.
#[derive(Component)]
pub(crate) struct LoadingScreen;

/// What the loading screen says it's doing.
#[derive(Component)]
pub(crate) struct LoadingStatus;

/// How far loading a match has got, which the loading screen says.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    Data,
    Objects,
    Resources,
    Finished,
    /// Set up, and waiting on the match's host to begin it.
    Waiting,
}

/// What the loading screen says for each [`Stage`], in the game's words.
#[derive(Resource)]
pub(crate) struct StageWords(pub(crate) [String; 5]);

impl StageWords {
    pub(crate) fn of(strings: &tgdata::Strings) -> Self {
        let said = ui::said(strings);
        StageWords([
            said("app_status_loading_data", "Loading Data"),
            said("app_status_loading_objects", "Loading Objects"),
            said("app_status_loading_resources", "Loading Resources"),
            said("app_status_finished", "Finished Loading"),
            said(
                "app_status_waiting_on_other_players",
                "Waiting on Other Players",
            ),
        ])
    }

    fn of_stage(&self, stage: Stage) -> &str {
        &self.0[stage as usize]
    }
}

/// A match set up behind the loading screen; whether it waits there for the
/// person to continue.
#[derive(Resource)]
pub(crate) struct Readying {
    wait: bool,
}

/// A loading screen left up over a match that's set up, until its host,
/// which is elsewhere and may be waiting on others, begins it.
#[derive(Component)]
pub(crate) struct AwaitingHost;

/// Takes down a loading screen that's [`AwaitingHost`] once the match has
/// begun, or its host has gone.
pub(crate) fn stop_awaiting(
    mut commands: Commands,
    orders: Res<Orders>,
    awaiting: Query<Entity, With<AwaitingHost>>,
) {
    if !orders.waiting() {
        awaiting.iter().for_each(|e| commands.entity(e).despawn());
    }
}

/// Where the loading screen puts its Continue button.
#[derive(Component)]
pub(crate) struct ContinueSlot;

/// A match being set up off the main thread, which would otherwise stop the
/// window responding. A thread of its own rather than a task pool's, which
/// would hold up quitting mid-load until it finished.
#[derive(Resource)]
pub(crate) struct Preparing {
    thread: std::thread::JoinHandle<Result<Prepared, String>>,
    /// Where the thread has got to.
    stage: Arc<Mutex<Stage>>,
    /// What the loading screen last said.
    shown: Stage,
    from_menu: bool,
    back: Option<Screen>,
}

/// What [`prepare`] sets a match up with.
pub(crate) struct Prepared {
    loaded: Loaded,
    /// Those of a match hosted here; a match hosted elsewhere has had its
    /// own since it began loading.
    orders: Option<Orders>,
    sim: World,
    raising: Recruits,
    person: Option<u32>,
    leaders: Leaders,
    effects: effects::Effects,
}

/// How long the host of a match is waited on to say whether the game is
/// let in.
const ANSWERED: Duration = Duration::from_secs(10);

/// Asks the host at `at` to let the game in to its match to give the
/// orders of `players`, for a person going by `name`: the link to it, and
/// what it let the game in with.
pub(crate) fn join(
    at: SocketAddr,
    players: Vec<u32>,
    name: String,
) -> Result<(orders::Line, Welcome, HostedAt), String> {
    let mut link = tgserver::quic::connect(at)?;
    let welcome = tgserver::enter(&mut link, players, name, ANSWERED)
        .map_err(|e| format!("{at}: not let in: {e:?}"))?;
    Ok((Box::new(link), welcome, HostedAt::Elsewhere(at.to_string())))
}

/// Whether `game` is the data of the game tagged `tag`.
pub(crate) fn own_game(game: &GameData, tag: &str) -> Result<(), String> {
    match game.tag() {
        _ if game.is_of(tag) => Ok(()),
        Some(t) => Err(format!("the data of {t}, not {tag}")),
        None => Err(format!("no game named in the data, not {tag}")),
    }
}

/// Loads a map and sets up the sim and recruits for a match on it, given
/// the players the game was let in to give the orders of and the name it
/// was let in by, where it's `joined` a match hosted elsewhere.
pub(crate) fn prepare(
    game: Arc<GameData>,
    pending: Pending,
    joined: Option<(Vec<u32>, String)>,
    side: Side,
    name: String,
    stage: &Mutex<Stage>,
) -> Result<Prepared, String> {
    let reached = |s| *stage.lock().unwrap_or_else(|e| e.into_inner()) = s;
    let Pending {
        setup,
        person,
        from_menu,
        ..
    } = pending;
    let elsewhere = joined.is_some();
    info!("seed {}", setup.seed);
    let tgserver::Started {
        map,
        raising,
        world: sim,
        factions,
    } = tgserver::start(&game, &setup, || reached(Stage::Objects))?;
    let person = match (&joined, from_menu) {
        // Its host says who the game plays.
        (Some((players, _)), _) => players.first().copied(),
        (None, true) => person,
        (None, false) => lobby::default_person(&map.players, side),
    };
    let own = match joined {
        Some(own) => own,
        None => (person.into_iter().collect(), name.clone()),
    };
    let leaders = Leaders(factions, Some(own));
    if !from_menu && side.0.is_some() && person.is_none() {
        warn!("{}: no such side to play", map.title);
    }
    let effects = effects::Effects::new(&game, &map);
    // Built here, off the main thread, and kept for the interface to read
    // as the match starts.
    game.strings();
    game.listings();
    game.technologies();
    game.property_looks();
    Ok(Prepared {
        loaded: Loaded { game, map },
        // The host's copy, made here to keep it off the window's thread.
        // A host elsewhere is heard from while the match is set up.
        orders: (!elsewhere).then(|| Orders::hosting(setup, sim.clone(), person, &name)),
        sim,
        raising,
        person,
        leaders,
        effects,
    })
}

/// Puts a [`prepare`]d match in place.
pub(crate) fn begin(world: &mut bevy::ecs::world::World, prepared: Prepared) {
    let Prepared {
        loaded,
        orders,
        sim,
        raising,
        person,
        leaders,
        effects,
    } = prepared;
    let lineup = lobby::Lineup::new(&loaded.map.players, person);
    for &(id, control) in &lineup.0 {
        if let Some(p) = sim.sides.player(id) {
            let team = p
                .team
                .map_or("no team".into(), |t| format!("team {}", t + 1));
            let nation = p.nation.as_deref().unwrap_or("no nation");
            let faction = leaders.0.get(&id).map_or("no faction", |g| g.id.as_str());
            info!(
                "{} ({}, {team}, {nation}, {faction}): {control:?}",
                p.name, p.color
            );
        }
    }
    world.insert_resource(Local(lineup.person()));
    world.insert_resource(leaders);
    world.insert_resource(lineup);
    if let Some(orders) = orders {
        world.insert_resource(orders);
    }
    world.insert_resource(Sim(sim));
    world.insert_resource(control_groups::ControlGroups::default());
    world.insert_resource(Raising(raising));
    world.insert_resource(effects);
    world.insert_resource(sunlight::Sunlight::new(&loaded.game, &loaded.map.sun));
    world.insert_resource(loaded);
}

/// Shows the map being loaded, as the game does: its name over what it's
/// about, and how loading goes.
pub(crate) fn open_screen(mut commands: Commands, pending: Res<Pending>, words: Res<StageWords>) {
    use ui::{Edge, Font, Ink, Surface, label, text};
    // After the map's camera, which is set up behind the screen and draws
    // to a texture of its own.
    let camera = Camera {
        order: 1,
        ..default()
    };
    commands.spawn((Camera2d, camera, DespawnOnExit(Screen::Loading)));
    let root = commands
        .spawn((
            LoadingScreen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(40)),
                row_gap: px(24),
                ..default()
            },
            Surface::Solid,
            GlobalZIndex(ui::layer::PAGE),
        ))
        .id();
    commands.spawn((ChildOf(root), text(pending.title.clone(), Font::Title)));
    if !pending.description.is_empty() {
        commands.spawn((
            ChildOf(root),
            Node {
                width: percent(50),
                min_height: percent(40),
                padding: UiRect::all(px(16)),
                border: UiRect::all(px(2)),
                ..default()
            },
            Edge::Accent,
            children![text(pending.description.clone(), Font::Body)],
        ));
    }
    commands.spawn((
        ChildOf(root),
        Node {
            flex_grow: 1.0,
            ..default()
        },
    ));
    commands.spawn((
        ChildOf(root),
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                LoadingStatus,
                label(words.of_stage(Stage::Data), Font::Body, Ink::Help)
            ),
            (
                ContinueSlot,
                Node {
                    width: px(240),
                    justify_content: JustifyContent::End,
                    ..default()
                },
            ),
        ],
    ));
}

/// Starts [`prepare`]ing the pending map, then once it's ready starts the
/// match on it behind the loading screen, or goes back to the lobby or exits
/// if it can't be loaded.
pub(crate) fn load(world: &mut bevy::ecs::world::World) {
    if let Some(mut pending) = world.remove_resource::<Pending>() {
        let Some(game) = world.get_resource::<Data>().map(|d| d.0.clone()) else {
            return;
        };
        let side = *world.resource::<Side>();
        let name = world.resource::<Nickname>().0.clone();
        let (from_menu, back) = (pending.from_menu, pending.back);
        world.insert_resource(crate::leaving::Mission {
            title: pending.title.clone(),
        });
        // Heard from all the while the match is set up, as its host hears
        // from the game.
        let joined = pending.joined.take().map(|(link, welcome, at)| {
            let own = (welcome.players.clone(), welcome.name.clone());
            world.insert_resource(match at {
                HostedAt::Elsewhere(at) => Orders::joined(link, welcome, at),
                HostedAt::Here(host) => Orders::sharing(*host, link, welcome),
            });
            own
        });
        let stage = Arc::new(Mutex::new(Stage::Data));
        let reached = stage.clone();
        let thread =
            std::thread::spawn(move || prepare(game, pending, joined, side, name, &reached));
        world.insert_resource(Preparing {
            thread,
            stage,
            shown: Stage::Data,
            from_menu,
            back,
        });
        return;
    }
    let Some(mut preparing) = world.get_resource_mut::<Preparing>() else {
        return;
    };
    if !preparing.thread.is_finished() {
        let stage = *preparing.stage.lock().unwrap_or_else(|e| e.into_inner());
        if stage != preparing.shown {
            preparing.shown = stage;
            show_stage(world, stage);
        }
        return;
    }
    let Some(Preparing {
        thread,
        from_menu,
        back,
        ..
    }) = world.remove_resource::<Preparing>()
    else {
        return;
    };
    let prepared = thread
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    match prepared {
        Ok(prepared) => {
            if let Some(mut orders) = world.get_resource_mut::<Orders>() {
                orders.build(prepared.sim.clone());
            }
            // Shown this frame, before the match is set up on the next.
            show_stage(world, Stage::Resources);
            begin(world, prepared);
            world.insert_resource(Readying { wait: from_menu });
            world.resource_mut::<NextState<Screen>>().set(Screen::Match);
        }
        Err(e) if let Some(back) = back => {
            error!("{e}");
            let mut screens = world.query_filtered::<Entity, With<LoadingScreen>>();
            for screen in screens.iter(world).collect::<Vec<_>>() {
                world.despawn(screen);
            }
            // Leaving any match between machines it was to be.
            world.remove_resource::<Orders>();
            world.resource_mut::<NextState<Screen>>().set(back);
            if back == Screen::Lan {
                let failed = world.resource::<lan::Words>().failed.clone();
                world.insert_resource(lan::Notice(failed));
            }
        }
        Err(e) => {
            error!("{e}");
            world.write_message(AppExit::error());
        }
    }
}

/// Says on the loading screen how far loading has got.
fn show_stage(world: &mut bevy::ecs::world::World, stage: Stage) {
    let said = world.resource::<StageWords>().of_stage(stage).to_string();
    let mut status = world.query_filtered::<&mut Text, With<LoadingStatus>>();
    for mut text in status.iter_mut(world) {
        text.0.clone_from(&said);
    }
}

/// Once the match is set up, says so and holds it until the person
/// continues, or starts it straight away: which, for a match hosted
/// elsewhere, is to tell its host and wait for it to begin.
#[allow(clippy::too_many_arguments)]
pub(crate) fn ready(
    mut commands: Commands,
    mut orders: ResMut<Orders>,
    readying: Option<Res<Readying>>,
    mut time: ResMut<Time<Virtual>>,
    mut status: Query<&mut Text, With<LoadingStatus>>,
    words: Res<StageWords>,
    slot: Query<Entity, With<ContinueSlot>>,
    screen: Query<Entity, With<LoadingScreen>>,
) {
    let Some(readying) = readying else {
        return;
    };
    commands.remove_resource::<Readying>();
    if !readying.wait {
        orders.ready();
        if !orders.waiting() {
            screen.iter().for_each(|e| commands.entity(e).despawn());
            return;
        }
        for screen in &screen {
            commands.entity(screen).insert(AwaitingHost);
        }
        for mut text in &mut status {
            text.0 = words.of_stage(Stage::Waiting).into();
        }
        return;
    }
    time.pause();
    for mut text in &mut status {
        text.0 = words.of_stage(Stage::Finished).into();
    }
    for slot in &slot {
        commands
            .spawn((
                ChildOf(slot),
                ui::button("Continue", ui::Font::Large),
                ui::called("loading.continue"),
            ))
            .observe(start);
    }
}

/// Takes the loading screen away and starts the match.
pub(crate) fn start(
    _: On<bevy::ui_widgets::Activate>,
    mut commands: Commands,
    mut orders: ResMut<Orders>,
    mut time: ResMut<Time<Virtual>>,
    screen: Query<Entity, With<LoadingScreen>>,
) {
    orders.ready();
    screen.iter().for_each(|e| commands.entity(e).despawn());
    time.unpause();
}
