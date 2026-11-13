//! The special operations panel in the top left corner: the player's
//! side's experience, and a button for each of its general's special
//! operations, greyed out while it can't be used.

use std::collections::HashMap;

use aaa_data::SpecialOperation;
use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use crate::actions::{Blocks, GLYPH_DIR};
use crate::keys::Bindings;
use crate::ui::{Edge, Font, Gauge, Hint, Ink, Marked, Meter, Shade, Surface, label, meter, panel};
use crate::{Leaders, Loaded, Local, Screen, Sim};

/// A button's size, and its icon's within it, on the game's layout.
const BUTTON: Vec2 = Vec2::new(42.0, 44.0);
const ICON: Vec2 = Vec2::new(36.0, 31.0);
const BAR: f32 = 3.0;
const GAP: f32 = 4.0;
const ROW: f32 = 22.0;
const MEDAL: f32 = 16.0;
const MEDAL_GLYPH: &str = "FactionExperience";

/// How far right of the left edge, and down from the top, the panel
/// leaves clear for what shares the corner with it.
pub const CLEAR: Vec2 = Vec2::new(
    crate::MARGIN + BUTTON.x + 2.0 * GAP,
    crate::MARGIN + ROW + 2.0 * GAP,
);

/// The key action that uses the operation in the `n`th button.
fn action(n: usize) -> String {
    format!("UseFactionAbility {n}")
}

pub struct SpecialOpsPlugin;

impl Plugin for SpecialOpsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Match), spawn).add_systems(
            Update,
            refresh
                .run_if(in_state(Screen::Match))
                .run_if(resource_exists::<Panel>),
        );
    }
}

/// What the panel shows of the player's side, which has a general.
#[derive(Resource)]
struct Panel {
    player: u32,
    general: String,
    max: Option<f32>,
    words: Words,
    /// What the experience was last shown as, to redo only on a change.
    shown: Option<i64>,
}

/// The tooltips' words from the game's strings.
struct Words {
    title: String,
    general: String,
    experience: String,
    help: String,
}

#[derive(Component)]
struct ExperienceText;

/// The experience's box, whose tooltip tells it too.
#[derive(Component)]
struct ExperienceBox;

/// An operation's button, by what using it costs.
#[derive(Component)]
struct OperationButton {
    cost: f32,
}

#[derive(Component)]
struct OperationIcon;

fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    bindings: Res<Bindings>,
) {
    let Some(player) = local.0 else {
        return;
    };
    let Some(general) = leaders.0.get(&player) else {
        return;
    };
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let medal_path = format!("{GLYPH_DIR}/{MEDAL_GLYPH}.png");
    let medal = (game.read(&medal_path))
        .and_then(|bytes| textures::decode(&bytes, &medal_path, formats.0, true))
        .map(|i| images.add(i));
    let mut icons: HashMap<String, Option<Handle<Image>>> = HashMap::new();
    let operations = loaded.game.special_operations();
    let words = Words {
        title: said("game_interface_FactionExperience_ToolTip", "Experience"),
        general: said("ui_tooltip_item_general", "General"),
        experience: said("ui_tooltip_item_experience", "Experience"),
        help: said(
            "game_interface_FactionExperience_ToolTip_2",
            "Spend experience to activate Special Operations.",
        ),
    };
    let cost = said("ui_tooltip_item_title_cost", "Cost");
    let area = said("ui_tooltip_item_title_area", "Area");
    let buttons: Vec<_> = (general.operations.iter().enumerate())
        .filter_map(|(n, o)| {
            let op = operations.get(&o.id)?;
            let icon = op.icon.as_ref().and_then(|path| {
                let game = &mut loaded.game;
                (icons.entry(path.clone()))
                    .or_insert_with(|| {
                        let decoded = textures::decode(&game.read(path)?, path, formats.0, true)?;
                        Some(images.add(decoded))
                    })
                    .clone()
            });
            let name = bindings.marked(&action(n), &op.name);
            Some((o.cost, icon, name, Hint(tip(op, o.cost, &cost, &area))))
        })
        .collect();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::MARGIN),
                left: px(crate::MARGIN),
                flex_direction: FlexDirection::Column,
                row_gap: px(GAP),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(2),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|corner| {
            corner
                .spawn((
                    ExperienceBox,
                    Blocks,
                    Marked::plain(words.title.clone()),
                    Hint(String::new()),
                    panel(
                        Surface::Hud,
                        Node {
                            height: px(ROW),
                            padding: UiRect::horizontal(px(GAP)),
                            column_gap: px(GAP),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ),
                    Edge::Border,
                ))
                .with_children(|row| {
                    if let Some(medal) = medal {
                        row.spawn((
                            ImageNode::new(medal),
                            Node {
                                width: px(MEDAL),
                                height: px(MEDAL),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    row.spawn((ExperienceText, label("", Font::Hud, Ink::Body)));
                });
            for (cost, icon, name, hint) in buttons {
                corner
                    .spawn((
                        OperationButton { cost },
                        Blocks,
                        name,
                        hint,
                        panel(
                            Surface::Slot,
                            Node {
                                width: px(BUTTON.x),
                                height: px(BUTTON.y),
                                border: UiRect::all(px(1)),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceEvenly,
                                ..default()
                            },
                        ),
                        Edge::Border,
                    ))
                    .with_children(|button| {
                        let size = Node {
                            width: px(ICON.x),
                            height: px(ICON.y),
                            ..default()
                        };
                        match icon {
                            Some(icon) => {
                                button.spawn((
                                    OperationIcon,
                                    ImageNode::new(icon),
                                    Shade::Dimmed,
                                    size,
                                    Pickable::IGNORE,
                                ));
                            }
                            None => {
                                button.spawn((size, Pickable::IGNORE));
                            }
                        }
                        // How far it's recharged, and none is recharging
                        // until operations can be used.
                        let bar = Node {
                            width: px(ICON.x),
                            height: px(BAR),
                            ..default()
                        };
                        button.spawn(meter(bar, Gauge::Recharge)).insert(Meter {
                            gauge: Gauge::Recharge,
                            share: 1.0,
                        });
                    });
            }
        });
    commands.insert_resource(Panel {
        player,
        general: general.name.clone(),
        max: general.experience_max,
        words,
        shown: None,
    });
}

/// An operation's tooltip under its name: what its effects do, what it
/// costs and the size of its area, then its description.
fn tip(op: &SpecialOperation, cost: f32, cost_word: &str, area_word: &str) -> String {
    let effects: Vec<String> = (op.effects.iter())
        .map(|l| match l.indented {
            true => format!("    {}", l.text),
            false => l.text.clone(),
        })
        .collect();
    let mut figures = Vec::new();
    if cost > 0.0 {
        figures.push(format!("{cost_word}  {}", cost.round()));
    }
    if let Some(area) = &op.area {
        figures.push(format!("{area_word}  {area}"));
    }
    let parts = [effects.join("\n"), figures.join("\n")];
    let parts = parts.into_iter().chain(op.description.clone());
    parts
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// "474/5000": the experience rounded, and the ceiling when there is one.
fn experience_text(experience: i64, max: Option<f32>) -> String {
    match max {
        Some(max) => format!("{experience}/{}", max.round()),
        None => experience.to_string(),
    }
}

/// Keeps the experience and its tooltip up to date, and greys out the
/// operations the side can't afford.
fn refresh(
    mut panel: ResMut<Panel>,
    sim: Res<Sim>,
    mut text: Query<&mut Text, With<ExperienceText>>,
    mut boxes: Query<&mut Hint, With<ExperienceBox>>,
    buttons: Query<(&OperationButton, &Children)>,
    mut icons: Query<&mut Shade, With<OperationIcon>>,
) {
    let experience = sim.0.experience(panel.player).unwrap_or(0.0);
    let rounded = experience.round() as i64;
    if panel.shown == Some(rounded) {
        return;
    }
    panel.shown = Some(rounded);
    for mut t in &mut text {
        t.0 = experience_text(rounded, panel.max);
    }
    let w = &panel.words;
    let mut said = Vec::new();
    if !panel.general.trim().is_empty() {
        said.push(format!("{}  {}", w.general, panel.general));
    }
    said.push(format!("{}  {rounded}", w.experience));
    let hint = format!("{}\n\n{}", said.join("\n"), w.help);
    for mut h in &mut boxes {
        h.set_if_neq(Hint(hint.clone()));
    }
    for (button, children) in &buttons {
        let shade = match experience >= button.cost {
            true => Shade::Full,
            false => Shade::Dimmed,
        };
        for &child in children {
            if let Ok(mut s) = icons.get_mut(child) {
                s.set_if_neq(shade);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_data::EffectLine;

    fn line(text: &str, indented: bool) -> EffectLine {
        EffectLine {
            text: text.into(),
            indented,
        }
    }

    #[test]
    fn experience_shows_its_ceiling_where_there_is_one() {
        assert_eq!(experience_text(474, Some(5000.0)), "474/5000");
        assert_eq!(experience_text(474, None), "474");
    }

    #[test]
    fn tooltips_put_effects_then_figures_then_the_description() {
        let op = SpecialOperation {
            name: "Armored Resupply".into(),
            description: Some("Provides supply to target armor regiments.".into()),
            icon: None,
            recharge: 300.0,
            area: Some("Large".into()),
            effects: vec![
                line("Resupply for 30 Seconds", false),
                line("Supplied", true),
            ],
        };
        assert_eq!(
            tip(&op, 300.0, "Cost", "Area"),
            "Resupply for 30 Seconds\n    Supplied\n\nCost  300\nArea  Large\n\n\
             Provides supply to target armor regiments."
        );
        let free = SpecialOperation {
            area: None,
            effects: Vec::new(),
            description: None,
            ..op
        };
        assert_eq!(tip(&free, 0.0, "Cost", "Area"), "");
    }
}
