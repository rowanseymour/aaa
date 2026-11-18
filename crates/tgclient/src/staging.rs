//! What the lobby has for a game between machines on top of what it has
//! for one played alone: the game's name and how full it is, Ready for
//! those who joined and Launch for its host, the log of what's said and
//! told, and launching when the host's countdown runs out.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgserver::{Choice, NotJoined, Refusal, Told};

use crate::fields::{Entered, Field, field};
use crate::lan::{Hosting, Notice, Staged, fill};
use crate::lobby::Lobby;
use crate::ui::{self, Font, Ink, Surface, button, called, label, panel, text};
use crate::{Data, HostedAt, Pending, Screen, confirm};

/// The most lines the log keeps.
const KEPT: usize = 200;

/// The most characters a line of chat takes, as the lobby's edit box.
const SAID_LONGEST: usize = 512;

pub struct StagingPlugin;

impl Plugin for StagingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Log>()
            .init_resource::<Reported>()
            .add_systems(OnEnter(Screen::Lobby), clear_log)
            .add_systems(
                Update,
                (launch_hosted, follow, show_controls, show_log)
                    .chain()
                    .run_if(in_state(Screen::Lobby))
                    .run_if(resource_exists::<Staged>),
            );
    }
}

/// What the lobby says, as the game's data has it.
#[derive(Resource)]
pub struct Words {
    joined: String,
    quit: String,
    kicked: String,
    asked_host: String,
    asked: String,
    counting: String,
    aborted: String,
    counting_refused: String,
    refused: String,
    side_refused: String,
    ready_refused: String,
    said: String,
    players: String,
    exit: String,
    lost: String,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let said = ui::said(strings);
        Words {
            joined: said("networking_player_joined|s", "%s has joined the game"),
            quit: said("networking_player_quit|s", "Player %s quit the game"),
            kicked: said(
                "networking_player_kicked|s",
                "Player %s was kicked from the game",
            ),
            asked_host: said(
                "host_requested_players_ready",
                "Requesting that all players become ready.",
            ),
            asked: said(
                "host_requested_players_ready_client_is_ready",
                "Host has requested that all players become ready.",
            ),
            counting: said("launch_countdown_begin", "Beginning Launch Countdown"),
            aborted: said("launch_countdown_abort", "Launch Countdown Aborted"),
            counting_refused: said(
                "session_order_rejected_currently_counting_down",
                "Cannot issue command while launch is counting down",
            ),
            refused: said("session_order_failed", "Session Order Failed"),
            side_refused: said("set_player_side_failed", "Set Player Side Failed"),
            ready_refused: said("set_player_ready_failed", "Set Player Ready Failed"),
            said: said("public_chat|ss", "%s: %s"),
            players: said("staging_players_label|dd", "%d/%d Players"),
            exit: said("ui_staging_confirm_exit", "Exit Staging?"),
            lost: said("ui_lobby_connect_failed", "Connection to game failed"),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
    }
}

/// The lines the lobby has shown, oldest first.
#[derive(Resource, Default)]
struct Log(Vec<String>);

/// Where the lobby's multiplayer controls go, and its log.
#[derive(Component)]
pub(crate) struct Controls;
#[derive(Component)]
pub(crate) struct LogPanel;
#[derive(Component)]
struct LogLines;
#[derive(Component)]
struct ChatBox;

/// The host of a match launched here, waiting for the game's own end to
/// hear that it has been.
#[derive(Resource)]
struct Launched(Box<tgserver::Host<crate::orders::Line>>);

/// The map this machine last told the lobby it had or not.
#[derive(Resource, Default)]
struct Reported(Option<String>);

fn clear_log(mut log: ResMut<Log>, mut reported: ResMut<Reported>) {
    log.0.clear();
    reported.0 = None;
}

/// Lays out the log and the box to chat in, in `panel`.
pub(crate) fn spawn_log(commands: &mut Commands, parent: Entity) {
    let log = commands
        .spawn((
            ChildOf(parent),
            LogPanel,
            panel(
                Surface::Panel,
                Node {
                    height: px(220),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(12)),
                    row_gap: px(6),
                    ..default()
                },
            ),
        ))
        .id();
    commands.spawn((
        ChildOf(log),
        LogLines,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::End,
            overflow: Overflow::clip(),
            ..default()
        },
    ));
    let row = commands
        .spawn((
            ChildOf(log),
            Node {
                column_gap: px(8),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            ChildOf(row),
            ChatBox,
            field("", SAID_LONGEST, percent(100)),
            called("lobby.chat"),
        ))
        .observe(send_entered);
    commands
        .spawn((
            ChildOf(row),
            button("Send", Font::Body),
            called("lobby.send"),
        ))
        .observe(send_pressed);
}

fn send_entered(
    entered: On<Entered>,
    mut boxes: Query<&mut Field, With<ChatBox>>,
    staged: ResMut<Staged>,
) {
    if let Ok(mut field) = boxes.get_mut(entered.entity) {
        say(&mut field, staged);
    }
}

fn send_pressed(
    _: On<Activate>,
    mut boxes: Query<&mut Field, With<ChatBox>>,
    staged: ResMut<Staged>,
) {
    if let Some(mut field) = boxes.iter_mut().next() {
        say(&mut field, staged);
    }
}

fn say(field: &mut Field, mut staged: ResMut<Staged>) {
    let words = std::mem::take(&mut field.text);
    if !words.trim().is_empty() {
        // A host that's gone is found so as the lobby next hears from it.
        let _ = staged.entry.say(words);
    }
}

/// Hears from the game's host: what it has come to, what's said and told,
/// and once it launches, the match to load; or that the game is gone.
#[allow(clippy::too_many_arguments)]
fn follow(
    mut commands: Commands,
    mut staged: ResMut<Staged>,
    mut lobby: ResMut<Lobby>,
    mut log: ResMut<Log>,
    launched: Option<Res<Launched>>,
    data: Res<Data>,
    words: Res<Words>,
    lan: Res<crate::lan::Words>,
    mut next: ResMut<NextState<Screen>>,
    mut reported: ResMut<Reported>,
) {
    let before = staged.entry.staged();
    let heard = staged.entry.listen();
    let me = staged.entry.me();
    if staged.entry.staged() != before {
        let staging = staged.entry.staging();
        lobby.follow(&data, staging, me);
        let map = staging.map.clone();
        if reported.0.as_ref() != Some(&map) {
            let holds = crate::lobby::has_map(&data, &map);
            // A host that's gone is found so as the lobby next hears from
            // it.
            let _ = staged.entry.choose(Choice::Holds(map.clone(), holds));
            reported.0 = Some(map);
        }
    }
    let host = staged.entry.staging().guest(me).is_some_and(|g| g.host);
    for told in staged.entry.take_told() {
        let line = match told {
            Told::Joined(name) => fill(&words.joined, &[&name]),
            Told::AskedReady if host => words.asked_host.clone(),
            Told::AskedReady => words.asked.clone(),
            Told::Counting => words.counting.clone(),
            Told::Count(n) => n.to_string(),
            Told::Aborted => words.aborted.clone(),
            Told::Refused { counting: true, .. } => words.counting_refused.clone(),
            Told::Refused { choice, .. } => match choice {
                Choice::Side(_) => words.side_refused.clone(),
                Choice::Ready(_) => words.ready_refused.clone(),
                _ => words.refused.clone(),
            },
        };
        log.0.push(line);
    }
    for said in staged.entry.take_said() {
        log.0.push(fill(&words.said, &[&said.name, &said.words]));
    }
    for gone in staged.entry.take_left() {
        let format = match gone.kicked {
            true => &words.kicked,
            false => &words.quit,
        };
        log.0.push(fill(format, &[&gone.name]));
    }
    let kept = log.0.len().saturating_sub(KEPT);
    log.0.drain(..kept);
    let gone_to = |commands: &mut Commands, next: &mut NextState<Screen>, words: String| {
        commands.remove_resource::<Staged>();
        commands.remove_resource::<Hosting>();
        commands.remove_resource::<Launched>();
        commands.insert_resource(Notice(words));
        next.set(Screen::Lan);
    };
    match heard {
        Ok(None) => {}
        Ok(Some(welcome)) => {
            if staged.at.is_none() && launched.is_none() {
                // Launched by a host here that was never made.
                gone_to(&mut commands, &mut next, words.lost.clone());
                return;
            }
            let (title, description) = (lobby.setup(welcome.setup.seed)).map_or_else(
                || {
                    (
                        tgdata::map_title(&welcome.setup.map).to_string(),
                        String::new(),
                    )
                },
                |(_, title, description)| (title, description),
            );
            commands.queue(move |world: &mut World| {
                let Some(Staged { entry, at }) = world.remove_resource::<Staged>() else {
                    return;
                };
                // Its choices were the game's, not the custom game's.
                world.remove_resource::<Lobby>();
                let at = match at {
                    Some(at) => HostedAt::Elsewhere(at),
                    None => match world.remove_resource::<Launched>() {
                        Some(Launched(host)) => HostedAt::Here(host),
                        None => return,
                    },
                };
                world.insert_resource(Pending {
                    title,
                    description,
                    person: welcome.players.first().copied(),
                    from_menu: false,
                    back: Some(Screen::Lan),
                    setup: welcome.setup.clone(),
                    joined: Some((entry.into_link(), welcome, at)),
                });
                world
                    .resource_mut::<NextState<Screen>>()
                    .set(Screen::Loading);
            });
        }
        Err(NotJoined::Refused(Refusal::Kicked)) => {
            gone_to(&mut commands, &mut next, lan.kicked.clone());
        }
        Err(_) => gone_to(&mut commands, &mut next, words.lost.clone()),
    }
}

/// Launches the game hosted here once its countdown has run out: what's
/// played is as the lobby has it, its chances drawn afresh.
fn launch_hosted(mut commands: Commands, hosting: Option<ResMut<Hosting>>, lobby: Res<Lobby>) {
    let Some(mut hosting) = hosting.filter(|h| h.launch && h.lobby.is_some()) else {
        return;
    };
    let Some((setup, _, _)) = lobby.setup(tgserver::fresh_seed()) else {
        // Nothing to play: it doesn't launch after all.
        hosting.launch = false;
        if let Some(lobby_host) = hosting.lobby.as_mut() {
            lobby_host.stop();
        }
        return;
    };
    let lobby_host = hosting.lobby.take().expect("a lobby to launch");
    let host = lobby_host.launch(setup, lobby.sides());
    commands.insert_resource(Launched(Box::new(host)));
}

/// What the controls were drawn for, to draw them again only once it
/// changes.
#[derive(PartialEq)]
struct ControlsShown {
    name: String,
    people: usize,
    most: usize,
    locked: bool,
    countdown: Option<u32>,
    ready: bool,
    host: bool,
    watching: bool,
}

/// Fills the lobby's right-hand controls: the game's name and how full it
/// is, Launch or Abort for its host and Ready for the rest, and Exit.
fn show_controls(
    mut commands: Commands,
    // Gone once the game has launched.
    staged: Option<Res<Staged>>,
    words: Res<Words>,
    controls: Single<Entity, With<Controls>>,
    mut shown: Local<Option<ControlsShown>>,
) {
    let Some(staged) = staged else {
        return;
    };
    let staging = staged.entry.staging();
    let me = staging.guest(staged.entry.me());
    let host = me.is_some_and(|g| g.host);
    let ready = me.is_some_and(|g| g.ready);
    let watching = me.is_none_or(|g| g.side.is_none());
    let now = ControlsShown {
        name: staging.name.clone(),
        people: staging.guests.len(),
        most: staging.most,
        locked: staging.locked,
        countdown: staging.countdown,
        ready,
        host,
        watching,
    };
    if shown.as_ref() == Some(&now) && !staged.is_added() {
        return;
    }
    *shown = Some(now);
    let controls = *controls;
    commands.entity(controls).despawn_children();
    let lock = match staging.locked {
        true => " (password)",
        false => "",
    };
    commands.spawn((
        ChildOf(controls),
        text(format!("{}{lock}", staging.name), Font::Large),
    ));
    let players = fill(
        &words.players,
        &[&staging.guests.len().to_string(), &staging.most.to_string()],
    );
    commands.spawn((ChildOf(controls), label(players, Font::Body, Ink::Muted)));
    match host {
        true if staging.countdown.is_some() => commands
            .spawn((
                ChildOf(controls),
                button("Abort", Font::Large),
                called("lobby.abort"),
            ))
            .observe(|_: On<Activate>, mut staged: ResMut<Staged>| {
                let _ = staged.entry.choose(Choice::Launch(false));
            }),
        true => commands
            .spawn((
                ChildOf(controls),
                button("Launch", Font::Large),
                called("lobby.launch"),
            ))
            .observe(|_: On<Activate>, mut staged: ResMut<Staged>| {
                let _ = staged.entry.choose(Choice::Launch(true));
            }),
        false if watching => {
            commands.spawn((ChildOf(controls), ui::disabled("Ready", Font::Large)));
            return_exit(&mut commands, controls);
            return;
        }
        false => commands
            .spawn((
                ChildOf(controls),
                ui::toggle("Ready", Font::Large),
                called("lobby.ready"),
            ))
            .insert(ui::Chosen(ready))
            .observe(move |_: On<Activate>, mut staged: ResMut<Staged>| {
                let _ = staged.entry.choose(Choice::Ready(!ready));
            }),
    };
    return_exit(&mut commands, controls);
}

fn return_exit(commands: &mut Commands, controls: Entity) {
    commands
        .spawn((
            ChildOf(controls),
            button("Exit", Font::Large),
            called("lobby.exit"),
        ))
        .observe(ask_exit);
}

fn ask_exit(
    _: On<Activate>,
    mut commands: Commands,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
) {
    confirm::confirm(
        &mut commands,
        &confirm,
        Screen::Lobby,
        words.exit.clone(),
        leave,
    );
}

/// Leaves the game for the LAN screen; a host leaving ends it for
/// everyone.
fn leave(_: On<Activate>, mut commands: Commands, mut next: ResMut<NextState<Screen>>) {
    commands.queue(|world: &mut World| {
        if let Some(Staged { entry, .. }) = world.remove_resource::<Staged>() {
            entry.leave();
        }
        world.remove_resource::<Hosting>();
    });
    next.set(Screen::Lan);
}

/// Shows the log's lines.
fn show_log(mut commands: Commands, log: Res<Log>, lines: Single<Entity, With<LogLines>>) {
    if !log.is_changed() {
        return;
    }
    let lines = *lines;
    commands.entity(lines).despawn_children();
    let shown = log.0.len().saturating_sub(10);
    for line in &log.0[shown..] {
        commands.spawn((ChildOf(lines), label(line.clone(), Font::Body, Ink::Body)));
    }
}

/// Marks a person's slot in `row`: the host's mark on whoever runs the
/// host, a ready mark on the others with a side, how fast their link is,
/// and Kick for the host on anyone else's.
pub(crate) fn marks(
    commands: &mut Commands,
    row: Entity,
    guest: &tgserver::Guest,
    hosting: bool,
    look: &crate::connection::Look,
) {
    let mark = match (guest.host, guest.side, guest.ready) {
        (true, _, _) => "Game Host",
        (false, None, _) => "",
        (false, Some(_), true) => "Ready",
        (false, Some(_), false) => "Not Ready",
    };
    commands.spawn((ChildOf(row), label(mark, Font::Body, Ink::Muted)));
    // The host's own measure, so its own slot is always quick.
    let ping = guest.ping.unwrap_or(0);
    if let Some(glyph) = look.ping_glyph(ping) {
        commands.spawn((
            ChildOf(row),
            ImageNode::new(glyph),
            ui::Hint(format!("Ping: {ping}")),
        ));
    }
    if hosting && !guest.host {
        let client = guest.client;
        commands
            .spawn((
                ChildOf(row),
                button("Kick", Font::Body),
                called(format!("lobby.kick.{}", ui::slug(&guest.name))),
            ))
            .observe(move |_: On<Activate>, mut staged: ResMut<Staged>| {
                let _ = staged.entry.choose(Choice::Kick(client));
            });
    }
}
