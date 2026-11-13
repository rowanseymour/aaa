//! The technology display: a window listing the technologies the player's
//! side holds, one of the [display panels](crate::displays).

use std::collections::HashMap;

use aaa_data::{Id, PropertyLook};
use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use crate::displays::{self, Contents, Displays, Panel, Rim, Window};
use crate::keys::Bindings;
use crate::ui::{Font, Hint, Ink, Marked, label};
use crate::{Loaded, Local, Screen, Sim};

const NAME: &str = "control_panel_ButtonTechnology_ToolTip";
const HELP: &str = "control_panel_ButtonTechnology_tooltip_2";
const TITLE: &str = "technology_display_Child_text";
const BUTTON_ICON: &[u8] = include_bytes!("../../../assets/icons/gear.png");

/// A line is at least this tall, and its icon this wide.
const LINE: f32 = 20.0;
const ICON: f32 = 16.0;

/// When the list was last brought up to date, and what it showed, to
/// rebuild it only when that changes.
#[derive(Resource, Default)]
struct Shown {
    refreshed: Option<f64>,
    ids: Vec<Id>,
}

/// How each technology shows in the list, by id.
#[derive(Resource)]
struct Looks {
    lines: HashMap<Id, Line>,
    formats: bevy::image::CompressedImageFormats,
}

struct Line {
    name: String,
    icon: String,
    image: Option<Option<Handle<Image>>>,
    /// Its tooltip under its name: what it does, then its description.
    hint: String,
}

pub struct TechnologyPlugin;

impl Plugin for TechnologyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Match), spawn).add_systems(
            Update,
            refresh
                .after(displays::Keys)
                .run_if(in_state(Screen::Match))
                .run_if(resource_exists::<Looks>),
        );
    }
}

fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    bindings: Res<Bindings>,
) {
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    let looks: HashMap<Id, PropertyLook> = game.property_looks();
    let lines = (game.technologies().into_iter())
        .filter_map(|(id, t)| {
            let look = looks.get(&id)?;
            let mut hint = look.lines.join("\n");
            if let Some(d) = t.description {
                hint = match hint.is_empty() {
                    true => d,
                    false => format!("{hint}\n\n{d}"),
                };
            }
            let line = Line {
                name: t.name,
                icon: look.icon.clone(),
                image: None,
                hint,
            };
            Some((id, line))
        })
        .collect();
    displays::spawn_button(
        &mut commands,
        &bindings,
        Panel::Technologies,
        Rim::TopRight,
        displays::icon(BUTTON_ICON, &mut images, formats.0),
        said(NAME, "Display Technologies"),
        said(HELP, "Displays a list of all your technologies."),
    );
    displays::spawn_window(
        &mut commands,
        Window {
            panel: Panel::Technologies,
            title: said(TITLE, "Technology Panel"),
            at: Vec2::new(321.0, 53.0),
            size: Vec2::new(385.0, 513.0),
            pane: true,
        },
    );
    commands.insert_resource(Shown::default());
    commands.insert_resource(Looks {
        lines,
        formats: formats.0,
    });
}

/// Brings the open window's list up to date: at once on opening, then once
/// a second.
#[allow(clippy::too_many_arguments)]
fn refresh(
    mut commands: Commands,
    displays: Res<Displays>,
    mut shown: ResMut<Shown>,
    mut looks: ResMut<Looks>,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    real: Res<Time<Real>>,
    sim: Res<Sim>,
    local: Res<Local>,
    lists: Query<(Entity, &Contents)>,
) {
    if !displays.is_open(Panel::Technologies) {
        shown.refreshed = None;
        return;
    }
    let now = real.elapsed_secs_f64();
    if shown.refreshed.is_some_and(|then| now - then < 1.0) {
        return;
    }
    shown.refreshed = Some(now);
    let held = local.0.and_then(|p| sim.0.technologies.get(&p));
    let ids: Vec<Id> = (held.into_iter().flatten())
        .filter(|t| looks.lines.contains_key(*t))
        .cloned()
        .collect();
    if ids == shown.ids {
        return;
    }
    let Some((list, _)) = lists.iter().find(|(_, c)| c.0 == Panel::Technologies) else {
        return;
    };
    let formats = looks.formats;
    let mut entries = Vec::new();
    for id in &ids {
        let Some(line) = looks.lines.get_mut(id) else {
            continue;
        };
        let image = line.image.get_or_insert_with(|| {
            let path = &line.icon;
            let decoded = textures::decode(&loaded.game.read(path)?, path, formats, true)?;
            Some(images.add(decoded))
        });
        entries.push((line.name.clone(), image.clone(), line.hint.clone()));
    }
    let mut list = commands.entity(list);
    list.despawn_children();
    list.with_children(|list| {
        for (name, image, hint) in entries {
            list.spawn((
                Node {
                    min_height: px(LINE),
                    column_gap: px(4),
                    align_items: AlignItems::Center,
                    ..default()
                },
                Hovered::default(),
                Marked::plain(name.as_str()),
                Hint(hint),
            ))
            .with_children(|line| {
                line.spawn((
                    ImageNode::new(image.unwrap_or_default()),
                    Node {
                        width: px(ICON),
                        height: px(ICON),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                line.spawn(label(name, Font::Small, Ink::Body));
            });
        }
    });
    shown.ids = ids;
}
