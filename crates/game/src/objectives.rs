//! The Objectives window: the side's primary and secondary objectives,
//! each behind a box showing how it stands, one of the [display
//! panels](crate::displays). `spec/objectives.md` is what it's built to.

use aaa_render::textures;
use aaa_sim::{Objective, Standing};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::ScrollArea;

use crate::displays::{self, Displays, Panel, Rim, Window};
use crate::keys::Bindings;
use crate::ui::{Font, Ink, coloured, label};
use crate::{Loaded, Local, Screen, Sim};

const BOX: f32 = 20.0;
/// Between a line's box and its text, and between lines.
const BOX_GAP: f32 = 3.0;
const LINE_GAP: f32 = 2.0;

/// What the window's lines are made from, read as the match starts.
#[derive(Resource)]
struct Look {
    /// Each state's text colour and box, in [`State`] order.
    states: [(Color, Handle<Image>); 4],
    /// The line a side with no objectives is shown.
    defeat_all: String,
    /// The lists the window's two halves show, primary then secondary.
    lists: [Entity; 2],
}

/// When the lists were last brought up to date, and what they showed, to
/// rebuild them only when that changes.
#[derive(Resource, Default)]
struct Shown {
    refreshed: Option<f64>,
    objectives: Option<Vec<Objective>>,
}

pub struct ObjectivesPlugin;

impl Plugin for ObjectivesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Match), spawn).add_systems(
            Update,
            refresh
                .after(displays::Keys)
                .run_if(in_state(Screen::Match))
                .run_if(resource_exists::<Look>),
        );
    }
}

fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    bindings: Res<Bindings>,
    mut icons: ResMut<displays::Icons>,
) {
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    let looks = game.objective_looks();
    let states = [
        looks.required,
        looks.optional,
        looks.completed,
        looks.failed,
    ]
    .map(|look| {
        let path = look.icon.as_deref();
        let image =
            path.and_then(|path| textures::decode(&game.read(path)?, path, formats.0, true));
        let [r, g, b] = look.colour;
        (
            Color::srgb(r, g, b),
            image.map(|i| images.add(i)).unwrap_or_default(),
        )
    });
    displays::spawn_button(
        &mut commands,
        &bindings,
        Panel::Objectives,
        Rim::BottomLeft,
        icons.get(displays::icon!("objectives"), &mut images, formats.0),
        said(
            "control_panel_ButtonObjectives_ToolTip",
            "Display Objectives",
        ),
        said(
            "control_panel_ButtonObjectives_tooltip_2",
            "Displays a list of mission objectives.",
        ),
    );
    let contents = displays::spawn_window(
        &mut commands,
        Window {
            panel: Panel::Objectives,
            title: said("objectives_display_Child_text", "Objectives"),
            at: Vec2::new(321.0, 53.0),
            size: Vec2::new(385.0, 513.0),
            pane: false,
        },
    );
    let headings = [
        said("objectives_display_Child_text_2", "Primary"),
        said("objectives_display_Child_text_3", "Secondary"),
    ];
    let lists = headings.map(|heading| {
        let half = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    min_height: px(0),
                    row_gap: px(4),
                    ..default()
                },
                ChildOf(contents),
            ))
            .id();
        commands.spawn((label(heading, Font::Hud, Ink::Body), ChildOf(half)));
        commands
            .spawn((
                ScrollArea,
                displays::pane(Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_height: px(0),
                    row_gap: px(LINE_GAP),
                    overflow: Overflow::scroll_y(),
                    ..default()
                }),
                ScrollPosition::default(),
                ChildOf(half),
            ))
            .id()
    });
    commands.insert_resource(Look {
        states,
        defeat_all: said("objective_defeat_all_enemies", "Defeat all enemies"),
        lists,
    });
    commands.insert_resource(Shown::default());
}

/// What the window lists for a side holding `held`: those, or the line a
/// side with none is shown.
fn listed(held: &[Objective], defeat_all: &str) -> Vec<Objective> {
    match held.is_empty() {
        true => vec![Objective {
            name: String::new(),
            text: defeat_all.to_string(),
            primary: true,
            standing: Standing::Active,
        }],
        false => held.to_vec(),
    }
}

/// The states an objective is drawn in, as the layout names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Required,
    Optional,
    Completed,
    Failed,
}

fn state(objective: &Objective) -> State {
    match objective.standing {
        Standing::Active if objective.primary => State::Required,
        Standing::Active => State::Optional,
        Standing::Completed => State::Completed,
        Standing::Failed => State::Failed,
    }
}

/// Brings the open window's lists up to date: at once on opening, then
/// once a second.
fn refresh(
    mut commands: Commands,
    displays: Res<Displays>,
    mut shown: ResMut<Shown>,
    look: Res<Look>,
    real: Res<Time<Real>>,
    sim: Res<Sim>,
    local: Res<Local>,
) {
    if !displays.is_open(Panel::Objectives) {
        shown.refreshed = None;
        return;
    }
    let now = real.elapsed_secs_f64();
    if shown.refreshed.is_some_and(|then| now - then < 1.0) {
        return;
    }
    shown.refreshed = Some(now);
    let held = local.0.and_then(|p| sim.0.objectives.get(&p));
    let objectives = listed(held.map_or(&[][..], |h| &h.0[..]), &look.defeat_all);
    if shown.objectives.as_ref() == Some(&objectives) {
        return;
    }
    for (list, primary) in look.lists.into_iter().zip([true, false]) {
        let mut list = commands.entity(list);
        list.despawn_children();
        list.with_children(|list| {
            for objective in objectives.iter().filter(|o| o.primary == primary) {
                let (colour, boxed) = look.states[state(objective) as usize].clone();
                list.spawn(Node {
                    column_gap: px(BOX_GAP),
                    align_items: AlignItems::FlexStart,
                    ..default()
                })
                .with_children(|line| {
                    line.spawn((
                        ImageNode::new(boxed),
                        Node {
                            width: px(BOX),
                            height: px(BOX),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    line.spawn((
                        coloured(objective.text.clone(), Font::Small, colour),
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_height: px(BOX),
                            ..default()
                        },
                    ));
                });
            }
        });
    }
    shown.objectives = Some(objectives);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objective(text: &str, primary: bool, standing: Standing) -> Objective {
        Objective {
            name: text.into(),
            text: text.into(),
            primary,
            standing,
        }
    }

    #[test]
    fn a_side_with_none_is_told_to_defeat_all_enemies() {
        let lines = listed(&[], "Defeat all enemies");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].primary && lines[0].standing == Standing::Active);
        let held = [objective("1. Capture Maleme.", true, Standing::Active)];
        assert_eq!(listed(&held, "Defeat all enemies"), held);
    }

    #[test]
    fn active_ones_are_drawn_by_kind_and_settled_ones_by_outcome() {
        let drawn = |primary, standing| state(&objective("a", primary, standing));
        assert_eq!(drawn(true, Standing::Active), State::Required);
        assert_eq!(drawn(false, Standing::Active), State::Optional);
        assert_eq!(drawn(false, Standing::Completed), State::Completed);
        assert_eq!(drawn(true, Standing::Failed), State::Failed);
    }
}
