//! The lobby's scenario map dialog: the maps to play, what the one picked
//! is, and the world rules to play it under, taken back to the lobby on
//! Load.

use bevy::prelude::*;
use bevy::ui_widgets::{Activate, ScrollArea, SliderValue, ValueChange};
use tgdata::{Id, LobbyRule, RuleKind, RuleValues};
use tgmap::Brief;

use crate::fields::{Field, field};
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
    /// What the world rules are set to, those typed in aside, which are
    /// read on Load.
    rules: RuleValues,
    /// The world rule whose list of choices is open.
    open: Option<Id>,
}

/// The world rules the dialog offers, in the order the data has them.
#[derive(Resource)]
pub(crate) struct WorldRules(Vec<LobbyRule>);

impl WorldRules {
    /// What each rule is called and what it's set to in `values`.
    pub(crate) fn shown(&self, values: &RuleValues) -> Vec<(String, String)> {
        (self.0.iter())
            .map(|rule| (rule.name.clone(), rule.shown(values)))
            .collect()
    }
}

/// A map in the dialog's list, by archive path.
#[derive(Component)]
struct MapPick(String);

/// A choice of a world rule, by the rule's id.
#[derive(Component)]
struct RulePick(Id, String);

/// Where a world rule of choices shows what it's set to, by its id.
#[derive(Component)]
struct ChoiceCell(Id);

/// The box a world rule's number is typed in, by the rule's id.
#[derive(Component)]
struct RuleField(Id);

/// The slider a world rule's number is set on, by the rule's id.
#[derive(Component)]
struct RuleSlider(Id);

/// Says what a world rule on a slider is set to, by its id.
#[derive(Component)]
struct SlidValue(Id);

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
            .add_observer(slide)
            .add_systems(
                Update,
                (
                    show_details,
                    show_rules,
                    show_choices,
                    show_slid,
                    mark_picked,
                )
                    .run_if(in_state(Screen::Lobby)),
            );
    }
}

fn read_rules(mut commands: Commands, data: Res<Data>, rules: Option<Res<WorldRules>>) {
    if rules.is_none() {
        commands.insert_resource(WorldRules(data.0.lobby_rules()));
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
    let map = match &lobby {
        Some(l) => l.map().to_string(),
        None => lobby::first_scenario(&data).unwrap_or_default(),
    };
    let brief = Brief::load(&data.0, &map);
    let rules = match (&lobby, &brief) {
        (Some(l), _) => l.rules().clone(),
        (None, Ok(b)) => b.rules.values.clone(),
        (None, Err(_)) => RuleValues::new(),
    };
    let root = commands
        .spawn((
            confirm::Question,
            DespawnOnExit(Screen::Lobby),
            ui::scrim(layer::DIALOG),
            Picking {
                brief,
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
        ui::panel(
            Surface::Panel,
            Node {
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::all(px(12)),
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
        // The rules start again from what this map stores.
        picking.rules = (picking.brief.as_ref())
            .map(|b| b.rules.values.clone())
            .unwrap_or_default();
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
        picking.rules.insert(rule.clone(), choice.clone());
        picking.open = None;
    }
}

/// Takes the picked map and rules back to the lobby, and the dialog down.
fn load(
    _: On<Activate>,
    mut commands: Commands,
    picking: Single<(Entity, &Picking)>,
    (data, asked): (Res<Data>, Res<Side>),
    lobby: Option<ResMut<Lobby>>,
    staged: Option<ResMut<Staged>>,
    (world_rules, typed): (Res<WorldRules>, Query<(&RuleField, &Field)>),
) {
    let (root, picking) = *picking;
    let mut rules = picking.rules.clone();
    for (RuleField(id), field) in &typed {
        let Some(rule) = world_rules.0.iter().find(|r| r.id == *id) else {
            continue;
        };
        let max = match rule.kind {
            RuleKind::Typed { max } => max,
            _ => u32::MAX,
        };
        // Left empty, it's left as the map has it.
        match tgdata::number(&field.text) {
            Some(n) => rules.insert(id.clone(), n.min(max).to_string()),
            None => rules.remove(id),
        };
    }
    // What the map fixes is played as it has it.
    if let Ok(brief) = &picking.brief {
        for id in &brief.rules.fixed {
            rules.remove(id);
        }
    }
    lobby::play(
        &mut commands,
        &data,
        (*asked, lobby),
        staged,
        picking.map.clone(),
        rules,
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
    picking: Single<(Entity, &Picking), Changed<Picking>>,
    details: Single<Entity, With<Details>>,
    gem: Res<ui::Gem>,
    mut shown: Local<Option<(Entity, String)>>,
) {
    let (dialog, picking) = *picking;
    // Only a map picked changes what's described.
    let now = Some((dialog, picking.map.clone()));
    if *shown == now {
        return;
    }
    *shown = now;
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

/// A world rule's row in `panel`: its name, and what sets it, at what
/// `values` set it to.
fn rule_row(commands: &mut Commands, panel: Entity, rule: &LobbyRule, values: &RuleValues) {
    let row = commands
        .spawn((
            ChildOf(panel),
            Node {
                // Two to a line, as the game has them.
                width: percent(50),
                column_gap: px(16),
                align_items: AlignItems::Center,
                ..default()
            },
            children![(
                label(rule.name.clone(), Font::Body, Ink::Muted),
                Node {
                    width: px(180),
                    ..default()
                },
            )],
        ))
        .id();
    let id = format!("scenario.{}", slug(rule.id.as_str()));
    let value = rule.value(values);
    match rule.kind {
        RuleKind::Choice(_) => {
            commands.spawn((ChildOf(row), ChoiceCell(rule.id.clone()), Node::default()));
        }
        RuleKind::Typed { max } => {
            let longest = max.to_string().len();
            let shown = tgdata::number(value).map_or_else(String::new, |n| n.to_string());
            commands
                .spawn((
                    ChildOf(row),
                    field(shown.clone(), longest, px(200)),
                    RuleField(rule.id.clone()),
                    called(id),
                ))
                .insert(Field {
                    text: shown,
                    longest,
                    digits: true,
                    ..default()
                });
        }
        RuleKind::Slid { min, max, .. } => {
            let at = tgdata::number(value).unwrap_or(min);
            let share = match max > min {
                true => (at.clamp(min, max) - min) as f32 / (max - min) as f32,
                false => 0.0,
            };
            let node = Node {
                width: px(160),
                ..default()
            };
            commands.spawn((
                ChildOf(row),
                ui::slider(node, share),
                RuleSlider(rule.id.clone()),
                called(id),
            ));
            commands.spawn((
                ChildOf(row),
                SlidValue(rule.id.clone()),
                text(at.to_string(), Font::Body),
            ));
        }
    }
}

/// Sets the world rule a slider's moved to, in its steps.
fn slide(
    change: On<ValueChange<f32>>,
    mut commands: Commands,
    sliders: Query<&RuleSlider>,
    world_rules: Res<WorldRules>,
    mut picking: Single<&mut Picking>,
) {
    let Ok(RuleSlider(id)) = sliders.get(change.source) else {
        return;
    };
    let Some(rule) = world_rules.0.iter().find(|r| r.id == *id) else {
        return;
    };
    let RuleKind::Slid { min, max, step } = rule.kind else {
        return;
    };
    let span = max.saturating_sub(min);
    let raw = min as f32 + change.value.clamp(0.0, 1.0) * span as f32;
    let at = (min + ((raw - min as f32) / step as f32).round() as u32 * step).min(max);
    let share = match span {
        0 => 0.0,
        _ => (at - min) as f32 / span as f32,
    };
    commands.entity(change.source).insert(SliderValue(share));
    picking.rules.insert(id.clone(), at.to_string());
}

/// Lays out the world rules for the map picked, each as it's set, but for
/// those the map fixes. Laid out afresh only when a map's picked, so as
/// not to take a slider or a box from under the person using it.
fn show_rules(
    mut commands: Commands,
    picking: Single<(Entity, &Picking), Changed<Picking>>,
    panel: Single<Entity, With<Rules>>,
    world_rules: Res<WorldRules>,
    mut shown: Local<Option<(Entity, String)>>,
) {
    let (dialog, picking) = *picking;
    let now = Some((dialog, picking.map.clone()));
    if *shown == now {
        return;
    }
    *shown = now;
    let panel = *panel;
    commands.entity(panel).despawn_children();
    let fixed = (picking.brief.as_ref()).map_or(&[][..], |b| &b.rules.fixed[..]);
    for rule in world_rules.0.iter().filter(|r| !fixed.contains(&r.id)) {
        rule_row(&mut commands, panel, rule, &picking.rules);
    }
}

/// Says what each world rule on a slider is set to.
fn show_slid(
    picking: Single<&Picking, Changed<Picking>>,
    world_rules: Res<WorldRules>,
    mut values: Query<(&SlidValue, &mut Text)>,
) {
    for (SlidValue(id), mut text) in &mut values {
        if let Some(rule) = world_rules.0.iter().find(|r| r.id == *id) {
            text.0 = rule.shown(&picking.rules);
        }
    }
}

/// What each world rule of choices is set to, with its list of them while
/// that's open.
fn show_choices(
    mut commands: Commands,
    picking: Single<Ref<Picking>>,
    cells: Query<(Entity, Ref<ChoiceCell>)>,
    world_rules: Res<WorldRules>,
) {
    for (cell, chosen) in &cells {
        if !picking.is_changed() && !chosen.is_added() {
            continue;
        }
        let ChoiceCell(id) = &*chosen;
        let Some(rule) = world_rules.0.iter().find(|r| r.id == *id) else {
            continue;
        };
        let RuleKind::Choice(choices) = &rule.kind else {
            continue;
        };
        commands.entity(cell).despawn_children();
        let choices = choices.iter().map(|c| {
            let pick = RulePick(rule.id.clone(), c.id.to_string());
            (slug(c.id.as_str()), pick, c.name.clone())
        });
        let list = Dropdown {
            id: format!("scenario.{}", slug(rule.id.as_str())),
            row: cell,
            width: px(200),
            label: rule.shown(&picking.rules).into(),
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
