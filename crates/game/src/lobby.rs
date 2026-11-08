//! The lobby of a custom game: pick a map, how its sides are teamed up, and
//! the side you play on it, or watch, and the computer plays every other
//! side.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};

use aaa_data::{Id, Nation};
use aaa_map::{Brief, Player};
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea};

use crate::menu::go;
use crate::presets::Preset;
use crate::ui::{
    Chosen, Font, Ink, Surface, button, clickable, dropdown, inert, label, opener, padded, panel,
    text,
};
use crate::{Data, Pending, Screen, Side};

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
            aaa_map::sides(players)
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

/// The player a person plays unless they choose another: the side asked for
/// on the command line, else the first a person may play.
pub fn default_person(players: &[Player], asked: Side) -> Option<u32> {
    let mut sides = aaa_map::sides(players);
    let side = match asked.0 {
        Some(n) => sides.nth(n - 1),
        None => sides.find(|p| p.playable),
    };
    side.map(|p| p.id)
}

/// The nations sides can play as.
#[derive(Resource)]
pub struct Nations(pub Vec<Nation>);

/// The nation each side plays as, by player id: the one the map fixes, else
/// the one `chosen`, else one at random.
pub fn nations(
    players: &[Player],
    chosen: &HashMap<u32, Id>,
    nations: &[Nation],
) -> HashMap<u32, Nation> {
    let random = RandomState::new();
    let find = |id: &str| nations.iter().find(|n| n.id.as_str() == id);
    aaa_map::sides(players)
        .filter_map(|p| {
            let nation = match (&p.nation, chosen.get(&p.id)) {
                (Some(fixed), _) => find(fixed),
                (None, Some(id)) => find(id.as_str()),
                (None, None) if !nations.is_empty() => {
                    let roll = random.hash_one(p.id) as usize;
                    nations.get(roll % nations.len())
                }
                (None, None) => None,
            };
            Some((p.id, nation?.clone()))
        })
        .collect()
}

/// Which of the lobby's lists is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    Nothing,
    Presets,
    /// The nations for this player's side.
    Nations(u32),
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
    /// The nation chosen for each side, by player id; one at random where
    /// none is.
    nations: HashMap<u32, Id>,
    open: Open,
}

impl Lobby {
    fn new(data: &mut Data, map: String, asked: Side) -> Lobby {
        let brief = Brief::load(&mut data.0, &map);
        let (preset, person) = match &brief {
            Ok(b) => (
                Preset::made_for(&b.players),
                default_person(&b.players, asked),
            ),
            Err(_) => (Preset::Custom, None),
        };
        let mut lobby = Lobby {
            map,
            brief,
            preset,
            person,
            nations: HashMap::new(),
            open: Open::Nothing,
        };
        lobby.keep_person_playing();
        lobby
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

    /// Moves the person to the first side they may play if theirs has left
    /// the match.
    fn keep_person_playing(&mut self) {
        let Ok(brief) = &self.brief else {
            return;
        };
        let playing: Vec<u32> = self.teams().into_iter().flat_map(|(_, t)| t).collect();
        if self.person.is_some_and(|p| !playing.contains(&p)) {
            self.person = brief
                .sides()
                .find(|p| p.playable && playing.contains(&p.id))
                .map(|p| p.id);
        }
    }
}

/// A map in the list.
#[derive(Component)]
struct MapChoice(String);

/// A player to play, or none to watch.
#[derive(Component)]
struct SideChoice(Option<u32>);

/// The button that opens a side's nations, by player id.
#[derive(Component)]
struct NationsOf(u32);

/// One of the nations in a side's list, by player id; none for one at
/// random.
#[derive(Component)]
struct NationChoice(u32, Option<Id>);

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
        app.add_systems(OnEnter(Screen::Lobby), (enter, spawn).chain())
            .add_systems(
                Update,
                (show_presets, show_slots, show_about, mark_chosen)
                    .chain()
                    .run_if(in_state(Screen::Lobby))
                    .run_if(resource_exists_and_changed::<Lobby>),
            );
    }
}

/// Picks the first of the scenarios, unless the lobby has been here before.
fn enter(
    mut commands: Commands,
    mut data: ResMut<Data>,
    asked: Res<Side>,
    lobby: Option<ResMut<Lobby>>,
) {
    if let Some(mut lobby) = lobby {
        lobby.open = Open::Nothing;
        return;
    }
    let Some(map) = scenarios(&data).into_iter().next() else {
        return;
    };
    commands.insert_resource(Lobby::new(&mut data, map, *asked));
}

/// The maps a custom game is played on, by archive path: those in the maps
/// folder itself, not the campaign's or the developers' below it.
fn scenarios(data: &Data) -> Vec<String> {
    let names = data.0.map_names().into_iter();
    let top = names.filter(|n| {
        n.rsplit_once('/')
            .is_some_and(|(f, _)| f.eq_ignore_ascii_case("Maps"))
    });
    top.map(str::to_string).collect()
}

/// Lays out the lobby as the game does: the preset over the sides on the
/// left, the map and the buttons on the right.
fn spawn(mut commands: Commands, mut data: ResMut<Data>) {
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
        GlobalZIndex(1),
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
    let list = commands
        .spawn((
            ChildOf(right),
            ScrollArea,
            panel(
                Surface::Panel,
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    min_height: px(0),
                    padding: UiRect::all(px(12)),
                    row_gap: px(2),
                    ..default()
                },
            ),
        ))
        .id();
    commands.spawn((ChildOf(list), text("Scenario Maps", Font::Body)));
    for name in scenarios(&data) {
        let label = Brief::load(&mut data.0, &name)
            .map_or_else(|_| aaa_data::map_title(&name).to_string(), |b| b.name);
        commands
            .spawn((ChildOf(list), button(label, Font::Body), MapChoice(name)))
            .observe(choose_map);
    }
    commands
        .spawn((ChildOf(right), button("Launch", Font::Large)))
        .observe(launch);
    commands
        .spawn((ChildOf(right), button("Back", Font::Large)))
        .observe(go(Screen::NewGame));
}

fn choose_map(
    activate: On<Activate>,
    choices: Query<&MapChoice>,
    mut data: ResMut<Data>,
    asked: Res<Side>,
    mut commands: Commands,
) {
    if let Ok(MapChoice(name)) = choices.get(activate.entity) {
        commands.insert_resource(Lobby::new(&mut data, name.clone(), *asked));
    }
}

fn choose_side(activate: On<Activate>, choices: Query<&SideChoice>, mut lobby: ResMut<Lobby>) {
    if let Ok(SideChoice(person)) = choices.get(activate.entity) {
        lobby.person = *person;
        lobby.open = Open::Nothing;
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

fn choose_nation(activate: On<Activate>, choices: Query<&NationChoice>, mut lobby: ResMut<Lobby>) {
    if let Ok(NationChoice(player, nation)) = choices.get(activate.entity) {
        match nation {
            Some(n) => lobby.nations.insert(*player, n.clone()),
            None => lobby.nations.remove(player),
        };
        lobby.open = Open::Nothing;
    }
}

fn choose_preset(activate: On<Activate>, choices: Query<&PresetChoice>, mut lobby: ResMut<Lobby>) {
    if let Ok(PresetChoice(preset)) = choices.get(activate.entity) {
        lobby.preset = *preset;
        lobby.open = Open::Nothing;
        lobby.keep_person_playing();
    }
}

fn launch(
    _: On<Activate>,
    lobby: Option<Res<Lobby>>,
    all: Res<Nations>,
    mut commands: Commands,
    mut next: ResMut<NextState<Screen>>,
) {
    let Some(lobby) = lobby else {
        return;
    };
    let Ok(brief) = &lobby.brief else {
        return;
    };
    commands.insert_resource(Pending {
        name: lobby.map.clone(),
        title: brief.name.clone(),
        description: brief.description.clone(),
        teams: lobby.preset.arrange(&brief.players),
        person: lobby.person,
        nations: nations(&brief.players, &lobby.nations, &all.0),
        from_menu: true,
    });
    next.set(Screen::Loading);
}

/// A side's colour, as the lobby marks it.
fn swatch(player: &Player) -> impl Bundle {
    let [r, g, b] = player.rgb;
    (
        Node {
            width: px(6),
            height: px(20),
            ..default()
        },
        BackgroundColor(Color::srgb(r, g, b)),
    )
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
                PresetChoice(preset),
                children![text(preset.label(), Font::Body)],
            ))
            .observe(choose_preset);
    }
}

/// Lists the sides by team, each with who plays it, and a place to watch
/// from.
fn show_slots(
    mut commands: Commands,
    lobby: Res<Lobby>,
    all: Res<Nations>,
    slots: Single<Entity, With<Slots>>,
) {
    let slots = *slots;
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
    for (heading, members) in lobby.teams() {
        commands.spawn((ChildOf(slots), text(heading, Font::Heading)));
        for id in members {
            let Some(i) = number(id) else {
                continue;
            };
            let player = sides[i];
            let who = match lobby.person == Some(id) {
                true => "Player",
                false => "Computer",
            };
            let cell = |width: f32| Node {
                width: px(width),
                justify_content: JustifyContent::Start,
                ..padded()
            };
            let row = commands
                .spawn((
                    ChildOf(slots),
                    Node {
                        column_gap: px(12),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    children![
                        swatch(player),
                        (
                            text(format!("Side {}", i + 1), Font::Body),
                            Node {
                                width: px(64),
                                ..default()
                            },
                        ),
                    ],
                ))
                .id();
            match player.playable {
                true => commands.spawn((
                    ChildOf(row),
                    clickable(cell(140.0)),
                    SideChoice(Some(id)),
                    children![text(who, Font::Body)],
                )),
                false => commands.spawn((
                    ChildOf(row),
                    inert(cell(140.0)),
                    children![text(who, Font::Body)],
                )),
            }
            .observe(choose_side);
            match &player.nation {
                Some(fixed) => {
                    commands.spawn((
                        ChildOf(row),
                        inert(cell(200.0)),
                        children![label(name_of(&all.0, fixed), Font::Body, Ink::Muted)],
                    ));
                }
                None => {
                    let chosen = lobby.nations.get(&id);
                    let label = chosen.map_or("Random".into(), |n| name_of(&all.0, n.as_str()));
                    nation_list(&mut commands, row, &lobby, id, label, &all.0);
                }
            }
        }
    }
    let label = match lobby.person {
        None => "Observer: Player",
        Some(_) => "Observer",
    };
    let watch = Node {
        margin: UiRect::top(px(8)),
        justify_content: JustifyContent::Start,
        ..padded()
    };
    commands
        .spawn((
            ChildOf(slots),
            clickable(watch),
            SideChoice(None),
            children![text(label, Font::Body)],
        ))
        .observe(choose_side);
}

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
    player: u32,
    label: String,
    nations: &[Nation],
) {
    let cell = Node {
        width: px(200),
        justify_content: JustifyContent::Start,
        ..padded()
    };
    let holder = commands.spawn((ChildOf(row), Node::default())).id();
    commands
        .spawn((
            ChildOf(holder),
            opener(cell.clone()),
            NationsOf(player),
            children![text(label, Font::Body)],
        ))
        .observe(open_nations);
    if lobby.open != Open::Nations(player) {
        return;
    }
    let list = commands
        .spawn((ChildOf(holder), dropdown(), GlobalZIndex(2)))
        .id();
    let random = std::iter::once((None, "Random".to_string()));
    let choices = nations.iter().map(|n| (Some(n.id.clone()), n.name.clone()));
    for (nation, name) in random.chain(choices) {
        commands
            .spawn((
                ChildOf(list),
                clickable(cell.clone()),
                NationChoice(player, nation),
                children![text(name, Font::Body)],
            ))
            .observe(choose_nation);
    }
}

/// Describes the chosen map: its name, size and sides.
fn show_about(mut commands: Commands, lobby: Res<Lobby>, about: Single<Entity, With<About>>) {
    let about = *about;
    commands.entity(about).despawn_children();
    let Ok(brief) = &lobby.brief else {
        commands.spawn((
            ChildOf(about),
            text(aaa_data::map_title(&lobby.map), Font::Large),
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
    for (i, player) in brief.sides().enumerate() {
        commands.spawn((
            ChildOf(about),
            Node {
                column_gap: px(8),
                align_items: AlignItems::Center,
                ..default()
            },
            children![swatch(player), text(format!("Side {}", i + 1), Font::Body)],
        ));
    }
}

/// A button and what it chooses.
type Choice<'a> = (
    &'a mut Chosen,
    Option<&'a MapChoice>,
    Option<&'a SideChoice>,
    Option<&'a PresetChoice>,
    Option<&'a NationChoice>,
);

fn mark_chosen(lobby: Res<Lobby>, mut buttons: Query<Choice>) {
    for (mut chosen, map, side, preset, nation) in &mut buttons {
        let now = map.is_some_and(|m| m.0 == lobby.map)
            || side.is_some_and(|s| s.0 == lobby.person)
            || preset.is_some_and(|p| p.0 == lobby.preset)
            || nation.is_some_and(|NationChoice(p, n)| lobby.nations.get(p) == n.as_ref());
        chosen.set_if_neq(Chosen(now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_map::Relation;

    fn player(id: u32, independent: bool, playable: bool) -> Player {
        Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [0.0, 0.0, 1.0],
            team: None,
            independent,
            playable,
            nation: None,
            relations: vec![Relation::Neutral; 4],
            team_relations: Vec::new(),
            stockpile: 0.0,
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
    fn sides_play_the_fixed_nation_else_the_chosen_else_any() {
        let nation = |id: &str| Nation {
            id: Id::from(id),
            name: id.to_uppercase(),
            placeholders: HashMap::new(),
        };
        let all = [nation("nation_usa"), nation("nation_ger")];
        let mut players = players();
        players[0].nation = Some("nation_ger".into());
        let chosen = HashMap::from([(10, Id::from("nation_usa")), (12, Id::from("nation_usa"))]);
        let nations = nations(&players, &chosen, &all);
        let id = |p| nations[&p].id.as_str();
        assert_eq!(id(10), "nation_ger");
        assert_eq!(id(12), "nation_usa");
        assert!(["nation_usa", "nation_ger"].contains(&id(13)));
        // Independent players play as no nation.
        assert!(!nations.contains_key(&11));
    }

    #[test]
    fn plays_the_first_playable_side_unless_asked() {
        assert_eq!(default_person(&players(), Side(None)), Some(12));
        assert_eq!(default_person(&players(), Side(Some(3))), Some(13));
        assert_eq!(default_person(&players(), Side(Some(4))), None);
        let unplayable = vec![player(10, false, false)];
        assert_eq!(default_person(&unplayable, Side(None)), None);
    }
}
