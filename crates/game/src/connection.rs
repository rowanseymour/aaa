//! The connection dialog: how everyone in a match stands as its host sees
//! them. It opens by itself once the match has been held for a moment and
//! closes when it goes on, and it's where players vote to drop those
//! holding it.

use std::collections::HashMap;

use aaa_data::PingLook;
use aaa_server::protocol::{Seated, Standing, Status};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::ui::{self, Font, Ink};
use crate::{Leaders, Loaded, Local, Orders, Screen, Sim, confirm, menu, messages};

/// How long the match is held before the dialog opens by itself, so that
/// a moment's hold doesn't flash it.
const OPENS_AFTER: f32 = 2.0;

/// How tall the dialog's icons are drawn.
const ICON: f32 = 16.0;

/// What the dialog says.
#[derive(Resource, Clone)]
pub struct Words {
    hosting: String,
    connected_to: String,
    name: String,
    ping: String,
    status: String,
    kick: String,
    votes: String,
    disconnecting: String,
    close: String,
    leave: String,
    leave_confirm: String,
    observer: String,
    standings: HashMap<Standing, String>,
}

impl Words {
    pub fn of(strings: &aaa_data::Strings) -> Self {
        let said = ui::said(strings);
        let standings = [
            (
                Standing::Disconnected,
                "status_icon_disconnected",
                "Disconnected",
            ),
            (
                Standing::OutOfSync,
                "status_icon_out_of_sync",
                "Out of Sync",
            ),
            (
                Standing::CatchingUp,
                "status_icon_catching_up",
                "Catching Up",
            ),
        ];
        Words {
            hosting: said("csd_title_hosting", "Hosting"),
            connected_to: said("csd_title_connected_to|s", "Connected to %s"),
            name: said(
                "connection_status_dialog_ClientSlotWidget_NameLabel_Label_text",
                "Name",
            ),
            ping: said(
                "connection_status_dialog_ClientSlotWidget_PingLabel_Label_text",
                "Ping",
            ),
            status: said(
                "connection_status_dialog_PlayerPanel_KickLaggingPlayersPanel_StatusLabel_Label_text",
                "Status",
            ),
            kick: said("connection_status_dialog_Child_text_2", "Kick"),
            votes: said("csd_votes|dd", "%d/%d Votes"),
            disconnecting: said(
                "csd_disconnecting_in_seconds|d",
                "Disconnecting in %d seconds",
            ),
            close: said(
                "connection_status_dialog_CloseDialogButton_Client_View_text",
                "Close",
            ),
            leave: said(
                "connection_status_dialog_LeaveGameButton_Client_View_text",
                "Leave Game",
            ),
            leave_confirm: said("csd_leave_game_confirm", "Exit the game?"),
            observer: said("game_interface_KingdomDropDown_observer_text", "Observer"),
            standings: (standings.into_iter())
                .map(|(s, key, stock)| (s, said(key, stock)))
                .collect(),
        }
    }

    /// What the game menu's entry that opens the dialog says.
    pub fn menu_entry(strings: &aaa_data::Strings) -> String {
        ui::said(strings)(
            "game_menu_ButtonConnectionStatus_Client_View_text",
            "Connection Status",
        )
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&aaa_data::Strings::default())
    }
}

/// The images the dialog draws.
#[derive(Resource, Default)]
pub struct Look {
    host: Option<Handle<Image>>,
    standings: HashMap<Standing, Handle<Image>>,
    /// The glyphs drawn before a ping, by their images' names.
    pings: HashMap<String, Handle<Image>>,
    ping: PingLook,
}

/// Whether the dialog is open, and why.
#[derive(Resource, Default)]
pub struct Dialog {
    opened: Option<Opened>,
    /// When, by the real clock, the match was found held.
    held_since: Option<f32>,
    /// How it's laid out, to lay it out again only when that changes.
    drawn: Option<Drawn>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opened {
    /// By the player, from the game menu: it stays until they close it.
    ByPlayer,
    /// By the match being held: it closes when the match goes on.
    ByHold,
}

/// How the dialog is laid out: why it's open, whether the match is held,
/// and each client's slot by who it is and the players it plays. What's
/// in the slots is kept up to date in place.
#[derive(Clone, PartialEq)]
struct Drawn {
    opened: Opened,
    held: bool,
    seats: Vec<(u32, Vec<u32>, bool)>,
}

impl Drawn {
    fn of(opened: Opened, status: &Status) -> Drawn {
        let seats = status.seats.iter();
        Drawn {
            opened,
            held: status.held,
            seats: seats
                .map(|s| (s.client, s.players.clone(), s.host))
                .collect(),
        }
    }

    /// Whether it's how the dialog would be laid out for `status`.
    fn fits(&self, opened: Opened, status: &Status) -> bool {
        let same = |(client, players, host): &(u32, Vec<u32>, bool), s: &Seated| {
            *client == s.client && *players == s.players && *host == s.host
        };
        self.opened == opened
            && self.held == status.held
            && self.seats.len() == status.seats.len()
            && self
                .seats
                .iter()
                .zip(&status.seats)
                .all(|(d, s)| same(d, s))
    }
}

/// A client's ping, its glyph and its standing's mark, by the client.
#[derive(Component)]
struct PingOf(u32);
#[derive(Component)]
struct PingGlyphOf(u32);
#[derive(Component)]
struct StandingOf(u32);

/// The vote to drop those holding the match, how it stands, and the
/// countdown.
#[derive(Component)]
struct Kick;
#[derive(Component)]
struct Votes;
#[derive(Component)]
struct Countdown;

#[derive(Component)]
struct Root;

pub struct ConnectionPlugin;

impl Plugin for ConnectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Dialog>()
            .init_resource::<Look>()
            .add_systems(OnEnter(Screen::Match), |mut dialog: ResMut<Dialog>| {
                *dialog = Dialog::default()
            })
            .add_systems(
                Update,
                (watch, draw, fill_in)
                    .chain()
                    .run_if(in_state(Screen::Match))
                    .run_if(resource_exists::<Orders>),
            );
    }
}

/// Reads the images the dialog draws.
pub fn load(
    data: Res<crate::Data>,
    formats: Option<Res<bevy::image::CompressedImageFormatSupport>>,
    mut images: ResMut<Assets<Image>>,
    mut look: ResMut<Look>,
) {
    let formats = formats.map(|f| f.0).unwrap_or_default();
    let mut image = |path: &str| {
        let image = aaa_render::textures::decode(&data.0.read(path)?, path, formats, true)?;
        Some(images.add(image))
    };
    let connection = data.0.connection_look();
    let standings = [
        (Standing::Disconnected, "disconnected"),
        (Standing::OutOfSync, "out_of_sync"),
        (Standing::CatchingUp, "catching_up"),
    ];
    let ping = data.0.ping_look();
    let glyphs = [&ping.good, &ping.middling, &ping.bad];
    let pings = (glyphs.into_iter().flatten())
        .filter_map(|name| {
            let path = format!("{}/{name}.png", crate::actions::GLYPH_DIR);
            Some((name.clone(), image(&path)?))
        })
        .collect();
    *look = Look {
        host: connection.host_icon.as_deref().and_then(&mut image),
        standings: (standings.into_iter())
            .filter_map(|(s, name)| Some((s, image(connection.status_icons.get(name)?)?)))
            .collect(),
        pings,
        ping,
    };
}

/// Whether the dialog is closed, so that what's under it takes input.
pub fn closed(dialog: Res<Dialog>) -> bool {
    dialog.opened.is_none()
}

/// Opens the dialog at the player's asking, as the game menu does.
pub fn open(dialog: &mut Dialog) {
    dialog.opened = Some(Opened::ByPlayer);
}

/// Whether the dialog can be opened: only in a match with others in it.
pub fn openable(orders: &Orders) -> bool {
    orders.joined_to()
}

/// Opens the dialog once the match has been held for a moment, and
/// closes it as the match goes on if that's what opened it.
fn watch(orders: Res<Orders>, time: Res<Time<Real>>, mut dialog: ResMut<Dialog>) {
    // With the host gone, what it last said no longer stands.
    let held = !orders.gone() && orders.status().is_some_and(|s| s.held);
    let now = time.elapsed_secs();
    if !held {
        dialog.held_since = None;
        if dialog.opened == Some(Opened::ByHold) {
            dialog.opened = None;
        }
        return;
    }
    let since = *dialog.held_since.get_or_insert(now);
    if dialog.opened.is_none() && now - since >= OPENS_AFTER {
        dialog.opened = Some(Opened::ByHold);
    }
}

/// What the dialog needs to say who each client is.
#[derive(bevy::ecs::system::SystemParam)]
struct Naming<'w> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    leaders: Res<'w, Leaders>,
    /// Only for the names the map gives sides.
    loaded: Option<Res<'w, Loaded>>,
    gem: Res<'w, ui::Gem>,
}

/// Lays the dialog out as it stands, or takes it down once it's closed.
fn draw(
    mut commands: Commands,
    orders: Res<Orders>,
    mut dialog: ResMut<Dialog>,
    words: Res<Words>,
    look: Res<Look>,
    naming: Naming,
    roots: Query<Entity, With<Root>>,
) {
    let empty = Status::default();
    let status = orders.status().unwrap_or(&empty);
    let fits = match (dialog.opened, &dialog.drawn) {
        (Some(opened), Some(drawn)) => drawn.fits(opened, status),
        (None, None) => true,
        _ => false,
    };
    if fits {
        return;
    }
    roots.iter().for_each(|e| commands.entity(e).despawn());
    dialog.drawn = dialog.opened.map(|opened| Drawn::of(opened, status));
    let Some(opened) = dialog.opened else {
        return;
    };
    let title = match orders.host_at() {
        None => words.hosting.clone(),
        Some(at) => messages::filled(&words.connected_to, &[at]),
    };
    let root = commands
        .spawn((
            Root,
            DespawnOnExit(Screen::Match),
            ui::scrim(ui::layer::PAGE),
        ))
        .id();
    let panel = commands
        .spawn((
            ChildOf(root),
            ui::panel(
                ui::Surface::Panel,
                Node {
                    width: px(440),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(24)),
                    row_gap: px(10),
                    ..default()
                },
            ),
        ))
        .id();
    commands.spawn((ChildOf(panel), ui::text(title, Font::Large)));
    let heading = commands.spawn((ChildOf(panel), row())).id();
    commands.spawn((
        ChildOf(heading),
        ui::label(words.name.clone(), Font::Body, Ink::Muted),
        Node {
            flex_grow: 1.0,
            margin: UiRect::left(px(2.0 * (ICON + 6.0))),
            ..default()
        },
    ));
    commands.spawn((
        ChildOf(heading),
        ui::label(words.ping.clone(), Font::Body, Ink::Muted),
    ));
    for seat in &status.seats {
        slot(&mut commands, panel, seat, &words, &look, &naming);
    }
    if status.held {
        lower_panel(&mut commands, panel, &words);
    }
    let buttons = commands
        .spawn((
            ChildOf(panel),
            Node {
                justify_content: JustifyContent::End,
                column_gap: px(12),
                ..row()
            },
        ))
        .id();
    // Only one the player opened, with nothing holding the match.
    let closable = opened == Opened::ByPlayer && !status.held;
    if closable {
        commands
            .spawn((
                ChildOf(buttons),
                ui::button(words.close.clone(), Font::Body),
                ui::called("connection.close"),
            ))
            .observe(close);
    } else {
        commands.spawn((
            ChildOf(buttons),
            ui::disabled(words.close.clone(), Font::Body),
        ));
    }
    commands
        .spawn((
            ChildOf(buttons),
            ui::button(words.leave.clone(), Font::Body),
            ui::called("connection.leave"),
        ))
        .observe(ask_leave);
}

/// Keeps what's in the dialog's slots and lower panel as the host last
/// said.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn fill_in(
    orders: Res<Orders>,
    dialog: Res<Dialog>,
    words: Res<Words>,
    look: Res<Look>,
    mut texts: Query<
        (&mut Text, Option<&PingOf>, Has<Votes>),
        Or<(With<PingOf>, With<Votes>, With<Countdown>)>,
    >,
    mut glyphs: Query<(&mut ImageNode, &PingGlyphOf), Without<StandingOf>>,
    mut marks: Query<(&mut ImageNode, &mut ui::Hint, &StandingOf)>,
    mut kick: Query<&mut ui::Chosen, With<Kick>>,
) {
    let Some(status) = orders.status().filter(|_| dialog.opened.is_some()) else {
        return;
    };
    let seat = |client: u32| status.seats.iter().find(|s| s.client == client);
    let left = status.countdown.filter(|&left| left > 0);
    let left = left.filter(|_| status.votes >= status.needed);
    for (mut text, ping, votes) in &mut texts {
        let said = match (ping, votes) {
            (Some(PingOf(client)), _) => {
                let ms = seat(*client).and_then(|s| s.ping);
                ms.map(|ms| ms.to_string()).unwrap_or_default()
            }
            (None, true) => {
                let counts = [status.votes, status.needed].map(|n| n.to_string());
                messages::filled(&words.votes, &counts)
            }
            (None, false) => match left {
                Some(left) => messages::filled(&words.disconnecting, &[left.to_string()]),
                None => String::new(),
            },
        };
        if text.0 != said {
            text.0 = said;
        }
    }
    for (mut image, PingGlyphOf(client)) in &mut glyphs {
        let ms = seat(*client).and_then(|s| s.ping);
        let glyph = ms.and_then(|ms| look.pings.get(look.ping.glyph(ms)?));
        let glyph = glyph.cloned().unwrap_or_else(none);
        if image.image != glyph {
            image.image = glyph;
        }
    }
    for (mut image, mut hint, StandingOf(client)) in &mut marks {
        let standing = seat(*client).map_or(Standing::Ok, |s| s.standing);
        let mark = look.standings.get(&standing).cloned().unwrap_or_else(none);
        if image.image != mark {
            image.image = mark;
        }
        let said = words.standings.get(&standing).cloned().unwrap_or_default();
        if hint.0 != said {
            hint.0 = said;
        }
    }
    let voted = orders.voted();
    for mut chosen in &mut kick {
        if chosen.0 != voted {
            chosen.0 = voted;
        }
    }
}

fn row() -> Node {
    Node {
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(6),
        ..default()
    }
}

/// What an icon with nothing to draw draws: nothing, where a default
/// handle would draw white.
fn none() -> Handle<Image> {
    ImageNode::default().image
}

/// An icon, or the room for one where there's none to draw.
fn icon(image: Option<&Handle<Image>>) -> impl Bundle {
    let node = Node {
        width: px(ICON),
        height: px(ICON),
        flex_shrink: 0.0,
        ..default()
    };
    match image {
        Some(image) => (node, ImageNode::new(image.clone())),
        None => (node, ImageNode::default()),
    }
}

/// A client's line: the host's mark, its standing, who it is and its
/// ping, the last two filled in by [`fill_in`].
fn slot(
    commands: &mut Commands,
    panel: Entity,
    seat: &Seated,
    words: &Words,
    look: &Look,
    naming: &Naming,
) {
    let line = commands.spawn((ChildOf(panel), row())).id();
    let host = look.host.as_ref().filter(|_| seat.host);
    commands.spawn((ChildOf(line), icon(host)));
    commands.spawn((
        ChildOf(line),
        icon(None),
        ui::Hint(String::new()),
        StandingOf(seat.client),
    ));
    let name = commands
        .spawn((
            ChildOf(line),
            Node {
                flex_grow: 1.0,
                ..row()
            },
        ))
        .id();
    let player = (seat.players.first()).and_then(|&p| naming.sim.0.sides.player(p));
    match player {
        Some(player) => {
            let map = naming.loaded.as_ref().map(|l| l.map.strings.clone());
            let called = (naming.leaders).owner(player, naming.local.0, &map.unwrap_or_default());
            let node = Node {
                width: px(ui::GEM),
                height: px(ui::GEM),
                ..default()
            };
            let mark = ui::side_mark(&naming.gem, player.rgb, node);
            commands.spawn((ChildOf(name), mark));
            commands.spawn((ChildOf(name), ui::text(called, Font::Body)));
        }
        None => {
            commands.spawn((ChildOf(name), ui::text(words.observer.clone(), Font::Body)));
        }
    }
    commands.spawn((ChildOf(line), icon(None), PingGlyphOf(seat.client)));
    commands.spawn((ChildOf(line), ui::text("", Font::Body), PingOf(seat.client)));
}

/// What shows while the match is held: the vote to drop those holding it,
/// how it stands, and the countdown once enough have voted, all filled in
/// by [`fill_in`].
fn lower_panel(commands: &mut Commands, panel: Entity, words: &Words) {
    let line = commands.spawn((ChildOf(panel), row())).id();
    commands.spawn((
        ChildOf(line),
        ui::label(words.status.clone(), Font::Body, Ink::Muted),
        Node {
            flex_grow: 1.0,
            ..default()
        },
    ));
    commands
        .spawn((
            ChildOf(line),
            ui::toggle(words.kick.clone(), Font::Body),
            ui::called("connection.kick"),
            Kick,
        ))
        .observe(vote);
    commands.spawn((ChildOf(line), ui::text("", Font::Body), Votes));
    commands.spawn((
        ChildOf(panel),
        ui::label("", Font::Body, Ink::Warn),
        Countdown,
    ));
}

/// Sets or takes back the game's vote to drop those holding the match.
fn vote(_: On<Activate>, mut orders: ResMut<Orders>) {
    let voted = orders.voted();
    orders.vote(!voted);
}

fn close(_: On<Activate>, mut dialog: ResMut<Dialog>) {
    dialog.opened = None;
}

/// Asks whether to leave the game, and does if so, as the game menu does.
fn ask_leave(
    _: On<Activate>,
    mut commands: Commands,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
) {
    let question = words.leave_confirm.clone();
    confirm::confirm(&mut commands, &confirm, Screen::Match, question, menu::quit);
}

#[cfg(test)]
mod tests {
    use aaa_server::{Clock, Host, Local as Linked};
    use bevy::state::app::StatesPlugin;
    use std::time::Duration;

    use super::*;
    use crate::orders::testing::{fresh, let_in};

    /// A game joined to a match of players 1 and 2 with another client,
    /// all on `clock`, the host's own copy played by the test.
    struct Match {
        app: App,
        host: Host<Linked>,
        other: aaa_server::Client<Linked>,
        other_world: aaa_sim::World,
        clock: Clock,
    }

    impl Match {
        fn new() -> Match {
            let clock = Clock::stopped();
            let mut host = Host::new(aaa_server::Setup::default(), fresh());
            host.clock(clock.clone());
            let (ours, welcome) = let_in(&mut host, &[1]);
            let (theirs, _) = let_in(&mut host, &[2]);
            let mut orders = Orders::joined(Box::new(ours), welcome, "10.0.0.1:4000".into());
            orders.clock(clock.clone());
            let mut other = aaa_server::Client::new(theirs);
            other.clock(clock.clone());
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                StatesPlugin,
                ConnectionPlugin,
                confirm::ConfirmPlugin,
            ))
            .insert_state(Screen::Match)
            .insert_resource(Words::default())
            .insert_resource(confirm::Words::default())
            .insert_resource(orders)
            .insert_resource(Sim(fresh()))
            .insert_resource(Local(Some(1)))
            .init_resource::<Leaders>()
            .init_resource::<ui::Gem>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::console::Console>();
            app.update();
            Match {
                app,
                host,
                other,
                other_world: fresh(),
                clock,
            }
        }

        /// A second, by both clocks, in which the other client keeps up
        /// unless it's `silent`.
        fn second(&mut self, silent: bool) {
            self.clock.pass(Duration::from_secs(1));
            self.host.turn();
            if !silent {
                self.other
                    .advance(&mut self.other_world, usize::MAX)
                    .unwrap();
            }
            let world = self.app.world_mut();
            let mut sim = world.remove_resource::<Sim>().unwrap();
            world.resource_mut::<Orders>().step(&mut sim.0);
            world.insert_resource(sim);
            let mut time = world.resource_mut::<Time<Real>>();
            time.update_with_duration(Duration::from_secs(1));
            self.app.update();
        }

        fn shown(&mut self) -> Vec<String> {
            let world = self.app.world_mut();
            let mut texts = world.query::<&Text>();
            let shown = texts.iter(world).map(|t| t.0.clone());
            let mut shown: Vec<String> = shown.filter(|t| !t.is_empty()).collect();
            shown.sort();
            shown
        }

        fn press(&mut self, words: &str) {
            let world = self.app.world_mut();
            let mut labels = world.query::<(&Text, &ChildOf)>();
            let found = labels.iter(world).find(|(text, _)| text.0 == words);
            let entity = found.expect("no such button").1.parent();
            world.trigger(Activate { entity });
            self.app.update();
        }
    }

    #[test]
    fn it_opens_once_the_match_is_held_for_a_moment_and_closes_as_it_goes_on() {
        let mut m = Match::new();
        for _ in 0..3 {
            m.second(false);
        }
        assert!(m.shown().is_empty());
        // Held once the other is two seconds unheard, and shown two more.
        for _ in 0..4 {
            m.second(true);
        }
        assert!(m.shown().is_empty());
        m.second(true);
        let shown = m.shown();
        assert!(shown.contains(&"Connected to 10.0.0.1:4000".to_string()));
        // Only the game is still heard from, so its vote alone would do.
        assert!(shown.contains(&"0/1 Votes".to_string()), "{shown:?}");
        assert!(shown.contains(&"kingdom_2".to_string()), "{shown:?}");
        m.second(false);
        m.second(false);
        assert!(m.shown().is_empty());
    }

    #[test]
    fn voting_counts_down_once_enough_have() {
        let mut m = Match::new();
        m.second(false);
        for _ in 0..5 {
            m.second(true);
        }
        m.press("Kick");
        // Not until the host has heard it.
        assert!(!m.app.world().resource::<Orders>().voted());
        assert!(!m.shown().iter().any(|s| s.starts_with("Disconnecting")));
        m.second(true);
        let shown = m.shown();
        assert!(shown.contains(&"1/1 Votes".to_string()), "{shown:?}");
        assert!(
            shown.contains(&"Disconnecting in 11 seconds".to_string()),
            "{shown:?}"
        );
    }

    #[test]
    fn the_vote_shown_is_the_hosts_which_drops_clear() {
        let mut m = Match::new();
        m.second(false);
        for _ in 0..5 {
            m.second(true);
        }
        m.press("Kick");
        m.second(true);
        assert!(m.app.world().resource::<Orders>().voted());
        // Dropped once its allowance runs out, which clears the votes.
        for _ in 0..12 {
            m.second(true);
        }
        assert!(!m.app.world().resource::<Orders>().voted());
        assert_eq!(m.host.clients(), 1);
    }

    #[test]
    fn it_closes_once_the_host_is_found_gone() {
        let mut m = Match::new();
        m.second(false);
        for _ in 0..5 {
            m.second(true);
        }
        assert!(!m.shown().is_empty());
        let Match {
            mut app,
            host,
            clock,
            ..
        } = m;
        drop(host);
        for _ in 0..4 {
            clock.pass(Duration::from_secs(1));
            let world = app.world_mut();
            let mut sim = world.remove_resource::<Sim>().unwrap();
            world.resource_mut::<Orders>().step(&mut sim.0);
            world.insert_resource(sim);
            app.update();
        }
        assert!(app.world().resource::<Orders>().gone());
        let world = app.world_mut();
        let mut texts = world.query::<&Text>();
        assert_eq!(texts.iter(world).count(), 0);
    }

    #[test]
    fn opened_by_the_player_it_stays_until_closed() {
        let mut m = Match::new();
        m.second(false);
        open(&mut m.app.world_mut().resource_mut::<Dialog>());
        m.second(false);
        m.second(false);
        assert!(m.shown().contains(&"Close".to_string()));
        m.press("Close");
        assert!(m.shown().is_empty());
    }
}
