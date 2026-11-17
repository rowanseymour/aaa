//! The chat bar: where the player types to everyone, to their allies or
//! to one other person, while the match goes on. It's hidden until opened.
//! What's said travels as an order, and the message log shows it.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgrender::camera::{self, RtsCamera};
use tgsim::{Action, Channel, Relation, World};

use crate::keys::Bindings;
use crate::messages::MessageLog;
use crate::ui::{self, Font};
use crate::{Leaders, Local, Orders, Screen, Sim};

/// Where the bar is and how wide, as its layout places it on a screen
/// 1024 by 768.
const AT: Vec2 = Vec2::new(318.0, 496.0);
const WIDTH: f32 = 461.0;
/// What opens the bar, as the hotkey files name it.
const ACTIVATE: &str = "ChatActivate";

/// What the bar says.
#[derive(Resource, Clone)]
pub struct Words {
    all: String,
    allies: String,
    observers: String,
    send: String,
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let said = ui::said(strings);
        Words {
            all: said("ui_game_chat_filter_all", "All"),
            allies: said("ui_game_chat_filter_allies", "Allies"),
            observers: said("ui_game_chat_filter_observers", "Observers"),
            send: said("chat_panel_SendChatButton_Client_Label_text", "Send"),
        }
    }
}

impl Default for Words {
    fn default() -> Self {
        Words::of(&tgdata::Strings::default())
    }
}

/// Who the bar sends to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    All,
    /// The player's allies.
    Allies,
    /// Those watching, for one who watches.
    Observers,
    /// The person playing this side.
    To(u32),
}

impl Filter {
    fn channel(self) -> Channel {
        match self {
            Filter::All => Channel::All,
            Filter::Allies | Filter::Observers => Channel::Team,
            Filter::To(side) => Channel::To(side),
        }
    }
}

/// Whether the bar is open, what's being typed in it and who it's for,
/// and what was sent before.
#[derive(Resource, Default)]
pub struct Chat {
    open: bool,
    input: String,
    filter: Option<Filter>,
    sent: Vec<String>,
    /// Which of those is being looked at again, if one is.
    recall: Option<usize>,
}

#[derive(Component)]
struct Bar;
#[derive(Component)]
struct Typed;
#[derive(Component)]
struct Shown;

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Chat>()
            .add_systems(OnEnter(Screen::Match), |mut chat: ResMut<Chat>| {
                // What was sent before is kept, as in a console.
                chat.open = false;
                chat.input.clear();
                chat.filter = None;
            })
            .add_systems(
                Update,
                (
                    (draw, fill_in)
                        .chain()
                        .run_if(resource_exists::<Orders>)
                        .run_if(resource_changed::<Chat>),
                    (give_way, hold_camera_keys)
                        .chain()
                        .before(camera::Controls),
                )
                    .run_if(in_state(Screen::Match)),
            );
    }
}

impl Chat {
    pub fn is_open(&self) -> bool {
        self.open
    }
}

/// Whether the bar is closed, so that the keyboard is the match's; it is
/// where there's none.
pub fn closed(chat: Option<Res<Chat>>) -> bool {
    chat.is_none_or(|c| !c.is_open())
}

/// The people other than the player here who play sides, by side: those
/// the bar can whisper to.
fn people(orders: &Orders, local: Option<u32>) -> Vec<u32> {
    let seats = orders.status().into_iter().flat_map(|s| &s.seats);
    let sides = seats.flat_map(|s| s.players.iter().copied());
    sides.filter(|&side| Some(side) != local).collect()
}

/// What the bar can send to, in the order it offers them.
fn offered(orders: &Orders, local: Option<u32>) -> Vec<Filter> {
    match local {
        Some(_) => {
            let whispers = people(orders, local).into_iter().map(Filter::To);
            [Filter::All, Filter::Allies]
                .into_iter()
                .chain(whispers)
                .collect()
        }
        None => vec![Filter::All, Filter::Observers],
    }
}

/// What the bar starts on: to allies where the player has any.
fn first(sim: &World, local: Option<u32>) -> Filter {
    let allied = |me: u32| {
        let others = sim.sides.players.iter().filter(|p| p.id != me);
        others
            .map(|p| sim.sides.relation(me, p.id))
            .any(|r| r == Relation::Ally)
    };
    match local {
        Some(me) if allied(me) => Filter::Allies,
        _ => Filter::All,
    }
}

/// Takes the keyboard while the bar is open: what's typed, sending, and
/// stepping through who it's for and what was sent before. Opens it when
/// asked, as its key does while the match has the keyboard.
#[allow(clippy::too_many_arguments)]
pub fn keys(
    mut presses: MessageReader<KeyboardInput>,
    mut intents: MessageReader<crate::intent::Intent>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mut chat: ResMut<Chat>,
    mut orders: ResMut<Orders>,
    sim: Res<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    mut log: ResMut<MessageLog>,
    time: Res<Time>,
) {
    let activate = crate::intent::asked(&mut intents, ACTIVATE);
    if !chat.open {
        presses.clear();
        if activate {
            open(&mut chat, &orders, &sim.0, local.0);
        }
        return;
    }
    let step = |chat: &mut Chat, by: isize| {
        chat.filter = stepped(&offered(&orders, local.0), chat.filter, by);
    };
    if bindings.pressed(&keys, "ChatSend") {
        presses.clear();
        send(
            &mut chat,
            &mut orders,
            &sim.0,
            local.0,
            &leaders,
            &mut log,
            &time,
        );
        return;
    }
    if bindings.pressed(&keys, "GameChatPrevFilter") {
        step(&mut chat, -1);
    }
    if bindings.pressed(&keys, "GameChatNextFilter") {
        step(&mut chat, 1);
    }
    if bindings.pressed(&keys, "PreviousText") {
        let back = match chat.recall {
            Some(at) => Some(at.saturating_sub(1)),
            None => chat.sent.len().checked_sub(1),
        };
        if let Some(back) = back {
            chat.recall = Some(back);
            chat.input = chat.sent[back].clone();
        }
    }
    if bindings.pressed(&keys, "NextText")
        && let Some(at) = chat.recall
    {
        match chat.sent.get(at + 1).cloned() {
            Some(line) => {
                chat.input = line;
                chat.recall = Some(at + 1);
            }
            None => {
                chat.input.clear();
                chat.recall = None;
            }
        }
    }
    type_into(&mut chat.input, tgsim::CHAT_LONGEST, &mut presses);
}

/// Types what's been pressed into `input`, as far as `longest`
/// characters; Backspace takes the last character back.
pub(crate) fn type_into(
    input: &mut String,
    longest: usize,
    presses: &mut MessageReader<KeyboardInput>,
) {
    for press in presses.read() {
        if press.state != ButtonState::Pressed {
            continue;
        }
        if press.key_code == KeyCode::Backspace {
            input.pop();
            continue;
        }
        let typed = match &press.logical_key {
            Key::Character(c) => c.as_str(),
            Key::Space => " ",
            _ => continue,
        };
        let room = longest.saturating_sub(input.chars().count());
        let typed: String = typed
            .chars()
            .filter(|c| !c.is_control())
            .take(room)
            .collect();
        input.push_str(&typed);
    }
}

/// The choice `by` along from `from` in `offered`, wrapping round; the
/// first if `from` isn't among them.
fn stepped(offered: &[Filter], from: Option<Filter>, by: isize) -> Option<Filter> {
    let at = offered.iter().position(|&f| Some(f) == from);
    let n = offered.len() as isize;
    let next = at.map_or(0, |at| (at as isize + by).rem_euclid(n.max(1)));
    offered.get(next as usize).copied()
}

/// Opens the bar on what it last sent to while that's still on offer.
fn open(chat: &mut Chat, orders: &Orders, sim: &World, local: Option<u32>) {
    chat.open = true;
    chat.recall = None;
    let offered = offered(orders, local);
    if chat.filter.is_none_or(|f| !offered.contains(&f)) {
        chat.filter = Some(first(sim, local));
    }
}

/// Sends what's typed, if anything, and closes the bar. A whisper to one
/// no longer there to hear it is warned of instead.
fn send(
    chat: &mut Chat,
    orders: &mut Orders,
    sim: &World,
    local: Option<u32>,
    leaders: &Leaders,
    log: &mut MessageLog,
    time: &Time,
) {
    chat.open = false;
    chat.recall = None;
    let words = std::mem::take(&mut chat.input);
    let Some(filter) = chat.filter.filter(|_| !words.trim().is_empty()) else {
        return;
    };
    if chat.sent.last() != Some(&words) {
        chat.sent.push(words.clone());
    }
    let now = time.elapsed_secs();
    if let Filter::To(side) = filter {
        let Some(player) = sim.sides.player(side) else {
            log.warn(
                "private_chat_warning_unrecognized_chat_target",
                "The person you were attempting to chat with was not found",
                &[],
                now,
            );
            return;
        };
        let name = leaders.owner(player, &sim.sides, &log.map_strings());
        if !people(orders, local).contains(&side) {
            log.warn(
                "private_chat_warning_bad_kingdom|s",
                "%s is not a valid chat target",
                &[name],
                now,
            );
            return;
        }
        log.whispering(name, words.clone(), now);
    }
    orders.act(Action::Chat {
        from: local,
        to: filter.channel(),
        words,
    });
}

/// Closes the bar, keeping what's typed, once the connection dialog
/// opens, whose own chat then has the keyboard.
fn give_way(mut chat: ResMut<Chat>, dialog: Option<Res<crate::connection::Dialog>>) {
    if chat.open && dialog.is_some_and(|d| !crate::connection::closed(d)) {
        chat.open = false;
        chat.recall = None;
    }
}

/// Keeps the camera off the keyboard while the bar or the connection
/// dialog's chat has it; the mouse still moves it.
fn hold_camera_keys(
    chat: Option<Res<Chat>>,
    dialog: Option<Res<crate::connection::Dialog>>,
    mut cameras: Query<&mut RtsCamera>,
) {
    let keys = crate::keys_free(chat, dialog);
    for mut camera in &mut cameras {
        if camera.keys != keys {
            camera.keys = keys;
        }
    }
}

/// Puts the bar up as it opens and takes it down as it closes.
fn draw(
    mut commands: Commands,
    chat: Res<Chat>,
    words: Res<Words>,
    bars: Query<Entity, With<Bar>>,
) {
    if chat.open != bars.is_empty() {
        return;
    }
    bars.iter().for_each(|e| commands.entity(e).despawn());
    if !chat.open {
        return;
    }
    let bar = commands
        .spawn((
            Bar,
            DespawnOnExit(Screen::Match),
            ui::panel(
                ui::Surface::Panel,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(AT.x),
                    top: px(AT.y),
                    width: px(WIDTH),
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    padding: UiRect::axes(px(6), px(3)),
                    ..default()
                },
            ),
            GlobalZIndex(ui::layer::DIALOG),
        ))
        .id();
    commands
        .spawn((
            ChildOf(bar),
            ui::button("", Font::Body),
            ui::called("chat.filter"),
            Shown,
        ))
        .observe(next_filter);
    commands.spawn((
        ChildOf(bar),
        ui::text("", Font::Body),
        Typed,
        Node {
            flex_grow: 1.0,
            overflow: Overflow::clip(),
            ..default()
        },
    ));
    commands
        .spawn((
            ChildOf(bar),
            ui::button(words.send.clone(), Font::Body),
            ui::called("chat.send"),
        ))
        .observe(send_pressed);
}

/// Keeps what the bar shows as it stands: what's typed, and who it's for.
#[allow(clippy::too_many_arguments)]
fn fill_in(
    chat: Res<Chat>,
    words: Res<Words>,
    sim: Res<Sim>,
    leaders: Res<Leaders>,
    log: Res<MessageLog>,
    mut typed: Query<&mut Text, With<Typed>>,
    shown: Query<&Children, With<Shown>>,
    mut texts: Query<&mut Text, Without<Typed>>,
) {
    if !chat.open {
        return;
    }
    let input = format!("{}_", chat.input);
    for mut text in &mut typed {
        if text.0 != input {
            text.0 = input.clone();
        }
    }
    let said = match chat.filter {
        Some(Filter::All) | None => words.all.clone(),
        Some(Filter::Allies) => words.allies.clone(),
        Some(Filter::Observers) => words.observers.clone(),
        Some(Filter::To(side)) => match sim.0.sides.player(side) {
            Some(player) => leaders.owner(player, &sim.0.sides, &log.map_strings()),
            None => String::new(),
        },
    };
    for children in &shown {
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != said
            {
                text.0 = said.clone();
            }
        }
    }
}

fn next_filter(_: On<Activate>, mut chat: ResMut<Chat>, orders: Res<Orders>, local: Res<Local>) {
    chat.filter = stepped(&offered(&orders, local.0), chat.filter, 1);
}

#[allow(clippy::too_many_arguments)]
fn send_pressed(
    _: On<Activate>,
    mut chat: ResMut<Chat>,
    mut orders: ResMut<Orders>,
    sim: Res<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    mut log: ResMut<MessageLog>,
    time: Res<Time>,
) {
    send(
        &mut chat,
        &mut orders,
        &sim.0,
        local.0,
        &leaders,
        &mut log,
        &time,
    );
}

#[cfg(test)]
mod tests {
    use bevy::input::InputPlugin;
    use bevy::state::app::StatesPlugin;

    use super::*;

    /// A match hosted here of sides 1 to 3, the player as 1, with the chat
    /// bar's keys bound as the game's files bind them.
    fn app() -> App {
        let world = crate::orders::testing::fresh();
        let orders = Orders::hosting(Default::default(), world.clone(), Some(1), "Player");
        let hotkeys = [
            ("vworld", "ChatActivate"),
            ("GameChatPanel|editbox", "ChatSend"),
        ]
        .map(|(target, command)| tgdata::Hotkey {
            chord: "enter".into(),
            target: target.into(),
            command: Some(command.into()),
        });
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, InputPlugin, ChatPlugin))
            .insert_state(Screen::Match)
            .insert_resource(Words::default())
            .insert_resource(orders)
            .insert_resource(Sim(world))
            .insert_resource(Local(Some(1)))
            .insert_resource(Bindings::new(&hotkeys, &Default::default()))
            .insert_resource(crate::messages::testing::log())
            .init_resource::<Leaders>()
            .init_resource::<crate::displays::Displays>()
            .add_message::<crate::intent::Intent>()
            .add_systems(
                Update,
                (crate::keys::send.run_if(crate::keys_free), keys).chain(),
            );
        app.update();
        app
    }

    /// Presses and lets go of `chord`, a frame each.
    fn press(app: &mut App, chord: &str) {
        let keys = crate::drive::press(app.world_mut(), chord).unwrap();
        app.update();
        crate::drive::release(app.world_mut(), &keys);
        app.update();
    }

    fn asked(app: &mut App) -> Vec<Action> {
        let world = app.world_mut();
        let mut sim = world.remove_resource::<Sim>().unwrap();
        world.resource_mut::<Orders>().carry_out(&mut sim.0);
        world.insert_resource(sim);
        crate::orders::testing::asked(world.resource::<Orders>().record())
    }

    #[test]
    fn enter_opens_the_bar_and_sends_what_is_typed() {
        let mut app = app();
        press(&mut app, "enter");
        assert!(app.world().resource::<Chat>().is_open());
        for key in ["h", "i", "space", "x", "backspace"] {
            press(&mut app, key);
        }
        // The match keeps nothing from what's typed.
        assert_eq!(app.world().resource::<Chat>().input, "hi ");
        press(&mut app, "enter");
        assert!(!app.world().resource::<Chat>().is_open());
        // With no allies, to everyone.
        let sent = Action::Chat {
            from: Some(1),
            to: Channel::All,
            words: "hi ".into(),
        };
        assert_eq!(asked(&mut app), [sent]);
    }

    #[test]
    fn the_camera_leaves_the_keys_to_an_open_bar() {
        let mut app = app();
        let camera = app.world_mut().spawn(RtsCamera::new(Vec3::ZERO)).id();
        let keys = |app: &App| app.world().get::<RtsCamera>(camera).unwrap().keys;
        press(&mut app, "enter");
        assert!(!keys(&app));
        press(&mut app, "a");
        press(&mut app, "enter");
        assert!(keys(&app));
    }

    #[test]
    fn the_bar_gives_way_to_the_connection_dialog_keeping_what_is_typed() {
        let mut app = app();
        app.init_resource::<crate::connection::Dialog>();
        press(&mut app, "enter");
        press(&mut app, "a");
        crate::connection::open(&mut app.world_mut().resource_mut());
        app.update();
        let chat = app.world().resource::<Chat>();
        assert!(!chat.is_open());
        assert_eq!(chat.input, "a");
    }

    #[test]
    fn nothing_typed_sends_nothing() {
        let mut app = app();
        press(&mut app, "enter");
        press(&mut app, "enter");
        assert!(!app.world().resource::<Chat>().is_open());
        assert_eq!(asked(&mut app), []);
    }

    #[test]
    fn a_whisper_to_a_side_nobody_plays_is_warned_of() {
        let mut app = app();
        press(&mut app, "enter");
        app.world_mut().resource_mut::<Chat>().filter = Some(Filter::To(2));
        press(&mut app, "a");
        press(&mut app, "enter");
        assert_eq!(asked(&mut app), []);
        let log = app.world().resource::<MessageLog>();
        assert_eq!(
            crate::messages::testing::texts(log),
            ["kingdom_2 is not a valid chat target"]
        );
    }

    #[test]
    fn the_filter_steps_round_what_is_on_offer() {
        let offered = [Filter::All, Filter::Allies, Filter::To(2)];
        assert_eq!(stepped(&offered, Some(Filter::To(2)), 1), Some(Filter::All));
        assert_eq!(
            stepped(&offered, Some(Filter::All), -1),
            Some(Filter::To(2))
        );
        assert_eq!(stepped(&offered, None, 1), Some(Filter::All));
    }
}
