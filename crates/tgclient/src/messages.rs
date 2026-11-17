//! The message log at the top left of the screen: a line, behind its kind's
//! icon, for each thing the player is told, fading a while after it's
//! posted. Clicking a line's icon, or the key for it, looks at where it
//! happened. `spec/messages.md` is what it's built to.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bevy::ecs::system::SystemParam;
use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use tgdata::{GameEvent, Id, InterfaceVars};
use tgrender::camera::RtsCamera;
use tgrender::terrain::world;
use tgrender::textures;
use tgserver::protocol::Said;
use tgsim::{Channel, Notice, Pause, Pausing, Piece, Relation, Seen, Votes, World};

use crate::actions::{self, Aim, fill};
use crate::displays::{self, Contents, Displays, Panel, Rim, Window};
use crate::keys::Bindings;
use crate::sound::Sounds;
use crate::ui::{Font, Hint, font};
use crate::{Leaders, Loaded, Local, Orders, Screen, Sim};

/// The key that steps the camera back through the lines.
pub const CYCLE: &str = "CycleMessageLog";
/// The log's width, before it wraps a line.
const WIDTH: f32 = 564.0;
const ICON: Vec2 = Vec2::new(16.0, 17.0);
/// Between the icon and its text.
const ICON_GAP: f32 = 8.0;
const LINE_GAP: f32 = 3.0;
/// What keeps the text legible over pale ground, as there's no box behind
/// it.
const SHADOW: TextShadow = TextShadow {
    offset: Vec2::splat(1.0),
    color: Color::srgba(0.0, 0.0, 0.0, 0.9),
};
/// How long a line takes to fade once it's been up its while.
const FADE_SECONDS: f32 = 1.0;

/// Stock settings, for data that lacks them.
const SHOWN: usize = 5;
const KEPT: usize = 50;
const UP_MS: f32 = 10000.0;
const CYCLE_TIMEOUT: f32 = 1.0;

/// A kind of message, as its lines look.
struct Kind {
    icon: Option<Handle<Image>>,
    colour: Color,
    filter: Option<String>,
    sound: Option<String>,
}

/// What lines are made from, read as the match starts.
struct Look {
    kinds: HashMap<Id, Kind>,
    events: HashMap<Id, GameEvent>,
    vars: InterfaceVars,
    strings: Arc<tgdata::Strings>,
    names: HashMap<Id, String>,
    /// The map's own text, for the names it gives things and sides.
    map: Arc<tgdata::Strings>,
    /// What an icon that looks somewhere says.
    to_location: String,
    /// What marks a side named in a line.
    gem: crate::ui::Gem,
}

impl Look {
    fn name(&self, template: &str) -> String {
        (self.names.get(&Id::from(template)).cloned()).unwrap_or_else(|| template.to_string())
    }

    fn organization(&self, sim: &World, r: usize) -> String {
        crate::object_display::organization_name(sim, &self.names, &self.strings, &self.map, r)
    }

    fn building(&self, sim: &World, b: usize) -> String {
        crate::object_display::building_name(sim, &self.names, &self.strings, &self.map, b)
    }

    /// An HQ's name: its building's, numbered as its organization is,
    /// packed or not.
    fn hq(&self, sim: &World, hq: &tgsim::Hq) -> String {
        let standing = (sim.buildings.iter())
            .position(|b| !b.gone() && b.organization.as_ref().is_some_and(|o| o.id == hq.id));
        if let Some(b) = standing {
            return self.building(sim, b);
        }
        let number = sim
            .organizations
            .iter()
            .find(|r| r.id == hq.id)
            .and_then(|r| r.number);
        crate::object_display::numbered(&self.strings, number, self.name(hq.building.as_str()))
    }

    /// The text under `key`, or `or` where the strings lack it.
    fn said(&self, key: &str, or: &str) -> String {
        self.strings.get(key).map_or(or, String::as_str).to_string()
    }

    /// Whether lines of `kind` are posted, as its filter has it.
    fn shows(&self, kind: &Id) -> bool {
        let filter = self.kinds.get(kind).and_then(|k| k.filter.as_deref());
        filter.is_none_or(|f| self.vars.flag(f, true))
    }
}

/// What a line is about, for looking at it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum About {
    Organization(usize),
    Building(usize),
}

/// Where a line happened: at what it's about, while that's there, else
/// where it was when the line was posted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub about: Option<About>,
    pub at: [f32; 2],
}

impl Place {
    fn of(sim: &World, about: About) -> Place {
        let at = match about {
            About::Organization(r) => sim.leader_position(r),
            About::Building(b) => sim.buildings[b].position,
        };
        Place {
            about: Some(about),
            at,
        }
    }

    /// Where it is now, as `player` sees it.
    fn now(&self, sim: &World, player: Option<u32>) -> [f32; 2] {
        let now = match self.about {
            Some(About::Organization(r)) if r < sim.organizations.len() && !sim.destroyed(r) => {
                let sees = |p| sim.survivors(r).any(|u| sim.sees_unit(p, u));
                player.is_none_or(sees).then(|| sim.leader_position(r))
            }
            Some(About::Building(b)) if b < sim.buildings.len() && !sim.buildings[b].gone() => {
                let sees = |p| sim.sees_building(p, b);
                player.is_none_or(sees).then_some(sim.buildings[b].position)
            }
            _ => None,
        };
        now.unwrap_or(self.at)
    }
}

/// A line the player was told.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Counts up from the match's first line.
    pub serial: u64,
    pub kind: Id,
    pub text: String,
    /// Where it names sides.
    pub marks: Vec<Mark>,
    /// Where its words take a colour of their own.
    pub inks: Vec<Ink>,
    pub place: Option<Place>,
    /// When it was posted, in seconds of the app's time.
    pub posted: f32,
}

/// Where a line names a side: the byte of its text the name starts at,
/// and the side's colour, which the side's gem before the name is drawn
/// in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: usize,
    pub rgb: [f32; 3],
}

/// Where a line's words change colour: from the byte of its text at
/// `at`, to `rgb`, or back to the colour of its kind if none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub at: usize,
    pub rgb: Option<[f32; 3]>,
}

/// A line's text, where it names sides, and where its words take a
/// colour of their own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Words {
    pub text: String,
    pub marks: Vec<Mark>,
    pub inks: Vec<Ink>,
}

impl From<String> for Words {
    fn from(text: String) -> Words {
        Words {
            text,
            ..Default::default()
        }
    }
}

/// What fills one of a line's placeholders: words, or a side's name.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    words: String,
    /// The colour of the side the words name.
    side: Option<[f32; 3]>,
    /// The colour the words are written in, if not the line's.
    ink: Option<[f32; 3]>,
}

impl Fill {
    /// Words written in the colour `rgb`.
    fn inked(words: String, rgb: [f32; 3]) -> Fill {
        Fill {
            ink: Some(rgb),
            ..Fill::from(words)
        }
    }

    /// The name of a side of colour `rgb`.
    fn side(name: String, rgb: [f32; 3]) -> Fill {
        Fill {
            side: Some(rgb),
            ..Fill::from(name)
        }
    }

    /// The name of `player`'s side, marked unless it's independent, as
    /// the selection panel marks an owner.
    fn side_of(name: String, player: &tgmap::Player) -> Fill {
        match player.independent {
            true => Fill::from(name),
            false => Fill::side(name, player.rgb),
        }
    }
}

impl From<String> for Fill {
    fn from(words: String) -> Fill {
        Fill {
            words,
            side: None,
            ink: None,
        }
    }
}

impl From<&str> for Fill {
    fn from(words: &str) -> Fill {
        Fill::from(words.to_string())
    }
}

/// Every line the player has been told, oldest first, as many as the
/// settings keep.
#[derive(Resource)]
pub struct MessageLog {
    look: Look,
    lines: VecDeque<Line>,
    next: u64,
    /// The line the key last looked at, and when.
    cycled: Option<(u64, f32)>,
    /// How many lines show at once, how long each stays up, how many are
    /// kept, and how soon the key has to be pressed again to step on.
    shown: usize,
    up: f32,
    kept: usize,
    cycle_timeout: f32,
}

impl MessageLog {
    fn new(look: Look) -> MessageLog {
        let vars = &look.vars;
        MessageLog {
            shown: vars.get("UIMessageLogMaximumLines").unwrap_or(SHOWN),
            up: vars.get::<f32>("UIMessageLogFadeTime").unwrap_or(UP_MS) / 1000.0,
            kept: vars.get("MessageLogMaximumLines").unwrap_or(KEPT),
            cycle_timeout: vars
                .get("UIMessageLogCycleTimeout")
                .unwrap_or(CYCLE_TIMEOUT),
            look,
            lines: VecDeque::new(),
            next: 0,
            cycled: None,
        }
    }

    /// Posts a line of `kind`, unless its filter hides that kind; whether
    /// it did.
    pub fn post(
        &mut self,
        kind: &str,
        words: impl Into<Words>,
        place: Option<Place>,
        now: f32,
    ) -> bool {
        let kind = Id::from(kind);
        if !self.look.shows(&kind) {
            return false;
        }
        let Words { text, marks, inks } = words.into();
        info!("{text}");
        self.lines.push_back(Line {
            serial: self.next,
            kind,
            text,
            marks,
            inks,
            place,
            posted: now,
        });
        self.next += 1;
        while self.lines.len() > self.kept.max(1) {
            self.lines.pop_front();
        }
        true
    }

    /// The map's own text, for the names it gives sides.
    pub(crate) fn map_strings(&self) -> std::sync::Arc<tgdata::Strings> {
        self.look.map.clone()
    }

    /// Posts the warning line the string `key` says, or `stock` where the
    /// strings don't, its placeholders filled with `with` in turn.
    pub(crate) fn warn(&mut self, key: &str, stock: &str, with: &[String], now: f32) {
        let said = self.look.said(key, stock);
        self.post("GENERIC_WARNING", fill_all(&said, with), None, now);
    }

    /// Posts what the player here whispers to the person `to`, as they
    /// send it.
    pub(crate) fn whispering(&mut self, to: String, words: String, now: f32) {
        let said = self
            .look
            .said("ui_lobby_whispering_to|ss", "Whispering to %s: %s");
        let name = self.look.vars.rgb("UIChatPlayerColorNormal");
        let with = [Fill::inked(to, name.unwrap_or([1.0; 3])), words.into()];
        self.post("CHAT_PRIVATE", fill_all(&said, &with), None, now);
    }

    /// Posts a line of `kind`, heard as its kind sounds.
    fn say(
        &mut self,
        kind: &str,
        words: impl Into<Words>,
        place: Option<Place>,
        time: &Time,
        to: &mut Sounds,
    ) {
        if self.post(kind, words, place, time.elapsed_secs()) {
            self.sound(time, to);
        }
    }

    /// Plays the sound of the kind of line just posted.
    fn sound(&self, time: &Time, to: &mut Sounds) {
        let kind = self.lines.back().and_then(|l| self.look.kinds.get(&l.kind));
        if let Some(sound) = kind.and_then(|k| k.sound.as_deref()) {
            to.interface(time, sound);
        }
    }

    /// Posts the line of the game event `event`, its placeholders filled
    /// with `with` in turn, unless the event's filters or its kind's drop
    /// it.
    pub fn event(
        &mut self,
        event: &str,
        with: &[impl Into<Fill> + Clone],
        place: Option<Place>,
        now: f32,
    ) -> bool {
        let Some(e) = self.look.events.get(&Id::from(event)) else {
            return false;
        };
        // Event filters the files don't set are off.
        let off = |f: &Option<String>| f.as_deref().is_some_and(|f| !self.look.vars.flag(f, false));
        if off(&e.filter) || off(&e.message_filter) {
            return false;
        }
        let (Some(text), Some(kind)) = (e.message.clone(), e.kind.clone()) else {
            return false;
        };
        self.post(kind.as_str(), fill_all(&text, with), place, now)
    }

    /// The lines on screen at `now`, oldest first, with how opaque each is:
    /// the newest still up, as many as show at once.
    fn on_screen(&self, now: f32) -> Vec<(&Line, f32)> {
        let mut out: Vec<(&Line, f32)> = (self.lines.iter().rev())
            .map(|l| (l, self.opacity(l, now)))
            .take_while(|&(_, a)| a > 0.0)
            .take(self.shown.max(1))
            .collect();
        out.reverse();
        out
    }

    fn opacity(&self, line: &Line, now: f32) -> f32 {
        let age = now - line.posted;
        (1.0 - (age - self.up) / FADE_SECONDS).clamp(0.0, 1.0)
    }

    /// The line the key looks at next: the newest with a place, or, pressed
    /// again soon enough, the next older one, round to the newest again.
    fn cycle(&mut self, now: f32) -> Option<&Line> {
        let placed: Vec<&Line> = self
            .lines
            .iter()
            .rev()
            .filter(|l| l.place.is_some())
            .collect();
        let after = (self.cycled)
            .filter(|&(_, at)| now - at <= self.cycle_timeout)
            .and_then(|(serial, _)| placed.iter().position(|l| l.serial == serial));
        let next = match after {
            Some(i) => placed.get(i + 1).or(placed.first()),
            None => placed.first(),
        };
        let serial = next?.serial;
        self.cycled = Some((serial, now));
        self.lines.iter().find(|l| l.serial == serial)
    }
}

/// [`fill_all`]'s words alone, for text shown outside the message log.
pub(crate) fn filled(text: &str, with: &[impl Into<Fill> + Clone]) -> String {
    fill_all(text, with).text
}

/// Fills the `%s`, `%d`, `%i` and `%f` placeholders of a line in turn,
/// the last also as `%.2f` and the like, though what fills it is written
/// as its caller has it; reads `%%` as `%`, and takes `<color=r,g,b>` as
/// the colour of what follows.
fn fill_all(text: &str, with: &[impl Into<Fill> + Clone]) -> Words {
    let mut out = String::new();
    let mut marks = Vec::new();
    let mut inks = Vec::new();
    let mut with = with.iter().cloned().map(Into::into);
    let mut rest = text;
    while let Some(i) = rest.find(['%', '<']) {
        out.push_str(&rest[..i]);
        if let Some((rgb, after)) = colour_markup(&rest[i..]) {
            inks.push(Ink {
                at: out.len(),
                rgb: Some(rgb),
            });
            rest = after;
            continue;
        }
        if rest[i..].starts_with('<') {
            out.push('<');
            rest = &rest[i + 1..];
            continue;
        }
        let after = &rest[i + 1..];
        // A placeholder for a number with its places given, e.g. `%.0f`.
        let places = after
            .strip_prefix('.')
            .map(|p| p.trim_start_matches(|c: char| c.is_ascii_digit()))
            .filter(|p| p.starts_with('f'));
        let after = places.unwrap_or(after);
        match after.chars().next() {
            Some('%') => {
                out.push('%');
                rest = &after[1..];
            }
            Some('s' | 'd' | 'i' | 'f') => {
                if let Some(Fill { words, side, ink }) = with.next() {
                    if let Some(rgb) = side {
                        marks.push(Mark { at: out.len(), rgb });
                    }
                    if ink.is_some() {
                        inks.push(Ink {
                            at: out.len(),
                            rgb: ink,
                        });
                    }
                    out.push_str(&words);
                    if ink.is_some() {
                        let at = out.len();
                        inks.push(Ink { at, rgb: None });
                    }
                }
                rest = &after[1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Words {
        text: out,
        marks,
        inks,
    }
}

/// The log's box.
#[derive(Component)]
pub struct LogBox;

/// A line on screen, by its serial.
#[derive(Component)]
pub struct Row(u64);

/// A row's icon and text, faded with it.
#[derive(Component)]
pub struct Faded(Color);

/// A row's parts, to fade.
type Parts<'a> = (
    &'a Faded,
    Option<&'a mut TextColor>,
    Option<&'a mut TextShadow>,
    Option<&'a mut ImageNode>,
);

/// The icon of the line with this serial, which looks where it happened.
#[derive(Component)]
struct LookAt(u64);

impl Look {
    /// What `game` has lines look like, short of what the map has things
    /// called, which [`Look::name_things`] gives once the match is set up.
    fn of(
        game: &tgdata::GameData,
        images: &mut Assets<Image>,
        formats: CompressedImageFormats,
        gem: &crate::ui::Gem,
    ) -> Look {
        let kinds = game
            .message_kinds()
            .into_iter()
            .map(|k| {
                let icon = k.icon.as_deref().and_then(|path| {
                    let image = textures::decode(&game.read(path)?, path, formats, true)?;
                    Some(images.add(image))
                });
                let [r, g, b] = k.colour;
                let kind = Kind {
                    icon,
                    colour: Color::srgb(r, g, b),
                    filter: k.filter,
                    sound: k.sound,
                };
                (k.id, kind)
            })
            .collect();
        let strings = game.strings();
        Look {
            kinds,
            events: game.game_events(),
            vars: game.interface_vars(),
            to_location: (strings.get("ui_tooltip_message_location").cloned())
                .unwrap_or_else(|| "Move camera to message location".into()),
            gem: gem.clone(),
            strings,
            map: Default::default(),
            names: HashMap::new(),
        }
    }

    /// Has it call things by what `loaded`'s data and map call them.
    fn name_things(&mut self, loaded: &Loaded) {
        let game = &loaded.game;
        self.map = loaded.map.strings.clone();
        self.names = (game.listings().iter())
            .map(|(id, l)| (id.clone(), l.name.clone()))
            .chain((game.technologies().iter()).map(|(id, t)| (id.clone(), t.name.clone())))
            .collect();
    }
}

/// Starts the log as a match begins loading, so that who goes and what's
/// said while it loads is told then and kept.
pub fn start(
    mut commands: Commands,
    data: Res<crate::Data>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    gem: Res<crate::ui::Gem>,
) {
    let formats = formats.map(|f| f.0).unwrap_or_default();
    let look = Look::of(&data.0, &mut images, formats, &gem);
    commands.insert_resource(MessageLog::new(look));
}

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut log: ResMut<MessageLog>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    bindings: Res<Bindings>,
    (mut icons, mut images, formats): (
        ResMut<displays::Icons>,
        ResMut<Assets<Image>>,
        Res<CompressedImageFormatSupport>,
    ),
) {
    let corner = crate::special_ops::corner(&local, &leaders);
    log.look.name_things(&loaded);
    let said = crate::ui::said(&log.look.strings);
    displays::spawn_button(
        &mut commands,
        &bindings,
        Panel::MessageLog,
        Rim::BottomRight,
        icons.get(displays::icon!("message-log"), &mut images, formats.0),
        said(
            "control_panel_ButtonMessageLog_ToolTip",
            "Display Message Log",
        ),
        said(
            "control_panel_ButtonMessageLog_tooltip_2",
            "Displays a log of all recent messages.",
        ),
    );
    displays::spawn_window(
        &mut commands,
        Window {
            panel: Panel::MessageLog,
            title: said("game_interface_Child_text", "Message Log"),
            at: Vec2::new(279.0, 53.0),
            size: Vec2::new(542.0, 514.0),
            pane: true,
        },
    );
    commands.spawn((
        LogBox,
        Node {
            position_type: PositionType::Absolute,
            top: px(corner.y),
            left: px(corner.x),
            width: px(WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(LINE_GAP),
            ..default()
        },
        Pickable::IGNORE,
        GlobalZIndex(1),
        DespawnOnExit(Screen::Match),
    ));
}

/// A click that aimed an order at something it can't be given on: `by`
/// the order that refuses such a thing, or none where none of the selected
/// can attack it.
#[derive(Message)]
pub struct Refused {
    pub about: About,
    pub by: Option<Aim>,
}

/// An order the sim refused, as the player's told.
#[derive(Message)]
pub struct Failed {
    pub text: String,
    /// What it was given to, where the message points, if anything.
    pub about: Option<About>,
}

/// Tells the player of each order just [`Refused`] or [`Failed`].
pub fn refuse(
    mut refused: MessageReader<Refused>,
    mut failed: MessageReader<Failed>,
    sim: Res<Sim>,
    looks: Res<actions::Looks>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    for Refused { about, by } in refused.read() {
        let name = match *about {
            About::Organization(r) => log.look.organization(&sim.0, r),
            About::Building(b) => log.look.building(&sim.0, b),
        };
        let text = match by {
            Some(aim) => {
                let said = log.look.said(
                    "ui_error_command_target_actor|ss",
                    "%s is an invalid target for %s",
                );
                fill(&said, &[&name, looks.order(*aim)])
            }
            None => {
                let said = log
                    .look
                    .said("kill_error_invalid_target|s", "Invalid target: %s");
                fill(&said, &[&name])
            }
        };
        let place = Some(Place::of(&sim.0, *about));
        log.say("GENERIC_ERROR", text, place, &time, &mut sounds);
    }
    for Failed { text, about } in failed.read() {
        let place = about.map(|a| Place::of(&sim.0, a));
        log.say("GENERIC_ERROR", text.clone(), place, &time, &mut sounds);
    }
}

/// Posts a line once the host is leaving a match that's over, then one
/// for each machine that has left the match since last time, whether by
/// choice, by being lost or by being kicked out, the host's among them,
/// and one for each kick vote cast or taken back. While the match is
/// still being set up, nobody's side is marked.
pub fn tell_leaving(
    mut orders: ResMut<Orders>,
    sim: Option<Res<Sim>>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    if orders.take_ended() {
        let said = log.look.said(
            "ui_mission_message_host_has_ended_game",
            "The host has ended the game.",
        );
        log.say("GENERIC_EVENT", said, None, &time, &mut sounds);
    }
    let sim = sim.as_deref().map(|s| &s.0);
    let person = |name: &str, players: &[u32]| person(sim, name, players);
    let mut lines = Vec::new();
    // The votes came before any kick they brought.
    for kick in orders.take_kicking() {
        let named = |client| {
            orders
                .seen(client)
                .map(|(players, name)| person(name, players))
        };
        let (Some(voter), Some(target)) = (named(kick.voter), named(kick.target)) else {
            continue;
        };
        let said = match kick.vote {
            true => log.look.said(
                "networking_vote_to_kick_player_voted_to_kick_player|ssdd",
                "%s has voted to kick %s (%d/%d)",
            ),
            false => log.look.said(
                "networking_vote_to_kick_player_has_retracted_vote_to_kick_player|ssdd",
                "%s has retracted vote to kick %s (%d/%d)",
            ),
        };
        let counts = [kick.votes, kick.needed].map(|n| Fill::from(n.to_string()));
        lines.push(fill_all(&said, &[[voter, target], counts].concat()));
    }
    for gone in orders.take_left() {
        let (client, players) = (gone.client, &gone.players);
        info!("client {client} has left the match, giving the orders of {players:?}");
        let said = match gone.kicked {
            true => log.look.said(
                "networking_player_kicked|s",
                "Player %s was kicked from the game",
            ),
            false => log
                .look
                .said("networking_player_quit|s", "Player %s quit the game"),
        };
        lines.push(fill_all(&said, &[person(&gone.name, players)]));
    }
    for words in lines {
        log.say("GENERIC_EVENT", words, None, &time, &mut sounds);
    }
    // What's said in the connection dialog makes no sound.
    for said in orders.take_said() {
        let words = said_line(&log.look, said);
        log.post("GENERIC_EVENT", words, None, time.elapsed_secs());
    }
}

/// A person as lines name them: by their own name, marked as the first of
/// `players`, the sides they play, is; unmarked if they only watch.
fn person(sim: Option<&World>, name: &str, players: &[u32]) -> Fill {
    let side = players.first().and_then(|&p| sim?.sides.player(p));
    match side {
        Some(player) => Fill::side_of(name.to_string(), player),
        None => Fill::from(name.to_string()),
    }
}

/// The line for what's said in the connection dialog: plain, in none of
/// the chat bar's colours.
fn said_line(look: &Look, said: Said) -> Words {
    fill_all(
        &look.said("public_chat|ss", "%s: %s"),
        &[said.name, said.words],
    )
}

/// The line for what came of someone asking to pause or unpause, with
/// each person named by `side`, as their side's name.
fn paused_line(look: &Look, pausing: Pausing, side: impl Fn(u32) -> Fill) -> Words {
    let count = |n: u32| Fill::from(n.to_string());
    let (key, or, fills) = match pausing {
        Pausing::Paused { by } => (
            "networking_vote_to_pause_player_game_paused|s",
            "%s has paused the game",
            vec![side(by)],
        ),
        // The count needed comes first here.
        Pausing::Needed(Votes { votes, needed }) => (
            "networking_vote_to_pause_needed_votes|dd",
            "Votes needed to unpause %d, current votes %d",
            vec![count(needed), count(votes)],
        ),
        Pausing::Unpaused { by } => (
            "networking_vote_to_pause_player_game_unpaused|s",
            "%s has unpaused the game",
            vec![side(by)],
        ),
        Pausing::Voted { by, cast, votes } => {
            let (key, or) = match cast {
                true => (
                    "networking_vote_to_pause_player_votes_to_unpause|sdd",
                    "%s votes to unpause the game (%d/%d)",
                ),
                false => (
                    "networking_vote_to_pause_player_retracts_vote|sdd",
                    "%s has retracted vote to unpause (%d/%d)",
                ),
            };
            (
                key,
                or,
                vec![side(by), count(votes.votes), count(votes.needed)],
            )
        }
        Pausing::VotedOff => (
            "networking_vote_to_pause_game_unpaused",
            "Game unpaused",
            vec![],
        ),
        Pausing::Refused { by } => (
            "networking_vote_to_pause_pause_rejected|s",
            "%s attempted to pause but failed due to recent votes",
            vec![side(by)],
        ),
    };
    fill_all(&look.said(key, or), &fills)
}

/// What one who only watches is called.
fn observer(look: &Look) -> String {
    look.said("game_interface_KingdomDropDown_observer_text", "Observer")
}

/// Posts a line for each thing the sim has told the player since last
/// time.
pub fn tell(
    mut sim: ResMut<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    // Taking them marks the sim changed, which what redraws on a change
    // would then do every frame.
    if sim.0.notices().is_empty() {
        return;
    }
    let notices = sim.0.take_notices();
    let now = time.elapsed_secs();
    for notice in notices {
        if let Some(sound) = sound(&notice, local.0) {
            sounds.interface(&time, sound);
        }
        let posted = match told(&sim.0, local.0, &leaders, &log.look, notice) {
            Some(Told::Event(event, with, place)) => log.event(event, &with, place, now),
            Some(Told::Line(kind, text, place)) => log.post(kind, text, place, now),
            Some(Told::Words(kind, words)) => log.post(kind, words, None, now),
            // Chat sounds whether or not its line shows, and as itself
            // rather than as its line's kind.
            Some(Told::Chat(kind, words)) => {
                log.post(kind, words, None, now);
                if log.look.vars.flag("UIEventFilterChatSound", true) {
                    sounds.interface(&time, "chat_sfx");
                }
                false
            }
            None => false,
        };
        if posted {
            log.sound(&time, &mut sounds);
        }
    }
}

/// The sound `local`, the player here, hears for a notice: none for one
/// told to anyone else.
fn sound(notice: &Notice, local: Option<u32>) -> Option<&'static str> {
    match *notice {
        Notice::Lost { player, .. } if Some(player) == local => Some("actor_captured_by_enemy_sfx"),
        Notice::Captured { player, .. } if Some(player) == local => Some("actor_captured_sfx"),
        _ => None,
    }
}

/// What a notice tells the player.
enum Told {
    /// Chat, as a line of a kind of message.
    Chat(&'static str, Words),
    /// A game event, with what fills its line.
    Event(&'static str, Vec<Fill>, Option<Place>),
    /// A line of a kind of message.
    Line(&'static str, String, Option<Place>),
    /// A line, with what fills it, of a kind of message.
    Words(&'static str, Words),
}

/// The line chat from the person playing `from`, or watching for none,
/// is posted as for `local`, the side here, with its kind, if it reaches
/// them.
fn chat(
    sim: &World,
    local: Option<u32>,
    leaders: &Leaders,
    look: &Look,
    from: Option<u32>,
    to: Channel,
    words: String,
) -> Option<(&'static str, Words)> {
    let allied = |a: u32, b: u32| a == b || sim.sides.relation(a, b) == Relation::Ally;
    let hears = match to {
        Channel::All => true,
        // Those watching talk among themselves.
        Channel::Team => match (local, from) {
            (Some(local), Some(from)) => allied(local, from),
            (None, None) => true,
            _ => false,
        },
        // Whoever whispers was shown it as they sent it.
        Channel::To(side) => from.is_some_and(|from| local == Some(side) && from != side),
    };
    if !hears {
        return None;
    }
    let (key, channel_ink) = match to {
        Channel::All => ("public_chat|ss", ("ChatPublicColor", [1.0, 1.0, 0.0])),
        Channel::Team => ("team_chat|ss", ("ChatTeamColor", [0.0, 1.0, 0.0])),
        Channel::To(_) => ("private_chat|ss", ("ChatWhisperColor", [0.0, 0.9, 0.9])),
    };
    let ink = |(var, stock): (&str, [f32; 3])| look.vars.rgb(var).unwrap_or(stock);
    let name = match from.and_then(|f| sim.sides.player(f)) {
        Some(player) => leaders.owner(player, &sim.sides, &look.map),
        None => observer(look),
    };
    let with = [
        Fill::inked(name, ink(("UIChatPlayerColorNormal", [1.0; 3]))),
        Fill::inked(words, ink(channel_ink)),
    ];
    let friendly = from
        .zip(local)
        .is_some_and(|(from, local)| allied(from, local));
    let kind = match (friendly, to) {
        (true, _) => "GENERIC_EVENT",
        (false, Channel::To(_)) => "CHAT_PRIVATE",
        (false, _) => "CHAT_PUBLIC",
    };
    Some((kind, fill_all(&look.said(key, "%s: %s"), &with)))
}

/// What `notice` tells `local`, if it's for them.
fn told(
    sim: &World,
    local: Option<u32>,
    leaders: &Leaders,
    look: &Look,
    notice: Notice,
) -> Option<Told> {
    let ours = |player: u32| Some(player) == local;
    let at = |about| Some(Place::of(sim, about));
    // What happens to another side's thing is told only where it's seen.
    let sees = |owner: u32, place: &Place| {
        ours(owner) || local.is_some_and(|p| sim.seen(p, place.at) == Seen::Visible)
    };
    let side = |id: u32| match sim.sides.player(id) {
        Some(p) => Fill::side_of(leaders.owner(p, &sim.sides, &look.map), p),
        None => Fill::from(String::new()),
    };
    let team_name = |team: usize| {
        let name = &sim.sides.teams[team].name;
        look.map.get(name).unwrap_or(name).clone()
    };
    Some(match notice {
        Notice::Pausing(pausing) => Told::Words("GENERIC_EVENT", paused_line(look, pausing, side)),
        Notice::Speed(speed) => {
            let said = look.said(
                "networking_change_game_speed|f",
                "Game speed changed to %.0f%%",
            );
            let percent = format!("{:.0}", speed * 100.0);
            Told::Words("GENERIC_EVENT", fill_all(&said, &[percent]))
        }
        Notice::Chat { from, to, words } => {
            let (kind, words) = chat(sim, local, leaders, look, from, to, words)?;
            Told::Chat(kind, words)
        }
        Notice::Short { player, percent } if ours(player) => {
            let percent = (percent as i64).to_string();
            Told::Event("kingdom_upkeep_shortage", vec![percent.into()], None)
        }
        Notice::Repaired { player } if ours(player) => {
            Told::Event("kingdom_upkeep_repaired", vec![], None)
        }
        Notice::Paused {
            player,
            building,
            template,
            why,
        } if ours(player) => {
            let text = match why {
                Pause::Limit(_) => {
                    let said = look.said(
                        "recruit_error_unit_limit|s",
                        "Insufficient unit limit to recruit %s",
                    );
                    fill(&said, &[&look.name(&template)])
                }
                Pause::Disabled | Pause::Supply => {
                    let said = match why {
                        Pause::Supply => look.said(
                            "structure_error_queue_no_supply|s",
                            "%s paused production queue - no supply",
                        ),
                        _ => look.said(
                            "structure_error_queue_disabled|s",
                            "%s paused production queue - disabled",
                        ),
                    };
                    fill(&said, &[&look.building(sim, building)])
                }
            };
            Told::Line("GENERIC_ERROR", text, at(About::Building(building)))
        }
        Notice::NotCreated { player, template } if ours(player) => {
            let said = look.said("recruit_error_creating|s", "Could not create %s");
            let text = fill(&said, &[&look.name(template.as_str())]);
            Told::Line("GENERIC_ERROR", text, None)
        }
        Notice::HeroKilled {
            player,
            hero,
            organization,
        } if ours(player) => Told::Event(
            "hero_killed",
            vec![look.name(hero.as_str()).into()],
            at(About::Organization(organization)),
        ),
        Notice::SummonFailed { player, template } if ours(player) => {
            let said = look.said("ability_effect_summon_failed|s", "Failed to summon %s");
            let text = fill(&said, &[&look.name(template.as_str())]);
            Told::Line("GENERIC_ERROR", text, None)
        }
        Notice::CannotUnpack {
            player,
            organization,
        } if ours(player) => {
            let said = look.said("tai_error_unpack|s", "Cannot unpack %s");
            let text = fill(
                &said,
                &[&look.name(sim.organizations[organization].template.as_str())],
            );
            Told::Line("GENERIC_ERROR", text, at(About::Organization(organization)))
        }
        Notice::Attached {
            player,
            hq,
            organization,
        }
        | Notice::Detached {
            player,
            hq,
            organization,
        } if ours(player) => {
            let event = match notice {
                Notice::Attached { .. } => "hq_attach",
                _ => "hq_detach",
            };
            let hqs = sim.hqs();
            let hq = hqs.iter().find(|h| h.id == hq);
            let hq = hq.map(|h| look.hq(sim, h)).unwrap_or_default();
            let joining = look.organization(sim, organization);
            Told::Event(
                event,
                vec![hq.into(), joining.into()],
                at(About::Organization(organization)),
            )
        }
        Notice::PutOut {
            player,
            organization,
            ..
        } if ours(player) => {
            // An organization is commissioned, a unit on its own such as a
            // truck recruited.
            let event = match crate::object_display::lone(sim, organization) {
                true => "company_recruited",
                false => "company_commissioned",
            };
            let name = look.organization(sim, organization);
            Told::Event(
                event,
                vec![name.into()],
                at(About::Organization(organization)),
            )
        }
        // Named by its template: it has no number until it's dropped.
        Notice::Recruited {
            player, delivery, ..
        } if ours(player) => {
            let delivery = &sim.deliveries[delivery];
            let dropped = delivery
                .organization
                .map(|r| &sim.organizations[r].template);
            let name = delivery.cargo().or(dropped).map(|t| look.name(t.as_str()));
            let place = Place {
                about: None,
                at: delivery.position,
            };
            Told::Event(
                "company_recruited",
                name.into_iter().map(Fill::from).collect(),
                Some(place),
            )
        }
        Notice::Researched {
            player,
            building,
            technology,
        } if ours(player) => {
            let with = vec![look.name(technology.as_str()).into()];
            Told::Event("research_complete", with, at(About::Building(building)))
        }
        Notice::Gained { player, technology } if ours(player) => Told::Event(
            "technology_gained",
            vec![look.name(technology.as_str()).into()],
            None,
        ),
        // A side always sees its own, so is always told: the loser while
        // it's still theirs, the taker once it is.
        Notice::Lost {
            player,
            building,
            by,
        } if ours(player) => {
            let with = vec![look.building(sim, building).into(), side(by)];
            Told::Event(
                "actor_captured_by_enemy",
                with,
                at(About::Building(building)),
            )
        }
        Notice::Captured { player, building } if ours(player) => {
            let with = vec![look.building(sim, building).into()];
            Told::Event("actor_captured", with, at(About::Building(building)))
        }
        Notice::Sold {
            player,
            building,
            paid,
        } if ours(player) => {
            let with = vec![look.building(sim, building).into(), paid.to_string().into()];
            Told::Event("building_sold", with, at(About::Building(building)))
        }
        // Its owner is told it's lost, and its enemies who see it go that
        // it's destroyed, whoever did it. What's nobody's tells no one.
        Notice::Destroyed { owner, what } if sim.sides.player(owner).is_some() => {
            let (name, about, event) = match what {
                Piece::Organization(r) => (
                    look.organization(sim, r),
                    About::Organization(r),
                    "mobile_actor_lost",
                ),
                Piece::Building(b) => (
                    look.building(sim, b),
                    About::Building(b),
                    "static_actor_lost",
                ),
            };
            let place = Place::of(sim, about);
            let enemy = local.is_some_and(|p| sim.sides.relation(p, owner) == Relation::Enemy);
            match ours(owner) {
                true => Told::Event(event, vec![name.into()], Some(place)),
                false if enemy && sees(owner, &place) => Told::Event(
                    "actor_destroyed",
                    vec![side(owner), name.into()],
                    Some(place),
                ),
                false => return None,
            }
        }
        // How the match comes out is told to everyone.
        Notice::Defeated { player } => Told::Event("kingdom_defeated", vec![side(player)], None),
        Notice::Victorious { player } => Told::Event("kingdom_victory", vec![side(player)], None),
        Notice::Resigned { player } => Told::Event("kingdom_resigned", vec![side(player)], None),
        Notice::TeamResigned { team } => {
            Told::Event("team_resigned", vec![team_name(team).into()], None)
        }
        Notice::ResignVote { player, team } => {
            let said = look.said(
                "networking_player_voted_to_resign_team|ss",
                "%s voted for %s to resign",
            );
            let with = [side(player), team_name(team).into()];
            Told::Words("GENERIC_EVENT", fill_all(&said, &with))
        }
        Notice::TeamDefeated { team } => {
            Told::Event("team_defeated", vec![team_name(team).into()], None)
        }
        Notice::TeamVictorious { team } => {
            Told::Event("team_victory", vec![team_name(team).into()], None)
        }
        Notice::CitiesHeld { player, cities } => {
            let team = sim.sides.player(player).and_then(|p| p.team);
            let name = team.map_or_else(|| side(player), |t| team_name(t).into());
            Told::Event(
                "city_ratio_victory",
                vec![name, cities.to_string().into()],
                None,
            )
        }
        _ => return None,
    })
}

/// Lays out the lines on screen, and fades each as it goes.
pub fn show(
    mut commands: Commands,
    time: Res<Time>,
    log: Res<MessageLog>,
    root: Single<(Entity, Option<&Children>), With<LogBox>>,
    rows: Query<(&Row, &Children)>,
    tree: Query<&Children>,
    mut faded: Query<Parts>,
) {
    let now = time.elapsed_secs();
    let shown = log.on_screen(now);
    let (root, children) = *root;
    let children = children.map_or(&[][..], |c| &c[..]);
    let laid: Vec<u64> = (children.iter())
        .filter_map(|&c| Some(rows.get(c).ok()?.0.0))
        .collect();
    let wanted: Vec<u64> = shown.iter().map(|(l, _)| l.serial).collect();
    if laid != wanted {
        for &c in children {
            commands.entity(c).despawn();
        }
        for &(line, _) in &shown {
            spawn_row(&mut commands, root, &log.look, line);
        }
        return;
    }
    for (&row, &(_, opacity)) in children.iter().zip(&shown) {
        // A line naming a side has its words and gems further down.
        for part in tree.iter_descendants(row) {
            let Ok((Faded(colour), text, shadow, image)) = faded.get_mut(part) else {
                continue;
            };
            let colour = colour.with_alpha(colour.alpha() * opacity);
            if let Some(mut t) = text {
                t.set_if_neq(TextColor(colour));
            }
            if let Some(mut shadow) = shadow {
                let faded = SHADOW.color.with_alpha(SHADOW.color.alpha() * opacity);
                if shadow.color != faded {
                    shadow.color = faded;
                }
            }
            if let Some(mut i) = image
                && i.color != colour
            {
                i.color = colour;
            }
        }
    }
}

/// The lines the Message Log window shows, by the serials of its oldest
/// and newest, once it's shown any: `None` until then, `Some(None)` for none.
#[derive(Default)]
pub struct Listed(Option<Option<(u64, u64)>>);

/// Lists the whole log in its window while that's up, oldest first, kept
/// scrolled to the newest.
pub fn list(
    mut commands: Commands,
    mut listed: bevy::prelude::Local<Listed>,
    displays: Res<Displays>,
    log: Res<MessageLog>,
    mut contents: Query<(Entity, &Contents, &mut ScrollPosition)>,
) {
    if !displays.is_open(Panel::MessageLog) {
        listed.0 = None;
        return;
    }
    let span = (log.lines.front())
        .zip(log.lines.back())
        .map(|(a, b)| (a.serial, b.serial));
    if listed.0 == Some(span) {
        return;
    }
    let Some((list, _, mut scroll)) = contents
        .iter_mut()
        .find(|(_, c, _)| c.0 == Panel::MessageLog)
    else {
        return;
    };
    list_all(&mut commands, list, &log, &mut scroll);
    listed.0 = Some(span);
}

/// A list of the whole log, oldest first and unfaded, kept scrolled to
/// the newest, by the serials of the oldest and newest it shows once it
/// shows any.
#[derive(Component, Default)]
pub struct WholeLog(Option<Option<(u64, u64)>>);

/// Keeps each [`WholeLog`] up with the log.
pub fn relist(
    mut commands: Commands,
    log: Res<MessageLog>,
    mut lists: Query<(Entity, &mut WholeLog, &mut ScrollPosition)>,
) {
    let span = (log.lines.front())
        .zip(log.lines.back())
        .map(|(a, b)| (a.serial, b.serial));
    for (list, mut shown, mut scroll) in &mut lists {
        if shown.0 != Some(span) {
            list_all(&mut commands, list, &log, &mut scroll);
            shown.0 = Some(span);
        }
    }
}

/// Lists every line of the log in `list`, scrolled to the newest.
fn list_all(commands: &mut Commands, list: Entity, log: &MessageLog, scroll: &mut ScrollPosition) {
    commands.entity(list).despawn_children();
    for line in &log.lines {
        spawn_row(commands, list, &log.look, line);
    }
    // Layout holds it to the bottom of the list.
    scroll.y = f32::MAX;
}

fn spawn_row(commands: &mut Commands, root: Entity, look: &Look, line: &Line) {
    let kind = look.kinds.get(&line.kind);
    let colour = kind.map_or(Color::WHITE, |k| k.colour);
    let icon = kind.and_then(|k| k.icon.clone()).unwrap_or_default();
    commands
        .spawn((
            Row(line.serial),
            Node {
                column_gap: px(ICON_GAP),
                align_items: AlignItems::FlexStart,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .with_children(|row| {
            let mut icon = row.spawn((
                Faded(Color::WHITE),
                ImageNode::new(icon),
                Node {
                    width: px(ICON.x),
                    height: px(ICON.y),
                    flex_shrink: 0.0,
                    margin: UiRect::top(px(1)),
                    ..default()
                },
            ));
            match line.place {
                Some(_) => {
                    let id = crate::ui::called(format!("message.{}.look", line.serial));
                    icon.insert((
                        LookAt(line.serial),
                        Button,
                        id,
                        Hint(look.to_location.clone()),
                    ))
                    .observe(look_at);
                }
                None => {
                    icon.insert(Pickable::IGNORE);
                }
            }
            let words = |words: String, colour: Color| {
                (
                    Faded(colour),
                    Text::new(words),
                    font(Font::Hud),
                    TextColor(colour),
                    SHADOW,
                    Pickable::IGNORE,
                )
            };
            // The rest of the row, wrapping only at the log's edge.
            let rest = Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                ..default()
            };
            if line.marks.is_empty() && line.inks.is_empty() {
                row.spawn((words(line.text.clone(), colour), rest));
                return;
            }
            // Text can't hold a picture, so a line naming a side is laid
            // out a word at a time, wrapping between words, each gem kept
            // with the first word of the name it marks.
            let rest = Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                column_gap: px(WORD_GAP),
                ..rest
            };
            row.spawn((rest, Pickable::IGNORE)).with_children(|parts| {
                for Part { gem, word, ink } in parts_of(line) {
                    let colour = ink.map_or(colour, |[r, g, b]| Color::srgb(r, g, b));
                    let Some(rgb) = gem else {
                        parts.spawn(words(word, colour));
                        continue;
                    };
                    let mark = Node {
                        width: px(LINE_GEM),
                        height: px(LINE_GEM),
                        margin: UiRect::right(px(GEM_GAP)),
                        ..default()
                    };
                    let [red, green, blue] = rgb;
                    let together = Node {
                        align_items: AlignItems::Center,
                        ..default()
                    };
                    parts
                        .spawn((together, Pickable::IGNORE))
                        .with_children(|together| {
                            together.spawn((
                                Faded(Color::srgb(red, green, blue)),
                                crate::ui::side_mark(&look.gem, rgb, mark),
                            ));
                            together.spawn(words(word, colour));
                        });
                }
            });
        });
}

/// How big a side's gem is in a line, the space after it, and the space
/// between a line's words where it's laid out a word at a time.
const LINE_GEM: f32 = 15.0;
const GEM_GAP: f32 = 3.0;
const WORD_GAP: f32 = 3.0;

/// A word of a line as it's laid out, with the gem in a side's colour
/// before it where it starts a side's name, and its own colour if it has
/// one.
#[derive(Debug, PartialEq)]
struct Part {
    gem: Option<[f32; 3]>,
    word: String,
    ink: Option<[f32; 3]>,
}

/// A line's words, each that starts a side's name with its gem, and
/// each in the colour its first byte is inked in.
fn parts_of(line: &Line) -> Vec<Part> {
    let ink = |at: usize| {
        let before = line.inks.iter().rev().find(|i| i.at <= at);
        before.and_then(|i| i.rgb)
    };
    let mut parts = Vec::new();
    let mut cuts = line.marks.iter().map(|m| (m.at, Some(m.rgb))).peekable();
    let mut from = (0, None);
    loop {
        let to = cuts.peek().map_or(line.text.len(), |&(at, _)| at);
        let segment = line.text.get(from.0..to).unwrap_or_default();
        let mut gem = from.1;
        let words = segment.split_whitespace().map(|word| {
            // Each word is a slice of the segment, so where it starts.
            let at = from.0 + (word.as_ptr() as usize - segment.as_ptr() as usize);
            (at, word)
        });
        let mut words = words.peekable();
        // A side with no name still has its gem.
        if gem.is_some() && words.peek().is_none() {
            parts.push(Part {
                gem,
                word: String::new(),
                ink: ink(from.0),
            });
        }
        for (at, word) in words {
            parts.push(Part {
                gem: gem.take(),
                word: word.to_string(),
                ink: ink(at),
            });
        }
        match cuts.next() {
            Some(cut) => from = cut,
            None => break,
        }
    }
    parts
}

/// The colour `<color=r,g,b>` at the start of `text` sets, and what
/// follows it.
fn colour_markup(text: &str) -> Option<([f32; 3], &str)> {
    let (inside, after) = text.strip_prefix("<color=")?.split_once('>')?;
    let mut parts = inside.split(',').map(|p| p.trim().parse::<f32>().ok());
    let rgb = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((rgb, after))
}

/// What looks at where a line happened.
#[derive(SystemParam)]
pub struct Viewer<'w, 's> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

impl Viewer<'_, '_> {
    fn look_at(&mut self, place: Place) {
        let [x, y] = place.now(&self.sim.0, self.local.0);
        let height = self.loaded.map.ground.height_at([x, y]);
        for mut camera in &mut self.cameras {
            camera.focus = world(x, y, height);
        }
    }
}

/// Looks at where the clicked icon's line happened.
fn look_at(
    activate: On<Activate>,
    icons: Query<&LookAt>,
    log: Res<MessageLog>,
    mut viewer: Viewer,
) {
    let Ok(LookAt(serial)) = icons.get(activate.entity) else {
        return;
    };
    let line = log.lines.iter().find(|l| l.serial == *serial);
    if let Some(place) = line.and_then(|l| l.place) {
        viewer.look_at(place);
    }
}

/// Looks at the next line back when asked.
pub fn cycle(
    mut intents: MessageReader<crate::intent::Intent>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut viewer: Viewer,
) {
    if !crate::intent::asked(&mut intents, CYCLE) {
        return;
    }
    if let Some(place) = log.cycle(time.elapsed_secs()).and_then(|l| l.place) {
        viewer.look_at(place);
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A log of no kinds of line or events but those its tests know.
    pub(crate) fn log() -> MessageLog {
        MessageLog::new(super::tests::look(&[]))
    }

    /// The text of each line it holds, oldest first.
    pub(crate) fn texts(log: &MessageLog) -> Vec<&str> {
        log.lines.iter().map(|l| l.text.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn look(vars: &[(&str, &str)]) -> Look {
        let kind = |filter: Option<&str>| Kind {
            icon: None,
            colour: Color::WHITE,
            filter: filter.map(Into::into),
            sound: None,
        };
        let event = |id: &str, kind: &str, message: &str, filter: Option<&str>| {
            let e = GameEvent {
                id: Id::from(id),
                message: Some(message.into()),
                kind: Some(Id::from(kind)),
                filter: filter.map(Into::into),
                message_filter: None,
            };
            (Id::from(id), e)
        };
        Look {
            kinds: HashMap::from([
                (
                    Id::from("GENERIC_EVENT"),
                    kind(Some("UIMessageFilterGeneric")),
                ),
                (Id::from("GENERIC_ERROR"), kind(None)),
            ]),
            events: HashMap::from([
                event("hq_attach", "GENERIC_EVENT", "%s: attaching %s", None),
                event(
                    "engaged",
                    "GENERIC_EVENT",
                    "%s engaged",
                    Some("UIEventFilterEngagement"),
                ),
            ]),
            vars: vars_of(vars),
            strings: Arc::default(),
            names: HashMap::new(),
            map: Default::default(),
            to_location: String::new(),
            gem: Default::default(),
        }
    }

    fn vars_of(vars: &[(&str, &str)]) -> InterfaceVars {
        let text: String = vars.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
        InterfaceVars::parse(&format!("[Vars]\n{{\n{text}}}\n"))
    }

    fn place(at: [f32; 2]) -> Option<Place> {
        Some(Place { about: None, at })
    }

    /// A match of sides 1 and 2, allies, and 3, their enemy.
    fn allied_sides() -> World {
        let mut players: Vec<_> = [1, 2, 3].map(crate::tests::player).into();
        for p in &mut players {
            p.relations = vec![tgmap::Relation::Enemy; 3];
        }
        players[0].relations[1] = tgmap::Relation::Ally;
        players[1].relations[0] = tgmap::Relation::Ally;
        let sides = tgsim::Sides {
            names: Default::default(),
            players,
            teams: Vec::new(),
        };
        World::from_organizations(sides, tgmap::Pathing::uniform(8, tgmap::Domain::Land), &[])
    }

    #[test]
    fn chat_reaches_those_its_channel_does() {
        let sim = allied_sides();
        let look = look(&[]);
        let leaders = Leaders::default();
        let heard = |local, from, to| {
            let said = chat(&sim, local, &leaders, &look, from, to, "hi".into());
            said.map(|(kind, _)| kind)
        };
        // Everyone hears all, those watching too, and an enemy's in its
        // own kind.
        assert_eq!(heard(Some(2), Some(1), Channel::All), Some("GENERIC_EVENT"));
        assert_eq!(heard(Some(3), Some(1), Channel::All), Some("CHAT_PUBLIC"));
        assert_eq!(heard(None, Some(1), Channel::All), Some("CHAT_PUBLIC"));
        // Team chat reaches the sender and allies, or those watching.
        assert_eq!(
            heard(Some(1), Some(1), Channel::Team),
            Some("GENERIC_EVENT")
        );
        assert_eq!(
            heard(Some(2), Some(1), Channel::Team),
            Some("GENERIC_EVENT")
        );
        assert_eq!(heard(Some(3), Some(1), Channel::Team), None);
        assert_eq!(heard(None, Some(1), Channel::Team), None);
        assert_eq!(heard(None, None, Channel::Team), Some("CHAT_PUBLIC"));
        assert_eq!(heard(Some(1), None, Channel::Team), None);
        // A whisper, only where it's for, and not back to its sender.
        assert_eq!(
            heard(Some(3), Some(1), Channel::To(3)),
            Some("CHAT_PRIVATE")
        );
        assert_eq!(heard(Some(2), Some(1), Channel::To(3)), None);
        assert_eq!(heard(Some(1), Some(1), Channel::To(3)), None);
        assert_eq!(heard(Some(3), None, Channel::To(3)), None);
    }

    #[test]
    fn chat_shows_its_sender_and_words_in_their_colours() {
        let sim = allied_sides();
        let look = look(&[("point3 ChatTeamColor", "0,0.5,0")]);
        let leaders = Leaders::default();
        let said = chat(
            &sim,
            Some(2),
            &leaders,
            &look,
            Some(1),
            Channel::Team,
            "go".into(),
        );
        let (_, words) = said.unwrap();
        assert_eq!(words.text, "kingdom_1: go");
        let ink = |at, rgb| Ink { at, rgb };
        assert_eq!(
            words.inks,
            [
                ink(0, Some([1.0; 3])),
                ink(9, None),
                ink(11, Some([0.0, 0.5, 0.0])),
                ink(13, None),
            ]
        );
        let said = chat(
            &sim,
            None,
            &leaders,
            &look,
            None,
            Channel::All,
            "hello".into(),
        );
        assert_eq!(said.unwrap().1.text, "Observer: hello");
    }

    #[test]
    fn a_person_is_named_as_themselves_marked_as_their_side_if_they_have_one() {
        let sim = allied_sides();
        let player = sim.sides.player(1).unwrap();
        let bob = person(Some(&sim), "Bob", &[1]);
        assert_eq!(bob, Fill::side_of("Bob".into(), player));
        assert_eq!(
            person(Some(&sim), "Ann", &[]),
            Fill::from("Ann".to_string())
        );
    }

    #[test]
    fn what_is_said_in_the_connection_dialog_is_plain() {
        let look = look(&[]);
        let said = |from| Said {
            from,
            name: "Bob".into(),
            words: "wait".into(),
        };
        let line = said_line(&look, said(Some(1)));
        assert_eq!(line.text, "Bob: wait");
        assert!(line.inks.is_empty() && line.marks.is_empty());
        // One who watches is named all the same.
        assert_eq!(said_line(&look, said(None)).text, "Bob: wait");
    }

    #[test]
    fn a_vote_to_resign_names_the_voter_and_their_team() {
        let mut sim = allied_sides();
        sim.sides.teams.push(tgsim::Team {
            id: 10,
            name: "Allies".into(),
            relations: Vec::new(),
            team_relations: Vec::new(),
        });
        sim.sides.names.insert(1, "Bob".into());
        let (look, leaders) = (look(&[]), Leaders::default());
        let told = |notice| told(&sim, Some(3), &leaders, &look, notice);
        let vote = told(Notice::ResignVote { player: 1, team: 0 });
        let Some(Told::Words(kind, words)) = vote else {
            panic!("a vote is a line of its own");
        };
        assert_eq!(kind, "GENERIC_EVENT");
        assert_eq!(words.text, "Bob voted for Allies to resign");
        let resigned = told(Notice::TeamResigned { team: 0 });
        assert!(matches!(resigned, Some(Told::Event("team_resigned", ..))));
        let resigned = told(Notice::Resigned { player: 1 });
        assert!(matches!(
            resigned,
            Some(Told::Event("kingdom_resigned", ..))
        ));
    }

    #[test]
    fn a_number_with_its_places_given_fills_like_any_other() {
        let line = fill_all("Game speed changed to %.0f%%", &["141"]);
        assert_eq!(line.text, "Game speed changed to 141%");
        assert_eq!(fill_all("%f and %.f", &["1", "2"]).text, "1 and 2");
        // What's not one is left as it is.
        assert_eq!(fill_all("%.x %s", &["a"]).text, "%.x a");
    }

    #[test]
    fn pausing_names_each_person_and_counts_the_votes() {
        let look = look(&[]);
        let line = |pausing| {
            let side = |id: u32| Fill::from(format!("P{id}"));
            paused_line(&look, pausing, side).text
        };
        let votes = Votes {
            votes: 1,
            needed: 3,
        };
        assert_eq!(line(Pausing::Paused { by: 1 }), "P1 has paused the game");
        assert_eq!(
            line(Pausing::Needed(votes)),
            "Votes needed to unpause 3, current votes 1"
        );
        let voted = |cast| Pausing::Voted { by: 2, cast, votes };
        assert_eq!(line(voted(true)), "P2 votes to unpause the game (1/3)");
        assert_eq!(line(voted(false)), "P2 has retracted vote to unpause (1/3)");
        assert_eq!(
            line(Pausing::Unpaused { by: 1 }),
            "P1 has unpaused the game"
        );
        assert_eq!(line(Pausing::VotedOff), "Game unpaused");
        assert_eq!(
            line(Pausing::Refused { by: 1 }),
            "P1 attempted to pause but failed due to recent votes"
        );
    }

    #[test]
    fn a_line_takes_colours_its_text_marks() {
        let words = fill_all("Whispering to %s: <color=1,1,1>%s", &["Bob", "hi <there>"]);
        assert_eq!(words.text, "Whispering to Bob: hi <there>");
        let white = Ink {
            at: 19,
            rgb: Some([1.0; 3]),
        };
        assert_eq!(words.inks, [white]);
        let line = Line {
            serial: 0,
            kind: Id::from("CHAT_PRIVATE"),
            text: words.text,
            marks: Vec::new(),
            inks: words.inks,
            place: None,
            posted: 0.0,
        };
        let inks: Vec<_> = parts_of(&line)
            .into_iter()
            .map(|p| (p.word, p.ink))
            .collect();
        assert_eq!(inks[2], ("Bob:".to_string(), None));
        assert_eq!(inks[3], ("hi".to_string(), Some([1.0; 3])));
    }

    #[test]
    fn only_the_player_here_hears_buildings_change_hands() {
        let lost = |player, by| Notice::Lost {
            player,
            building: 0,
            by,
        };
        let captured = |player| Notice::Captured {
            player,
            building: 0,
        };
        assert_eq!(
            sound(&lost(1, 2), Some(1)),
            Some("actor_captured_by_enemy_sfx")
        );
        assert_eq!(sound(&captured(1), Some(1)), Some("actor_captured_sfx"));
        // One AI taking from another.
        assert_eq!(sound(&lost(2, 3), Some(1)), None);
        assert_eq!(sound(&captured(3), Some(1)), None);
        assert_eq!(sound(&captured(1), None), None);
        assert_eq!(sound(&Notice::Repaired { player: 1 }, Some(1)), None);
    }

    #[test]
    fn placeholders_fill_in_turn() {
        assert_eq!(
            fill_all("%s: attaching %s", &["1st HQ", "2nd Regiment"]).text,
            "1st HQ: attaching 2nd Regiment"
        );
        assert_eq!(
            fill_all("Your production is %d%% short", &["40"]).text,
            "Your production is 40% short"
        );
        assert_eq!(
            fill_all("%i%s given to %s", &["50", "$", "Bob"]).text,
            "50$ given to Bob"
        );
        assert_eq!(fill_all("100% %x", &[] as &[&str]).text, "100% %x");
    }

    #[test]
    fn a_side_named_in_a_line_is_marked_by_its_gem() {
        let red = [1.0, 0.0, 0.0];
        let with = [Fill::side("Germany".into(), red), "Tank".into()];
        let words = fill_all("%s's %s destroyed", &with);
        assert_eq!(words.text, "Germany's Tank destroyed");
        assert_eq!(words.marks, [Mark { at: 0, rgb: red }]);
        let line = Line {
            serial: 0,
            kind: Id::from("GENERIC_EVENT"),
            text: words.text,
            marks: words.marks,
            inks: Vec::new(),
            place: None,
            posted: 0.0,
        };
        let part = |gem, word: &str| Part {
            gem,
            word: word.into(),
            ink: None,
        };
        assert_eq!(
            parts_of(&line),
            [
                part(Some(red), "Germany's"),
                part(None, "Tank"),
                part(None, "destroyed"),
            ]
        );
        // And mid-line, only where the side is.
        let words = fill_all("Player %s quit the game", &[Fill::side("Bob".into(), red)]);
        assert_eq!(words.marks, [Mark { at: 7, rgb: red }]);
        let line = Line {
            text: words.text,
            marks: words.marks,
            ..line
        };
        let parts = parts_of(&line);
        assert_eq!(parts[..2], [part(None, "Player"), part(Some(red), "Bob")]);
        // A side with no name still shows its gem, at the end too.
        let words = fill_all("captured by %s", &[Fill::side(String::new(), red)]);
        let line = Line {
            text: words.text,
            marks: words.marks,
            ..line
        };
        assert_eq!(parts_of(&line).last(), Some(&part(Some(red), "")));
        // An independent side's name is left unmarked.
        let mut civilian = crate::tests::player(3);
        civilian.independent = true;
        assert_eq!(Fill::side_of("Civilian".into(), &civilian).side, None);
        let kingdom = crate::tests::player(1);
        assert_eq!(
            Fill::side_of("Bob".into(), &kingdom).side,
            Some(kingdom.rgb)
        );
    }

    #[test]
    fn events_post_their_lines_unless_filtered() {
        let mut log = MessageLog::new(look(&[]));
        assert!(log.event("hq_attach", &["HQ", "Regiment"], None, 0.0));
        // An event filter the files don't set is off.
        assert!(!log.event("engaged", &["Regiment"], None, 0.0));
        assert!(!log.event("unknown", &[] as &[&str], None, 0.0));
        let texts: Vec<&str> = log.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["HQ: attaching Regiment"]);

        let mut log = MessageLog::new(look(&[
            ("UIMessageFilterGeneric", "false"),
            ("UIEventFilterEngagement", "true"),
        ]));
        assert!(!log.event("engaged", &["Regiment"], None, 0.0));
        assert!(log.post("GENERIC_ERROR", "Cannot unpack".to_string(), None, 0.0));
    }

    #[test]
    fn lines_stay_up_then_fade_and_the_newest_show() {
        let mut log = MessageLog::new(look(&[
            ("UIMessageLogMaximumLines", "2"),
            ("UIMessageLogFadeTime", "10000"),
            ("MessageLogMaximumLines", "3"),
        ]));
        for (i, at) in [0.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
            log.post("GENERIC_ERROR", format!("{i}"), None, at);
        }
        let shown = |log: &MessageLog, now| -> Vec<(String, f32)> {
            (log.on_screen(now).into_iter())
                .map(|(l, a)| (l.text.clone(), a))
                .collect()
        };
        assert_eq!(shown(&log, 3.0), [("2".into(), 1.0), ("3".into(), 1.0)]);
        assert_eq!(shown(&log, 12.5), [("2".into(), 0.5), ("3".into(), 1.0)]);
        assert_eq!(shown(&log, 13.0), [("3".into(), 1.0)]);
        assert_eq!(shown(&log, 14.0), []);
        // Only so many are kept.
        let kept: Vec<&str> = log.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(kept, ["1", "2", "3"]);
    }

    #[test]
    fn the_key_steps_back_through_placed_lines_and_round() {
        let mut log = MessageLog::new(look(&[("UIMessageLogCycleTimeout", "1.0")]));
        log.post("GENERIC_ERROR", "a".to_string(), place([1.0, 0.0]), 0.0);
        log.post("GENERIC_ERROR", "unplaced".to_string(), None, 0.0);
        log.post("GENERIC_ERROR", "b".to_string(), place([2.0, 0.0]), 0.0);
        let mut at = |now| log.cycle(now).map(|l| l.text.clone());
        assert_eq!(at(10.0).as_deref(), Some("b"));
        assert_eq!(at(10.5).as_deref(), Some("a"));
        // Round to the newest after the oldest.
        assert_eq!(at(11.0).as_deref(), Some("b"));
        assert_eq!(at(11.5).as_deref(), Some("a"));
        // A longer pause starts from the newest again.
        assert_eq!(at(20.0).as_deref(), Some("b"));
    }
}
