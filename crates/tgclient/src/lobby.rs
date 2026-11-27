//! The lobby of a custom game: pick a map, how its sides are teamed up, and
//! the side you play on it, or watch, and the computer plays every other
//! side.

use std::collections::{BTreeMap, HashMap};

use bevy::image::CompressedImageFormatSupport;
use bevy::picking::events::PointerDragDrop;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgdata::{Faction, Handicap, Id, Nation, RuleValues, Sai};
use tgmap::{Brief, Player};

use crate::lan::Staged;
use crate::loading::Pending;
use crate::menu::{MenuPage, open};
use crate::presets::Preset;
use crate::ui::GLYPH_DIR;
use crate::ui::{
    Chosen, Dropdown, Font, Hint, Ink, Shown, Surface, button, called, clickable, disabled,
    dropdown, inert, label, opener, padded, panel, slug, text,
};
use crate::{Data, Screen, Seed, Side, confirm};

/// Who plays a side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Person,
    Computer,
}

/// Who plays each side of a match, by player id, in side order.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Lineup(pub Vec<(u32, Control)>);

impl Lineup {
    /// A person at the `person` player's side, if any, and the computer at
    /// the rest.
    pub fn new(players: &[Player], person: Option<u32>) -> Lineup {
        let mut lineup = Lineup(
            tgmap::sides(players)
                .map(|p| (p.id, Control::Computer))
                .collect(),
        );
        lineup.seat(person);
        lineup
    }

    /// The player the person plays; none while they watch.
    pub fn person(&self) -> Option<u32> {
        self.0
            .iter()
            .find(|(_, c)| *c == Control::Person)
            .map(|&(p, _)| p)
    }

    /// Moves the person to `player`'s side, or out to watch where it isn't
    /// one, the computer taking over the side they leave.
    pub fn seat(&mut self, player: Option<u32>) {
        for (p, control) in &mut self.0 {
            *control = match Some(*p) == player {
                true => Control::Person,
                false => Control::Computer,
            };
        }
    }
}

/// The profile each computer player of a match plays by, by player id.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Computers(pub BTreeMap<u32, Id>);

/// The player a person plays unless they choose another: the side asked for
/// on the command line, else the first a person may play; none for side 0.
pub fn default_person(players: &[Player], asked: Side) -> Option<u32> {
    let mut sides = tgmap::sides(players);
    let side = match asked.0 {
        // Nobody's: computer players play every side.
        Some(0) => None,
        Some(n) => sides.nth(n - 1),
        None => sides.find(|p| p.playable),
    };
    side.map(|p| p.id)
}

/// The sides a match with no lobby gives computer players: every side but
/// the `person`'s.
pub fn computer_sides(players: &[Player], person: Option<u32>) -> BTreeMap<u32, Option<Id>> {
    tgmap::sides(players)
        .filter(|p| Some(p.id) != person)
        .map(|p| (p.id, None))
        .collect()
}

/// The game's question before leaving the lobby.
#[derive(Resource)]
pub struct Leaving(String);

impl Leaving {
    pub fn of(strings: &tgdata::Strings) -> Self {
        let question = strings.get("ui_staging_confirm_exit");
        Leaving(question.map_or("Exit Staging?", String::as_str).to_string())
    }
}

/// The nations sides can play as.
#[derive(Resource)]
pub struct Nations(pub Vec<Nation>);

/// The computer player profiles, those the lobby offers among them.
#[derive(Resource)]
pub struct Sais(pub Vec<Sai>);

impl Sais {
    fn find(&self, id: &Id) -> Option<&Sai> {
        self.0.iter().find(|s| s.id == *id)
    }

    /// Those the lobby offers.
    fn offered(&self) -> impl Iterator<Item = &Sai> {
        self.0.iter().filter(|s| s.public)
    }
}

/// The flags the lobby shows for the nations, by id, and for a nation left
/// to chance.
#[derive(Resource)]
struct Flags {
    nations: HashMap<Id, Handle<Image>>,
    random: Option<Handle<Image>>,
}

impl Flags {
    fn of(&self, nation: Option<&str>) -> Option<Handle<Image>> {
        match nation {
            Some(n) => self.nations.get(&Id::from(n)).cloned(),
            None => self.random.clone(),
        }
    }
}

/// The handicaps the lobby offers the computer's sides, with their icons
/// by id once the lobby has been shown.
#[derive(Resource)]
pub struct Handicaps {
    pub all: Vec<Handicap>,
    icons: HashMap<Id, Handle<Image>>,
}

impl Handicaps {
    pub fn new(all: Vec<Handicap>) -> Handicaps {
        Handicaps {
            all,
            icons: HashMap::new(),
        }
    }

    fn find(&self, id: &Id) -> Option<&Handicap> {
        self.all.iter().find(|h| h.id == *id)
    }

    /// The one a side has: the one `chosen` for it, else the map's, else
    /// the one that changes nothing, as having none doesn't either.
    fn of(&self, chosen: Option<&Id>, map: Option<&Id>) -> Option<&Handicap> {
        (chosen.or(map).and_then(|id| self.find(id)))
            .or_else(|| self.all.iter().find(|h| h.property.is_none()))
            .or(self.all.first())
    }

    fn shown(&self, handicap: &Handicap) -> Shown {
        let icon = self.icons.get(&handicap.id).cloned();
        let words = match (&icon, &handicap.name) {
            (Some(_), _) => String::new(),
            (None, Some(name)) => name.clone(),
            (None, None) => handicap.id.to_string(),
        };
        Shown {
            icon,
            words,
            hint: Some(handicap.description.clone()).filter(|d| !d.is_empty()),
            ink: None,
        }
    }
}

/// The handicaps chosen for the sides the computer plays, by player id; the
/// lobby offers none for the person's.
fn computer_handicaps(
    players: &[Player],
    person: Option<u32>,
    chosen: &BTreeMap<u32, Id>,
) -> BTreeMap<u32, Id> {
    let lineup = Lineup::new(players, person);
    (lineup.0.into_iter())
        .filter(|&(_, control)| control == Control::Computer)
        .filter_map(|(p, _)| Some((p, chosen.get(&p)?.clone())))
        .collect()
}

/// Which of the lobby's lists is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Nothing,
    Presets,
    /// The nations for this player's side.
    Nations(u32),
    /// The factions for this player's side.
    Factions(u32),
    /// The handicaps for this player's side.
    Handicaps(u32),
    /// The computer player profiles for this player's side.
    Computers(u32),
}

/// What the lobby has chosen.
#[derive(Resource)]
pub struct Lobby {
    /// The map's archive path.
    map: String,
    brief: Result<Brief, String>,
    preset: Preset,
    /// The player the person plays; none to watch.
    person: Option<u32>,
    /// The sides others take in a game between machines, by player id.
    others: Vec<u32>,
    /// The sides computer players of other machines play in a game between
    /// machines, by player id, with whose machine each belongs to.
    theirs: BTreeMap<u32, String>,
    /// What's chosen for each side: a nation or faction at random, and
    /// the default handicap, where none is.
    slots: tgserver::Slots,
    /// What each world rule is set to, by id.
    rules: RuleValues,
    open: Open,
}

impl Lobby {
    fn new(data: &Data, map: String, asked: Side) -> Lobby {
        let brief = Brief::load(&data.0, &map);
        let (preset, person) = match &brief {
            Ok(b) => (
                Preset::made_for(&b.players),
                default_person(&b.players, asked),
            ),
            Err(_) => (Preset::Custom, None),
        };
        let mut lobby = Lobby {
            map,
            preset,
            person,
            others: Vec::new(),
            theirs: BTreeMap::new(),
            slots: tgserver::Slots::default(),
            open: Open::Nothing,
            // As the map stores them, until the scenario map dialog
            // changes them.
            rules: (brief.as_ref())
                .map(|b| b.rules.values.clone())
                .unwrap_or_default(),
            brief,
        };
        lobby.keep_person_playing();
        lobby
    }

    /// The map's archive path.
    pub(crate) fn map(&self) -> &str {
        &self.map
    }

    /// What each world rule is set to, by id.
    pub(crate) fn rules(&self) -> &RuleValues {
        &self.rules
    }

    /// The teams the sides play in, by player id, each with what to call
    /// it; a side not on one leaves the match.
    fn teams(&self) -> Vec<(String, Vec<u32>)> {
        let Ok(brief) = &self.brief else {
            return Vec::new();
        };
        let named = |teams: Vec<Vec<u32>>| -> Vec<(String, Vec<u32>)> {
            let n = |i: usize| format!("Team {}", i + 1);
            teams
                .into_iter()
                .enumerate()
                .map(|(i, t)| (n(i), t))
                .collect()
        };
        match self.preset {
            Preset::Custom => {
                let on = |t| {
                    brief
                        .sides()
                        .filter(|p| p.team == t)
                        .map(|p| p.id)
                        .collect()
                };
                let teams = (0..brief.teams.len()).map(|t| on(Some(t)));
                let teams: Vec<Vec<u32>> = teams.filter(|t: &Vec<u32>| !t.is_empty()).collect();
                let mut named = named(teams);
                let alone: Vec<u32> = on(None);
                if !alone.is_empty() {
                    named.push(("No team".into(), alone));
                }
                named
            }
            Preset::FreeForAll(_) => {
                let all = self.preset.arrange(&brief.players).unwrap_or_default();
                vec![("Free For All".into(), all.concat())]
            }
            Preset::Teams { .. } => named(self.preset.arrange(&brief.players).unwrap_or_default()),
        }
    }

    /// Takes what the host of a game between machines says it has come
    /// to, `me` being what the host knows the person here as.
    pub(crate) fn follow(&mut self, data: &Data, staging: &tgserver::Staging, me: u32) {
        if self.map != staging.map {
            self.map.clone_from(&staging.map);
            self.brief = Brief::load(&data.0, &staging.map);
        }
        self.preset = staging.preset;
        self.rules.clone_from(&staging.rules);
        self.person = staging.guest(me).and_then(|g| g.side);
        let others = staging.guests.iter().filter(|g| g.client != me);
        self.others = others.filter_map(|g| g.side).collect();
        self.slots.clone_from(&staging.slots);
        let name = |client| staging.guest(client).map(|g| g.name.clone());
        let sides = self.brief.iter().flat_map(|b| b.sides().map(|p| p.id));
        self.theirs = (sides.filter(|&p| self.computer(p) && !staging.owns(me, p)))
            .filter_map(|p| Some((p, name(staging.owner(p)?)?)))
            .collect();
    }

    /// Whether nobody holds `player`'s side.
    fn empty(&self, player: u32) -> bool {
        self.slots.is_open(player)
    }

    /// Whether a computer player plays `player`'s side, as no person takes
    /// it and it isn't left open.
    fn computer(&self, player: u32) -> bool {
        self.person != Some(player) && !self.others.contains(&player) && !self.empty(player)
    }

    /// Whether a computer player belonging to the machine here plays
    /// `player`'s side, and so its choices are made here.
    fn mine(&self, player: u32) -> bool {
        self.computer(player) && !self.theirs.contains_key(&player)
    }

    /// What dropping the slot of side `from` on side `to` does, both sides
    /// a person may move to, `hosting` being whether this machine runs the
    /// host: the person moves, or one of the host's computer players does,
    /// either trading places with a computer player of this machine's
    /// already there. None where neither may.
    fn dropped(&self, from: u32, to: u32, hosting: bool) -> Option<Moved> {
        if from == to {
            return None;
        }
        let takes = self.empty(to) || self.mine(to);
        if self.person == Some(from) {
            return takes.then_some(Moved::Seat(to));
        }
        if !self.mine(from) || !hosting {
            return None;
        }
        if self.person == Some(to) {
            return Some(Moved::Seat(from));
        }
        takes.then_some(Moved::Shift(from, to))
    }

    /// The team side `player` is on in the lobby's team setup, by its
    /// place there; none on no team.
    fn team_of(&self, player: u32) -> Option<usize> {
        let brief = self.brief.as_ref().ok()?;
        match self.preset.arrange(&brief.players) {
            Some(teams) => teams.iter().position(|t| t.contains(&player)),
            None => brief.players.iter().find(|p| p.id == player)?.team,
        }
    }

    /// Whether what's said for their team from side `from` reaches a person
    /// on side `to`, none being one watching: those on a team reach its
    /// sides, and those watching each other.
    pub(crate) fn hears_team(&self, from: Option<u32>, to: Option<u32>) -> bool {
        match (from, to) {
            (None, None) => true,
            (Some(from), Some(to)) => self
                .team_of(from)
                .is_some_and(|t| self.team_of(to) == Some(t)),
            _ => false,
        }
    }

    /// The profile chosen for `player`'s side, while the computer plays it.
    fn profile<'a>(&self, sais: &'a Sais, player: u32) -> Option<&'a Sai> {
        let id = self.slots.computers.get(&player)?;
        sais.find(id).filter(|_| self.computer(player))
    }

    /// What the match is played on and by as the lobby has it, drawing
    /// its chances from `seed`, with what the loading screen says of it;
    /// none without a map to play.
    pub(crate) fn setup(&self, seed: u64) -> Option<(tgserver::Setup, String, String)> {
        let brief = self.brief.as_ref().ok()?;
        let playing: Vec<u32> = self.teams().into_iter().flat_map(|(_, t)| t).collect();
        let computers: BTreeMap<u32, Option<Id>> = (playing.iter().copied())
            .filter(|&p| self.computer(p))
            .map(|p| (p, self.slots.computers.get(&p).cloned()))
            .collect();
        let open = playing.into_iter().filter(|&p| self.empty(p)).collect();
        let slots = &self.slots;
        let setup = tgserver::Setup {
            map: self.map.clone(),
            seed,
            teams: self.preset.arrange(&brief.players),
            nations: slots.nations.clone(),
            factions: slots.factions.clone(),
            handicaps: computer_handicaps(&brief.players, self.person, &slots.handicaps),
            computers,
            open,
            rules: self.rules.clone(),
        };
        Some((setup, brief.name.clone(), brief.description.clone()))
    }

    /// The map's sides, as the match has them before it's set up.
    pub(crate) fn sides(&self) -> tgsim::Sides {
        let players = self.brief.as_ref().map(|b| b.players.clone());
        tgsim::Sides {
            players: players.unwrap_or_default(),
            ..Default::default()
        }
    }

    /// Moves the person to the first side they may play if theirs has left
    /// the match.
    fn keep_person_playing(&mut self) {
        let Ok(brief) = &self.brief else {
            return;
        };
        let playing: Vec<u32> = self.teams().into_iter().flat_map(|(_, t)| t).collect();
        if self.person.is_some_and(|p| !playing.contains(&p)) {
            let to = (brief.sides())
                .find(|p| p.playable && playing.contains(&p.id))
                .map(|p| p.id);
            let from = std::mem::replace(&mut self.person, to);
            self.slots.seat(from, to);
        }
    }
}

/// A player to play, or none to watch.
#[derive(Component)]
struct SideChoice(Option<u32>);

/// What a slot dropped on another moves.
#[derive(Debug, PartialEq)]
enum Moved {
    /// The person, to this side.
    Seat(u32),
    /// A computer player, from the first side to the second.
    Shift(u32, u32),
}

/// A side's slot, by player id, which can be dragged or dropped on, and
/// whether a person may move to it.
#[derive(Component)]
struct SlotRow {
    id: u32,
    movable: bool,
}

/// A team's header, which a slot from another team can be dropped on:
/// its sides, and the first of them a person may move to that nobody
/// holds.
#[derive(Component)]
struct TeamRow {
    members: Vec<u32>,
    free: Option<u32>,
}

/// The button that opens a side's nations, by player id.
#[derive(Component)]
struct NationsOf(u32);

/// One of the nations in a side's list, by player id; none for one at
/// random.
#[derive(Component)]
struct NationChoice(u32, Option<Id>);

/// The button that opens a side's factions, by player id.
#[derive(Component)]
struct FactionsOf(u32);

/// One of the factions in a side's list, by player id; none for one at
/// random.
#[derive(Component)]
struct FactionChoice(u32, Option<Id>);

/// The button that opens a side's handicaps, by player id.
#[derive(Component)]
struct HandicapsOf(u32);

/// One of the handicaps in a side's list, by player id.
#[derive(Component)]
struct HandicapChoice(u32, Id);

/// The button that opens the profiles for a side's computer player, by
/// player id.
#[derive(Component)]
struct ComputersOf(u32);

/// One of the choices in the list of who plays a side, by player id.
#[derive(Component)]
struct ComputerChoice(u32, Holder);

/// Who a lobby can put on a side nobody takes.
#[derive(Clone, PartialEq)]
enum Holder {
    /// Nobody.
    Open,
    /// A computer player playing by a profile, or with none, one at
    /// random.
    Computer(Option<Id>),
}

/// One of the presets in their list.
#[derive(Component)]
struct PresetChoice(Preset);

/// Where the lobby shows the preset and lists the others to choose from.
#[derive(Component)]
struct Presets;

/// Where the lobby lists the sides to play.
#[derive(Component)]
struct Slots;

/// Where the lobby describes the map.
#[derive(Component)]
struct About;

pub struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(Screen::Lobby),
            (load_pictures, enter, spawn).chain(),
        )
        .add_systems(
            Update,
            (show_presets, show_slots, show_about, mark_chosen)
                .chain()
                // Each its own: a launch can take the lobby away between
                // them, as what one does is carried out before the next.
                .distributive_run_if(in_state(Screen::Lobby))
                .distributive_run_if(resource_exists_and_changed::<Lobby>),
        );
    }
}

/// Decodes the flags and handicap icons on the first visit; later ones
/// keep them.
fn load_pictures(
    mut commands: Commands,
    data: Res<Data>,
    all: Res<Nations>,
    flags: Option<Res<Flags>>,
    mut handicaps: ResMut<Handicaps>,
    formats: Res<CompressedImageFormatSupport>,
    mut images: ResMut<Assets<Image>>,
) {
    if flags.is_some() {
        return;
    }
    let random = data.0.random_flag();
    let mut load =
        |path: &str| crate::ui::read_image(&data.0, path, formats.0).map(|i| images.add(i));
    let nations = (all.0.iter())
        .filter_map(|n| Some((n.id.clone(), load(n.flag.as_deref()?)?)))
        .collect();
    let random = random.and_then(|p| load(&p));
    let icons = (handicaps.all.iter())
        .filter_map(|h| {
            let glyph = h.glyph.as_deref()?;
            Some((h.id.clone(), load(&format!("{GLYPH_DIR}/{glyph}.png"))?))
        })
        .collect();
    handicaps.icons = icons;
    commands.insert_resource(Flags { nations, random });
}

/// Picks the first of the scenarios, unless the lobby has been here before;
/// or for a game between machines, what its host says.
fn enter(
    mut commands: Commands,
    data: Res<Data>,
    asked: Res<Side>,
    lobby: Option<ResMut<Lobby>>,
    staged: Option<Res<Staged>>,
) {
    if let Some(staged) = staged {
        let staging = staged.entry.staging();
        let mut lobby = Lobby::new(&data, staging.map.clone(), Side(None));
        lobby.follow(&data, staging, staged.entry.me());
        commands.insert_resource(lobby);
        return;
    }
    if let Some(mut lobby) = lobby {
        lobby.open = Open::Nothing;
        return;
    }
    let Some(map) = scenarios(&data).into_iter().next() else {
        return;
    };
    commands.insert_resource(Lobby::new(&data, map, *asked));
}

/// The first map a custom game offers, by archive path.
pub(crate) fn first_scenario(data: &Data) -> Option<String> {
    scenarios(data).into_iter().next()
}

/// How a game played on `map` starts out: its sides in teams as the map
/// has them, and those a person may take.
pub(crate) fn opening(data: &Data, map: &str) -> (Preset, Vec<u32>) {
    let Ok(brief) = Brief::load(&data.0, map) else {
        return (Preset::Custom, Vec::new());
    };
    let preset = Preset::made_for(&brief.players);
    (preset, shared_sides(&brief, preset))
}

/// The sides of `brief`'s map a person may take with them in teams as
/// `preset` has them, by player id: those it plays that a person may.
fn shared_sides(brief: &Brief, preset: Preset) -> Vec<u32> {
    let playing: Option<Vec<u32>> = preset.arrange(&brief.players).map(|t| t.concat());
    let sides = brief.sides().filter(|p| p.playable);
    sides
        .filter(|p| playing.as_ref().is_none_or(|t| t.contains(&p.id)))
        .map(|p| p.id)
        .collect()
}

/// Whether this machine has `map`, to play it.
pub(crate) fn has_map(data: &Data, map: &str) -> bool {
    Brief::load(&data.0, map).is_ok()
}

/// The maps a custom game is played on, by archive path: those in the maps
/// folder itself, not the campaign's or the developers' below it.
pub(crate) fn scenarios(data: &Data) -> Vec<String> {
    let names = data.0.map_names().into_iter();
    let top = names.filter(|n| {
        n.rsplit_once('/')
            .is_some_and(|(f, _)| f.eq_ignore_ascii_case("Maps"))
    });
    top.map(str::to_string).collect()
}

/// Lays out the lobby as the game does: the preset over the sides on the
/// left, the map and the buttons on the right.
fn spawn(mut commands: Commands, staged: Option<Res<Staged>>) {
    commands.spawn((Camera2d, DespawnOnExit(Screen::Lobby)));
    let root = commands
        .spawn((
            DespawnOnExit(Screen::Lobby),
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
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        Presets,
        ChildOf(left),
        Node {
            width: px(240),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        // Its list opens over the sides.
        GlobalZIndex(crate::ui::layer::RAISED),
    ));
    commands.spawn((
        Slots,
        ChildOf(left),
        panel(
            Surface::Panel,
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(4),
                ..default()
            },
        ),
    ));
    if staged.is_some() {
        crate::staging::spawn_log(&mut commands, left);
    }
    let right = commands
        .spawn((
            ChildOf(root),
            Node {
                width: px(360),
                flex_direction: FlexDirection::Column,
                // Otherwise it grows to fit the list of maps instead of
                // letting it scroll.
                min_height: px(0),
                row_gap: px(12),
                ..default()
            },
        ))
        .id();
    commands.spawn((
        About,
        ChildOf(right),
        panel(
            Surface::Panel,
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(4),
                ..default()
            },
        ),
    ));
    // Only the host of a game between machines chooses its map.
    let hosting = staged.as_ref().is_none_or(|s| {
        let me = s.entry.me();
        s.entry.staging().guest(me).is_some_and(|g| g.host)
    });
    if hosting {
        commands
            .spawn((
                ChildOf(right),
                button("Scenario Map", Font::Large),
                called("lobby.scenario"),
            ))
            .observe(crate::scenario::open);
    }
    if staged.is_some() {
        commands.spawn((
            ChildOf(right),
            crate::staging::Controls,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                ..default()
            },
        ));
        return;
    }
    commands
        .spawn((
            ChildOf(right),
            button("Launch", Font::Large),
            called("lobby.launch"),
        ))
        .observe(launch);
    commands
        .spawn((
            ChildOf(right),
            button("Exit", Font::Large),
            called("lobby.exit"),
        ))
        .observe(ask_exit);
}

/// Asks whether to leave the lobby, and goes back to the new game menu if so.
fn ask_exit(
    _: On<Activate>,
    mut commands: Commands,
    words: Res<confirm::Words>,
    leaving: Res<Leaving>,
) {
    let question = leaving.0.clone();
    confirm::confirm(
        &mut commands,
        &words,
        Screen::Lobby,
        question,
        open(MenuPage::NewGame),
    );
}

/// Plays `map` under the world rules' choices in `rules`, as the scenario
/// map dialog loads it: what was chosen for another map's sides goes.
pub(crate) fn play(
    commands: &mut Commands,
    data: &Data,
    (asked, lobby): (Side, Option<ResMut<Lobby>>),
    staged: Option<ResMut<Staged>>,
    map: String,
    rules: RuleValues,
) {
    if let Some(mut lobby) = lobby.filter(|l| l.map == map) {
        // Only the rules change, so what's chosen for the sides stays.
        if !share(staged, tgserver::Choice::Rules(rules.clone())) {
            lobby.rules = rules;
        }
        return;
    }
    let (preset, sides) = opening(data, &map);
    let choice = tgserver::Choice::Map {
        map: map.clone(),
        preset,
        sides,
        rules: rules.clone(),
    };
    if !share(staged, choice) {
        let mut lobby = Lobby::new(data, map, asked);
        lobby.rules = rules;
        commands.insert_resource(lobby);
    }
}

/// Asks the host of a game between machines for `choice`, where the
/// lobby is one's. Whether it was, and so the lobby waits to hear what
/// the host made of it rather than making it itself.
fn share(staged: Option<ResMut<Staged>>, choice: tgserver::Choice) -> bool {
    let Some(mut staged) = staged else {
        return false;
    };
    // A host that's gone is found so as the lobby next hears from it.
    let _ = staged.entry.choose(choice);
    true
}

fn choose_side(
    activate: On<Activate>,
    choices: Query<&SideChoice>,
    lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    if let Ok(SideChoice(person)) = choices.get(activate.entity) {
        seat(lobby, staged, *person);
    }
}

/// Moves the person to side `person`, or to watch with none.
fn seat(mut lobby: ResMut<Lobby>, staged: Option<ResMut<Staged>>, person: Option<u32>) {
    lobby.open = Open::Nothing;
    if !share(staged, tgserver::Choice::Side(person)) {
        let from = std::mem::replace(&mut lobby.person, person);
        lobby.slots.seat(from, person);
    }
}

/// Takes a slot dropped on another slot or a team's header, as
/// [`Lobby::dropped`] has it.
fn drop_slot(
    mut drop: On<PointerDragDrop>,
    rows: Query<&SlotRow>,
    teams: Query<&TeamRow>,
    parents: Query<&ChildOf>,
    mut lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    drop.propagate(false);
    let up = |e: &Entity| parents.get(*e).ok().map(ChildOf::parent);
    let dragged = std::iter::successors(Some(drop.dropped), up).find_map(|e| rows.get(e).ok());
    let Some(&SlotRow { id: from, movable }) = dragged else {
        return;
    };
    let to = match (rows.get(drop.entity), teams.get(drop.entity)) {
        (Ok(row), _) if row.movable => row.id,
        (_, Ok(team)) if !team.members.contains(&from) => match team.free {
            Some(free) => free,
            None => return,
        },
        _ => return,
    };
    let hosting = staged.as_ref().is_none_or(|s| {
        let staging = s.entry.staging();
        staging.guest(s.entry.me()).is_some_and(|g| g.host)
    });
    if !movable {
        return;
    }
    match lobby.dropped(from, to, hosting) {
        Some(Moved::Seat(side)) => seat(lobby, staged, Some(side)),
        Some(Moved::Shift(from, to)) if !share(staged, tgserver::Choice::Shift(from, to)) => {
            lobby.open = Open::Nothing;
            lobby.slots.exchange(from, to);
        }
        Some(Moved::Shift(..)) | None => {}
    }
}

fn open_presets(_: On<Activate>, mut lobby: ResMut<Lobby>) {
    lobby.open = match lobby.open {
        Open::Presets => Open::Nothing,
        _ => Open::Presets,
    };
}

/// Opens the list of nations for a side, or closes it.
fn open_nations(activate: On<Activate>, sides: Query<&NationsOf>, mut lobby: ResMut<Lobby>) {
    if let Ok(NationsOf(player)) = sides.get(activate.entity) {
        lobby.open = match lobby.open {
            Open::Nations(p) if p == *player => Open::Nothing,
            _ => Open::Nations(*player),
        };
    }
}

fn choose_nation(
    activate: On<Activate>,
    choices: Query<&NationChoice>,
    mut lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    if let Ok(NationChoice(player, nation)) = choices.get(activate.entity) {
        lobby.open = Open::Nothing;
        if share(staged, tgserver::Choice::Nation(*player, nation.clone())) {
            return;
        }
        match nation {
            Some(n) => lobby.slots.nations.insert(*player, n.clone()),
            None => lobby.slots.nations.remove(player),
        };
        // Factions lead only their own nation's sides.
        lobby.slots.factions.remove(player);
        lobby.open = Open::Nothing;
    }
}

/// Opens the list of factions for a side, or closes it.
fn open_factions(activate: On<Activate>, sides: Query<&FactionsOf>, mut lobby: ResMut<Lobby>) {
    if let Ok(FactionsOf(player)) = sides.get(activate.entity) {
        lobby.open = match lobby.open {
            Open::Factions(p) if p == *player => Open::Nothing,
            _ => Open::Factions(*player),
        };
    }
}

fn choose_faction(
    activate: On<Activate>,
    choices: Query<&FactionChoice>,
    mut lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    if let Ok(FactionChoice(player, faction)) = choices.get(activate.entity) {
        lobby.open = Open::Nothing;
        if share(staged, tgserver::Choice::Faction(*player, faction.clone())) {
            return;
        }
        match faction {
            Some(g) => lobby.slots.factions.insert(*player, g.clone()),
            None => lobby.slots.factions.remove(player),
        };
        lobby.open = Open::Nothing;
    }
}

fn open_handicaps(activate: On<Activate>, sides: Query<&HandicapsOf>, mut lobby: ResMut<Lobby>) {
    if let Ok(HandicapsOf(player)) = sides.get(activate.entity) {
        lobby.open = match lobby.open {
            Open::Handicaps(p) if p == *player => Open::Nothing,
            _ => Open::Handicaps(*player),
        };
    }
}

fn choose_handicap(
    activate: On<Activate>,
    choices: Query<&HandicapChoice>,
    mut lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    if let Ok(HandicapChoice(player, handicap)) = choices.get(activate.entity) {
        lobby.open = Open::Nothing;
        let choice = tgserver::Choice::Handicap(*player, Some(handicap.clone()));
        if share(staged, choice) {
            return;
        }
        lobby.slots.handicaps.insert(*player, handicap.clone());
        lobby.open = Open::Nothing;
    }
}

fn open_computers(activate: On<Activate>, sides: Query<&ComputersOf>, mut lobby: ResMut<Lobby>) {
    if let Ok(ComputersOf(player)) = sides.get(activate.entity) {
        lobby.open = match lobby.open {
            Open::Computers(p) if p == *player => Open::Nothing,
            _ => Open::Computers(*player),
        };
    }
}

fn choose_computer(
    activate: On<Activate>,
    choices: Query<&ComputerChoice>,
    sais: Res<Sais>,
    mut lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    let Ok(ComputerChoice(player, holder)) = choices.get(activate.entity) else {
        return;
    };
    let player = *player;
    lobby.open = Open::Nothing;
    let Holder::Computer(sai) = holder else {
        if !share(staged, tgserver::Choice::Open(player)) {
            lobby.slots.empty(player);
        }
        return;
    };
    // The side plays as the profile's first nation and faction, or as
    // ones at random with none.
    let profile = sai.as_ref().and_then(|s| sais.find(s));
    let nation = profile.and_then(|s| s.nations.first().cloned());
    let faction = profile.and_then(|s| s.factions.first().cloned());
    if let Some(mut staged) = staged {
        use tgserver::Choice;
        let choices = [
            Choice::Computer(player, sai.clone()),
            Choice::Nation(player, nation),
            Choice::Faction(player, faction),
        ];
        for choice in choices {
            // A host that's gone is found so as the lobby next hears from it.
            let _ = staged.entry.choose(choice);
        }
        return;
    }
    let set = |map: &mut BTreeMap<u32, Id>, id: Option<Id>| match id {
        Some(id) => _ = map.insert(player, id),
        None => _ = map.remove(&player),
    };
    let slots = &mut lobby.slots;
    slots.computer(player, sai.clone());
    set(&mut slots.nations, nation);
    set(&mut slots.factions, faction);
}

fn choose_preset(
    activate: On<Activate>,
    choices: Query<&PresetChoice>,
    mut lobby: ResMut<Lobby>,
    staged: Option<ResMut<Staged>>,
) {
    if let Ok(PresetChoice(preset)) = choices.get(activate.entity) {
        lobby.open = Open::Nothing;
        let sides = lobby.brief.as_ref().map(|b| shared_sides(b, *preset));
        let choice = tgserver::Choice::Preset {
            preset: *preset,
            sides: sides.unwrap_or_default(),
        };
        if share(staged, choice) {
            return;
        }
        lobby.preset = *preset;
        lobby.open = Open::Nothing;
        lobby.keep_person_playing();
    }
}

fn launch(
    _: On<Activate>,
    lobby: Option<Res<Lobby>>,
    seed: Res<Seed>,
    mut commands: Commands,
    mut next: ResMut<NextState<Screen>>,
) {
    let Some(lobby) = lobby else {
        return;
    };
    let Some((setup, title, description)) = lobby.setup(seed.get()) else {
        return;
    };
    commands.insert_resource(Pending {
        title,
        description,
        person: lobby.person,
        from_menu: true,
        back: Some(Screen::Lobby),
        joined: None,
        setup,
    });
    next.set(Screen::Loading);
}

/// A side's gem in its colour, as the lobby marks it.
pub(crate) fn side_mark(gem: &crate::ui::Gem, player: &Player) -> impl Bundle {
    let node = Node {
        width: px(crate::ui::GEM),
        height: px(crate::ui::GEM),
        ..default()
    };
    crate::ui::side_mark(gem, player.rgb, node)
}

/// Shows the preset, and the others the map can be played as while picking.
fn show_presets(mut commands: Commands, lobby: Res<Lobby>, presets: Single<Entity, With<Presets>>) {
    let presets = *presets;
    commands.entity(presets).despawn_children();
    let row = Node {
        justify_content: JustifyContent::Start,
        ..padded()
    };
    commands
        .spawn((
            ChildOf(presets),
            opener(row.clone()),
            called("lobby.preset"),
            children![text(lobby.preset.label(), Font::Body)],
        ))
        .observe(open_presets);
    let picking = lobby.open == Open::Presets;
    let Some(brief) = lobby.brief.as_ref().ok().filter(|_| picking) else {
        return;
    };
    let list = commands.spawn((ChildOf(presets), dropdown())).id();
    for preset in Preset::offered(&brief.players) {
        commands
            .spawn((
                ChildOf(list),
                clickable(row.clone()),
                called(format!("lobby.preset.{}", slug(&preset.label()))),
                PresetChoice(preset),
                children![text(preset.label(), Font::Body)],
            ))
            .observe(choose_preset);
    }
}

/// Lists the sides by team, each with who plays it, and a place to watch
/// from.
#[allow(clippy::too_many_arguments)]
fn show_slots(
    mut commands: Commands,
    lobby: Res<Lobby>,
    all: Res<Nations>,
    flags: Res<Flags>,
    handicaps: Res<Handicaps>,
    sais: Res<Sais>,
    gem: Res<crate::ui::Gem>,
    slots: Single<Entity, With<Slots>>,
    staged: Option<Res<Staged>>,
    look: Res<crate::connection::Look>,
) {
    let slots = *slots;
    let staging = staged.as_ref().map(|s| (s.entry.staging(), s.entry.me()));
    let hosting = staging.is_some_and(|(s, me)| s.guest(me).is_some_and(|g| g.host));
    commands.entity(slots).despawn_children();
    let brief = match &lobby.brief {
        Ok(b) => b,
        Err(e) => {
            commands.spawn((ChildOf(slots), text(e.clone(), Font::Body)));
            return;
        }
    };
    let sides: Vec<&Player> = brief.sides().collect();
    let number = |id: u32| sides.iter().position(|p| p.id == id);
    // The sides a person may move to.
    let movable = |id: u32| match staging {
        Some((s, _)) => s.sides.contains(&id),
        None => number(id).is_some_and(|i| sides[i].playable),
    };
    for (heading, members) in lobby.teams() {
        let header = commands
            .spawn((
                ChildOf(slots),
                Node {
                    column_gap: px(SLOT_GAP),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![text(heading.clone(), Font::Heading)],
            ))
            .id();
        // Joining a team takes its first side nobody holds.
        let joined = lobby.person.is_some_and(|p| members.contains(&p));
        let free = members.iter().find(|&&p| lobby.empty(p) && movable(p));
        commands
            .entity(header)
            .insert((
                TeamRow {
                    members: members.clone(),
                    free: free.copied(),
                },
                called(format!("lobby.team.{}", slug(&heading))),
            ))
            .observe(drop_slot);
        match free.filter(|_| !joined) {
            Some(&free) => {
                commands
                    .spawn((
                        ChildOf(header),
                        button("Join Team", Font::Body),
                        called(format!("lobby.join.{}", slug(&heading))),
                        Hint("Switch to this team".into()),
                        SideChoice(Some(free)),
                    ))
                    .observe(choose_side);
            }
            None => _ = commands.spawn((ChildOf(header), disabled("Join Team", Font::Body))),
        }
        for id in members {
            let Some(i) = number(id) else {
                continue;
            };
            let player = sides[i];
            let side = format!("lobby.side{}", i + 1);
            let holder = staging.and_then(|(s, _)| s.guests.iter().find(|g| g.side == Some(id)));
            let computer = holder.is_none() && lobby.computer(id);
            let empty = lobby.empty(id);
            let profile = lobby.profile(&sais, id);
            let cell = |width: f32| Node {
                width: px(width),
                justify_content: JustifyContent::Start,
                ..padded()
            };
            let row = commands
                .spawn((
                    ChildOf(slots),
                    Node {
                        column_gap: px(SLOT_GAP),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![side_mark(&gem, player)],
                    SlotRow {
                        id,
                        movable: movable(id),
                    },
                    called(side.clone()),
                ))
                .observe(drop_slot)
                .id();
            // Anyone may put a computer player on an open side, and
            // changes their own; others' are shown with whose machine
            // they belong to.
            let who = match (holder, lobby.theirs.get(&id)) {
                (Some(guest), _) => Some(guest.name.clone()),
                (None, _) if !computer && !empty => Some("Player".into()),
                (None, Some(owner)) => {
                    let name = profile.map_or("Random", |s| s.name.as_str());
                    Some(format!("{name} ({owner})"))
                }
                (None, None) => None,
            };
            match who {
                Some(who) => {
                    commands.spawn((
                        ChildOf(row),
                        inert(cell(WHO_WIDTH)),
                        children![text(who, Font::Body)],
                    ));
                }
                None => computer_list(&mut commands, row, &lobby, (player, &side), &sais),
            }
            // Nobody plays a side left open, so nothing is chosen for it:
            // its other buttons line up with the rest.
            if empty {
                let gaps = 2.0 * SLOT_GAP;
                let width = NATION_WIDTH + FACTION_WIDTH + HANDICAP_WIDTH + gaps;
                let spacer = Node {
                    width: px(width),
                    ..default()
                };
                commands.spawn((ChildOf(row), spacer));
            } else {
                let nation =
                    (player.nation.as_deref()).or(lobby.slots.nations.get(&id).map(Id::as_str));
                // The game shows a side's nation by its flag alone, naming it
                // only in the list to choose from.
                let flag = Shown {
                    icon: flags.of(nation),
                    words: String::new(),
                    hint: Some(nation.map_or("Random".into(), |n| name_of(&all.0, n))),
                    ink: None,
                };
                // Only the machine a computer player belongs to marks what its
                // profile doesn't list.
                let owned = profile.filter(|_| lobby.mine(id));
                let listing = (id, side.as_str(), owned);
                // Only what's the viewer's to choose opens a list.
                let ours = staging.is_none() || lobby.person == Some(id) || lobby.mine(id);
                match (&player.nation, ours) {
                    (Some(_), _) | (None, false) => {
                        let fixed = commands
                            .spawn((ChildOf(row), inert(cell(NATION_WIDTH))))
                            .id();
                        flag.spawn(&mut commands, fixed);
                    }
                    (None, true) => {
                        nation_list(&mut commands, row, &lobby, listing, flag, &all.0);
                    }
                }
                // A random nation's faction is random too.
                let nation = nation.and_then(|n| all.0.iter().find(|a| a.id.as_str() == n));
                let fixed = player.faction.as_deref().map(Id::from);
                let fixed = fixed.or_else(|| {
                    let chosen = lobby.slots.factions.get(&id).cloned();
                    (!ours).then_some(chosen).flatten()
                });
                let fixed = match fixed {
                    Some(f) => Some(nation.and_then(|n| n.factions.iter().find(|g| g.id == f))),
                    None if !ours => Some(None),
                    None => None,
                };
                match (fixed, nation) {
                    (None, Some(nation)) => {
                        faction_list(&mut commands, row, &lobby, listing, &nation.factions);
                    }
                    (fixed, _) => {
                        let name = fixed
                            .flatten()
                            .map_or("Random".into(), |g| g.short_name.clone());
                        commands.spawn((
                            ChildOf(row),
                            inert(cell(FACTION_WIDTH)),
                            children![label(name, Font::Body, Ink::Muted)],
                        ));
                    }
                }
            }
            if let Some(guest) = holder {
                crate::staging::marks(&mut commands, row, guest, hosting, &look);
                if let Some((staging, me)) = staging
                    && guest.client != me
                {
                    whisper(&mut commands, row, guest, &format!("{side}.whisper"));
                    offers(&mut commands, row, staging, me, guest, &side);
                }
            } else if computer {
                let mine = (player, side.as_str(), lobby.mine(id));
                handicap_list(&mut commands, row, &lobby, mine, &handicaps);
            }
            // Whoever runs the host puts out other machines' computer
            // players.
            if hosting && lobby.theirs.contains_key(&id) {
                commands
                    .spawn((
                        ChildOf(row),
                        button("Kick", Font::Body),
                        called(format!("{side}.kick")),
                    ))
                    .observe(move |_: On<Activate>, mut staged: ResMut<Staged>| {
                        let _ = staged.entry.choose(tgserver::Choice::Open(id));
                    });
            }
            // The person stops playing to watch, where watching is allowed.
            if lobby.person == Some(id) && staging.is_none_or(|(s, _)| s.observers) {
                commands
                    .spawn((
                        ChildOf(row),
                        button("Observer", Font::Body),
                        called(format!("{side}.observer")),
                        Hint("Switch to observer".into()),
                        SideChoice(None),
                    ))
                    .observe(choose_side);
            }
            // The person can move to a side no other person takes,
            // trading places with their own computer player.
            if (lobby.mine(id) || empty) && movable(id) {
                let hint = match empty {
                    true => "Switch to this slot",
                    false => "Swap slots with AI player",
                };
                commands
                    .spawn((
                        ChildOf(row),
                        button("Swap", Font::Body),
                        called(format!("{side}.seat")),
                        Hint(hint.into()),
                        SideChoice(Some(id)),
                    ))
                    .observe(choose_side);
            }
        }
    }
    // Those watching, under a header of their own whose Join Team takes
    // the viewer off their side.
    let header = commands
        .spawn((
            ChildOf(slots),
            Node {
                column_gap: px(SLOT_GAP),
                align_items: AlignItems::Center,
                margin: UiRect::top(px(8)),
                ..default()
            },
            children![text("Observers", Font::Heading)],
        ))
        .id();
    match lobby.person {
        Some(_) => {
            commands
                .spawn((
                    ChildOf(header),
                    button("Join Team", Font::Body),
                    called("lobby.join.observers"),
                    Hint("Switch to this team".into()),
                    SideChoice(None),
                ))
                .observe(choose_side);
        }
        None => _ = commands.spawn((ChildOf(header), disabled("Join Team", Font::Body))),
    }
    let row = |commands: &mut Commands, name: String| {
        let who = Node {
            width: px(WHO_WIDTH),
            justify_content: JustifyContent::Start,
            ..padded()
        };
        commands
            .spawn((
                ChildOf(slots),
                Node {
                    column_gap: px(SLOT_GAP),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![(inert(who), children![text(name, Font::Body)])],
            ))
            .id()
    };
    match staging {
        Some((s, _)) => {
            for guest in s.guests.iter().filter(|g| g.side.is_none()) {
                let row = row(&mut commands, guest.name.clone());
                crate::staging::marks(&mut commands, row, guest, hosting, &look);
                if staging.is_some_and(|(_, me)| guest.client != me) {
                    let id = format!("lobby.observers.{}.whisper", slug(&guest.name));
                    whisper(&mut commands, row, guest, &id);
                }
            }
        }
        // Played alone, the one person.
        None if lobby.person.is_none() => _ = row(&mut commands, "Player".into()),
        None => {}
    }
}

/// The button in `row` that has the next line typed go to `guest` alone.
fn whisper(commands: &mut Commands, row: Entity, guest: &tgserver::Guest, id: &str) {
    commands
        .spawn((
            ChildOf(row),
            button("Whisper", Font::Body),
            called(id),
            Hint("Whisper to player".into()),
            crate::staging::WhisperTo(guest.client),
        ))
        .observe(crate::staging::whisper_to);
}

/// The mark and buttons for offers to trade sides between the person at
/// `me` and `guest`, on the slot `side` of `guest`'s `row`. Only someone
/// on a side may offer.
fn offers(
    commands: &mut Commands,
    row: Entity,
    staging: &tgserver::Staging,
    me: u32,
    guest: &tgserver::Guest,
    side: &str,
) {
    let them = guest.client;
    if staging.offered(me, them) {
        let pending = label("Swap Pending", Font::Body, Ink::Muted);
        commands.spawn((
            ChildOf(row),
            pending,
            Hint("Slot Swap Offer Pending".into()),
        ));
    }
    if staging.guest(me).is_none_or(|g| g.side.is_none()) {
        return;
    }
    let mut buttons = Vec::new();
    if staging.offers.contains(&(them, me)) {
        buttons.push((
            "Accept",
            "accept",
            "Accept offer to swap slots",
            tgserver::Choice::Answer(them, true),
        ));
        buttons.push((
            "Reject",
            "reject",
            "Reject offer to swap slots",
            tgserver::Choice::Answer(them, false),
        ));
    }
    buttons.push(match staging.offers.contains(&(me, them)) {
        true => (
            "Cancel",
            "cancel",
            "Cancel offer to swap slots",
            tgserver::Choice::Offer(them, false),
        ),
        false => (
            "Swap",
            "offer",
            "Offer to swap slots with player",
            tgserver::Choice::Offer(them, true),
        ),
    });
    for (words, name, hint, choice) in buttons {
        commands
            .spawn((
                ChildOf(row),
                button(words, Font::Body),
                called(format!("{side}.{name}")),
                Hint(hint.into()),
            ))
            .observe(move |_: On<Activate>, mut staged: ResMut<Staged>| {
                let _ = staged.entry.choose(choice.clone());
            });
    }
}

/// What a choice of a nation, a faction or a handicap is called after its
/// list: its id in the data, or `random` for none.
fn key(id: Option<&Id>) -> String {
    id.map_or("random".into(), |id| slug(id.as_str()))
}

/// How wide a side's nation is shown: its flag, padded.
const NATION_WIDTH: f32 = 60.0;

/// How wide who plays a side is shown.
const WHO_WIDTH: f32 = 180.0;

/// How wide a side's faction is shown.
const FACTION_WIDTH: f32 = 160.0;

/// How wide a side's handicap is shown: its icon, padded.
const HANDICAP_WIDTH: f32 = 64.0;

/// The room between what's shown of a side.
const SLOT_GAP: f32 = 12.0;

/// What a nation is called, by id.
fn name_of(nations: &[Nation], id: &str) -> String {
    let nation = nations.iter().find(|n| n.id.as_str() == id);
    nation.map_or_else(|| id.to_string(), |n| n.name.clone())
}

/// A side's nation in `row`, `label` saying which, and the list to choose
/// another from while it's open.
fn nation_list(
    commands: &mut Commands,
    row: Entity,
    lobby: &Lobby,
    (player, side, profile): (u32, &str, Option<&Sai>),
    label: Shown,
    nations: &[Nation],
) {
    let listed = profile.map(|s| s.nations.as_slice());
    let choices = nations.iter().map(|n| (Some(n.id.clone()), n.name.clone()));
    let choices = std::iter::once((None, "Random".to_string())).chain(choices);
    let all: Vec<Id> = nations.iter().map(|n| n.id.clone()).collect();
    let choices = choices.map(|(n, name)| {
        let shown = unlisted(listed, &all, n.as_ref(), name);
        (key(n.as_ref()), NationChoice(player, n), shown)
    });
    let open = lobby.open == Open::Nations(player);
    let list = Dropdown {
        id: format!("{side}.nation"),
        row,
        width: px(NATION_WIDTH),
        label,
        open,
    };
    list.spawn(
        commands,
        NationsOf(player),
        open_nations,
        choices,
        choose_nation,
    );
}

/// A nation or faction in its list, `name` in red where its side's
/// computer player's profile `listed` others but not it, and "Random" (a
/// `choice` of none) where it doesn't list every one of `all`; still to be
/// chosen.
fn unlisted(listed: Option<&[Id]>, all: &[Id], choice: Option<&Id>, name: String) -> Shown {
    let has = |id: &Id| listed.is_none_or(|l| l.is_empty() || l.contains(id));
    let red = match choice {
        Some(id) => !has(id),
        None => !all.iter().all(has),
    };
    Shown {
        words: name,
        ink: red.then_some(Ink::Warn),
        ..default()
    }
}

/// What a side left open reads as, saying so where the map requires it.
fn open_label(player: &Player) -> String {
    match player.required {
        true => "Open (Required)".into(),
        false => "Open".into(),
    }
}

/// The profile of a side the computer plays in `row`, and the list to
/// choose another from while it's open.
fn computer_list(
    commands: &mut Commands,
    row: Entity,
    lobby: &Lobby,
    (player, side): (&Player, &str),
    sais: &Sais,
) {
    let id = player.id;
    let label = match lobby.profile(sais, id) {
        _ if lobby.empty(id) => open_label(player),
        Some(sai) => sai.name.clone(),
        None => "Random".into(),
    };
    let offered = (sais.offered()).map(|s| {
        let name = s.name.clone();
        (key(Some(&s.id)), Holder::Computer(Some(s.id.clone())), name)
    });
    let open = ("open".to_string(), Holder::Open, "Open".to_string());
    let random = (key(None), Holder::Computer(None), "Random".to_string());
    let choices = [open, random].into_iter().chain(offered);
    let choices = choices.map(|(key, holder, name)| (key, ComputerChoice(id, holder), name));
    let list = Dropdown {
        id: format!("{side}.computer"),
        row,
        width: px(WHO_WIDTH),
        label: label.into(),
        open: lobby.open == Open::Computers(id),
    };
    list.spawn(
        commands,
        ComputersOf(id),
        open_computers,
        choices,
        choose_computer,
    );
}

/// The handicap of a side the computer plays in `row`, and the list to
/// choose another from while it's open.
fn handicap_list(
    commands: &mut Commands,
    row: Entity,
    lobby: &Lobby,
    (player, side, mine): (&Player, &str, bool),
    handicaps: &Handicaps,
) {
    let id = player.id;
    let had = handicaps.of(lobby.slots.handicaps.get(&id), player.handicap.as_ref());
    let Some(had) = had else {
        return;
    };
    // Another machine's computer player's, shown but not chosen here.
    if !mine {
        let cell = Node {
            width: px(HANDICAP_WIDTH),
            justify_content: JustifyContent::Start,
            ..padded()
        };
        let shown = commands.spawn((ChildOf(row), inert(cell))).id();
        handicaps.shown(had).spawn(commands, shown);
        return;
    }
    let choices = (handicaps.all.iter()).map(|h| {
        let choice = HandicapChoice(id, h.id.clone());
        (key(Some(&h.id)), choice, handicaps.shown(h))
    });
    let list = Dropdown {
        id: format!("{side}.handicap"),
        row,
        width: px(HANDICAP_WIDTH),
        label: handicaps.shown(had),
        open: lobby.open == Open::Handicaps(id),
    };
    list.spawn(
        commands,
        HandicapsOf(id),
        open_handicaps,
        choices,
        choose_handicap,
    );
}

/// A side's faction in `row`, and the list of its nation's `factions` the
/// lobby offers to choose another from while it's open.
fn faction_list(
    commands: &mut Commands,
    row: Entity,
    lobby: &Lobby,
    (player, side, profile): (u32, &str, Option<&Sai>),
    factions: &[Faction],
) {
    let listed = profile.map(|s| s.factions.as_slice());
    let chosen = lobby.slots.factions.get(&player);
    let chosen = chosen.and_then(|id| factions.iter().find(|g| g.id == *id));
    let label = chosen.map_or("Random".into(), |g| g.short_name.clone());
    let mut offered: Vec<_> = factions.iter().filter(|g| g.public).collect();
    offered.sort_by_cached_key(|g| g.short_name.to_lowercase());
    let all: Vec<Id> = offered.iter().map(|g| g.id.clone()).collect();
    let choices = offered
        .into_iter()
        .map(|g| (Some(g.id.clone()), g.short_name.clone()));
    let choices = std::iter::once((None, "Random".to_string())).chain(choices);
    let choices = choices.map(|(g, name)| {
        let shown = unlisted(listed, &all, g.as_ref(), name);
        (key(g.as_ref()), FactionChoice(player, g), shown)
    });
    let open = lobby.open == Open::Factions(player);
    let list = Dropdown {
        id: format!("{side}.general"),
        row,
        width: px(FACTION_WIDTH),
        label: label.into(),
        open,
    };
    list.spawn(
        commands,
        FactionsOf(player),
        open_factions,
        choices,
        choose_faction,
    );
}

/// Describes the chosen map: its name, size and sides, and the time of
/// day it's played at.
fn show_about(
    mut commands: Commands,
    lobby: Res<Lobby>,
    gem: Res<crate::ui::Gem>,
    about: Single<Entity, With<About>>,
    rules: Option<Res<crate::scenario::WorldRules>>,
) {
    let about = *about;
    commands.entity(about).despawn_children();
    let Ok(brief) = &lobby.brief else {
        commands.spawn((
            ChildOf(about),
            text(tgdata::map_title(&lobby.map), Font::Large),
        ));
        return;
    };
    commands.spawn((ChildOf(about), text(brief.name.clone(), Font::Large)));
    if let Some([w, h]) = brief.size {
        commands.spawn((ChildOf(about), text(format!("{w}x{h}"), Font::Body)));
    }
    if brief.sides().next().is_none() {
        commands.spawn((
            ChildOf(about),
            label("No sides to play", Font::Body, Ink::Muted),
        ));
    }
    let sides = brief.sides().count();
    if sides > 0 {
        let marks = commands
            .spawn((
                ChildOf(about),
                Node {
                    column_gap: px(8),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![text(format!("{sides} sides"), Font::Body)],
            ))
            .id();
        for player in brief.sides() {
            commands.spawn((ChildOf(marks), side_mark(&gem, player)));
        }
    }
    let shown = rules.map(|r| r.shown(&lobby.rules)).unwrap_or_default();
    for (i, (rule, chosen)) in shown.into_iter().enumerate() {
        let gap = if i == 0 { 8.0 } else { 0.0 };
        commands.spawn((
            ChildOf(about),
            Node {
                margin: UiRect::top(px(gap)),
                ..default()
            },
            children![text(format!("{rule}: {chosen}"), Font::Body)],
        ));
    }
}

/// A button and what it chooses.
type Choice<'a> = (
    &'a mut Chosen,
    Option<&'a SideChoice>,
    Option<&'a PresetChoice>,
    Option<&'a NationChoice>,
);

/// Those [`mark_chosen`] marks, leaving others' buttons to them.
type Choosers = Or<(With<SideChoice>, With<PresetChoice>, With<NationChoice>)>;

fn mark_chosen(lobby: Res<Lobby>, mut buttons: Query<Choice, Choosers>) {
    for (mut chosen, side, preset, nation) in &mut buttons {
        let now = side.is_some_and(|s| s.0 == lobby.person)
            || preset.is_some_and(|p| p.0 == lobby.preset)
            || nation.is_some_and(|NationChoice(p, n)| lobby.slots.nations.get(p) == n.as_ref());
        chosen.set_if_neq(Chosen(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::state::app::StatesPlugin;
    use tgmap::Relation;

    /// A lobby with only its Exit button, which `answer` is pressed on
    /// once its question is up.
    fn exit_answering(answer: &str) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin, confirm::ConfirmPlugin))
            .insert_state(Screen::Lobby)
            .add_sub_state::<MenuPage>()
            .insert_resource(confirm::Words::default())
            .insert_resource(Leaving::of(&tgdata::Strings::default()))
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<crate::console::Console>();
        app.world_mut()
            .spawn(button("Exit", Font::Large))
            .observe(ask_exit);
        app.update();
        press(&mut app, "Exit");
        let world = app.world_mut();
        let mut texts = world.query::<&Text>();
        assert!(texts.iter(world).any(|t| t.0 == "Exit Staging?"));
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
        press(&mut app, answer);
        app.update();
        app
    }

    fn press(app: &mut App, words: &str) {
        let world = app.world_mut();
        let mut labels = world.query::<(&Text, &ChildOf)>();
        let found = labels.iter(world).find(|(text, _)| text.0 == words);
        let entity = found.expect("no such button").1.parent();
        world.trigger(Activate { entity });
        app.update();
    }

    #[test]
    fn exit_asks_first_and_ok_leaves() {
        let app = exit_answering("Ok");
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::MainMenu
        );
        assert_eq!(
            *app.world().resource::<State<MenuPage>>().get(),
            MenuPage::NewGame
        );
    }

    #[test]
    fn exit_cancelled_stays() {
        let app = exit_answering("Cancel");
        assert_eq!(
            *app.world().resource::<State<Screen>>().get(),
            Screen::Lobby
        );
    }

    fn player(id: u32, independent: bool, playable: bool) -> Player {
        Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [0.0, 0.0, 1.0],
            team: None,
            independent,
            playable,
            required: false,
            ai_profile: None,
            default_defeat: true,
            outcome: Default::default(),
            won_by_cities: false,
            resigned: false,
            nation: None,
            faction: None,
            relations: vec![Relation::Neutral; 4],
            team_relations: Vec::new(),
            stockpile: 0.0,
            default_stockpile: false,
            stockpile_bonus: 0.0,
            technologies: Vec::new(),
            handicap: None,
            barred: Vec::new(),
        }
    }

    fn players() -> Vec<Player> {
        vec![
            player(10, false, false),
            player(11, true, false),
            player(12, false, true),
            player(13, false, true),
        ]
    }

    #[test]
    fn computers_play_every_side_but_the_persons() {
        use Control::*;
        let lineup = Lineup::new(&players(), Some(12));
        assert_eq!(lineup.0, vec![(10, Computer), (12, Person), (13, Computer)]);
        assert_eq!(lineup.person(), Some(12));
        let watching = Lineup::new(&players(), None);
        assert_eq!(
            watching.0,
            vec![(10, Computer), (12, Computer), (13, Computer)]
        );
        assert_eq!(watching.person(), None);
    }

    #[test]
    fn sides_start_with_the_chosen_handicap_else_the_maps() {
        let handicap = |id: &str, property: Option<&str>| Handicap {
            id: Id::from(id),
            glyph: None,
            name: None,
            description: String::new(),
            property: property.map(Id::from),
            extra: Default::default(),
        };
        let handicaps = Handicaps::new(vec![
            handicap("easy", Some("penalty")),
            handicap("medium", None),
            handicap("hard", Some("bonus")),
        ]);
        let (easy, hard) = (Id::from("easy"), Id::from("hard"));
        let shown = |chosen, map| handicaps.of(chosen, map).map(|h| h.id.as_str());
        assert_eq!(shown(None, None), Some("medium"));
        assert_eq!(shown(None, Some(&easy)), Some("easy"));
        assert_eq!(shown(Some(&hard), Some(&easy)), Some("hard"));

        let mut players = players();
        players[1].handicap = Some(hard.clone());
        players[3].handicap = Some(easy.clone());
        let chosen = BTreeMap::from([(10, hard.clone()), (12, easy.clone())]);
        // The lobby offers the person's side none.
        let chosen = computer_handicaps(&players, Some(12), &chosen);
        assert_eq!(chosen, BTreeMap::from([(10, hard.clone())]));
        let given: HashMap<u32, Id> = (tgserver::side_handicaps(&players, &chosen, &handicaps.all)
            .into_iter())
        .filter_map(|(p, h)| Some((p, h.property?)))
        .collect();
        // The independent player gets none, even from the map.
        let bonus_and_penalty = [(10, Id::from("bonus")), (13, Id::from("penalty"))];
        assert_eq!(given, HashMap::from(bonus_and_penalty));
    }

    fn sai(id: &str, nation: &str, faction: &str, public: bool) -> Sai {
        Sai {
            id: Id::from(id),
            name: format!("{faction} AI"),
            nations: vec![Id::from(nation)],
            factions: vec![Id::from(faction)],
            public,
            easiest: false,
        }
    }

    #[test]
    fn the_lobby_offers_the_public_profiles() {
        let sais = Sais(vec![
            sai("chuikov_sai", "nation_rus", "chuikov", true),
            sai("rus_sai", "nation_rus", "zhukov", false),
        ]);
        let offered: Vec<&str> = sais.offered().map(|s| s.id.as_str()).collect();
        assert_eq!(offered, ["chuikov_sai"]);
    }

    #[test]
    fn a_profiles_lists_mark_in_red_what_it_does_not_list() {
        let (rus, usa) = (Id::from("nation_rus"), Id::from("nation_usa"));
        let all = [rus.clone(), usa.clone()];
        let red = |listed: Option<&[Id]>, choice: Option<&Id>| {
            unlisted(listed, &all, choice, String::new()).ink == Some(Ink::Warn)
        };
        let russian = [rus.clone()];
        assert!(!red(Some(&russian), Some(&rus)));
        assert!(red(Some(&russian), Some(&usa)));
        assert!(red(Some(&russian), None));
        // Listing every one, none, or with no profile, nothing is.
        assert!(!red(Some(&all), None));
        assert!(!red(Some(&[]), Some(&usa)));
        assert!(!red(None, None));
    }

    #[test]
    fn a_dropped_slot_moves_the_person_or_the_hosts_own_computer_player() {
        // The person on 12, this machine's computer player on 10, another
        // machine's on 11, and 13 open.
        let lobby = Lobby {
            theirs: BTreeMap::from([(11, "Bob".into())]),
            slots: tgserver::Slots {
                open: [13].into(),
                ..Default::default()
            },
            ..field_lobby()
        };
        // The person onto an open side or their machine's computer player.
        assert_eq!(lobby.dropped(12, 13, false), Some(Moved::Seat(13)));
        assert_eq!(lobby.dropped(12, 10, false), Some(Moved::Seat(10)));
        assert_eq!(lobby.dropped(12, 11, true), None);
        // Only the host moves its computer players, onto an open side or
        // the person's, never another machine's.
        assert_eq!(lobby.dropped(10, 13, true), Some(Moved::Shift(10, 13)));
        assert_eq!(lobby.dropped(10, 12, true), Some(Moved::Seat(10)));
        assert_eq!(lobby.dropped(10, 13, false), None);
        assert_eq!(lobby.dropped(10, 11, true), None);
        assert_eq!(lobby.dropped(11, 13, true), None);
        assert_eq!(lobby.dropped(12, 12, true), None);
    }

    #[test]
    fn team_chat_reaches_the_speakers_team_or_those_watching() {
        // 10 and 12 on the map's first team, 13 on its second, 11 on none.
        let mut players = players();
        players[0].team = Some(0);
        players[2].team = Some(0);
        players[3].team = Some(1);
        let mut lobby = field_lobby();
        if let Ok(brief) = &mut lobby.brief {
            brief.players = players;
        }
        assert!(lobby.hears_team(Some(10), Some(12)));
        assert!(!lobby.hears_team(Some(12), Some(13)));
        assert!(lobby.hears_team(None, None));
        assert!(!lobby.hears_team(None, Some(12)) && !lobby.hears_team(Some(12), None));
        // On no team, not even themselves.
        assert!(!lobby.hears_team(Some(11), Some(11)));
        // Each on their own in a free for all.
        lobby.preset = Preset::FreeForAll(2);
        assert!(!lobby.hears_team(Some(10), Some(12)));
    }

    /// A lobby for a map called Field with [`players`], the person on 12.
    fn field_lobby() -> Lobby {
        Lobby {
            map: "field".into(),
            brief: Ok(Brief {
                name: "Field".into(),
                description: String::new(),
                size: None,
                players: players(),
                teams: Vec::new(),
                rules: Default::default(),
                local: None,
            }),
            preset: Preset::Custom,
            person: Some(12),
            others: Vec::new(),
            theirs: BTreeMap::new(),
            slots: tgserver::Slots::default(),
            rules: Default::default(),
            open: Open::Nothing,
        }
    }

    #[test]
    fn launch_lists_every_side_the_computer_plays_with_its_chosen_profile() {
        let lobby = Lobby {
            slots: tgserver::Slots {
                computers: BTreeMap::from([
                    (10, Id::from("konev_sai")),
                    (12, Id::from("chuikov_sai")),
                ]),
                open: [11, 13].into(),
                ..Default::default()
            },
            ..field_lobby()
        };
        let (setup, _, _) = lobby.setup(1).unwrap();
        // The person's side has none, nor the one left open, nor the
        // independent one, open or not.
        let konev = Some(Id::from("konev_sai"));
        assert_eq!(setup.computers, BTreeMap::from([(10, konev)]));
        assert_eq!(setup.open, [13].into());
    }

    #[test]
    fn seating_moves_the_person() {
        use Control::*;
        let mut lineup = Lineup::new(&players(), Some(12));
        lineup.seat(Some(13));
        assert_eq!(lineup.0, vec![(10, Computer), (12, Computer), (13, Person)]);
        // An independent player has no seat, so the person watches.
        lineup.seat(Some(11));
        assert_eq!(lineup.person(), None);
    }

    #[test]
    fn plays_the_first_playable_side_unless_asked() {
        assert_eq!(default_person(&players(), Side(None)), Some(12));
        assert_eq!(default_person(&players(), Side(Some(3))), Some(13));
        assert_eq!(default_person(&players(), Side(Some(4))), None);
        let unplayable = vec![player(10, false, false)];
        assert_eq!(default_person(&unplayable, Side(None)), None);
    }

    #[test]
    fn with_no_lobby_computers_take_every_side_but_the_persons() {
        let sides = computer_sides(&players(), Some(12));
        assert_eq!(sides, [(10, None), (13, None)].into());
        // Watching, every side.
        assert_eq!(computer_sides(&players(), None).len(), 3);
    }
}
