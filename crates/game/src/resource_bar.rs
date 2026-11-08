//! The resource bar in the top right corner: the player's money and what
//! they net of each resource a minute, a second bar under it previewing
//! what the order under the cursor would make of them, and under those what
//! they're told about their economy.

use std::collections::HashMap;

use aaa_map::{Id, Regiment};
use aaa_render::textures;
use aaa_sim::{Balance, Change, Notice, Pause};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use crate::actions::fill;
use crate::ui::{Font, Ink, Surface, backed, label};
use crate::{Loaded, Local, Screen, Sim};

/// What each cell shows, in order: a rate, or a limit by what it uses.
const CELLS: [&str; 5] = [
    "money",
    "ammo",
    "oil",
    "building_limit_consumed",
    "unit_limit_consumed",
];

const CELL: f32 = 96.0;
const HEIGHT: f32 = 22.0;
const GAP: f32 = 4.0;
/// How long a message stays up.
const MESSAGE_SECONDS: f32 = 6.0;

/// What the order under the cursor would change, set by the action grid.
#[derive(Resource, Default)]
pub struct Preview(pub Option<Previewed>);

#[derive(Clone)]
pub enum Previewed {
    /// Commissioning this regiment.
    Commission(Box<Regiment>),
    /// The packed building at this index into the sim's regiments
    /// unpacking.
    Unpack(usize),
}

#[derive(Resource)]
pub struct Looks {
    shortage: String,
    repaired: String,
    /// What a queue held up for want of room under a limit says, given what
    /// it's held up on.
    unit_limit: String,
    /// What a disabled building's held up queue says, given the building.
    disabled: String,
    names: HashMap<Id, String>,
}

impl Looks {
    fn name(&self, template: &str) -> String {
        self.names
            .get(&Id::from(template))
            .cloned()
            .unwrap_or_else(|| template.to_string())
    }
}

/// One of the two bars: the player's economy now, or previewed.
#[derive(Component)]
pub struct Bar(bool);

/// The text of a cell of a bar, by index into [`CELLS`].
#[derive(Component)]
pub struct Cell(bool, usize);

/// The line saying what the player was last told, and until when.
#[derive(Component)]
pub struct Message(f32);

pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    commands.insert_resource(Looks {
        shortage: said(
            "game_events_kingdom_upkeep_shortage_game_message",
            "Your production is %d%% short of your costs",
        ),
        repaired: said(
            "game_events_kingdom_upkeep_repaired_game_message",
            "You have repaired your upkeep",
        ),
        unit_limit: said(
            "recruit_error_unit_limit",
            "Insufficient unit limit to recruit %s",
        ),
        disabled: said(
            "structure_error_queue_disabled",
            "%s paused production queue - disabled",
        ),
        names: game
            .listings()
            .into_iter()
            .map(|(id, l)| (id, l.name))
            .collect(),
    });
    commands.init_resource::<Preview>();
    let glyphs: HashMap<&str, Handle<Image>> = crate::actions::GLYPHS
        .iter()
        .filter_map(|&(kind, name)| {
            let path = format!("{}/{name}.png", crate::actions::GLYPH_DIR);
            let image = textures::decode(&game.read(&path)?, &path, formats.0, true)?;
            Some((kind, images.add(image)))
        })
        .collect();
    let bar = |root: &mut ChildSpawnerCommands, preview: bool| {
        root.spawn((
            Bar(preview),
            backed(
                Surface::Hud,
                Node {
                    display: match preview {
                        true => Display::None,
                        false => Display::Flex,
                    },
                    height: px(HEIGHT),
                    column_gap: px(GAP),
                    padding: UiRect::horizontal(px(GAP * 2.0)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
            ),
        ))
        .with_children(|bar| {
            for (i, kind) in CELLS.into_iter().enumerate() {
                bar.spawn(Node {
                    width: px(CELL),
                    column_gap: px(3),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|cell| {
                    if let Some(glyph) = glyphs.get(kind) {
                        cell.spawn((
                            ImageNode::new(glyph.clone()),
                            Node {
                                width: px(16),
                                height: px(16),
                                ..default()
                            },
                        ));
                    }
                    cell.spawn((Cell(preview, i), label("", Font::Hud, Ink::Body)));
                });
            }
        });
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::MARGIN),
                right: px(crate::MARGIN),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: px(GAP),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(1),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|root| {
            bar(root, false);
            bar(root, true);
            root.spawn((
                Message(0.0),
                label("", Font::HudTitle, Ink::Warn),
                TextShadow::default(),
            ));
        });
}

/// Fills both bars for the player, the preview only while there's
/// something to preview.
pub fn show(
    sim: Res<Sim>,
    local: Res<Local>,
    preview: Res<Preview>,
    mut bars: Query<(&Bar, &mut Node)>,
    mut cells: Query<(&Cell, &mut Text)>,
) {
    let Some(player) = local.0 else {
        for (_, mut node) in &mut bars {
            node.display = Display::None;
        }
        return;
    };
    let sim = &sim.0;
    let now = (sim.stockpile(player), sim.balance(player));
    let previewed = preview.0.as_ref().and_then(|p| {
        let change = match p {
            Previewed::Commission(r) => Change::Commission(r),
            Previewed::Unpack(r) if *r < sim.regiments.len() => Change::Unpack(*r),
            Previewed::Unpack(_) => return None,
        };
        Some(sim.preview(player, change))
    });
    for (bar, mut node) in &mut bars {
        let display = match bar.0 && previewed.is_none() {
            true => Display::None,
            false => Display::Flex,
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (cell, mut text) in &mut cells {
        let shown = match cell.0 {
            true => previewed.as_ref(),
            false => Some(&now),
        };
        let Some((stockpile, balance)) = shown else {
            continue;
        };
        let words = cell_text(CELLS[cell.1], *stockpile, balance);
        if text.0 != words {
            text.0 = words;
        }
    }
}

/// What a cell says: the stockpile and its rate, a resource's rate, or a
/// limit's use against what's provided.
fn cell_text(kind: &str, stockpile: f32, balance: &Balance) -> String {
    let rate = |r: f32| format!("{:+}", r.round() as i64);
    if let Some(limit) = balance.limits.iter().find(|l| l.consumed == kind) {
        return format!("{}/{}", limit.used.round(), limit.provided.round());
    }
    let net = balance.net.get(kind);
    match kind {
        "money" => format!("{}({})", stockpile.floor(), rate(net)),
        _ => rate(net),
    }
}

/// Puts up what the player's told about their economy, and takes it down
/// once it's been up a while.
pub fn tell(
    mut sim: ResMut<Sim>,
    local: Res<Local>,
    looks: Res<Looks>,
    time: Res<Time>,
    mut message: Single<(&mut Message, &mut Text)>,
) {
    let (until, text) = &mut *message;
    let now = time.elapsed_secs();
    for notice in std::mem::take(&mut sim.0.notices) {
        let words = match notice {
            Notice::Short { player, percent } if Some(player) == local.0 => looks
                .shortage
                .replace("%d", &(percent as i64).to_string())
                .replace("%%", "%"),
            Notice::Repaired { player } if Some(player) == local.0 => looks.repaired.clone(),
            Notice::Paused {
                player,
                building,
                template,
                why,
            } if Some(player) == local.0 => match why {
                Pause::Limit(_) => fill(&looks.unit_limit, &[&looks.name(&template)]),
                Pause::Disabled => {
                    let b = &sim.0.buildings[building].template;
                    fill(&looks.disabled, &[&looks.name(b)])
                }
            },
            _ => continue,
        };
        info!("{words}");
        text.0 = words;
        until.0 = now + MESSAGE_SECONDS;
    }
    if now > until.0 && !text.0.is_empty() {
        text.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_sim::{Ledger, economy::EconomyRules};

    #[test]
    fn cells_show_rates_and_limits_as_the_game_does() {
        let mut ledger = Ledger::default();
        ledger.production.set("money", 25.0);
        ledger.production.set("ammo", 20.0);
        ledger.upkeep.set("oil", 4.0);
        ledger.production.set("unit_limit_provided", 6.0);
        ledger.upkeep.set("building_limit_consumed", 1.0);
        let rules = EconomyRules {
            limits: vec![
                ("unit_limit_consumed", "unit_limit_provided"),
                ("building_limit_consumed", "building_limit_provided"),
            ],
            limited_max: 20.0,
            ..Default::default()
        };
        let balance = ledger.balance(&rules);
        let said: Vec<String> = CELLS
            .iter()
            .map(|k| cell_text(k, 518.7, &balance))
            .collect();
        assert_eq!(said, ["518(+25)", "+20", "-4", "1/0", "0/6"]);
    }
}
