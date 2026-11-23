//! Playing over the local network: the LAN screen, which lists the games
//! hosts on the network answer for, and creating, finding and joining
//! one, which leads to its lobby.

use std::net::{SocketAddr, ToSocketAddrs};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgserver::lan::{Advert, Advertiser, Found, Search};
use tgserver::{Entry, Lobby, NotJoined, Refusal, Staging};

use crate::fields::{Entered, Field, field};
use crate::messages::filled;
use crate::orders::Line;
use crate::ui::{self, Font, Ink, Surface, button, called, label, panel, text};
use crate::{Data, Nickname, Screen, confirm, menu};

/// The port a host takes joins on unless it's taken, as the game's
/// `HostPortDefault`, and how many after it are tried.
pub const PORT: u16 = 7290;
const PORTS_TRIED: u16 = 10;

/// How long a search for games waits for answers.
const SEARCHING: Duration = Duration::from_secs(1);

/// How long after a search ends the list searches again by itself, as
/// the game's `GameSpyLANLobbyRefreshTime`.
const SEARCH_AGAIN: Duration = Duration::from_secs(15);

/// How long a join is waited on, as the game's `JoinTimeout`.
const JOIN_TIMEOUT: u64 = 60;

/// The fewest and most players a game is made for, as the game's
/// `MultiplayerMinNumPlayers` and `MultiplayerMaxNumPlayers`.
const FEWEST: usize = 2;
const MOST: usize = 12;

/// The most characters a game's name, password or address takes.
const LONGEST: usize = 512;

pub struct LanPlugin;

impl Plugin for LanPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Browser>()
            .init_resource::<Made>()
            .add_systems(
                OnEnter(Screen::Lan),
                (forget, spawn, refresh_now, tell_notice).chain(),
            )
            .add_systems(
                Update,
                (search, show_games.run_if(resource_changed::<Browser>), join)
                    .chain()
                    .run_if(in_state(Screen::Lan)),
            )
            .add_systems(Update, host.run_if(resource_exists::<Hosting>))
            .add_systems(Update, show_made.run_if(resource_changed::<Made>))
            .add_systems(OnEnter(Screen::MainMenu), stop_hosting);
    }
}

/// What the screens say, as the game's data has it.
#[derive(Resource)]
pub struct Words {
    joining: String,
    timeout: String,
    password: String,
    wrong_password: String,
    choose: String,
    exit: String,
    default_name: String,
    connect_failed: String,
    unresolved: String,
    cancel: String,
    pub(crate) failed: String,
    refusals: Refusals,
    pub(crate) kicked: String,
}

struct Refusals {
    full: String,
    version: String,
    in_progress: String,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let said = ui::said(strings);
        Words {
            joining: said(
                "ui_lobby_joining_game_format|s",
                "Joining '%s', please wait ...",
            ),
            timeout: said("ui_lobby_join_timeout|d", "Timeout in %d seconds"),
            password: said(
                "networking_disconnect_prompt_password",
                "Enter Game Password",
            ),
            wrong_password: said(
                "networking_disconnect_prompt_incorrect_password",
                "Incorrect Game Password.  Enter Game Password",
            ),
            choose: said(
                "ui_lobby_select_server",
                "Please choose a game to connect to",
            ),
            exit: said("ui_lobby_confirm_exit", "Exit Lobby?"),
            default_name: said("ui_lobby_default_game_name|s", "%s's Game"),
            connect_failed: said("ui_lobby_connect_failed", "Connection to game failed"),
            unresolved: said(
                "ui_lobby_join_direct_failed|s",
                "Could not resolve %s to an IP address",
            ),
            kicked: said(
                "networking_disconnect_kicked",
                "You were kicked from the game",
            ),
            cancel: said("button_cancel", "Cancel"),
            failed: said("failed_to_create_map", "Failed to create map"),
            refusals: Refusals {
                full: said("networking_disconnect_game_full", "Game is full"),
                version: said("networking_disconnect_wrong_version", "Incorrect Version"),
                in_progress: said(
                    "networking_disconnect_game_in_progress",
                    "Game is already in progress",
                ),
            },
        }
    }

    /// What a joiner is told for `refusal`.
    fn refused(&self, refusal: Refusal) -> String {
        let r = &self.refusals;
        match refusal {
            Refusal::Full => r.full.clone(),
            Refusal::Protocol(_) => r.version.clone(),
            Refusal::Begun => r.in_progress.clone(),
            Refusal::Kicked => self.kicked.clone(),
            Refusal::Password => self.password.clone(),
            Refusal::Lost | Refusal::NoPlayer(_) | Refusal::Taken(_) => self.connect_failed.clone(),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
    }
}

/// The games found on the network, and the search for more.
#[derive(Resource, Default)]
struct Browser {
    search: Option<Search>,
    /// The games that have answered this search, by where they're joined.
    answered: Vec<SocketAddr>,
    /// Whether this search keeps the games already listed.
    keeps: bool,
    games: Vec<Found>,
    selected: Option<SocketAddr>,
    /// When a game was last pressed, for a second press to join it.
    pressed: Option<(SocketAddr, Instant)>,
    /// When the last search ended.
    ended: Option<Instant>,
}

/// What the person last made a game as, and joined directly, while the
/// game runs.
#[derive(Resource)]
struct Made {
    /// The name, unless it's the default, which follows the nickname.
    name: Option<String>,
    password: String,
    most: usize,
    observers: bool,
    address: String,
    port: String,
}

impl Default for Made {
    fn default() -> Self {
        Made {
            name: None,
            password: String::new(),
            most: FEWEST,
            observers: true,
            address: String::new(),
            port: PORT.to_string(),
        }
    }
}

/// A join under way.
#[derive(Resource)]
struct Joining {
    /// What the game is called on the list, or the address typed.
    name: String,
    at: SocketAddr,
    password: String,
    started: Instant,
    step: Step,
    /// The box saying so.
    shown: Entity,
}

enum Step {
    Connecting(JoinHandle<Result<tgserver::quic::Remote, String>>),
    Asking(Line),
    /// Taken, to move it on.
    Between,
}

/// The game hosted here, while it's being set up and then played: the
/// listener, which its links go with, its lobby until launch, and its
/// answer to searches.
#[derive(Resource)]
pub(crate) struct Hosting {
    listener: tgserver::quic::Listener,
    pub(crate) lobby: Option<Lobby<Line>>,
    advertiser: Option<Advertiser>,
    port: u16,
    /// Whether its lobby has counted down to launch.
    pub(crate) launch: bool,
}

/// The person's end of a game being set up, and where its host is if
/// that's elsewhere.
#[derive(Resource)]
pub(crate) struct Staged {
    pub(crate) entry: Entry<Line>,
    pub(crate) at: Option<String>,
}

/// A game's row in the list.
#[derive(Component)]
struct GameRow(SocketAddr);

/// Where the games are listed.
#[derive(Component)]
struct GameList;

/// The joining box's words.
#[derive(Component)]
struct JoiningText;

/// The fields of the dialog up.
#[derive(Component)]
enum FieldOf {
    Name,
    Password,
    Address,
    Port,
}

/// The create dialog's choices that aren't typed.
#[derive(Component)]
struct MostShown;
#[derive(Component)]
struct ObserversShown;

/// Takes away whatever was left of a game here or joined.
fn forget(mut commands: Commands) {
    commands.remove_resource::<crate::lobby::Lobby>();
    commands.remove_resource::<Joining>();
    commands.remove_resource::<Staged>();
    commands.remove_resource::<Hosting>();
}

fn stop_hosting(mut commands: Commands) {
    commands.remove_resource::<Hosting>();
    commands.remove_resource::<Staged>();
}

fn spawn(mut commands: Commands) {
    commands.spawn((Camera2d, DespawnOnExit(Screen::Lan)));
    let root = commands
        .spawn((
            DespawnOnExit(Screen::Lan),
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                column_gap: px(16),
                ..default()
            },
        ))
        .id();
    let left = commands
        .spawn((
            ChildOf(root),
            panel(
                Surface::Panel,
                Node {
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(16)),
                    row_gap: px(8),
                    ..default()
                },
            ),
        ))
        .id();
    let header = commands.spawn((ChildOf(left), row())).id();
    for (words, width) in COLUMNS {
        commands.spawn((
            ChildOf(header),
            cell(width),
            children![label(words, Font::Body, Ink::Muted)],
        ));
    }
    commands.spawn((
        ChildOf(left),
        GameList,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ));
    let under = commands
        .spawn((
            ChildOf(left),
            Node {
                column_gap: px(12),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            ChildOf(under),
            button("Refresh", Font::Body),
            called("lan.refresh"),
        ))
        .observe(refresh);
    let column = commands.spawn((ChildOf(root), menu::side())).id();
    let buttons: [(&str, &str); 3] = [
        ("Create", "lan.create"),
        ("Join", "lan.join"),
        ("Join Direct", "lan.join-direct"),
    ];
    for (i, (words, id)) in buttons.into_iter().enumerate() {
        let mut button = commands.spawn((ChildOf(column), button(words, Font::Large), called(id)));
        match i {
            0 => button.observe(open_create),
            1 => button.observe(join_selected),
            _ => button.observe(open_join_direct),
        };
    }
    commands
        .spawn((
            ChildOf(column),
            button("Options", Font::Large),
            called("lan.options"),
        ))
        .observe(menu::go(Screen::Options));
    commands.spawn((ChildOf(column), menu::spacer()));
    commands
        .spawn((
            ChildOf(column),
            button("Exit", Font::Large),
            called("lan.exit"),
        ))
        .observe(ask_exit);
}

/// The list's columns, with their widths.
const COLUMNS: [(&str, f32); 5] = [
    ("", 24.0),
    ("Game Name", 320.0),
    ("Players", 90.0),
    ("Ping", 60.0),
    ("Map Name", 320.0),
];

fn row() -> Node {
    Node {
        column_gap: px(8),
        padding: UiRect::axes(px(8), px(4)),
        ..default()
    }
}

fn cell(width: f32) -> Node {
    Node {
        width: px(width),
        flex_shrink: 0.0,
        overflow: Overflow::clip(),
        ..default()
    }
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
        Screen::Lan,
        words.exit.clone(),
        menu::open(menu::MenuPage::Multiplayer),
    );
}

fn refresh_now(mut browser: ResMut<Browser>) {
    start_search(&mut browser, false);
}

fn refresh(_: On<Activate>, mut browser: ResMut<Browser>) {
    start_search(&mut browser, false);
}

/// Starts a search, stopping any under way: a fresh one empties the list,
/// and one that `keeps` it updates the games that answer and drops those
/// that don't.
fn start_search(browser: &mut Browser, keeps: bool) {
    if !keeps {
        browser.games.clear();
        browser.selected = None;
    }
    browser.keeps = keeps;
    browser.answered.clear();
    browser.search = match Search::start() {
        Ok(search) => Some(search),
        Err(e) => {
            warn!("can't search the network for games: {e}");
            None
        }
    };
    browser.ended = None;
}

/// Gathers the answers to the search under way, ending it once it's had
/// time to hear them all, and searches again a while after.
fn search(mut browser: ResMut<Browser>, joining: Option<Res<Joining>>) {
    let Some(search) = browser.search.as_mut() else {
        let due = browser.ended.is_some_and(|e| e.elapsed() >= SEARCH_AGAIN);
        if due && joining.is_none() {
            start_search(&mut browser, true);
        }
        return;
    };
    let found = search.found();
    let over = search.age() >= SEARCHING;
    for game in found {
        if game.advert.most == 0 {
            continue;
        }
        browser.answered.push(game.at);
        // The game here answers both the broadcast and the query to this
        // machine: one row, by the address the network sees.
        let same = |g: &Found| {
            g.at == game.at
                || (g.at.port() == game.at.port()
                    && g.advert == game.advert
                    && (g.at.ip().is_loopback() || game.at.ip().is_loopback()))
        };
        match browser.games.iter_mut().find(|g| same(g)) {
            Some(listed) if game.at.ip().is_loopback() && !listed.at.ip().is_loopback() => {}
            Some(listed) => *listed = game,
            None => browser.games.push(game),
        }
    }
    if over {
        if browser.keeps {
            let answered = std::mem::take(&mut browser.answered);
            browser.games.retain(|g| answered.contains(&g.at));
        }
        browser.search = None;
        browser.ended = Some(Instant::now());
    }
}

fn show_games(
    mut commands: Commands,
    browser: Res<Browser>,
    list: Single<Entity, With<GameList>>,
    look: Res<crate::connection::Look>,
) {
    commands.entity(*list).despawn_children();
    for game in &browser.games {
        let advert = &game.advert;
        let ink = match (advert.compatible(), advert.playing) {
            (false, _) => Color::srgb(1.0, 0.2, 0.2),
            (true, true) => Color::srgb(0.6, 0.6, 0.6),
            (true, false) => Color::WHITE,
        };
        let ping = look.ping_glyph(game.ping.as_millis() as u32);
        let chosen = browser.selected == Some(game.at);
        let line = commands
            .spawn((
                ChildOf(*list),
                ui::clickable(ui::card(row())),
                GameRow(game.at),
                called(format!("lan.game.{}", ui::slug(&advert.name))),
            ))
            .insert(ui::Chosen(chosen))
            .observe(press_game)
            .id();
        let lock = match advert.locked {
            true => "*",
            false => "",
        };
        let cells: [(String, Color); 3] = [
            (lock.into(), Color::WHITE),
            (advert.name.clone(), ink),
            (format!("{}/{}", advert.people, advert.most), Color::WHITE),
        ];
        for ((words, colour), (_, width)) in cells.into_iter().zip(COLUMNS) {
            commands.spawn((
                ChildOf(line),
                cell(width),
                children![ui::coloured(words, Font::Body, colour)],
            ));
        }
        let glyph = commands.spawn((ChildOf(line), cell(COLUMNS[3].1))).id();
        if let Some(ping) = ping {
            commands.spawn((ChildOf(glyph), ImageNode::new(ping)));
        }
        let map = tgdata::map_title(&advert.map).to_string();
        commands.spawn((
            ChildOf(line),
            cell(COLUMNS[4].1),
            children![label(map, Font::Body, Ink::Body)],
        ));
    }
}

/// Selects the game pressed, or joins it on a second press.
fn press_game(
    pressed: On<Activate>,
    rows: Query<&GameRow>,
    mut browser: ResMut<Browser>,
    mut commands: Commands,
    words: Res<Words>,
    joining: Option<Res<Joining>>,
) {
    let Ok(GameRow(at)) = rows.get(pressed.entity) else {
        return;
    };
    let again = browser
        .pressed
        .is_some_and(|(was, when)| was == *at && when.elapsed() < Duration::from_millis(400));
    browser.pressed = Some((*at, Instant::now()));
    browser.selected = Some(*at);
    if again && joining.is_none() {
        let name = game_name(&browser, *at);
        start_join(&mut commands, &words, name, *at, String::new());
    }
}

/// What a game is called on the list, or its address if it has no name.
fn game_name(browser: &Browser, at: SocketAddr) -> String {
    let game = browser.games.iter().find(|g| g.at == at);
    match game.map(|g| g.advert.name.clone()) {
        Some(name) if !name.is_empty() => name,
        _ => at.to_string(),
    }
}

fn join_selected(
    _: On<Activate>,
    browser: Res<Browser>,
    mut commands: Commands,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    joining: Option<Res<Joining>>,
) {
    if joining.is_some() {
        return;
    }
    let Some(at) = browser.selected else {
        notice(&mut commands, &confirm, words.choose.clone());
        return;
    };
    let name = game_name(&browser, at);
    start_join(&mut commands, &words, name, at, String::new());
}

/// Something to tell the person in an OK box once they're back on the LAN
/// screen.
#[derive(Resource)]
pub(crate) struct Notice(pub(crate) String);

fn tell_notice(mut commands: Commands, notice: Option<Res<Notice>>, confirm: Res<confirm::Words>) {
    if let Some(notice) = notice {
        self::notice(&mut commands, &confirm, notice.0.clone());
        commands.remove_resource::<Notice>();
    }
}

/// An OK box saying `words`, leaving the person on the LAN screen.
pub(crate) fn notice(commands: &mut Commands, confirm: &confirm::Words, words: String) {
    notice_on(commands, confirm, Screen::Lan, words);
}

/// As [`notice`], on `screen`.
pub(crate) fn notice_on(
    commands: &mut Commands,
    confirm: &confirm::Words,
    screen: Screen,
    words: String,
) {
    let asked = confirm::ask(commands, screen, words);
    confirm::offer(
        commands,
        asked,
        confirm.ok.clone(),
        "notice.ok",
        confirm::nothing,
    );
}

/// Starts joining the game at `at`, called `name`, with `password`.
fn start_join(
    commands: &mut Commands,
    words: &Words,
    name: String,
    at: SocketAddr,
    password: String,
) {
    let asked = confirm::ask(commands, Screen::Lan, filled(&words.joining, &[&name]));
    let timeout = filled(&words.timeout, &[&JOIN_TIMEOUT.to_string()]);
    commands.spawn((
        ChildOf(asked.dialog()),
        JoiningText,
        text(timeout, Font::Body),
    ));
    confirm::offer(
        commands,
        asked,
        words.cancel.clone(),
        "lan.joining.cancel",
        cancel_join,
    );
    let thread = std::thread::spawn(move || tgserver::quic::connect(at));
    commands.insert_resource(Joining {
        name,
        at,
        password,
        started: Instant::now(),
        step: Step::Connecting(thread),
        shown: asked.root(),
    });
}

fn cancel_join(_: On<Activate>, mut commands: Commands) {
    commands.remove_resource::<Joining>();
}

/// Takes a join on: connected, it asks to come in, and let in, it goes to
/// the game's lobby; otherwise it says why not.
#[allow(clippy::too_many_arguments)]
fn join(
    mut commands: Commands,
    joining: Option<ResMut<Joining>>,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    nickname: Res<Nickname>,
    mut shown: Query<&mut Text, With<JoiningText>>,
    mut next: ResMut<NextState<Screen>>,
) {
    let Some(mut joining) = joining else {
        return;
    };
    let left = JOIN_TIMEOUT.saturating_sub(joining.started.elapsed().as_secs());
    for mut text in &mut shown {
        let now = filled(&words.timeout, &[&left.to_string()]);
        if text.0 != now {
            text.0 = now;
        }
    }
    let fail = |commands: &mut Commands, joining: &Joining, words: String| {
        commands.entity(joining.shown).try_despawn();
        commands.remove_resource::<Joining>();
        notice(commands, &confirm, words);
    };
    if left == 0 {
        fail(&mut commands, &joining, words.connect_failed.clone());
        return;
    }
    match std::mem::replace(&mut joining.step, Step::Between) {
        Step::Connecting(thread) if !thread.is_finished() => {
            joining.step = Step::Connecting(thread);
        }
        Step::Connecting(thread) => match thread.join() {
            Ok(Ok(mut link)) => {
                let password = joining.password.clone();
                match tgserver::enter_lobby(&mut link, nickname.0.clone(), password) {
                    Ok(()) => joining.step = Step::Asking(Box::new(link)),
                    Err(_) => fail(&mut commands, &joining, words.connect_failed.clone()),
                }
            }
            _ => fail(&mut commands, &joining, words.connect_failed.clone()),
        },
        Step::Asking(link) => match Entry::entered(link) {
            Ok(Err(link)) => joining.step = Step::Asking(link),
            Ok(Ok(entry)) => {
                commands.entity(joining.shown).try_despawn();
                commands.insert_resource(Staged {
                    entry,
                    at: Some(joining.at.to_string()),
                });
                commands.remove_resource::<Joining>();
                next.set(Screen::Lobby);
            }
            Err(NotJoined::Refused(Refusal::Password)) => {
                commands.entity(joining.shown).try_despawn();
                let wrong = !joining.password.is_empty();
                ask_password(
                    &mut commands,
                    &words,
                    &confirm,
                    joining.name.clone(),
                    joining.at,
                    wrong,
                );
                commands.remove_resource::<Joining>();
            }
            Err(NotJoined::Refused(refusal)) => {
                fail(&mut commands, &joining, words.refused(refusal))
            }
            Err(_) => fail(&mut commands, &joining, words.connect_failed.clone()),
        },
        Step::Between => {}
    }
}

/// The game a password is asked for.
#[derive(Component)]
struct PasswordFor(String, SocketAddr);

fn ask_password(
    commands: &mut Commands,
    words: &Words,
    confirm: &confirm::Words,
    name: String,
    at: SocketAddr,
    wrong: bool,
) {
    let question = match wrong {
        true => words.wrong_password.clone(),
        false => words.password.clone(),
    };
    let asked = confirm::ask(commands, Screen::Lan, question);
    let mut password = commands.spawn((
        ChildOf(asked.dialog()),
        field("", LONGEST, percent(100)),
        FieldOf::Password,
        PasswordFor(name, at),
    ));
    password.observe(enter_password);
    let id = password.id();
    commands.entity(id).insert(Field {
        longest: LONGEST,
        masked: true,
        ..default()
    });
    commands.insert_resource(crate::fields::Focused(Some(id)));
    confirm::offer(
        commands,
        asked,
        confirm.ok.clone(),
        "lan.password.ok",
        try_password,
    );
    confirm::offer(
        commands,
        asked,
        confirm.cancel(),
        "lan.password.cancel",
        confirm::nothing,
    );
}

fn enter_password(
    entered: On<Entered>,
    mut commands: Commands,
    fields: Query<(&Field, &PasswordFor)>,
    questions: Query<Entity, With<confirm::Question>>,
    words: Res<Words>,
) {
    let Ok((field, PasswordFor(name, at))) = fields.get(entered.entity) else {
        return;
    };
    questions
        .iter()
        .for_each(|q| commands.entity(q).try_despawn());
    start_join(&mut commands, &words, name.clone(), *at, field.text.clone());
}

fn try_password(
    _: On<Activate>,
    mut commands: Commands,
    fields: Query<(&Field, &PasswordFor)>,
    words: Res<Words>,
) {
    let Some((field, PasswordFor(name, at))) = fields.iter().next() else {
        return;
    };
    start_join(&mut commands, &words, name.clone(), *at, field.text.clone());
}

fn open_join_direct(
    _: On<Activate>,
    mut commands: Commands,
    made: Res<Made>,
    confirm: Res<confirm::Words>,
) {
    let asked = confirm::ask(&mut commands, Screen::Lan, "Join Direct");
    let dialog = asked.dialog();
    for (words, typed, which, longest) in [
        (
            "IP Address",
            made.address.clone(),
            FieldOf::Address,
            LONGEST,
        ),
        ("Port", made.port.clone(), FieldOf::Port, 5),
    ] {
        commands.spawn((ChildOf(dialog), label(words, Font::Body, Ink::Muted)));
        let digits = matches!(which, FieldOf::Port);
        commands
            .spawn((
                ChildOf(dialog),
                field(typed.clone(), longest, percent(100)),
                which,
            ))
            .insert(Field {
                text: typed,
                longest,
                digits,
                masked: false,
            })
            .observe(|_: On<Entered>, mut commands: Commands| {
                commands.run_system_cached(join_direct)
            });
    }
    confirm::offer(
        &mut commands,
        asked,
        "Join".into(),
        "lan.join-direct.join",
        |_: On<Activate>, mut commands: Commands| {
            commands.run_system_cached(join_direct);
        },
    );
    confirm::offer(
        &mut commands,
        asked,
        confirm.cancel(),
        "lan.join-direct.cancel",
        confirm::nothing,
    );
}

/// Joins the address typed in the Join Direct box, remembering it.
fn join_direct(
    mut commands: Commands,
    fields: Query<(&Field, &FieldOf)>,
    questions: Query<Entity, With<confirm::Question>>,
    mut made: ResMut<Made>,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    joining: Option<Res<Joining>>,
) {
    if joining.is_some() {
        return;
    }
    let typed = |which: fn(&FieldOf) -> bool| {
        let found = fields.iter().find(|(_, f)| which(f));
        found
            .map(|(f, _)| f.text.trim().to_string())
            .unwrap_or_default()
    };
    let address = typed(|f| matches!(f, FieldOf::Address));
    let port = typed(|f| matches!(f, FieldOf::Port));
    if address.is_empty() {
        return;
    }
    questions
        .iter()
        .for_each(|q| commands.entity(q).try_despawn());
    made.address.clone_from(&address);
    made.port.clone_from(&port);
    let port: u16 = port.parse().unwrap_or(PORT);
    let resolved = (address.as_str(), port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.find(SocketAddr::is_ipv4));
    match resolved {
        Some(at) => start_join(&mut commands, &words, address, at, String::new()),
        None => notice(
            &mut commands,
            &confirm,
            filled(&words.unresolved, &[&address]),
        ),
    }
}

fn open_create(
    _: On<Activate>,
    mut commands: Commands,
    made: Res<Made>,
    words: Res<Words>,
    nickname: Res<Nickname>,
    confirm: Res<confirm::Words>,
) {
    let asked = confirm::ask(&mut commands, Screen::Lan, "Create Game");
    let dialog = asked.dialog();
    let name = made
        .name
        .clone()
        .unwrap_or_else(|| filled(&words.default_name, &[&nickname.0]));
    for (words, typed, which, masked) in [
        ("Game Name", name, FieldOf::Name, false),
        ("Password", made.password.clone(), FieldOf::Password, true),
    ] {
        commands.spawn((ChildOf(dialog), label(words, Font::Body, Ink::Muted)));
        commands
            .spawn((
                ChildOf(dialog),
                field(typed.clone(), LONGEST, percent(100)),
                which,
            ))
            .insert(Field {
                text: typed,
                longest: LONGEST,
                digits: false,
                masked,
            });
    }
    let most = commands
        .spawn((
            ChildOf(dialog),
            Node {
                column_gap: px(12),
                align_items: AlignItems::Center,
                ..default()
            },
        ))
        .id();
    commands.spawn((ChildOf(most), label("Max Players", Font::Body, Ink::Muted)));
    commands
        .spawn((
            ChildOf(most),
            button("<", Font::Body),
            called("lan.create.fewer"),
        ))
        .observe(|_: On<Activate>, mut made: ResMut<Made>| made.most = (made.most - 1).max(FEWEST));
    commands.spawn((
        ChildOf(most),
        MostShown,
        text(made.most.to_string(), Font::Body),
    ));
    commands
        .spawn((
            ChildOf(most),
            button(">", Font::Body),
            called("lan.create.more"),
        ))
        .observe(|_: On<Activate>, mut made: ResMut<Made>| made.most = (made.most + 1).min(MOST));
    commands
        .spawn((
            ChildOf(dialog),
            ui::toggle("Observers", Font::Body),
            ObserversShown,
            called("lan.create.observers"),
        ))
        .insert(ui::Chosen(made.observers))
        .observe(|_: On<Activate>, mut made: ResMut<Made>| made.observers = !made.observers);
    confirm::offer(
        &mut commands,
        asked,
        "Create".into(),
        "lan.create.create",
        |_: On<Activate>, mut commands: Commands| {
            commands.run_system_cached(create);
        },
    );
    confirm::offer(
        &mut commands,
        asked,
        confirm.cancel(),
        "lan.create.cancel",
        confirm::nothing,
    );
}

/// Shows the create dialog's choices as they stand.
fn show_made(
    made: Res<Made>,
    mut most: Query<&mut Text, With<MostShown>>,
    mut observers: Query<&mut ui::Chosen, With<ObserversShown>>,
) {
    for mut text in &mut most {
        text.0 = made.most.to_string();
    }
    for mut chosen in &mut observers {
        chosen.0 = made.observers;
    }
}

/// Hosts a game as the create dialog has it, taking the person to its
/// lobby as its host.
#[allow(clippy::too_many_arguments)]
fn create(
    mut commands: Commands,
    fields: Query<(&Field, &FieldOf)>,
    mut made: ResMut<Made>,
    words: Res<Words>,
    confirm: Res<confirm::Words>,
    nickname: Res<Nickname>,
    data: Res<Data>,
    mut next: ResMut<NextState<Screen>>,
) {
    let typed = |which: fn(&FieldOf) -> bool| {
        let found = fields.iter().find(|(_, f)| which(f));
        found.map(|(f, _)| f.text.clone()).unwrap_or_default()
    };
    let default_name = filled(&words.default_name, &[&nickname.0]);
    let name = typed(|f| matches!(f, FieldOf::Name));
    let name = match name.trim() {
        "" => default_name.clone(),
        name => name.to_string(),
    };
    made.name = (name != default_name).then(|| name.clone());
    made.password = typed(|f| matches!(f, FieldOf::Password));
    let Some((listener, port)) = listen() else {
        let bind = "Unable to bind to port %d";
        notice(&mut commands, &confirm, filled(bind, &[&PORT.to_string()]));
        return;
    };
    let map = crate::lobby::first_scenario(&data).unwrap_or_default();
    let (preset, sides) = crate::lobby::opening(&data, &map);
    let staging = Staging {
        name,
        most: made.most,
        observers: made.observers,
        map,
        preset,
        slots: tgserver::Slots::left_open(&sides),
        sides,
        ..default()
    };
    let mut lobby: Lobby<Line> = Lobby::new(staging, made.password.clone());
    lobby.name_by(tgserver::Naming::of(&data.0.strings(), &Default::default()));
    let (ours, theirs) = tgserver::pair();
    if lobby
        .admit(Box::new(ours) as Line, &nickname.0, "")
        .is_err()
    {
        return;
    }
    let entry = match Entry::entered(Box::new(theirs) as Line) {
        Ok(Ok(entry)) => entry,
        _ => return,
    };
    let advertiser = Advertiser::new(advert(lobby.staging(), port, false))
        .inspect_err(|e| warn!("the game can't be found on the network: {e}"))
        .ok();
    commands.insert_resource(Hosting {
        listener,
        lobby: Some(lobby),
        advertiser,
        port,
        launch: false,
    });
    commands.insert_resource(Staged { entry, at: None });
    next.set(Screen::Lobby);
}

/// Listens for joins on the first free port from [`PORT`].
fn listen() -> Option<(tgserver::quic::Listener, u16)> {
    (PORT..PORT + PORTS_TRIED).find_map(|port| {
        let at = SocketAddr::from(([0, 0, 0, 0], port));
        tgserver::quic::listen(at).ok().map(|l| (l, port))
    })
}

/// What a host says of its game when searched for.
fn advert(staging: &Staging, port: u16, playing: bool) -> Advert {
    Advert {
        name: staging.name.clone(),
        people: staging.guests.len() as u32,
        most: staging.most as u32,
        locked: staging.locked,
        map: staging.map.clone(),
        protocol: tgserver::protocol::PROTOCOL,
        playing,
        port,
    }
}

/// Runs the game hosted here: lets people in to its lobby and hears from
/// them there until it launches, refusing them after, and answers
/// searches for it.
fn host(mut hosting: ResMut<Hosting>) {
    let hosting = &mut *hosting;
    while let Some(link) = hosting.listener.accept() {
        let mut link: Line = Box::new(link);
        match hosting.lobby.as_mut() {
            Some(lobby) => lobby.greet(link),
            None => tgserver::refuse(&mut link, Refusal::Begun),
        }
    }
    if let Some(lobby) = hosting.lobby.as_mut() {
        lobby.seat();
        hosting.launch |= lobby.turn();
    }
    let Some(advertiser) = hosting.advertiser.as_mut() else {
        return;
    };
    match &hosting.lobby {
        Some(lobby) => advertiser.advertise(advert(lobby.staging(), hosting.port, false)),
        None => advertiser.playing(),
    }
    advertiser.answer();
}
