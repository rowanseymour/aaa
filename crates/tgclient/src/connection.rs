//! The connection dialog: how everyone in a match stands as its host sees
//! them. It's open while a match hosted elsewhere loads, opens by itself
//! once the match has been held for a moment and closes when it goes on,
//! and it's where players vote to drop those holding it or to kick a
//! player out. Its chat gets through to everyone at once, held match or
//! not.

use std::collections::HashMap;

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgdata::PingLook;
use tgserver::protocol::{Seated, Standing, Status};

use crate::ui::{self, Font, Ink};
use crate::{LoadingScreen, Orders, Screen, Sim, confirm, menu, messages};

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
    send: String,
    standings: HashMap<Standing, String>,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
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
            send: said(
                "connection_status_dialog_ChatPanel_SendButton_Client_View_text",
                "Send",
            ),
            standings: (standings.into_iter())
                .map(|(s, key, stock)| (s, said(key, stock)))
                .collect(),
        }
    }

    /// What the game menu's entry that opens the dialog says.
    pub fn menu_entry(strings: &tgdata::Strings) -> String {
        ui::said(strings)(
            "game_menu_ButtonConnectionStatus_Client_View_text",
            "Connection Status",
        )
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
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

impl Look {
    /// The glyph drawn before a ping of `ms` milliseconds.
    pub(crate) fn ping_glyph(&self, ms: u32) -> Option<Handle<Image>> {
        self.pings.get(self.ping.glyph(ms)?).cloned()
    }
}

/// Whether the dialog is open, and why.
#[derive(Resource, Default)]
pub struct Dialog {
    opened: Option<Opened>,
    /// When, by the real clock, the match was found held.
    held_since: Option<f32>,
    /// How it's laid out, to lay it out again only when that changes.
    drawn: Option<Drawn>,
    /// What's being typed in its chat, kept while it's laid out again.
    typing: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opened {
    /// By the player, from the game menu: it stays until they close it.
    Player,
    /// By the match being held: it closes when the match goes on.
    Hold,
    /// By the match, hosted elsewhere, being loaded: it closes as play
    /// begins, unless the match has been held long enough to keep it open.
    Loading,
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

    /// Whether players are offered the vote to kick one out: never while
    /// the match is held, nor with two or fewer playing sides, where it
    /// could never pass without the one kicked.
    fn kicks(&self) -> bool {
        !self.held && self.seats.iter().filter(|(_, p, _)| !p.is_empty()).count() > 2
    }
}

/// A client's ping, its glyph and its standing's mark, by the client.
#[derive(Component)]
struct PingOf(u32);
#[derive(Component)]
struct PingGlyphOf(u32);
#[derive(Component)]
struct StandingOf(u32);

/// The vote to kick a client out, and how it stands, by the client.
#[derive(Component)]
struct KickOf(u32);
#[derive(Component)]
struct KicksOf(u32);

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
/// What's being typed in the chat, as shown.
#[derive(Component)]
struct Typing;

pub struct ConnectionPlugin;

impl Plugin for ConnectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Dialog>()
            .init_resource::<Look>()
            .add_systems(OnEnter(Screen::Loading), |mut dialog: ResMut<Dialog>| {
                *dialog = Dialog::default()
            })
            // What was laid out over the loading screen names no sides.
            .add_systems(OnEnter(Screen::Match), |mut dialog: ResMut<Dialog>| {
                dialog.drawn = None
            })
            .add_systems(
                Update,
                (watch, draw, fill_in, keys, show_typing)
                    .chain()
                    .run_if(in_state(Screen::Loading).or_else(in_state(Screen::Match)))
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
        let image = tgrender::textures::decode(&data.0.read(path)?, path, formats, true)?;
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
    dialog.opened = Some(Opened::Player);
}

/// Whether the dialog can be opened: only in a match with others in it.
pub fn openable(orders: &Orders) -> bool {
    orders.joined_to()
}

/// Keeps the dialog open while a match hosted elsewhere loads, opens it
/// once the match has been held for a moment, and closes it as play
/// begins or the match goes on if that's what opened it.
fn watch(
    orders: Res<Orders>,
    time: Res<Time<Real>>,
    mut dialog: ResMut<Dialog>,
    loading: Query<(), With<LoadingScreen>>,
) {
    // With the host gone, what it last said no longer stands.
    let held = !orders.gone() && orders.status().is_some_and(|s| s.held);
    let now = time.elapsed_secs();
    let since = held.then(|| *dialog.held_since.get_or_insert(now));
    if !held {
        dialog.held_since = None;
    }
    if orders.joined_to() && !orders.gone() && !loading.is_empty() {
        dialog.opened = Some(Opened::Loading);
        return;
    }
    let long_held = since.is_some_and(|since| now - since >= OPENS_AFTER);
    dialog.opened = match dialog.opened {
        Some(Opened::Player) => Some(Opened::Player),
        _ if long_held => Some(Opened::Hold),
        _ => None,
    };
}

/// What the dialog needs to mark each client's side.
#[derive(bevy::ecs::system::SystemParam)]
struct Naming<'w> {
    /// None while the match is still being set up.
    sim: Option<Res<'w, Sim>>,
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
    let kicks = (dialog.drawn.as_ref()).is_some_and(Drawn::kicks);
    let me = orders.me();
    // Only those who play a side vote.
    let voter = status
        .seats
        .iter()
        .any(|s| s.client == me && !s.players.is_empty());
    for seat in &status.seats {
        let line = slot(&mut commands, panel, seat, &look, &naming);
        if kicks && !seat.players.is_empty() {
            // Whoever runs the host is there for good.
            let toggle = seat.client != me && !seat.host;
            kick_vote(&mut commands, line, seat, &words, toggle, voter);
        }
    }
    if status.held {
        lower_panel(&mut commands, panel, &words);
    }
    chat_panel(&mut commands, panel, &words);
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
    if opened == Opened::Player && !status.held {
        commands
            .spawn((
                ChildOf(buttons),
                ui::button(words.close.clone(), Font::Body),
                ui::called("connection.close"),
            ))
            .observe(close);
    }
    // Loading, only once the match is held.
    if opened != Opened::Loading || status.held {
        commands
            .spawn((
                ChildOf(buttons),
                ui::button(words.leave.clone(), Font::Body),
                ui::called("connection.leave"),
            ))
            .observe(ask_leave);
    }
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
        (&mut Text, Option<&PingOf>, Option<&KicksOf>, Has<Votes>),
        Or<(With<PingOf>, With<KicksOf>, With<Votes>, With<Countdown>)>,
    >,
    mut glyphs: Query<(&mut ImageNode, &PingGlyphOf), Without<StandingOf>>,
    mut marks: Query<(&mut ImageNode, &mut ui::Hint, &StandingOf)>,
    mut kick: Query<(&mut ui::Chosen, Option<&KickOf>), Or<(With<Kick>, With<KickOf>)>>,
) {
    let Some(status) = orders.status().filter(|_| dialog.opened.is_some()) else {
        return;
    };
    let seat = |client: u32| status.seats.iter().find(|s| s.client == client);
    let left = status.countdown.filter(|&left| left > 0);
    let left = left.filter(|_| status.votes >= status.needed);
    let against = |client: u32| {
        let voters = status.seats.iter().filter(|s| s.kicking.contains(&client));
        voters.count() as u32
    };
    for (mut text, ping, kicks, votes) in &mut texts {
        let said = match (ping, kicks, votes) {
            (Some(PingOf(client)), ..) => {
                let ms = seat(*client).and_then(|s| s.ping);
                ms.map(|ms| ms.to_string()).unwrap_or_default()
            }
            (None, Some(KicksOf(client)), _) => {
                let counts = [against(*client), status.kicks_needed].map(|n| n.to_string());
                messages::filled(&words.votes, &counts)
            }
            (None, None, true) => {
                let counts = [status.votes, status.needed].map(|n| n.to_string());
                messages::filled(&words.votes, &counts)
            }
            (None, None, false) => match left {
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
    for (mut chosen, of) in &mut kick {
        let voted = match of {
            Some(KickOf(client)) => orders.kicking(*client),
            None => orders.voted(),
        };
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
    look: &Look,
    naming: &Naming,
) -> Entity {
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
    let sides = naming.sim.as_ref().map(|sim| &sim.0.sides);
    let player = (seat.players.first()).and_then(|&p| sides?.player(p));
    if let Some(player) = player {
        let node = Node {
            width: px(ui::GEM),
            height: px(ui::GEM),
            ..default()
        };
        let mark = ui::side_mark(&naming.gem, player.rgb, node);
        commands.spawn((ChildOf(name), mark));
    }
    commands.spawn((ChildOf(name), ui::text(seat.name.clone(), Font::Body)));
    commands.spawn((ChildOf(line), icon(None), PingGlyphOf(seat.client)));
    commands.spawn((ChildOf(line), ui::text("", Font::Body), PingOf(seat.client)));
    line
}

/// The vote to kick `seat`'s client out, on its line: the toggle, but on
/// the player's own line, and how the vote stands, filled in by
/// [`fill_in`]. The toggle only works for a `voter`.
fn kick_vote(
    commands: &mut Commands,
    line: Entity,
    seat: &Seated,
    words: &Words,
    toggle: bool,
    voter: bool,
) {
    let client = seat.client;
    if toggle && voter {
        commands
            .spawn((
                ChildOf(line),
                ui::toggle(words.kick.clone(), Font::Body),
                ui::called(format!("connection.kick.{client}")),
                KickOf(client),
            ))
            .observe(move |_: On<Activate>, mut orders: ResMut<Orders>| {
                let voted = orders.kicking(client);
                orders.kick(client, !voted);
            });
    } else {
        let mut toggle_room =
            commands.spawn((ChildOf(line), ui::disabled(words.kick.clone(), Font::Body)));
        // On the player's own line it only keeps the columns in line.
        if !toggle {
            toggle_room.insert(Visibility::Hidden);
        }
    }
    commands.spawn((ChildOf(line), ui::text("", Font::Body), KicksOf(client)));
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

/// The dialog's chat: the message log, all of it, then where to type.
fn chat_panel(commands: &mut Commands, panel: Entity, words: &Words) {
    commands.spawn((
        ChildOf(panel),
        messages::WholeLog::default(),
        ScrollPosition::default(),
        Node {
            flex_direction: FlexDirection::Column,
            height: px(150),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ));
    let line = commands.spawn((ChildOf(panel), row())).id();
    commands.spawn((
        ChildOf(line),
        ui::text("_", Font::Body),
        Typing,
        Node {
            flex_grow: 1.0,
            overflow: Overflow::clip(),
            ..default()
        },
    ));
    commands
        .spawn((
            ChildOf(line),
            ui::button(words.send.clone(), Font::Body),
            ui::called("connection.send"),
        ))
        .observe(send_pressed);
}

/// Takes the keyboard while the dialog is open, for its chat: what's
/// typed, and Enter to send it.
fn keys(
    mut presses: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut dialog: ResMut<Dialog>,
    mut orders: ResMut<Orders>,
    console: Res<crate::console::Console>,
    asked: Query<(), With<confirm::Question>>,
    options: Query<(), With<crate::options::Page>>,
) {
    // Not under the console, a question asked over the dialog, or the
    // options, whose nickname takes what's typed.
    let under = !crate::console::closed(console) || !asked.is_empty() || !options.is_empty();
    if dialog.opened.is_none() || under {
        presses.clear();
        return;
    }
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        presses.clear();
        send(&mut dialog, &mut orders);
        return;
    }
    let mut typing = std::mem::take(&mut dialog.typing);
    crate::chat::type_into(&mut typing, tgsim::CHAT_LONGEST, &mut presses);
    if dialog.typing != typing {
        dialog.typing = typing;
    }
}

/// Has what's typed said to everyone, if there's anything.
fn send(dialog: &mut Dialog, orders: &mut Orders) {
    let words = std::mem::take(&mut dialog.typing);
    if !words.trim().is_empty() {
        orders.say(words);
    }
}

fn send_pressed(_: On<Activate>, mut dialog: ResMut<Dialog>, mut orders: ResMut<Orders>) {
    send(&mut dialog, &mut orders);
}

/// Keeps what's typed on show, with where the next goes.
fn show_typing(dialog: Res<Dialog>, mut texts: Query<&mut Text, With<Typing>>) {
    let shown = format!("{}_", dialog.typing);
    for mut text in &mut texts {
        if text.0 != shown {
            text.0 = shown.clone();
        }
    }
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
    // Asked on the loading screen, it stays up as the match is set up.
    confirm::confirm(&mut commands, &confirm, Screen::Match, question, menu::quit);
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;
    use std::time::Duration;
    use tgserver::{Clock, Host, Local as Linked};

    use super::*;
    use crate::orders::testing::{fresh, let_in};

    /// A game joined to a match of players 1 and 2 with another client,
    /// all on `clock`, the host's own copy played by the test.
    struct Match {
        app: App,
        host: Host<Linked>,
        other: tgserver::Client<Linked>,
        other_world: tgsim::World,
        /// Clients that only ever keep up, playing these sides.
        more: Vec<(tgserver::Client<Linked>, tgsim::World)>,
        clock: Clock,
        /// Whether the match has yet to begin, every copy of it still
        /// being set up.
        loading: bool,
    }

    impl Match {
        fn new() -> Match {
            Match::with(&[])
        }

        /// As [`Match::new`], with a client more for each of `more`.
        fn with(more: &[u32]) -> Match {
            let clock = Clock::stopped();
            let mut host = Host::new(tgserver::Setup::default(), fresh());
            host.clock(clock.clone());
            let (ours, welcome) = let_in(&mut host, &[1]);
            let (theirs, _) = let_in(&mut host, &[2]);
            let more = (more.iter())
                .map(|&p| {
                    let mut client = tgserver::Client::new(let_in(&mut host, &[p]).0);
                    client.clock(clock.clone());
                    (client, fresh())
                })
                .collect();
            let mut orders = Orders::joined(Box::new(ours), welcome, "10.0.0.1:4000".into());
            orders.clock(clock.clone());
            let mut other = tgserver::Client::new(theirs);
            other.clock(clock.clone());
            let mut app = App::new();
            app.add_plugins((
                MinimalPlugins,
                StatesPlugin,
                bevy::input::InputPlugin,
                ConnectionPlugin,
                confirm::ConfirmPlugin,
            ))
            .insert_state(Screen::Match)
            .insert_resource(Words::default())
            .insert_resource(confirm::Words::default())
            .insert_resource(orders)
            .insert_resource(Sim(fresh()))
            .init_resource::<ui::Gem>()
            .init_resource::<crate::console::Console>();
            app.update();
            Match {
                app,
                host,
                other,
                other_world: fresh(),
                more,
                clock,
                loading: false,
            }
        }

        /// As [`Match::new`], on the loading screen, with the game's copy
        /// of the match still being set up and the host yet to begin it.
        fn loading() -> Match {
            let mut m = Match::new();
            m.loading = true;
            let world = m.app.world_mut();
            world.remove_resource::<Sim>();
            world.spawn(LoadingScreen);
            let mut screen = world.resource_mut::<NextState<Screen>>();
            screen.set(Screen::Loading);
            m.app.update();
            m
        }

        /// Has a [`Match::loading`] begin: the game's copy set up, and
        /// then its loading screen taken down as the host begins it.
        fn begin(&mut self) {
            self.loading = false;
            let world = self.app.world_mut();
            world.insert_resource(Sim(fresh()));
            let mut screen = world.resource_mut::<NextState<Screen>>();
            screen.set(Screen::Match);
            self.app.update();
            let world = self.app.world_mut();
            let mut screens = world.query_filtered::<Entity, With<LoadingScreen>>();
            for screen in screens.iter(world).collect::<Vec<_>>() {
                world.despawn(screen);
            }
        }

        /// A second, by both clocks, in which the other client keeps up
        /// unless it's `silent`.
        fn second(&mut self, silent: bool) {
            self.clock.pass(Duration::from_secs(1));
            if self.loading {
                self.host.wait();
                if !silent {
                    self.other.wait().unwrap();
                }
                for (client, _) in &mut self.more {
                    client.wait().unwrap();
                }
                let world = self.app.world_mut();
                world.resource_mut::<Orders>().wait();
                let mut time = world.resource_mut::<Time<Real>>();
                time.update_with_duration(Duration::from_secs(1));
                self.app.update();
                return;
            }
            self.host.turn();
            if !silent {
                self.other
                    .advance(&mut self.other_world, usize::MAX)
                    .unwrap();
            }
            for (client, world) in &mut self.more {
                client.advance(world, usize::MAX).unwrap();
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

        /// Presses and lets go of `chord`, a frame each.
        fn key(&mut self, chord: &str) {
            let keys = crate::drive::press(self.app.world_mut(), chord).unwrap();
            self.app.update();
            crate::drive::release(self.app.world_mut(), &keys);
            self.app.update();
        }

        /// Presses the button [`ui::called`] `id`.
        fn press_called(&mut self, id: &str) {
            let world = self.app.world_mut();
            let mut buttons = world.query::<(Entity, &ui::ButtonId)>();
            let found = buttons.iter(world).find(|(_, b)| b.0 == id);
            let entity = found.expect("no such button").0;
            world.trigger(Activate { entity });
            self.app.update();
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
        assert!(shown.contains(&"Player 2".to_string()), "{shown:?}");
        m.second(false);
        m.second(false);
        assert!(m.shown().is_empty());
    }

    #[test]
    fn it_is_open_while_the_match_loads_with_no_way_out_unless_it_is_held() {
        let mut m = Match::loading();
        m.second(false);
        let shown = m.shown();
        assert!(shown.contains(&"Connected to 10.0.0.1:4000".to_string()));
        assert!(shown.contains(&"Player 2".to_string()), "{shown:?}");
        assert!(!shown.contains(&"Close".to_string()), "{shown:?}");
        assert!(!shown.contains(&"Leave Game".to_string()), "{shown:?}");
        // What's typed gets through as in play.
        m.key("h");
        m.key("enter");
        m.host.wait();
        m.other.wait().unwrap();
        let said: Vec<_> = m.other.take_said().into_iter().map(|s| s.words).collect();
        assert_eq!(said, ["h"]);
        // The other goes quiet before its copy is set up.
        for _ in 0..3 {
            m.second(true);
        }
        let shown = m.shown();
        assert!(shown.contains(&"Leave Game".to_string()), "{shown:?}");
        assert!(shown.contains(&"0/1 Votes".to_string()), "{shown:?}");
        assert!(!shown.contains(&"Close".to_string()), "{shown:?}");
    }

    #[test]
    fn as_play_begins_it_closes_unless_the_match_has_been_held_a_while() {
        let mut m = Match::loading();
        m.second(false);
        m.begin();
        m.second(false);
        assert!(m.shown().is_empty());
        let mut m = Match::loading();
        m.second(false);
        for _ in 0..4 {
            m.second(true);
        }
        m.begin();
        m.second(true);
        let shown = m.shown();
        assert!(shown.contains(&"Leave Game".to_string()), "{shown:?}");
        m.second(false);
        m.second(false);
        assert!(m.shown().is_empty());
    }

    #[test]
    fn its_chat_gets_through_at_once_while_the_match_is_held() {
        let mut m = Match::new();
        m.second(false);
        for _ in 0..5 {
            m.second(true);
        }
        assert!(m.app.world().resource::<Orders>().status().unwrap().held);
        for key in ["h", "i", "space", "x", "backspace"] {
            m.key(key);
        }
        assert!(m.shown().contains(&"hi _".to_string()));
        m.key("enter");
        assert!(m.shown().contains(&"_".to_string()));
        // Sent with Send too, and nothing typed sends nothing.
        m.key("o");
        m.press_called("connection.send");
        m.press_called("connection.send");
        m.host.turn();
        m.other.advance(&mut m.other_world, usize::MAX).unwrap();
        let said: Vec<_> = m.other.take_said().into_iter().map(|s| s.words).collect();
        assert_eq!(said, ["hi ", "o"]);
        // Outside the steps.
        let given = crate::orders::testing::asked(m.host.schedule().record());
        assert!(given.is_empty());
    }

    #[test]
    fn nothing_is_typed_under_the_question_whether_to_leave() {
        let mut m = Match::new();
        open(&mut m.app.world_mut().resource_mut());
        m.app.update();
        m.press_called("connection.leave");
        m.key("h");
        m.key("enter");
        assert!(!m.shown().contains(&"h_".to_string()));
        m.host.turn();
        m.other.advance(&mut m.other_world, usize::MAX).unwrap();
        assert_eq!(m.other.take_said(), []);
    }

    #[test]
    fn nothing_is_typed_under_the_options() {
        let mut m = Match::new();
        open(&mut m.app.world_mut().resource_mut());
        m.app.world_mut().spawn(crate::options::Page);
        m.app.update();
        m.key("h");
        m.key("enter");
        assert!(!m.shown().contains(&"h_".to_string()));
        m.host.turn();
        m.other.advance(&mut m.other_world, usize::MAX).unwrap();
        assert_eq!(m.other.take_said(), []);
    }

    #[test]
    fn closed_it_types_nothing() {
        let mut m = Match::new();
        m.key("h");
        m.key("enter");
        m.host.turn();
        m.other.advance(&mut m.other_world, usize::MAX).unwrap();
        assert_eq!(m.other.take_said(), []);
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
    fn with_more_than_two_sides_players_vote_to_kick_others() {
        let mut m = Match::with(&[3]);
        m.second(false);
        open(&mut m.app.world_mut().resource_mut::<Dialog>());
        m.second(false);
        // Not on the game's own line.
        let kicks = |m: &mut Match| {
            let world = m.app.world_mut();
            world.query::<&KickOf>().iter(world).count()
        };
        assert_eq!(kicks(&mut m), 2);
        assert_eq!(m.shown().iter().filter(|s| *s == "0/2 Votes").count(), 3);
        m.press_called("connection.kick.2");
        m.second(false);
        assert!(m.app.world().resource::<Orders>().kicking(2));
        assert!(m.shown().contains(&"1/2 Votes".to_string()));
        // The other side with it kicks it out.
        let target = m.app.world().resource::<Orders>().status().unwrap().seats[1].client;
        m.more[0].0.kick(target, true).unwrap();
        // It's gone, so it isn't heard from.
        m.second(true);
        assert_eq!(m.host.clients(), 2);
        assert_eq!(kicks(&mut m), 0);
    }

    #[test]
    fn with_two_sides_there_is_no_vote_to_kick() {
        let mut m = Match::new();
        m.second(false);
        open(&mut m.app.world_mut().resource_mut::<Dialog>());
        m.second(false);
        assert!(!m.shown().contains(&"Kick".to_string()));
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
