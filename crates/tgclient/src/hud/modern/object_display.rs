//! Our object display, at the middle of the window's lower edge: what
//! [`Showing`] says, laid out as the game's own but without its chrome. Its
//! title names what's shown and its owner, and a line along the bottom says
//! what it earns and takes to keep, and what it's doing. What the pointer's
//! on says what it is in a box above the display.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;
use tgdata::{GameData, Id};
use tgrender::overlay::BannerImages;
use tgrender::portrait::Stage;
use tgsim::{Target, World};

use crate::actions::Blocks;
use crate::banners::Badges;
use crate::hud::original::{CardShows, Cards};
use crate::object_display::*;
use crate::ui::{self, Font, Gauge, Hint, Ink, Shade, Surface, framed, label, meter};
use crate::{Drawn, Leaders, Loaded, Local, Sim};

/// Our pixels to one of the game's interface units, in which its layout is
/// given, chosen so the display stands as tall as the action buttons beside it.
const SCALE: f32 = super::actions::SIZE.y / HEIGHT;
const WIDTH: f32 = 495.0;

/// The display's size, in logical pixels.
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

/// Where the game's layout puts an HQ's places, and how big the area they
/// share is and each of them.
const SLOTS_AT: Vec2 = Vec2::new(144.0, 44.0);
const SLOTS_AREA: Vec2 = Vec2::new(148.0, 101.0);
const SLOT: Vec2 = Vec2::new(42.0, 50.0);

/// Within a place, where its organization's banner goes and how big it is,
/// and its health bar's box.
const SLOT_BANNER: [f32; 3] = [2.0, 2.0, 38.0];
const SLOT_HEALTH: [f32; 4] = [2.0, 47.0, 38.0, 2.0];

/// The box of the bar a place held for an organization in the queue has
/// instead, of how far along it is.
const SLOT_PROGRESS: [f32; 4] = [2.0, 44.0, 38.0, 5.0];

/// Where the `i`th of an HQ's `count` places goes: across then down,
/// spread evenly over the area they share.
fn slot_at(i: usize, count: usize) -> [f32; 2] {
    (SLOTS_AT + spread(i, count, SLOTS_AREA, SLOT)).to_array()
}

/// The strip across the top: how tall it is, so what's below can start
/// clear of it, and how far its contents are in from its sides.
const HEADER: f32 = 40.0;
const HEADER_IN: f32 = 8.0;

/// The gap between the owner's gem and their name.
const OWNER_GAP: f32 = 6.0;

/// Each property's icon in the strip, and the gap between them.
const PROPERTY: f32 = 24.0;
const PROPERTY_GAP: f32 = 2.0;

/// What holds the properties' icons, so that they change without the
/// rest of the display being laid out again under the pointer.
#[derive(Component)]
pub struct PropertyRow;

/// The object display, whose children show what [`Showing`] says.
#[derive(Component)]
pub struct ObjectDisplay;

/// The line saying what an organization or building counts for in its side's
/// economy, with what it says now: see [`economy`](crate::object_display::economy).
#[derive(Component, Default)]
pub struct EconomyLine(Vec<(Id, String)>);

/// The game's layout's place for a lone thing's defence: its icon, with
/// the figure beside it.
const DEFENSE: [f32; 4] = [196.0, 46.0, 16.0, 16.0];

/// The game's layout's place for a lone thing's first weapon: its icon,
/// with the figure beside it, and how far down each next one goes.
const WEAPON: [f32; 4] = [144.0, 46.0, 16.0, 16.0];
const WEAPON_ROW: f32 = 21.0;

/// The game's layout's box for what a lone thing's template says of it,
/// and how far in its words start.
const DESCRIPTION: [f32; 4] = [244.0, 46.0, 239.0, 104.0];
const DESCRIPTION_IN: f32 = 5.0;

/// Where the portrait shows, holding the flat picture until there's a
/// portrait drawn to show.
#[derive(Component)]
pub struct PortraitBox;

/// Spawns our interface's object display, at the middle of the window's
/// lower edge.
pub fn spawn(mut commands: Commands) {
    // Above the display and as wide, growing upwards as its words wrap.
    let said = commands
        .spawn((
            ui::hint_box(Node {
                width: px(SIZE.x),
                ..default()
            }),
            GlobalZIndex(crate::ui::layer::HUD),
        ))
        .id();
    let display = commands
        .spawn((
            ObjectDisplay,
            ui::Docks(said),
            Blocks,
            RelativeCursorPosition::default(),
            ui::panel(
                Surface::Hud,
                Node {
                    width: px(SIZE.x),
                    height: px(SIZE.y),
                    ..default()
                },
            ),
            Visibility::Hidden,
            GlobalZIndex(crate::ui::layer::HUD),
        ))
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(crate::MARGIN),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(super::actions::PAD),
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(super::DRAWN),
        ))
        .add_children(&[said, display]);
}

/// Lays the display out afresh when what it should show changes.
#[allow(clippy::too_many_arguments)]
pub fn lay_out(
    mut commands: Commands,
    sim: Res<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    gem: Res<ui::Gem>,
    drawn: Res<Drawn>,
    badges: Res<Badges>,
    showing: Res<Showing>,
    loaded: Res<Loaded>,
    mut looks: ResMut<Looks>,
    mut banners: ResMut<BannerImages>,
    mut images: ResMut<Assets<Image>>,
    cards: Option<Res<Cards>>,
    mut display: Single<(Entity, &mut Visibility), With<ObjectDisplay>>,
    row: Query<Entity, With<PropertyRow>>,
) {
    if !showing.is_changed() {
        return;
    }
    // Only the icons, where nothing else has changed.
    let row = row.single().ok().filter(|_| showing.icons_only);
    let (root, visibility) = &mut *display;
    match row {
        Some(row) => drop(commands.entity(row).despawn_related::<Children>()),
        None => {
            commands.entity(*root).despawn_related::<Children>();
            visibility.set_if_neq(match showing.now {
                Some(_) => Visibility::Inherited,
                None => Visibility::Hidden,
            });
        }
    }
    let Some((mode, detail)) = &showing.now else {
        return;
    };
    let mut ui = Ui {
        cards: cards.as_deref(),
        sim: &sim.0,
        local: local.0,
        leaders: &leaders,
        gem: &gem,
        drawn: &drawn,
        badges: &badges,
        detail,
        game: &loaded.game,
        looks: &mut looks,
        banners: &mut banners,
        images: &mut images,
    };
    let root = *root;
    if let Some(row) = row {
        return ui.icons(&mut commands, row);
    }
    match mode {
        Mode::SimpleActor(actor) => ui.simple_actor(&mut commands, root, actor),
        Mode::Company(actor) => ui.company(&mut commands, root, actor),
        Mode::Settlement => {}
        Mode::Group(rs) => ui.group(&mut commands, root, rs),
    }
    if matches!(mode, Mode::Company(Actor::Organization(_)) | Mode::Group(_)) {
        formation_buttons(&mut commands, root);
    }
}

/// The game's layout's place for the formation buttons, the size of each
/// and how far apart their tops are.
const FORMATIONS_AT: Vec2 = Vec2::new(444.0, 36.0);
const FORMATION_BUTTON: f32 = 46.0;
const FORMATION_ROW: f32 = 47.0;

/// The formation buttons down the display's right edge, which
/// [`show`](crate::formations::show) fills.
fn formation_buttons(commands: &mut Commands, root: Entity) {
    commands.entity(root).with_children(|display| {
        for n in 0..crate::formations::MOST {
            let top = FORMATIONS_AT.y + FORMATION_ROW * n as f32;
            let node = at(FORMATIONS_AT.x, top, FORMATION_BUTTON, FORMATION_BUTTON);
            display.spawn((
                crate::formations::FormationButton(n),
                ui::called(format!("formation.{}", n + 1)),
                Button,
                Pickable::default(),
                Visibility::Hidden,
                Node {
                    border: UiRect::all(px(2)),
                    ..node
                },
                Surface::Slot,
                ui::Edge::Clear,
                children![(
                    crate::formations::FormationIcon,
                    ImageNode::default(),
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    Pickable::IGNORE,
                )],
            ));
        }
    });
}

/// What laying the display out reads.
struct Ui<'a> {
    /// The group display's cards, where the game's layouts have them.
    cards: Option<&'a Cards>,
    sim: &'a World,
    local: Option<u32>,
    leaders: &'a Leaders,
    gem: &'a ui::Gem,
    drawn: &'a Drawn,
    badges: &'a Badges,
    detail: &'a Detail,
    game: &'a GameData,
    looks: &'a mut Looks,
    banners: &'a mut BannerImages,
    images: &'a mut Assets<Image>,
}

impl Ui<'_> {
    fn picture(&mut self, template: &str) -> Option<Handle<Image>> {
        picture(self.drawn, self.looks, self.game, self.images, template)
    }

    fn organization_name(&self, r: usize) -> String {
        let looks = &self.looks;
        organization_name(self.sim, &looks.names, &looks.strings, &looks.map, r)
    }

    /// The strip across the top: the name over the owner, and, where it
    /// has `properties`, the icons for those that show on its right.
    fn title(
        &mut self,
        commands: &mut Commands,
        root: Entity,
        name: String,
        owner: u32,
        properties: bool,
    ) {
        let player = self.sim.sides.player(owner);
        let colour = owner_colour(self.sim, self.local, owner);
        let mut row = None;
        commands.entity(root).with_children(|display| {
            let strip = Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: px(0),
                right: px(0),
                height: px(HEADER * SCALE),
                padding: UiRect::horizontal(px(HEADER_IN * SCALE)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::top(px(ui::CORNER)),
                ..default()
            };
            display
                .spawn((strip, Surface::Slot, Pickable::IGNORE))
                .with_children(|strip| {
                    // Cut short rather than wrapped, which would outgrow the strip.
                    let lines = Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        min_width: px(0),
                        overflow: Overflow::clip(),
                        ..default()
                    };
                    let unwrapped = || TextLayout::new(Justify::Left, LineBreak::NoWrap);
                    strip
                        .spawn((lines, Pickable::IGNORE))
                        .with_children(|lines| {
                            lines.spawn((label(name, Font::HudTitle, Ink::Body), unwrapped()));
                            let Some(p) = player else {
                                return;
                            };
                            let line = Node {
                                column_gap: px(OWNER_GAP * SCALE),
                                ..default()
                            };
                            lines.spawn((line, Pickable::IGNORE)).with_children(|line| {
                                // The owner's gem in its own colour, whoever's looking,
                                // as the lobby marks it. An independent side's things
                                // show none.
                                if !p.independent {
                                    let size = px(ui::GEM * SCALE);
                                    let mark = Node {
                                        width: size,
                                        height: size,
                                        align_self: AlignSelf::Center,
                                        ..default()
                                    };
                                    line.spawn(ui::side_mark(self.gem, p.rgb, mark));
                                }
                                let owner = self.leaders.owner(p, &self.sim.sides, &self.looks.map);
                                line.spawn((ui::coloured(owner, Font::Hud, colour), unwrapped()));
                            });
                        });
                    if properties {
                        let icons = Node {
                            column_gap: px(PROPERTY_GAP * SCALE),
                            flex_shrink: 0.0,
                            ..default()
                        };
                        row = Some(strip.spawn((PropertyRow, icons, Pickable::IGNORE)).id());
                    }
                });
        });
        if let Some(row) = row {
            self.icons(commands, row);
        }
    }

    /// The portrait's box, holding `template`'s picture until [`portray`]
    /// has a portrait to show there, over a bar filled as `fill` says, both
    /// giving `of`'s tooltip.
    fn portrait(
        &mut self,
        commands: &mut Commands,
        root: Entity,
        template: &str,
        fill: Fill,
        of: Says,
    ) {
        let picture = self.picture(template);
        let [left, top, width, height] = PORTRAIT_BOX;
        let [in_left, in_top, _, in_bottom] = PORTRAIT_IN;
        commands.entity(root).with_children(|display| {
            display.spawn(framed(at(left, top, width, height)));
            let inside = || at(left + in_left, top + in_top, PORTRAIT.x, PORTRAIT.y);
            // What shows round a portrait that doesn't fill its box.
            display.spawn((inside(), BackgroundColor(Color::BLACK), Pickable::IGNORE));
            display.spawn((inside(), picture_node(picture), PortraitBox, says(of)));
            let bar_top = top + height - in_bottom + 1.0;
            bar(
                display,
                at(left + in_left, bar_top, PORTRAIT.x, in_bottom - 3.0),
                fill,
                Some(of),
            );
        });
    }

    /// The organization at `r`'s efficiency bar in `node`, on black, with a
    /// marker at what it's easing towards.
    fn efficiency(&mut self, parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
        let path = self.looks.efficiency_target.clone();
        let marker = self.looks.image(self.game, self.images, path.as_deref());
        let size = (marker.as_ref())
            .and_then(|h| self.images.get(h))
            .map(Image::size_f32);
        let mut bar = parent.spawn((
            meter(node, Gauge::Efficiency),
            Fill::Efficiency(r),
            BackgroundColor(Color::BLACK),
        ));
        if let (Some(marker), Some(size)) = (marker, size) {
            bar.with_child((
                Node {
                    position_type: PositionType::Absolute,
                    top: px(0),
                    left: towards(self.sim, r),
                    width: px(size.x * SCALE),
                    height: px(size.y * SCALE),
                    margin: UiRect::left(px(-size.x * SCALE)),
                    ..default()
                },
                ImageNode::new(marker),
                EfficiencyTarget(r),
                Pickable::IGNORE,
            ));
        }
    }

    /// An icon for each property that shows.
    fn icons(&mut self, commands: &mut Commands, row: Entity) {
        for id in &self.detail.properties {
            let Some(look) = self.looks.properties.get(id).cloned() else {
                continue;
            };
            let icon = self.looks.image(self.game, self.images, Some(&look.icon));
            let said = std::iter::once(look.name).chain(look.lines);
            let node = Node {
                width: px(PROPERTY * SCALE),
                height: px(PROPERTY * SCALE),
                ..default()
            };
            commands.entity(row).with_child((
                node,
                picture_node(icon),
                Hint(said.collect::<Vec<_>>().join("\n")),
            ));
        }
    }

    /// The line along the bottom saying what it counts for in its side's
    /// economy, and what it's doing.
    fn status(&self, commands: &mut Commands, root: Entity, status: Status) {
        commands.entity(root).with_children(|display| {
            display
                .spawn((at(144.0, 158.0, 286.0, 18.0), Surface::Track))
                .with_children(|line| {
                    // Filled in by [`refresh`], as what it counts for changes.
                    line.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(4),
                            top: px(0),
                            bottom: px(0),
                            align_items: AlignItems::Center,
                            column_gap: px(6),
                            ..default()
                        },
                        EconomyLine::default(),
                        status,
                        Pickable::IGNORE,
                    ));
                    line.spawn((
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
        });
    }

    /// An organization's banner, as it flies over the organization, filling
    /// `node`.
    fn banner(&mut self, parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
        let layers = Flown {
            sim: self.sim,
            detail: self.detail,
            leaders: self.leaders,
            badges: self.badges,
            game: self.game,
        }
        .layers(self.banners, self.images, r);
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

    /// An HQ's places: each with the banner and health of the organization in
    /// it, which a click selects, with the picture and progress of what
    /// `building`'s queue holds it for, which a click calls off, or empty.
    fn slots(&mut self, commands: &mut Commands, root: Entity, building: Option<usize>) {
        let count = self.detail.slots.len();
        for (i, place) in self.detail.slots.clone().into_iter().enumerate() {
            let [left, top] = slot_at(i, count);
            let node = at(left, top, SLOT.x, SLOT.y);
            let mut card = commands.spawn((framed(node), ChildOf(root)));
            let [left, top, side] = SLOT_BANNER;
            let inside = at(left, top, side, side);
            match (place, building) {
                (Place::Organization(r), _) => {
                    let id = ui::called(format!("object.slot.{}", i + 1));
                    card.insert((Button, id, Pick(r), says(Says::Organization(r))));
                    card.observe(pick);
                    card.with_children(|card| {
                        self.banner(card, inside, r);
                        let [left, top, width, height] = SLOT_HEALTH;
                        bar(
                            card,
                            at(left, top, width, height),
                            Fill::Organization(r),
                            None,
                        );
                    });
                }
                (Place::Queued(n), Some(b)) => {
                    let queue = &self.sim.buildings[b].queue;
                    let template = queue.get(n).map(|c| c.template().to_string());
                    let picture = template.and_then(|t| self.picture(&t));
                    let of = Says::Queued {
                        building: b,
                        place: n,
                        held: true,
                    };
                    let id = ui::called(format!("object.slot.{}", i + 1));
                    card.insert((Button, id, Queued(n), says(of)))
                        .observe(cancel);
                    card.with_children(|card| {
                        card.spawn((inside, picture_node(picture), Pickable::IGNORE));
                        let [left, top, width, height] = SLOT_PROGRESS;
                        let node = at(left, top, width, height);
                        // Only the first is under way.
                        match n {
                            0 => bar(card, node, Fill::Progress(b), None),
                            _ => drop(card.spawn(meter(node, Gauge::Progress))),
                        }
                    });
                }
                (Place::Flown(d), _) => {
                    let template = self.sim.deliveries[d].template.as_str();
                    let picture = self.picture(template);
                    card.insert(Hint(template_name(&self.looks.names, template)));
                    card.with_children(|card| {
                        card.spawn((inside, picture_node(picture), Pickable::IGNORE));
                        let [left, top, width, height] = SLOT_HEALTH;
                        bar(card, at(left, top, width, height), Fill::Flight, None);
                    });
                }
                _ => drop(card.insert(Hint(self.looks.said(EMPTY_SLOT)))),
            }
        }
    }

    /// The simple actor display: a building with what it's commissioning,
    /// or a unit on its own.
    fn simple_actor(&mut self, commands: &mut Commands, root: Entity, actor: &Actor) {
        match *actor {
            Actor::Organization(r) => {
                self.heading(commands, root, r);
                let organization = &self.sim.organizations[r];
                let captain = captain(self.sim, self.looks, r);
                let unit = self.sim.units[captain].template.clone();
                self.attacks(commands, root, Target::Unit(captain));
                self.defense(commands, root, Target::Unit(captain), &unit);
                let says = [&organization.template, &unit];
                let says = says.into_iter().find_map(|t| self.described(t));
                self.description(commands, root, says);
                self.status(commands, root, Status::Organization(r));
            }
            Actor::Building(b, ref queue) => {
                self.structure(commands, root, b);
                let template = self.sim.buildings[b].template.clone();
                self.attacks(commands, root, Target::Building(b));
                self.defense(commands, root, Target::Building(b), &template);
                // What it's commissioning shows where its description would.
                match queue.is_empty() {
                    true => self.description(commands, root, self.described(&template)),
                    false => self.queue(commands, root, b),
                }
                self.status(commands, root, Status::Building(b));
            }
        }
    }

    /// What `template` says of itself, if anything.
    fn described(&self, template: &Id) -> Option<String> {
        self.looks.listings.get(template)?.description.clone()
    }

    /// The shield and the defence of `target`, for one with a body, whose
    /// `template` gives the icon.
    fn defense(&mut self, commands: &mut Commands, root: Entity, target: Target, template: &Id) {
        if self.sim.defense(target).is_none() {
            return;
        }
        let icon = (self.looks.listings.get(template)).and_then(|l| l.defense_icon.clone());
        let icon = self.looks.image(self.game, self.images, icon.as_deref());
        let [left, top, width, height] = DEFENSE;
        commands.entity(root).with_children(|display| {
            let row = Node {
                align_items: AlignItems::Center,
                column_gap: px(2.0 * SCALE),
                ..at(left, top, 40.0, height)
            };
            // Filled in by [`refresh`], as its defence changes.
            let figure = ui::coloured("", Font::Small, Color::WHITE);
            let figure = (figure, Defends(target, None));
            let shield = Node {
                width: px(width * SCALE),
                height: px(height * SCALE),
                ..default()
            };
            display
                .spawn((row, Shield(target), Hint(String::new())))
                .with_children(|row| {
                    if let Some(icon) = icon {
                        row.spawn((shield, ImageNode::new(icon), Pickable::IGNORE));
                    }
                    row.spawn(figure);
                });
        });
    }

    /// `target`'s weapons, each its icon with its main damage by it, a row
    /// each.
    fn attacks(&mut self, commands: &mut Commands, root: Entity, target: Target) {
        let icons: Vec<Option<String>> = (self.sim.weapons(target).iter())
            .take(MOST_WEAPONS)
            .map(|w| w.shown.icon.clone())
            .collect();
        for (i, icon) in icons.into_iter().enumerate() {
            let icon = self.looks.image(self.game, self.images, icon.as_deref());
            let [left, top, width, height] = WEAPON;
            let top = top + WEAPON_ROW * i as f32;
            commands.entity(root).with_children(|display| {
                let row = Node {
                    align_items: AlignItems::Center,
                    column_gap: px(2.0 * SCALE),
                    ..at(left, top, 50.0, height)
                };
                // Filled in by [`refresh`], as its damage changes.
                let figure = ui::coloured("", Font::Small, Color::WHITE);
                let figure = (figure, Strikes(target, i, None));
                let icon_node = Node {
                    width: px(width * SCALE),
                    height: px(height * SCALE),
                    ..default()
                };
                display
                    .spawn((row, Armed(target, i), Hint(String::new())))
                    .with_children(|row| {
                        if let Some(icon) = icon {
                            row.spawn((icon_node, ImageNode::new(icon), Pickable::IGNORE));
                        }
                        row.spawn(figure);
                    });
            });
        }
    }

    /// The box of what a lone thing's template says of it; none for one
    /// that says nothing.
    fn description(&self, commands: &mut Commands, root: Entity, says: Option<String>) {
        let Some(says) = says else {
            return;
        };
        let [left, top, width, height] = DESCRIPTION;
        let node = Node {
            padding: UiRect::all(px(DESCRIPTION_IN * SCALE)),
            ..at(left, top, width, height)
        };
        commands.entity(root).with_children(|display| {
            display.spawn((framed(node), children![label(says, Font::Hud, Ink::Body)]));
        });
    }

    /// The company display: an organization with its units, or an HQ with its
    /// places.
    fn company(&mut self, commands: &mut Commands, root: Entity, actor: &Actor) {
        match *actor {
            Actor::Organization(r) => self.organization(commands, root, r),
            Actor::Building(b, _) => {
                self.structure(commands, root, b);
                // Its places show what it's been asked for.
                self.slots(commands, root, Some(b));
                self.status(commands, root, Status::Building(b));
            }
        }
    }

    /// An organization's name and owner, and its captain's portrait over the
    /// captain's health.
    fn heading(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        let organization = &self.sim.organizations[r];
        self.title(
            commands,
            root,
            self.organization_name(r),
            organization.owner,
            true,
        );
        let captain = captain(self.sim, self.looks, r);
        let template = self.sim.units[captain].template.clone();
        let (fill, of) = (Fill::Unit(captain), Says::Unit(captain));
        self.portrait(commands, root, template.as_str(), fill, of);
    }

    fn organization(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        self.heading(commands, root, r);
        let captain = captain(self.sim, self.looks, r);
        // An HQ shows its places where an organization shows its units.
        let is_hq = !self.detail.slots.is_empty();
        if !lone(self.sim, r) && !is_hq {
            // A block of cards for each kind of slot, as the game's has
            // them, for all but an officer, who has the portrait.
            let groups = card_groups(self.sim, self.looks, r, captain);
            let card = Vec2::new(42.0, 47.0);
            let places = card_places(
                &groups,
                card,
                Vec2::splat(2.0),
                5.0,
                Vec2::new(210.0, 110.0),
            );
            let pictures: Vec<_> = places
                .iter()
                .map(|&(u, _)| self.picture(self.sim.units[u].template.as_str()))
                .collect();
            commands.entity(root).with_children(|display| {
                display
                    .spawn((at(144.0, 44.0, 210.0, 110.0), Pickable::IGNORE))
                    .with_children(|cards| {
                        for (&(u, place), picture) in places.iter().zip(pictures) {
                            let card = framed(Node {
                                flex_direction: FlexDirection::Column,
                                ..at(place.x, place.y, card.x, card.y)
                            });
                            cards
                                .spawn((card, says(Says::Unit(u))))
                                .with_children(|card| {
                                    card.spawn((
                                        Node {
                                            width: percent(100),
                                            flex_grow: 1.0,
                                            ..default()
                                        },
                                        picture_node(picture),
                                        UnitPicture(u),
                                        Shade::Full,
                                    ));
                                    bar(
                                        card,
                                        Node {
                                            width: percent(100),
                                            height: px(4.0 * SCALE),
                                            ..default()
                                        },
                                        Fill::Unit(u),
                                        None,
                                    );
                                });
                        }
                    });
            });
        }
        if !lone(self.sim, r) {
            commands.entity(root).with_children(|display| {
                display
                    .spawn((
                        framed(at(366.0, 44.0, 63.0, 77.0)),
                        says(Says::Organization(r)),
                    ))
                    .with_children(|card| {
                        self.efficiency(card, at(0.0, 6.0, 63.0, 2.0), r);
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
                        morale(card, at(0.0, 69.0, 63.0, 2.0), r);
                    });
            });
        }
        self.slots(commands, root, None);
        self.status(commands, root, Status::Organization(r));
    }

    /// The group display: a card per organization.
    fn group(&mut self, commands: &mut Commands, root: Entity, rs: &[usize]) {
        let first = rs[0];
        self.title(
            commands,
            root,
            self.organization_name(first),
            self.sim.organizations[first].owner,
            false,
        );
        // The original's cards, so without its layouts there are none.
        let Some(cards) = self.cards else {
            return;
        };
        let mut left = 8.0;
        for r in newest_first(rs) {
            let lone = lone(self.sim, r);
            let (Some(size), Some(top)) = (cards.size(lone), cards.top(lone)) else {
                continue;
            };
            // Under the header, which has the name the card's top is for,
            // and cut off at it so the header keeps the pointer.
            let holder = Node {
                overflow: Overflow::clip(),
                ..at(left, HEADER, size.x, size.y - top)
            };
            left += size.x + 4.0;
            commands.entity(root).with_children(|display| {
                display
                    .spawn((holder, Pickable::IGNORE))
                    .with_children(|holder| {
                        holder
                            .spawn((at(0.0, -top, size.x, size.y), Pickable::IGNORE))
                            .with_children(|s| {
                                let shows = CardShows {
                                    sim: self.sim,
                                    local: self.local,
                                    leaders: self.leaders,
                                    gem: self.gem,
                                    drawn: self.drawn,
                                    badges: self.badges,
                                    detail: self.detail,
                                    game: self.game,
                                    looks: &mut *self.looks,
                                    banners: &mut *self.banners,
                                    images: &mut *self.images,
                                };
                                cards.draw(s, shows, r);
                            });
                    });
            });
        }
    }

    /// A building's name and owner, and its picture over its health and,
    /// for a relay, its stockpile.
    fn structure(&mut self, commands: &mut Commands, root: Entity, b: usize) {
        let building = &self.sim.buildings[b];
        let template = building.template.clone();
        let looks = &self.looks;
        let name = building_name(self.sim, &looks.names, &looks.strings, &looks.map, b);
        self.title(commands, root, name, building.owner, true);
        let of = Says::Building(b);
        self.portrait(commands, root, template.as_str(), Fill::Building(b), of);
        if self.sim.supply_of(b).is_some_and(|s| s.relay) {
            let node = at(PORTRAIT_BOX[0], 176.0, PORTRAIT_BOX[2], 3.0);
            commands.entity(root).with_children(|display| {
                bar(display, node, Fill::Stockpile(b), Some(of));
            });
        }
    }

    /// What a building is commissioning, the first with its progress.
    fn queue(&mut self, commands: &mut Commands, root: Entity, b: usize) {
        let queue = self.sim.buildings[b].queue.iter();
        let queue: Vec<String> = queue.map(|c| c.template().to_string()).collect();
        // Commissions are trucks, which show their building's button.
        let pictures: Vec<_> = queue.iter().map(|t| self.picture(t)).collect();
        commands.entity(root).with_children(|display| {
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
                let of = Says::Queued {
                    building: b,
                    place: i,
                    held: false,
                };
                let card = (framed(node), picture_node(picture), Button, Queued(i));
                let id = ui::called(format!("object.queued.{}", i + 1));
                display.spawn((card, id, says(of))).observe(cancel);
            }
            if !queue.is_empty() {
                let of = Says::Queued {
                    building: b,
                    place: 0,
                    held: false,
                };
                let node = at(240.0, 110.0, 64.0, 4.0);
                bar(display, node, Fill::Progress(b), Some(of));
            }
        });
    }
}

/// A bar in `node`, filled as `fill` says, giving `of`'s tooltip where
/// it has one of its own rather than leaving that to what it's on.
/// The organization at `r`'s morale bar.
fn morale(parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
    let gauge = meter(node, Gauge::Morale);
    parent.spawn((gauge, Fill::Morale(r), BackgroundColor(Color::BLACK)));
}

fn bar(parent: &mut ChildSpawnerCommands, node: Node, fill: Fill, of: Option<Says>) {
    let gauge = match fill {
        Fill::Progress(_) => Gauge::Progress,
        Fill::Stockpile(_) => Gauge::Stockpile,
        _ => Gauge::Health,
    };
    let mut bar = parent.spawn((meter(node, gauge), fill));
    if let Some(of) = of {
        // A meter takes no pointer of its own, which a tooltip needs.
        bar.insert((says(of), Pickable::default()));
    }
}

/// Shows the portrait in its box once it's there to show.
pub fn portray(
    stage: Res<Stage>,
    mut boxes: Query<(&mut ImageNode, &mut Node), With<PortraitBox>>,
) {
    let Some((portrait, fitted)) = stage.picture() else {
        return;
    };
    // A fitted model's picture keeps its shape, in a band across the box.
    let height = match fitted {
        true => PORTRAIT.x / tgrender::portrait::FIT_ASPECT,
        false => PORTRAIT.y,
    };
    let top = PORTRAIT_BOX[1] + PORTRAIT_IN[1] + (PORTRAIT.y - height) / 2.0;
    for (mut shown, mut node) in &mut boxes {
        if shown.image != portrait {
            shown.image = portrait.clone();
            // A face's picture fills its box whatever shape that is.
            shown.image_mode = bevy::ui::widget::NodeImageMode::Stretch;
            node.top = px(top * SCALE);
            node.height = px(height * SCALE);
        }
    }
}

/// Keeps the economy line up to date: what the thing it's of counts for
/// in its side's economy, by each resource's glyph.
pub fn economy(
    mut commands: Commands,
    sim: Res<Sim>,
    local: Res<Local>,
    looks: Res<Looks>,
    mut lines: Query<(Entity, &Status, &mut EconomyLine)>,
) {
    let sim = &sim.0;
    let resources = &sim.rules().economy.resources;
    for (row, status, mut line) in &mut lines {
        let now = (counted(sim, local.0, *status))
            .map_or_else(Vec::new, |l| crate::object_display::economy(&l, resources));
        if line.0 == now {
            continue;
        }
        commands.entity(row).despawn_related::<Children>();
        commands.entity(row).with_children(|row| {
            for (kind, figure) in &now {
                let item = Node {
                    align_items: AlignItems::Center,
                    column_gap: px(1),
                    ..default()
                };
                row.spawn((item, Pickable::IGNORE)).with_children(|item| {
                    if let Some(glyph) = looks.glyphs.get(kind) {
                        item.spawn((
                            ImageNode::new(glyph.clone()),
                            Node {
                                width: px(14),
                                height: px(14),
                                ..default()
                            },
                        ));
                    }
                    item.spawn(label(figure.clone(), Font::Hud, Ink::Body));
                });
            }
        });
        line.0 = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_hqs_places_spread_over_their_area() {
        // Three across, the last flush with the area's right edge.
        assert_eq!(slot_at(0, 6), [144.0, 44.0]);
        assert_eq!(slot_at(1, 6), [197.0, 44.0]);
        assert_eq!(slot_at(2, 6), [250.0, 44.0]);
        assert_eq!(slot_at(2, 6)[0] + SLOT.x, SLOTS_AT.x + SLOTS_AREA.x);
        // Then down, the second row flush with its bottom.
        assert_eq!(slot_at(3, 6), [144.0, 95.0]);
        assert_eq!(slot_at(5, 6)[1] + SLOT.y, SLOTS_AT.y + SLOTS_AREA.y);
        // A single row sits where the first of two would.
        assert_eq!(slot_at(2, 3), slot_at(2, 6));
    }
}
