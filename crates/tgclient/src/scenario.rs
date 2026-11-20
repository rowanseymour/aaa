//! The lobby's scenario map dialog: the maps to play, what the one picked
//! is, and the world rules to play it under, taken back to the lobby on
//! Load.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea};

use tgdata::{ChoiceRule, Id};
use tgmap::Brief;

use crate::lan::Staged;
use crate::lobby::{self, Lobby};
use crate::ui::{
    self, Chosen, Dropdown, Font, Ink, Surface, button, called, label, layer, panel, slug, switch,
    text,
};
use crate::{Data, Screen, Side, confirm};

/// The dialog while it's up, with what it has picked so far. Escape takes
/// it down as it does a question.
#[derive(Component)]
pub(crate) struct Picking {
    map: String,
    brief: Result<Brief, String>,
    /// The choice of each world rule not at its default, by the rule's id.
    rules: BTreeMap<Id, Id>,
    /// The world rule whose list of choices is open.
    open: Option<Id>,
}

/// The world rules the dialog offers, of those the game has, in the order
/// the game's dialog shows them.
#[derive(Resource)]
pub(crate) struct WorldRules(Vec<ChoiceRule>);

/// The world rules the dialog sets, by id.
const OFFERED: [&str; 2] = ["FogOfWar", "Sun"];

impl WorldRules {
    /// What each rule is called and what its choice in `chosen` is
    /// called, a rule not in it being at its default.
    pub(crate) fn shown(&self, chosen: &BTreeMap<Id, Id>) -> Vec<(String, String)> {
        (self.0.iter())
            .map(|rule| (rule.name.clone(), choice_name(rule, chosen.get(&rule.id))))
            .collect()
    }
}

/// What `chosen` is called as `rule`'s choice, none being its default.
fn choice_name(rule: &ChoiceRule, chosen: Option<&Id>) -> String {
    let chosen = chosen.or(rule.default.as_ref());
    let choice = chosen.and_then(|id| rule.choices.iter().find(|c| c.id == *id));
    choice.map_or_else(|| "Default".to_string(), |c| c.name.clone())
}

/// A map in the dialog's list, by archive path.
#[derive(Component)]
struct MapPick(String);

/// A choice of a world rule, by the rule's id; none for its default.
#[derive(Component)]
struct RulePick(Id, Option<Id>);

/// Opens and closes a world rule's list of choices, by its id.
#[derive(Component)]
struct RuleOpener(Id);

/// Where the picked map is described.
#[derive(Component)]
struct Details;

/// Where the world rules are set.
#[derive(Component)]
struct Rules;

pub struct ScenarioPlugin;

impl Plugin for ScenarioPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Lobby), read_rules)
            .add_systems(
                Update,
                (show_details, show_rules, mark_picked).run_if(in_state(Screen::Lobby)),
            );
    }
}

fn read_rules(mut commands: Commands, data: Res<Data>, rules: Option<Res<WorldRules>>) {
    if rules.is_none() {
        let rules = OFFERED.iter().filter_map(|r| data.0.choice_rule(r));
        commands.insert_resource(WorldRules(rules.collect()));
    }
}

/// Puts the dialog up over the lobby, at the lobby's map and rules.
pub(crate) fn open(
    _: On<Activate>,
    mut commands: Commands,
    data: Res<Data>,
    lobby: Option<Res<Lobby>>,
    words: Res<confirm::Words>,
) {
    let (map, rules) = match &lobby {
        Some(l) => (l.map().to_string(), l.rules().clone()),
        None => (
            lobby::first_scenario(&data).unwrap_or_default(),
            BTreeMap::new(),
        ),
    };
    let root = commands
        .spawn((
            confirm::Question,
            DespawnOnExit(Screen::Lobby),
            ui::scrim(layer::DIALOG),
            Picking {
                brief: Brief::load(&data.0, &map),
                map,
                rules,
                open: None,
            },
        ))
        .id();
    let dialog = commands
        .spawn((
            ChildOf(root),
            panel(
                Surface::Panel,
                Node {
                    width: px(960),
                    max_width: percent(95),
                    height: percent(85),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(24)),
                    row_gap: px(16),
                    border: UiRect::all(px(1)),
                    ..default()
                },
            ),
            ui::Edge::Card,
        ))
        .id();
    let top = commands
        .spawn((
            ChildOf(dialog),
            Node {
                flex_grow: 1.0,
                min_height: px(0),
                column_gap: px(16),
                ..default()
            },
        ))
        .id();
    let list = commands
        .spawn((
            ChildOf(top),
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
    for name in lobby::scenarios(&data) {
        let shown = Brief::load(&data.0, &name)
            .map_or_else(|_| tgdata::map_title(&name).to_string(), |b| b.name);
        let row = Node {
            justify_content: JustifyContent::Start,
            ..ui::padded()
        };
        commands
            .spawn((
                ChildOf(list),
                switch(row),
                children![text(shown, Font::Body)],
                called(format!("scenario.map.{}", slug(tgdata::map_title(&name)))),
                MapPick(name),
            ))
            .observe(pick_map);
    }
    commands.spawn((
        ChildOf(top),
        Details,
        panel(
            Surface::Panel,
            Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip_y(),
                padding: UiRect::all(px(12)),
                row_gap: px(4),
                ..default()
            },
        ),
    ));
    commands.spawn((
        ChildOf(dialog),
        Rules,
        panel(
            Surface::Panel,
            Node {
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::all(px(12)),
                column_gap: px(32),
                row_gap: px(8),
                ..default()
            },
        ),
        // Its lists open over the buttons under it.
        GlobalZIndex(layer::DIALOG + 1),
    ));
    let buttons = commands
        .spawn((
            ChildOf(dialog),
            Node {
                justify_content: JustifyContent::End,
                column_gap: px(16),
                ..default()
            },
        ))
        .id();
    commands
        .spawn((
            ChildOf(buttons),
            button("Load", Font::Large),
            called("scenario.load"),
        ))
        .observe(load);
    commands
        .spawn((
            ChildOf(buttons),
            button(words.cancel(), Font::Large),
            called("scenario.cancel"),
        ))
        .observe(close);
}

fn pick_map(
    activate: On<Activate>,
    picks: Query<&MapPick>,
    mut picking: Single<&mut Picking>,
    data: Res<Data>,
) {
    if let Ok(MapPick(name)) = picks.get(activate.entity) {
        picking.brief = Brief::load(&data.0, name);
        picking.map.clone_from(name);
        picking.open = None;
    }
}

fn open_choices(
    activate: On<Activate>,
    openers: Query<&RuleOpener>,
    mut picking: Single<&mut Picking>,
) {
    if let Ok(RuleOpener(rule)) = openers.get(activate.entity) {
        picking.open = match picking.open.as_ref() == Some(rule) {
            true => None,
            false => Some(rule.clone()),
        };
    }
}

fn pick_choice(activate: On<Activate>, picks: Query<&RulePick>, mut picking: Single<&mut Picking>) {
    if let Ok(RulePick(rule, choice)) = picks.get(activate.entity) {
        match choice {
            Some(c) => picking.rules.insert(rule.clone(), c.clone()),
            None => picking.rules.remove(rule),
        };
        picking.open = None;
    }
}

/// Takes the picked map and rules back to the lobby, and the dialog down.
fn load(
    _: On<Activate>,
    mut commands: Commands,
    picking: Single<(Entity, &Picking)>,
    data: Res<Data>,
    asked: Res<Side>,
    lobby: Option<ResMut<Lobby>>,
    staged: Option<ResMut<Staged>>,
) {
    let (root, picking) = *picking;
    lobby::play(
        &mut commands,
        &data,
        (*asked, lobby),
        staged,
        picking.map.clone(),
        picking.rules.clone(),
    );
    commands.entity(root).despawn();
}

fn close(_: On<Activate>, mut commands: Commands, picking: Single<Entity, With<Picking>>) {
    commands.entity(*picking).despawn();
}

/// Describes the picked map as the game does: its name, file, size, sides
/// and story.
fn show_details(
    mut commands: Commands,
    picking: Single<&Picking, Changed<Picking>>,
    details: Single<Entity, With<Details>>,
    gem: Res<ui::Gem>,
) {
    let details = *details;
    commands.entity(details).despawn_children();
    let file = picking.map.rsplit('/').next().unwrap_or(&picking.map);
    let Ok(brief) = &picking.brief else {
        commands.spawn((ChildOf(details), text(file.to_string(), Font::Large)));
        return;
    };
    let row = |commands: &mut Commands, name: &str, value: String| {
        commands.spawn((
            ChildOf(details),
            Node {
                column_gap: px(16),
                ..default()
            },
            children![
                (
                    label(name.to_string(), Font::Body, Ink::Muted),
                    Node {
                        width: px(160),
                        ..default()
                    },
                ),
                text(value, Font::Body),
            ],
        ));
    };
    commands.spawn((ChildOf(details), text(brief.name.clone(), Font::Large)));
    row(&mut commands, "File", file.to_string());
    if let Some([w, h]) = brief.size {
        row(&mut commands, "Size", format!("{w}x{h}"));
    }
    let sides = brief.sides().count();
    if sides > 0 {
        row(&mut commands, "Sides", format!("1-{sides}"));
    }
    let marks = commands
        .spawn((
            ChildOf(details),
            Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::vertical(px(8)),
                row_gap: px(2),
                ..default()
            },
        ))
        .id();
    for (i, player) in brief.sides().enumerate() {
        commands.spawn((
            ChildOf(marks),
            Node {
                column_gap: px(8),
                align_items: AlignItems::Center,
                ..default()
            },
            children![
                lobby::side_mark(&gem, player),
                text(format!("Side {}", i + 1), Font::Body)
            ],
        ));
    }
    if !brief.description.is_empty() {
        commands.spawn((
            ChildOf(details),
            text(brief.description.clone(), Font::Body),
        ));
    }
}

/// The world rules to play the map under, each with what it's set to.
fn show_rules(
    mut commands: Commands,
    picking: Single<&Picking, Changed<Picking>>,
    panel: Single<Entity, With<Rules>>,
    rules: Res<WorldRules>,
) {
    let panel = *panel;
    commands.entity(panel).despawn_children();
    for rule in &rules.0 {
        let row = commands
            .spawn((
                ChildOf(panel),
                Node {
                    column_gap: px(16),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![(
                    label(rule.name.clone(), Font::Body, Ink::Muted),
                    Node {
                        width: px(160),
                        ..default()
                    },
                )],
            ))
            .id();
        let choices = rule.choices.iter().map(|c| {
            let pick = RulePick(rule.id.clone(), rule.unless_default(Some(c.id.clone())));
            (slug(c.id.as_str()), pick, c.name.clone())
        });
        let list = Dropdown {
            id: format!("scenario.{}", slug(rule.id.as_str())),
            row,
            width: px(200),
            label: choice_name(rule, picking.rules.get(&rule.id)).into(),
            open: picking.open.as_ref() == Some(&rule.id),
        };
        let opener = RuleOpener(rule.id.clone());
        list.spawn(&mut commands, opener, open_choices, choices, pick_choice);
    }
}

fn mark_picked(
    picking: Option<Single<&Picking, Changed<Picking>>>,
    mut maps: Query<(&MapPick, &mut Chosen)>,
) {
    let Some(picking) = picking else {
        return;
    };
    for (pick, mut chosen) in &mut maps {
        chosen.set_if_neq(Chosen(pick.0 == picking.map));
    }
}
