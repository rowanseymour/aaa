//! The pane along the bottom, between the minimap and the action buttons,
//! describing what's selected, laid out as the game's own but without its
//! chrome. Its title names it and its owner, and a line along the bottom
//! says what it's doing.
//!
//! - One regiment: its officer's portrait over the regiment's health, a
//!   card per other unit with the unit's health, and its banner.
//! - Several: a card each, up to [`MAX_SELECTED`], with the banner, the
//!   regiment's health and each unit's; clicking one selects it alone.
//! - A building: its picture over its health, and what it's been asked to
//!   commission, the first with its progress; clicking one calls it off.

use std::collections::HashMap;

use aaa_data::{GameData, Id};
use aaa_render::overlay::BannerImages;
use aaa_render::textures;
use aaa_sim::{Order, State, World};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::Blocks;
use crate::banners::Divisions;
use crate::ui::{self, Font, Ink, Surface, framed, label};
use crate::{Drawn, Loaded, Local, Orders, Screen, Selected, SelectedBuilding, Sim, TINTS, tint};

/// How many regiments can be selected at once, as many as the pane has
/// cards for.
pub const MAX_SELECTED: usize = 6;

/// Our pixels to one of the game's interface units, in which its layout is
/// given, chosen so the pane stands as tall as the action buttons beside it.
const SCALE: f32 = crate::actions::SIZE.y / HEIGHT;
const WIDTH: f32 = 495.0;
/// The pane's size, in logical pixels.
pub const SIZE: Vec2 = Vec2::new(WIDTH * SCALE, HEIGHT * SCALE);
const HEIGHT: f32 = 183.0;

/// A box at `left`, `top`, `width` across and `height` down, in the
/// game's interface units.
fn at(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left * SCALE),
        top: px(top * SCALE),
        width: px(width * SCALE),
        height: px(height * SCALE),
        ..default()
    }
}

/// The pane, whose children show what [`Showing`] says.
#[derive(Component)]
pub struct Pane;

/// What the pane shows now.
#[derive(Resource, Default)]
pub struct Showing(Option<What>);

/// What the pane can show; a change in it lays the pane out afresh.
#[derive(Clone, PartialEq, Debug)]
enum What {
    Regiment(usize),
    Regiments(Vec<usize>),
    /// With the templates of what it's commissioning.
    Building(usize, Vec<String>),
}

/// A bar filled to a share of its width.
#[derive(Component, Clone, Copy)]
pub enum Fill {
    /// A unit's health.
    Unit(usize),
    /// A regiment's units' health together.
    Regiment(usize),
    Building(usize),
    /// How far along a building's first commission is.
    Progress(usize),
}

/// The line saying what a regiment or building is doing.
#[derive(Component)]
pub enum Status {
    Regiment(usize),
    Building(usize),
}

/// A unit's picture, darkened once it's dead.
#[derive(Component)]
pub struct UnitPicture(usize);

/// A regiment's card, which selects it alone.
#[derive(Component)]
pub struct Pick(usize);

/// One of what a building is commissioning, by place in its queue, which
/// calls it off.
#[derive(Component)]
pub struct Queued(usize);

/// What the pane is drawn with, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Templates' names, by id.
    names: HashMap<Id, String>,
    strings: HashMap<String, String>,
    images: HashMap<String, Option<Handle<Image>>>,
    formats: bevy::image::CompressedImageFormats,
}

impl Looks {
    fn image(
        &mut self,
        game: &mut GameData,
        images: &mut Assets<Image>,
        path: Option<&str>,
    ) -> Option<Handle<Image>> {
        let path = path?;
        if let Some(h) = self.images.get(path) {
            return h.clone();
        }
        let h =
            textures::decode(&game.read(path)?, path, self.formats, true).map(|i| images.add(i));
        self.images.insert(path.to_string(), h.clone());
        h
    }

    fn name(&self, template: &str) -> String {
        self.names
            .get(&Id::from(template))
            .cloned()
            .unwrap_or_else(|| template.to_string())
    }

    /// The game's name for an actor state, e.g. `mind_resting`.
    fn state(&self, ids: &str) -> String {
        self.strings
            .get(&format!("actor_states_{ids}_name"))
            .cloned()
            .unwrap_or_default()
    }
}

pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &mut loaded.game;
    commands.insert_resource(Looks {
        names: game
            .listings()
            .into_iter()
            .map(|(id, l)| (id, l.name))
            .collect(),
        strings: game.strings(),
        images: HashMap::new(),
        formats: formats.0,
    });
    commands.insert_resource(Showing::default());
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(crate::MARGIN),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ))
        .with_child((
            Pane,
            Blocks,
            RelativeCursorPosition::default(),
            Node {
                width: px(SIZE.x),
                height: px(SIZE.y),
                border_radius: BorderRadius::all(px(12)),
                ..default()
            },
            Surface::Hud,
            Visibility::Hidden,
            GlobalZIndex(1),
        ));
}

/// What the selection calls for the pane to show.
fn wanted(sim: &World, selected: &[usize], chosen: Option<usize>) -> Option<What> {
    match (selected, chosen) {
        ([], Some(b)) => {
            let queue = sim.buildings[b].queue.iter();
            let templates = queue.map(|c| c.regiment.template.clone()).collect();
            Some(What::Building(b, templates))
        }
        ([], None) => None,
        ([r], _) => Some(What::Regiment(*r)),
        (rs, _) => Some(What::Regiments(
            rs.iter().take(MAX_SELECTED).copied().collect(),
        )),
    }
}

/// Lays the pane out afresh when what it should show changes.
#[allow(clippy::too_many_arguments)]
pub fn lay_out(
    mut commands: Commands,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    local: Res<Local>,
    drawn: Res<Drawn>,
    divisions: Res<Divisions>,
    mut showing: ResMut<Showing>,
    mut loaded: ResMut<Loaded>,
    mut looks: ResMut<Looks>,
    mut banners: ResMut<BannerImages>,
    mut images: ResMut<Assets<Image>>,
    mut pane: Single<(Entity, &mut Visibility), With<Pane>>,
) {
    let what = wanted(&sim.0, &selected.0, chosen.0);
    if showing.0 == what {
        return;
    }
    let (root, visibility) = &mut *pane;
    commands.entity(*root).despawn_related::<Children>();
    visibility.set_if_neq(match what {
        Some(_) => Visibility::Inherited,
        None => Visibility::Hidden,
    });
    showing.0 = what.clone();
    let Some(what) = what else {
        return;
    };
    let mut ui = Ui {
        sim: &sim.0,
        local: local.0,
        drawn: &drawn,
        divisions: &divisions,
        game: &mut loaded.game,
        looks: &mut looks,
        banners: &mut banners,
        images: &mut images,
    };
    let root = *root;
    match what {
        What::Regiment(r) => ui.regiment(&mut commands, root, r),
        What::Regiments(rs) => ui.regiments(&mut commands, root, &rs),
        What::Building(b, _) => ui.building(&mut commands, root, b),
    }
}

/// What laying the pane out reads.
struct Ui<'a> {
    sim: &'a World,
    local: Option<u32>,
    drawn: &'a Drawn,
    divisions: &'a Divisions,
    game: &'a mut GameData,
    looks: &'a mut Looks,
    banners: &'a mut BannerImages,
    images: &'a mut Assets<Image>,
}

impl Ui<'_> {
    fn picture(&mut self, template: &str) -> Option<Handle<Image>> {
        let path = self.drawn.0.icon(template);
        self.looks.image(self.game, self.images, path)
    }

    /// A regiment's name as the game gives it: numbered among its owner's
    /// regiments of its kind, but for a lone unit such as a truck.
    fn regiment_name(&self, r: usize) -> String {
        let regiment = &self.sim.regiments[r];
        let name = self.looks.name(&regiment.template);
        if lone(self.sim, r) {
            return name;
        }
        let n = self.sim.regiments[..=r]
            .iter()
            .filter(|o| o.owner == regiment.owner && o.template == regiment.template)
            .count();
        format!("{} {name}", ordinal(n))
    }

    /// The name and owner across the top.
    fn title(&self, commands: &mut Commands, root: Entity, name: String, owner: u32) {
        let player = self.sim.sides.player(owner);
        let colour = TINTS[tint(self.sim, self.local, owner)];
        commands.entity(root).with_children(|pane| {
            pane.spawn((
                at(8.0, 5.0, 300.0, 14.0),
                label(name, Font::HudTitle, Ink::Body),
            ));
            if let Some(p) = player {
                pane.spawn((
                    at(8.0, 21.0, 300.0, 14.0),
                    Text::new(p.name.clone()),
                    ui::font(Font::Hud),
                    TextColor(colour),
                    Pickable::IGNORE,
                ));
            }
        });
    }

    /// A picture in a box, with a bar below filled as `fill` says.
    #[allow(clippy::too_many_arguments)]
    fn portrait(&mut self, commands: &mut Commands, root: Entity, template: &str, fill: Fill) {
        let picture = self.picture(template);
        commands.entity(root).with_children(|pane| {
            pane.spawn((framed(at(8.0, 44.0, 124.0, 124.0)), picture_node(picture)));
            bar(pane, at(8.0, 171.0, 124.0, 4.0), fill);
        });
    }

    /// The line along the bottom saying what it's doing.
    fn status(&self, commands: &mut Commands, root: Entity, status: Status) {
        commands.entity(root).with_children(|pane| {
            pane.spawn((at(144.0, 158.0, 286.0, 18.0), Surface::Track))
                .with_child((
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(6),
                        top: px(2),
                        ..default()
                    },
                    label("", Font::Hud, Ink::Body),
                    status,
                ));
        });
    }

    /// A regiment's banner, as it flies over the regiment, filling `node`.
    fn banner(&mut self, parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
        let regiment = &self.sim.regiments[r];
        let [red, green, blue] = self
            .sim
            .sides
            .player(regiment.owner)
            .map_or([1.0; 3], |p| p.rgb);
        let icon = self
            .banners
            .icon(self.game, self.images, &regiment.template);
        let badge = self
            .divisions
            .0
            .get(&regiment.id)
            .and_then(|d| self.banners.badge(self.game, self.images, d));
        let layers = [
            (self.banners.plate.clone(), Color::srgb(red, green, blue)),
            (icon, Color::WHITE),
            (self.banners.frame.clone(), Color::WHITE),
            (badge, Color::WHITE),
        ];
        parent
            .spawn((node, Pickable::IGNORE))
            .with_children(|banner| {
                for (image, colour) in layers {
                    let Some(image) = image else {
                        continue;
                    };
                    banner.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: percent(100),
                            height: percent(100),
                            ..default()
                        },
                        ImageNode {
                            image,
                            color: colour,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                }
            });
    }

    fn regiment(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        let regiment = &self.sim.regiments[r];
        self.title(commands, root, self.regiment_name(r), regiment.owner);
        let leader = leader(self.sim, r);
        let template = self.sim.units[leader].template.clone();
        self.portrait(commands, root, &template, Fill::Regiment(r));
        if !lone(self.sim, r) {
            // Cards across, wrapping, as the game's company display has them,
            // for all but an officer, who has the portrait.
            let officer = is_officer(&self.sim.units[leader].template);
            let mut units = self.sim.regiments[r].units.clone();
            units.retain(|&u| !(officer && u == leader));
            let pictures: Vec<_> = units
                .iter()
                .map(|&u| self.picture(&self.sim.units[u].template.clone()))
                .collect();
            commands.entity(root).with_children(|pane| {
                pane.spawn((
                    Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(2.0 * SCALE),
                        row_gap: px(2.0 * SCALE),
                        align_content: AlignContent::FlexStart,
                        ..at(144.0, 44.0, 210.0, 110.0)
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|cards| {
                    for (&u, picture) in units.iter().zip(pictures) {
                        cards
                            .spawn(framed(Node {
                                width: px(42.0 * SCALE),
                                height: px(47.0 * SCALE),
                                flex_direction: FlexDirection::Column,
                                ..default()
                            }))
                            .with_children(|card| {
                                card.spawn((
                                    Node {
                                        width: percent(100),
                                        flex_grow: 1.0,
                                        ..default()
                                    },
                                    picture_node(picture),
                                    UnitPicture(u),
                                ));
                                bar(
                                    card,
                                    Node {
                                        width: percent(100),
                                        height: px(4.0 * SCALE),
                                        ..default()
                                    },
                                    Fill::Unit(u),
                                );
                            });
                    }
                });
            });
            commands.entity(root).with_children(|pane| {
                pane.spawn(framed(at(366.0, 44.0, 63.0, 77.0)))
                    .with_children(|card| {
                        self.banner(
                            card,
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(6.0 * SCALE),
                                top: px(12.0 * SCALE),
                                width: px(51.0 * SCALE),
                                height: px(51.0 * SCALE),
                                ..default()
                            },
                            r,
                        );
                    });
            });
        }
        self.status(commands, root, Status::Regiment(r));
    }

    fn regiments(&mut self, commands: &mut Commands, root: Entity, rs: &[usize]) {
        let first = rs[0];
        self.title(
            commands,
            root,
            self.regiment_name(first),
            self.sim.regiments[first].owner,
        );
        const CARD: f32 = 66.0;
        for (i, &r) in rs.iter().enumerate() {
            let left = 8.0 + i as f32 * (CARD + 4.0);
            let icon = self.picture(&self.sim.regiments[r].template.clone());
            let units = self.sim.regiments[r].units.clone();
            let mut card = commands.spawn((
                framed(at(left, 40.0, CARD, 138.0)),
                Button,
                Pick(r),
                ChildOf(root),
            ));
            card.observe(pick);
            card.with_children(|card| {
                self.banner(
                    card,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(10.0 * SCALE),
                        top: px(2.0 * SCALE),
                        width: px(46.0 * SCALE),
                        height: px(46.0 * SCALE),
                        ..default()
                    },
                    r,
                );
                bar(card, at(2.0, 50.0, CARD - 6.0, 5.0), Fill::Regiment(r));
                // Two columns of the units' health.
                let half = (CARD - 8.0) / 2.0;
                for (n, &u) in units.iter().enumerate() {
                    let (col, row) = ((n % 2) as f32, (n / 2) as f32);
                    let node = at(2.0 + col * (half + 2.0), 58.0 + row * 5.0, half, 3.0);
                    bar(card, node, Fill::Unit(u));
                }
                card.spawn((
                    at(13.0, 106.0, 40.0, 28.0),
                    picture_node(icon),
                    Pickable::IGNORE,
                ));
            });
        }
    }

    fn building(&mut self, commands: &mut Commands, root: Entity, b: usize) {
        let building = &self.sim.buildings[b];
        let template = building.template.clone();
        self.title(commands, root, self.looks.name(&template), building.owner);
        self.portrait(commands, root, &template, Fill::Building(b));
        let queue: Vec<String> = building
            .queue
            .iter()
            .map(|c| c.regiment.template.clone())
            .collect();
        // Commissions are trucks, which show their building's button.
        let pictures: Vec<_> = queue.iter().map(|t| self.picture(t)).collect();
        commands.entity(root).with_children(|pane| {
            for (i, picture) in pictures.into_iter().enumerate().take(6) {
                let node = match i {
                    0 => at(240.0, 44.0, 64.0, 64.0),
                    _ => {
                        let n = (i - 1) as f32;
                        at(
                            313.0 + (n % 3.0) * 34.0,
                            44.0 + (n / 3.0).floor() * 34.0,
                            32.0,
                            32.0,
                        )
                    }
                };
                pane.spawn((framed(node), picture_node(picture), Button, Queued(i)))
                    .observe(cancel);
            }
            if !queue.is_empty() {
                bar(pane, at(240.0, 110.0, 64.0, 4.0), Fill::Progress(b));
            }
        });
        self.status(commands, root, Status::Building(b));
    }
}

/// Keeps the pane's bars, pictures and status up to date.
pub fn refresh(
    sim: Res<Sim>,
    looks: Res<Looks>,
    mut bars: Query<(&Fill, &mut Node, &mut BackgroundColor)>,
    mut pictures: Query<(&UnitPicture, &mut ImageNode)>,
    mut statuses: Query<(&Status, &mut Text)>,
) {
    let sim = &sim.0;
    for (fill, mut node, mut colour) in &mut bars {
        let share = match *fill {
            Fill::Unit(u) => unit_health(sim, u),
            Fill::Regiment(r) => {
                let units = &sim.regiments[r].units;
                let (have, full) = units.iter().fold((0.0, 0.0), |(h, f), &u| {
                    let max = sim.units[u].fighter.as_ref().map_or(0.0, |f| f.health);
                    let now = if sim.units[u].alive() {
                        sim.units[u].health
                    } else {
                        0.0
                    };
                    (h + now, f + max)
                });
                if full > 0.0 { have / full } else { 1.0 }
            }
            Fill::Building(b) => {
                let b = &sim.buildings[b];
                b.health / b.fighter.health
            }
            Fill::Progress(b) => {
                let b = &sim.buildings[b];
                b.queue
                    .front()
                    .map_or(0.0, |c| b.progress as f32 / c.ticks as f32)
            }
        };
        let width = percent(share.clamp(0.0, 1.0) * 100.0);
        if node.width != width {
            node.width = width;
        }
        let tone = match fill {
            Fill::Progress(_) => Color::srgb(0.95, 0.45, 0.1),
            _ => health_colour(share),
        };
        if colour.0 != tone {
            colour.0 = tone;
        }
    }
    for (picture, mut image) in &mut pictures {
        let tone = match sim.units[picture.0].alive() {
            true => Color::WHITE,
            false => Color::srgb(0.2, 0.2, 0.2),
        };
        if image.color != tone {
            image.color = tone;
        }
    }
    for (status, mut text) in &mut statuses {
        let ids = match *status {
            Status::Regiment(r) => regiment_state(sim, r),
            Status::Building(b) => building_state(sim, b),
        };
        let words = ids.map(|i| looks.state(i)).unwrap_or_default();
        if text.0 != words {
            text.0 = words;
        }
    }
}

/// What a regiment's doing, as the game's actor state ids.
fn regiment_state(sim: &World, r: usize) -> Option<&'static str> {
    let regiment = &sim.regiments[r];
    if sim.destroyed(r) {
        return None;
    }
    Some(match () {
        _ if !regiment.engaged.is_empty() => "mind_engaged",
        _ if matches!(regiment.order, Order::Pursue(_)) => "mind_attacking",
        _ if sim.underway(r) || regiment.unpack_at.is_some() => match regiment.order {
            Order::Run => "mind_retreating",
            Order::Attack | Order::Pursue(_) => "mind_attacking",
            Order::Move => "mind_moving",
        },
        _ => "mind_resting",
    })
}

/// What a building's doing, as the game's actor state ids; nothing for one
/// at rest.
fn building_state(sim: &World, b: usize) -> Option<&'static str> {
    let building = &sim.buildings[b];
    match () {
        _ if building.packing.is_some() => Some("structure_packing"),
        _ if building.unpacking.is_some() => Some("structure_unpacking"),
        _ if building.state == State::Disabled => Some("disabled"),
        _ if !building.queue.is_empty() => Some("structure_recruiting"),
        _ => None,
    }
}

fn unit_health(sim: &World, u: usize) -> f32 {
    let unit = &sim.units[u];
    match (&unit.fighter, unit.alive()) {
        (_, false) => 0.0,
        (Some(f), true) => unit.health / f.health,
        (None, true) => 1.0,
    }
}

/// Green when healthy, then yellow and red as health drops.
fn health_colour(share: f32) -> Color {
    match share {
        s if s > 0.66 => Color::srgb(0.1, 0.85, 0.1),
        s if s > 0.33 => Color::srgb(0.9, 0.85, 0.1),
        _ => Color::srgb(0.9, 0.15, 0.1),
    }
}

/// Whether a regiment is a single unit, such as a truck.
fn lone(sim: &World, r: usize) -> bool {
    let regiment = &sim.regiments[r];
    matches!(&regiment.units[..], [u] if sim.units[*u].template == regiment.template)
}

/// The unit whose portrait stands for the regiment: its commanding
/// officer's, else its first.
fn leader(sim: &World, r: usize) -> usize {
    let units = &sim.regiments[r].units;
    let officer = units
        .iter()
        .copied()
        .find(|&u| is_officer(&sim.units[u].template));
    officer.unwrap_or(units[0])
}

/// Whether a unit template is a regiment's commanding officer, which the
/// game names `..._co`.
fn is_officer(template: &str) -> bool {
    template.to_ascii_lowercase().ends_with("_co")
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn picture_node(picture: Option<Handle<Image>>) -> ImageNode {
    ImageNode::new(picture.unwrap_or_default())
}

/// A bar in `node`, on a dark track, filled as `fill` says.
fn bar(parent: &mut ChildSpawnerCommands, node: Node, fill: Fill) {
    parent
        .spawn((node, Surface::Track, Pickable::IGNORE))
        .with_child((
            Node {
                height: percent(100),
                width: percent(100),
                ..default()
            },
            BackgroundColor(Color::NONE),
            fill,
            Pickable::IGNORE,
        ));
}

/// A regiment's card clicked: selects it alone.
fn pick(activate: On<Activate>, cards: Query<&Pick>, mut selected: ResMut<Selected>) {
    if let Ok(card) = cards.get(activate.entity) {
        selected.0 = vec![card.0];
    }
}

/// One of a building's commissions clicked: calls it off.
fn cancel(
    activate: On<Activate>,
    places: Query<&Queued>,
    chosen: Res<SelectedBuilding>,
    local: Res<Local>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
) {
    if let (Ok(place), Some(b), Some(player)) = (places.get(activate.entity), chosen.0, local.0) {
        // By id, as what's at that place may change before it's carried out.
        if let Some(c) = sim.0.buildings[b].queue.get(place.0) {
            let cancel = aaa_sim::Command::CancelCommission {
                building: b,
                commission: c.id,
            };
            let _ = orders.give(&sim.0, player, cancel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regiments_are_numbered_in_english() {
        let words: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101].map(ordinal).into();
        assert_eq!(
            words,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st"
            ]
        );
    }
}
