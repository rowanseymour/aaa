//! The object display along the bottom, between the minimap and the action
//! buttons, describing what's selected, laid out as the game's own but
//! without its chrome. Its title names it and its owner, and a line along
//! the bottom says what it's doing. It shows in one of the game's four
//! [`Mode`]s:
//!
//! - Simple actor, for a building or a unit on its own: its portrait over
//!   its health, and what a building's been asked to commission, the first
//!   with its progress; clicking one calls it off.
//! - Company, for a regiment: its officer's portrait over the regiment's
//!   health, a card per other unit with the unit's health, and its banner.
//!   An HQ, packed or not, has instead of the cards a place for each
//!   regiment it takes, with the regiment attached there or the one it's
//!   been asked for.
//! - Group, for several: a card each, up to [`MAX_SELECTED`], with the
//!   banner, the regiment's health and each unit's; clicking one selects
//!   it alone.
//! - Settlement, for a city.
//!
//! Beside the title of a regiment, or of the building one unpacked into,
//! is an icon for each property it holds that has one. What the pointer's
//! on says what it is in a box above the display.
//!
//! The portrait of one thing is drawn from a model: the face its template
//! names, a regiment's being its commanding officer's, its owner's general
//! where its template says so, else all of its own model. Its flat picture
//! stands in only where none of those can be drawn.

use std::collections::HashMap;
use std::sync::Arc;

use aaa_data::{GUARDING, GameData, Id, PropertyLook};
use aaa_render::MapAssets;
use aaa_render::overlay::BannerImages;
use aaa_render::portrait::{Sitter, Stage};
use aaa_render::textures;
use aaa_sim::{Order, State, World};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::Blocks;
use crate::banners::{self, Badges};
use crate::ui::{self, Font, Gauge, Hint, Ink, Meter, Shade, Surface, framed, label, meter};
use crate::{
    Drawn, Leaders, Loaded, Local, Orders, Screen, Selected, SelectedBuilding, Sim, TINTS, tint,
};

/// How many regiments can be selected at once, as many as the group
/// display has cards for.
pub const MAX_SELECTED: usize = 6;

/// Our pixels to one of the game's interface units, in which its layout is
/// given, chosen so the display stands as tall as the action buttons beside it.
const SCALE: f32 = crate::actions::SIZE.y / HEIGHT;
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

/// The game's layout's box for the portrait, and how far in from its
/// left, top, right and bottom the portrait itself is.
const PORTRAIT_BOX: [f32; 4] = [9.0, 47.0, 129.0, 124.0];
const PORTRAIT_IN: [f32; 4] = [2.0, 2.0, 2.0, 7.0];
/// The portrait's size within its box, in the game's interface units.
const PORTRAIT: Vec2 = Vec2::new(
    PORTRAIT_BOX[2] - PORTRAIT_IN[0] - PORTRAIT_IN[2],
    PORTRAIT_BOX[3] - PORTRAIT_IN[1] - PORTRAIT_IN[3],
);
/// How many pixels the portrait is drawn with for each of the logical
/// pixels it shows in, enough for the densest screens.
const PORTRAIT_DENSITY: f32 = 2.0;

/// Where the game's layout puts an HQ's places, and how big the area they
/// share is and each of them.
const SLOTS_AT: Vec2 = Vec2::new(144.0, 44.0);
const SLOTS_AREA: Vec2 = Vec2::new(148.0, 101.0);
const SLOT: Vec2 = Vec2::new(42.0, 50.0);
/// How many places go across before the next row, and how many the
/// display has room for.
const SLOTS_ACROSS: usize = 3;
const MAX_SLOTS: usize = 6;
/// Within a place, where its regiment's banner goes and how big it is,
/// and its health bar's box.
const SLOT_BANNER: [f32; 3] = [2.0, 2.0, 38.0];
const SLOT_HEALTH: [f32; 4] = [2.0, 47.0, 38.0, 2.0];
/// The box of the bar a place held for a regiment in the queue has
/// instead, of how far along it is.
const SLOT_PROGRESS: [f32; 4] = [2.0, 44.0, 38.0, 5.0];

/// Where the `i`th of an HQ's `count` places goes: across then down,
/// spread evenly over the area they share.
fn slot_at(i: usize, count: usize) -> [f32; 2] {
    let rows = count.div_ceil(SLOTS_ACROSS).max(2);
    let gap = |area: f32, slot: f32, n: usize| (area - n as f32 * slot) / (n - 1) as f32;
    let step = SLOT
        + Vec2::new(
            gap(SLOTS_AREA.x, SLOT.x, SLOTS_ACROSS),
            gap(SLOTS_AREA.y, SLOT.y, rows),
        );
    let place = Vec2::new((i % SLOTS_ACROSS) as f32, (i / SLOTS_ACROSS) as f32);
    (SLOTS_AT + place * step).to_array()
}

/// The strings' keys for what an HQ's empty place says under the pointer,
/// and a relay's stockpile bar, given what's left and what it holds.
const EMPTY_SLOT: &str = "object_display_CompanyDisplay_HQAttached_HQAttachedOrganization_ToolTip";
const STOCKPILE: &str = "template_game_SupplyStockpileBar_ToolTip|ff";

/// The icons for what's selected's properties run along the top into the
/// display's right corner, clear of the longest name: how far the last is
/// from the right edge and the row from the top, how big each is and how
/// far it is to the next.
const PROPERTIES_IN: Vec2 = Vec2::new(8.0, 6.0);
const PROPERTY: f32 = 24.0;
const PROPERTY_PITCH: f32 = 26.0;
/// As many as there are places for.
const MAX_PROPERTIES: usize = 7;

/// What holds the properties' icons, so that they change without the
/// rest of the display being laid out again under the pointer.
#[derive(Component)]
pub struct PropertyRow;

/// The object display, whose children show what [`Showing`] says.
#[derive(Component)]
pub struct ObjectDisplay;

/// What the display shows now.
#[derive(Resource, Default)]
pub struct Showing(Option<(Mode, Detail)>);

/// What of the display's layout can change while it shows the same thing.
#[derive(Clone, PartialEq, Debug, Default)]
struct Detail {
    /// For an HQ, what's in each of its places.
    slots: Vec<Place>,
    /// For each regiment whose banner shows, the badge it wears and
    /// whether it's out of supply.
    banners: Vec<(usize, Option<banners::Worn>, bool)>,
    /// The properties whose icons show, those held longest first.
    properties: Vec<Id>,
}

/// What's in one of an HQ's places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    Empty,
    /// The regiment attached there.
    Regiment(usize),
    /// What the place is held for, by where it is in the HQ's queue.
    Queued(usize),
}

/// Which of the game's displays shows, and what of; a change in it lays
/// the display out afresh.
#[derive(Clone, PartialEq, Debug)]
enum Mode {
    /// A building's, or that of a unit on its own, such as a truck.
    SimpleActor(Actor),
    /// A regiment's, or an HQ's, packed or not.
    Company(Actor),
    /// A city's.
    #[allow(dead_code)]
    Settlement,
    /// That of several regiments.
    Group(Vec<usize>),
}

impl Mode {
    /// The one thing it shows, if it shows one.
    fn actor(&self) -> Option<&Actor> {
        match self {
            Mode::SimpleActor(actor) | Mode::Company(actor) => Some(actor),
            Mode::Settlement | Mode::Group(_) => None,
        }
    }
}

/// What a display of one thing shows.
#[derive(Clone, PartialEq, Debug)]
enum Actor {
    Regiment(usize),
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
    /// How much of a relay's supply stockpile is left.
    Stockpile(usize),
}

/// The line saying what a regiment or building is doing.
#[derive(Component)]
pub enum Status {
    Regiment(usize),
    Building(usize),
}

/// Where the portrait shows, holding the flat picture until there's a
/// portrait drawn to show.
#[derive(Component)]
pub struct PortraitBox;

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

/// What the display is drawn with, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Templates' names, by id.
    names: HashMap<Id, String>,
    strings: Arc<aaa_data::Strings>,
    /// How each property that has an icon shows, by id.
    properties: Arc<HashMap<Id, PropertyLook>>,
    /// Each technology's research button icon, by id, for what's being
    /// researched.
    technologies: HashMap<Id, String>,
    /// The template in each organization's primary slot, by the
    /// organization's; none for one with no such slot.
    primaries: HashMap<Id, Option<Id>>,
    images: HashMap<String, Option<Handle<Image>>>,
    formats: bevy::image::CompressedImageFormats,
}

impl Looks {
    fn image(
        &mut self,
        game: &GameData,
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

    /// What the game's strings have under `key`.
    fn said(&self, key: &str) -> String {
        self.strings.get(key).cloned().unwrap_or_default()
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
    loaded: Res<Loaded>,
    formats: Res<CompressedImageFormatSupport>,
    mut images: ResMut<Assets<Image>>,
) {
    let width = (PORTRAIT.x * SCALE * PORTRAIT_DENSITY).round() as u32;
    let stage = Stage::new(&mut commands, &mut images, width);
    commands
        .entity(stage.camera())
        .insert(DespawnOnExit(Screen::Match));
    commands.insert_resource(stage);
    let game = &loaded.game;
    let layouts = game.layouts();
    let organizations = game.organizations();
    let primaries = organizations.iter().map(|(id, o)| {
        let slot = layouts.get(&o.layout).and_then(|l| l.primary);
        (id.clone(), slot.and_then(|s| o.slots.get(s)?.clone()))
    });
    commands.insert_resource(Looks {
        primaries: primaries.collect(),
        names: (game.listings().iter())
            .map(|(id, l)| (id.clone(), l.name.clone()))
            .collect(),
        strings: game.strings(),
        properties: game.property_looks(),
        technologies: (game.technologies().iter())
            .filter_map(|(id, t)| Some((id.clone(), t.icon.clone()?)))
            .collect(),
        images: HashMap::new(),
        formats: formats.0,
    });
    commands.insert_resource(Showing::default());
    // Above the display and as wide, growing upwards as its words wrap.
    let said = commands
        .spawn((
            ui::hint_box(Node {
                width: px(SIZE.x),
                ..default()
            }),
            GlobalZIndex(1),
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
            GlobalZIndex(1),
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
                row_gap: px(crate::actions::PAD),
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ))
        .add_children(&[said, display]);
}

/// The mode the selection calls for the display to show in.
fn wanted(
    sim: &World,
    hqs: &[aaa_sim::Hq],
    selected: &[usize],
    chosen: Option<usize>,
) -> Option<Mode> {
    let actor = match (selected, chosen) {
        ([], Some(b)) => {
            let queue = sim.buildings[b].queue.iter();
            Actor::Building(b, queue.map(|c| c.template().to_string()).collect())
        }
        ([], None) => return None,
        ([r], _) => Actor::Regiment(*r),
        (rs, _) => {
            return Some(Mode::Group(rs.iter().take(MAX_SELECTED).copied().collect()));
        }
    };
    let company = match actor {
        Actor::Regiment(r) => !lone(sim, r) || hq(sim, hqs, &actor).is_some(),
        Actor::Building(..) => hq(sim, hqs, &actor).is_some(),
    };
    Some(match company {
        true => Mode::Company(actor),
        false => Mode::SimpleActor(actor),
    })
}

/// The HQ `actor` is, if it's one: a Division HQ's building, or its truck.
fn hq<'a>(sim: &World, hqs: &'a [aaa_sim::Hq], actor: &Actor) -> Option<&'a aaa_sim::Hq> {
    let id = match *actor {
        Actor::Building(b, _) => sim.hq_of(b)?,
        Actor::Regiment(r) => sim.regiments[r].id,
    };
    hqs.iter().find(|h| h.id == id)
}

/// The HQ the display shows, if it shows one.
fn shown_hq<'a>(sim: &World, hqs: &'a [aaa_sim::Hq], mode: &Mode) -> Option<&'a aaa_sim::Hq> {
    match mode {
        Mode::Company(actor) => hq(sim, hqs, actor),
        _ => None,
    }
}

/// The regiment whose properties show: the one selected alone, or the one
/// that unpacked into the building.
fn holder(sim: &World, mode: &Mode) -> Option<usize> {
    match mode.actor()? {
        Actor::Regiment(r) => Some(*r),
        Actor::Building(b, _) => (sim.regiments.iter()).position(|r| r.unpacked_into == Some(*b)),
    }
}

/// The properties a regiment holds that have an icon: its template's own,
/// then its terrain's, its stance's and those it has for a time. Those in
/// `shown` come first, as they stand there, so that icons keep the order
/// their properties came in while their regiment stays selected.
fn held(sim: &World, looks: &HashMap<Id, PropertyLook>, r: usize, shown: &[Id]) -> Vec<Id> {
    let regiment = &sim.regiments[r];
    let kind = sim.rules().kinds.get(&Id::from(regiment.template.as_str()));
    let own = kind.into_iter().flat_map(|k| &k.properties).cloned();
    let terrain = regiment.terrain_properties.iter().cloned();
    let guarding =
        (regiment.unpacked_into.is_none() && regiment.guarding()).then(|| Id::from(GUARDING));
    let timed = regiment.properties.iter().map(|h| h.property.clone());
    let mut now: Vec<Id> = Vec::new();
    for p in own.chain(terrain).chain(guarding).chain(timed) {
        if looks.contains_key(&p) && !now.contains(&p) {
            now.push(p);
        }
    }
    ordered(now, shown)
}

/// The properties held `now` as their icons show: those already `shown`
/// as they stand there, then the rest, as many as there are places for.
fn ordered(now: Vec<Id>, shown: &[Id]) -> Vec<Id> {
    let kept = shown.iter().filter(|p| now.contains(p)).cloned();
    let mut out: Vec<Id> = kept.collect();
    out.extend(now.into_iter().filter(|p| !shown.contains(p)));
    out.truncate(MAX_PROPERTIES);
    out
}

fn detail(
    sim: &World,
    hqs: &[aaa_sim::Hq],
    badges: &Badges,
    looks: &Looks,
    mode: &Mode,
    shown: &[Id],
) -> Detail {
    let queued = match mode.actor() {
        Some(Actor::Building(_, queue)) => queue.len(),
        _ => 0,
    };
    let slots: Vec<Place> = shown_hq(sim, hqs, mode).map_or(Vec::new(), |hq| {
        let attached = sim.attached(hq.id).map(Place::Regiment);
        let places = (hq.places as usize).min(MAX_SLOTS);
        attached
            .chain((0..queued).map(Place::Queued))
            .chain(std::iter::repeat(Place::Empty))
            .take(places)
            .collect()
    });
    let own: &[usize] = match mode {
        Mode::Group(rs) => rs,
        _ => match mode.actor() {
            Some(Actor::Regiment(r)) => std::slice::from_ref(r),
            _ => &[],
        },
    };
    let attached = slots.iter().filter_map(|place| match place {
        Place::Regiment(r) => Some(*r),
        _ => None,
    });
    let flying = own.iter().copied().chain(attached);
    let wears = |r| banners::wears(sim, hqs, r).map(|hq| (hq, badges.side(hq)));
    let banners = flying.map(|r| (r, wears(r), sim.lacks_supply(r))).collect();
    let properties =
        holder(sim, mode).map_or(Vec::new(), |r| held(sim, &looks.properties, r, shown));
    Detail {
        slots,
        banners,
        properties,
    }
}

/// Lays the display out afresh when what it should show changes.
#[allow(clippy::too_many_arguments)]
pub fn lay_out(
    mut commands: Commands,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    drawn: Res<Drawn>,
    badges: Res<Badges>,
    mut showing: ResMut<Showing>,
    loaded: Res<Loaded>,
    mut looks: ResMut<Looks>,
    mut banners: ResMut<BannerImages>,
    mut images: ResMut<Assets<Image>>,
    mut display: Single<(Entity, &mut Visibility), With<ObjectDisplay>>,
    row: Query<Entity, With<PropertyRow>>,
) {
    let hqs = sim.0.hqs();
    let mode = wanted(&sim.0, &hqs, &selected.0, chosen.0);
    let mode = mode.map(|m| {
        let shown = match &showing.0 {
            Some((was, detail)) if holder(&sim.0, was) == holder(&sim.0, &m) => {
                detail.properties.as_slice()
            }
            _ => &[],
        };
        let detail = detail(&sim.0, &hqs, &badges, &looks, &m, shown);
        (m, detail)
    });
    // Who plays decides what owners are called.
    if showing.0 == mode && !local.is_changed() {
        return;
    }
    // Only the icons, where nothing else has changed.
    let rest_stands = match (&showing.0, &mode) {
        (Some((a, was)), Some((b, now))) => {
            a == b && was.slots == now.slots && was.banners == now.banners
        }
        _ => false,
    };
    let row = row
        .single()
        .ok()
        .filter(|_| rest_stands && !local.is_changed());
    let (root, visibility) = &mut *display;
    match row {
        Some(row) => drop(commands.entity(row).despawn_related::<Children>()),
        None => {
            commands.entity(*root).despawn_related::<Children>();
            visibility.set_if_neq(match mode {
                Some(_) => Visibility::Inherited,
                None => Visibility::Hidden,
            });
        }
    }
    showing.0 = mode.clone();
    let Some((mode, detail)) = mode else {
        return;
    };
    let mut ui = Ui {
        sim: &sim.0,
        local: local.0,
        leaders: &leaders,
        drawn: &drawn,
        badges: &badges,
        detail: &detail,
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
        Mode::SimpleActor(actor) => ui.simple_actor(&mut commands, root, &actor),
        Mode::Company(actor) => ui.company(&mut commands, root, &actor),
        Mode::Settlement => {}
        Mode::Group(rs) => ui.group(&mut commands, root, &rs),
    }
}

/// What laying the display out reads.
struct Ui<'a> {
    sim: &'a World,
    local: Option<u32>,
    leaders: &'a Leaders,
    drawn: &'a Drawn,
    badges: &'a Badges,
    detail: &'a Detail,
    game: &'a GameData,
    looks: &'a mut Looks,
    banners: &'a mut BannerImages,
    images: &'a mut Assets<Image>,
}

impl Ui<'_> {
    /// The picture of a template, or of a technology being researched.
    fn picture(&mut self, template: &str) -> Option<Handle<Image>> {
        let path = (self.drawn.0.icon(template).map(str::to_string))
            .or_else(|| self.looks.technologies.get(&Id::from(template)).cloned());
        self.looks.image(self.game, self.images, path.as_deref())
    }

    fn regiment_name(&self, r: usize) -> String {
        regiment_name(self.sim, &self.looks.names, &self.looks.strings, r)
    }

    /// The name and owner across the top.
    fn title(&self, commands: &mut Commands, root: Entity, name: String, owner: u32) {
        let player = self.sim.sides.player(owner);
        let colour = TINTS[tint(self.sim, self.local, owner)];
        commands.entity(root).with_children(|display| {
            display.spawn((
                at(8.0, 5.0, 300.0, 14.0),
                label(name, Font::HudTitle, Ink::Body),
            ));
            if let Some(p) = player {
                display.spawn((
                    at(8.0, 21.0, 300.0, 14.0),
                    ui::coloured(self.leaders.owner(p, self.local), Font::Hud, colour),
                ));
            }
        });
    }

    /// The portrait's box, holding `template`'s picture until [`portray`]
    /// has a portrait to show there, over a bar filled as `fill` says.
    fn portrait(&mut self, commands: &mut Commands, root: Entity, template: &str, fill: Fill) {
        let picture = self.picture(template);
        let [left, top, width, height] = PORTRAIT_BOX;
        let [in_left, in_top, _, in_bottom] = PORTRAIT_IN;
        commands.entity(root).with_children(|display| {
            display.spawn(framed(at(left, top, width, height)));
            let inside = || at(left + in_left, top + in_top, PORTRAIT.x, PORTRAIT.y);
            // What shows round a portrait that doesn't fill its box.
            display.spawn((inside(), BackgroundColor(Color::BLACK), Pickable::IGNORE));
            display.spawn((
                inside(),
                picture_node(picture),
                PortraitBox,
                Pickable::IGNORE,
            ));
            let bar_top = top + height - in_bottom + 1.0;
            bar(
                display,
                at(left + in_left, bar_top, PORTRAIT.x, in_bottom - 3.0),
                fill,
            );
        });
    }

    /// The row of icons for the properties that show.
    fn properties(&mut self, commands: &mut Commands, root: Entity) {
        let node = Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        };
        let row = commands.spawn((PropertyRow, node, Pickable::IGNORE)).id();
        commands.entity(root).add_child(row);
        self.icons(commands, row);
    }

    /// An icon in the display's corner for each property that shows.
    fn icons(&mut self, commands: &mut Commands, row: Entity) {
        for (i, id) in self.detail.properties.iter().enumerate() {
            let Some(look) = self.looks.properties.get(id).cloned() else {
                continue;
            };
            let icon = self.looks.image(self.game, self.images, Some(&look.icon));
            let said = std::iter::once(look.name).chain(look.lines);
            let after = (self.detail.properties.len() - 1 - i) as f32;
            let left = WIDTH - PROPERTIES_IN.x - PROPERTY - after * PROPERTY_PITCH;
            let node = at(left, PROPERTIES_IN.y, PROPERTY, PROPERTY);
            commands.entity(row).with_child((
                node,
                picture_node(icon),
                Hint(said.collect::<Vec<_>>().join("\n")),
            ));
        }
    }

    /// The line along the bottom saying what it's doing.
    fn status(&self, commands: &mut Commands, root: Entity, status: Status) {
        commands.entity(root).with_children(|display| {
            display
                .spawn((at(144.0, 158.0, 286.0, 18.0), Surface::Track))
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
            .icon(self.game, self.images, regiment.template.as_str());
        let template = &regiment.template;
        let shown = self.detail.banners.iter().find(|(of, ..)| *of == r);
        let (wears, lacks) = shown.map_or((None, false), |&(_, wears, lacks)| (wears, lacks));
        let wears = wears.map(|(hq, _)| hq);
        let (badges, banners) = (self.badges, &mut *self.banners);
        // An HQ's own frame goes with its badge.
        let frame = match wears.filter(|&hq| hq == regiment.id) {
            Some(hq) => badges.frame(banners, self.game, self.images, template.as_str(), hq),
            None => banners.frame(self.game, self.images, template.as_str()),
        };
        let badge = wears
            .and_then(|hq| badges.badge(banners, self.game, self.images, template.as_str(), hq));
        let out_of_supply = self.banners.out_of_supply.clone().filter(|_| lacks);
        let layers = [
            (self.banners.plate.clone(), Color::srgb(red, green, blue)),
            (icon, Color::WHITE),
            (frame, Color::WHITE),
            (out_of_supply, Color::WHITE),
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

    /// An HQ's places: each with the banner and health of the regiment in
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
                (Place::Regiment(r), _) => {
                    card.insert((Button, Pick(r))).observe(pick);
                    card.with_children(|card| {
                        self.banner(card, inside, r);
                        let [left, top, width, height] = SLOT_HEALTH;
                        bar(card, at(left, top, width, height), Fill::Regiment(r));
                    });
                }
                (Place::Queued(n), Some(b)) => {
                    let queue = &self.sim.buildings[b].queue;
                    let template = queue.get(n).map(|c| c.template().to_string());
                    let picture = template.and_then(|t| self.picture(&t));
                    card.insert((Button, Queued(n))).observe(cancel);
                    card.with_children(|card| {
                        card.spawn((inside, picture_node(picture), Pickable::IGNORE));
                        let [left, top, width, height] = SLOT_PROGRESS;
                        let node = at(left, top, width, height);
                        // Only the first is under way.
                        match n {
                            0 => bar(card, node, Fill::Progress(b)),
                            _ => drop(card.spawn(meter(node, Gauge::Progress))),
                        }
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
            Actor::Regiment(r) => {
                self.heading(commands, root, r);
                self.properties(commands, root);
                self.status(commands, root, Status::Regiment(r));
            }
            Actor::Building(b, _) => {
                self.structure(commands, root, b);
                self.queue(commands, root, b);
                self.properties(commands, root);
                self.status(commands, root, Status::Building(b));
            }
        }
    }

    /// The company display: a regiment with its units, or an HQ with its
    /// places.
    fn company(&mut self, commands: &mut Commands, root: Entity, actor: &Actor) {
        match *actor {
            Actor::Regiment(r) => self.regiment(commands, root, r),
            Actor::Building(b, _) => {
                self.structure(commands, root, b);
                // Its places show what it's been asked for.
                self.slots(commands, root, Some(b));
                self.properties(commands, root);
                self.status(commands, root, Status::Building(b));
            }
        }
    }

    /// A regiment's name and owner, and its leader's portrait over its
    /// health.
    fn heading(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        let regiment = &self.sim.regiments[r];
        self.title(commands, root, self.regiment_name(r), regiment.owner);
        let leader = leader(self.sim, r);
        let template = self.sim.units[leader].template.clone();
        self.portrait(commands, root, template.as_str(), Fill::Regiment(r));
    }

    fn regiment(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        self.heading(commands, root, r);
        let leader = leader(self.sim, r);
        // An HQ shows its places where a regiment shows its units.
        let is_hq = !self.detail.slots.is_empty();
        if !lone(self.sim, r) && !is_hq {
            // Cards across, wrapping, as the game's has them,
            // for all but an officer, who has the portrait.
            let officer = is_officer(self.sim.units[leader].template.as_str());
            let mut units = self.sim.regiments[r].units.clone();
            units.retain(|&u| !(officer && u == leader));
            let pictures: Vec<_> = units
                .iter()
                .map(|&u| self.picture(self.sim.units[u].template.as_str()))
                .collect();
            commands.entity(root).with_children(|display| {
                display
                    .spawn((
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
                                    );
                                });
                        }
                    });
            });
        }
        if !lone(self.sim, r) {
            commands.entity(root).with_children(|display| {
                display
                    .spawn(framed(at(366.0, 44.0, 63.0, 77.0)))
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
        self.slots(commands, root, None);
        self.properties(commands, root);
        self.status(commands, root, Status::Regiment(r));
    }

    /// The group display: a card per regiment.
    fn group(&mut self, commands: &mut Commands, root: Entity, rs: &[usize]) {
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
            let icon = self.picture(self.sim.regiments[r].template.as_str());
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

    /// A building's name and owner, and its picture over its health and,
    /// for a relay, its stockpile.
    fn structure(&mut self, commands: &mut Commands, root: Entity, b: usize) {
        let building = &self.sim.buildings[b];
        let template = building.template.clone();
        let name = building_name(self.sim, &self.looks.names, &self.looks.strings, b);
        self.title(commands, root, name, building.owner);
        self.portrait(commands, root, template.as_str(), Fill::Building(b));
        if self.sim.supply_of(b).is_some_and(|s| s.relay) {
            let fill = (Fill::Stockpile(b), Hint(String::new()));
            let node = at(PORTRAIT_BOX[0], 176.0, PORTRAIT_BOX[2], 3.0);
            commands
                .entity(root)
                .with_child((meter(node, Gauge::Stockpile), fill));
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
                display
                    .spawn((framed(node), picture_node(picture), Button, Queued(i)))
                    .observe(cancel);
            }
            if !queue.is_empty() {
                bar(display, at(240.0, 110.0, 64.0, 4.0), Fill::Progress(b));
            }
        });
    }
}

/// The template whose portrait stands for what the display shows, and
/// its owner: a building's own, or a regiment's primary slot's, which for
/// an HQ is the building once it's unpacked. None for a regiment with no
/// such slot, and for several things.
fn portrayed<'a>(sim: &'a World, looks: &'a Looks, mode: &Mode) -> Option<(&'a str, u32)> {
    match mode.actor()? {
        Actor::Building(b, _) => {
            let building = &sim.buildings[*b];
            Some((building.template.as_str(), building.owner))
        }
        Actor::Regiment(r) => {
            let regiment = &sim.regiments[*r];
            let template = match regiment.unpacked_into {
                Some(b) => sim.buildings[b].template.as_str(),
                // A unit on its own is no organization.
                None => match looks.primaries.get(&Id::from(regiment.template.as_str())) {
                    Some(primary) => primary.as_ref()?.as_str(),
                    None => regiment.template.as_str(),
                },
            };
            Some((template, regiment.owner))
        }
    }
}

/// Who the portrait of `template`, owned by `owner`, is of: its owner's
/// general where it says so and there is one, else the face it names, and
/// failing either its own model.
fn sitter(drawn: &Drawn, leaders: &Leaders, template: &str, owner: u32) -> Sitter {
    let general = (drawn.0.shows_faction_portrait(template))
        .then(|| leaders.0.get(&owner))
        .flatten();
    let face = match general {
        Some(general) => general.portrait.clone(),
        None => drawn.0.portrait(template).cloned(),
    };
    face.map_or_else(
        || Sitter::Model {
            template: template.to_string(),
            owner: Some(owner),
        },
        Sitter::Face,
    )
}

/// The one thing a display of a thing on its own shows: whether it's a
/// building, and which.
type Alone = Option<(bool, usize)>;

/// Draws the portrait of what the display shows, and shows it in the
/// portrait's box once it's there to show.
#[allow(clippy::too_many_arguments)]
pub fn portray(
    mut commands: Commands,
    showing: Res<Showing>,
    sim: Res<Sim>,
    leaders: Res<Leaders>,
    looks: Res<Looks>,
    mut drawn: ResMut<Drawn>,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
    mut stage: ResMut<Stage>,
    mut asked: bevy::ecs::system::Local<Option<(Sitter, Alone)>>,
    mut boxes: Query<(&mut ImageNode, &mut Node), With<PortraitBox>>,
    suns: Query<&Transform, With<DirectionalLight>>,
) {
    let mode = showing.0.as_ref().map(|(mode, _)| mode);
    let wanted = mode.and_then(|mode| {
        let (template, owner) = portrayed(&sim.0, &looks, mode)?;
        // Each thing shown on its own gets a portrait afresh, where a
        // regiment keeps one that's already up.
        let alone = match mode {
            Mode::SimpleActor(Actor::Building(b, _)) => Some((true, *b)),
            Mode::SimpleActor(Actor::Regiment(r)) => Some((false, *r)),
            _ => None,
        };
        Some((sitter(&drawn, &leaders, template, owner), alone))
    });
    // Asked once, as one that can't be drawn still can't the next time.
    if *asked != wanted {
        match &wanted {
            Some((sitter, alone)) => {
                let (models, game) = (&mut drawn.0, &loaded.game);
                let assets = &mut assets.models();
                let greeting = alone.is_some();
                let sun = suns.iter().next().map_or(Dir3::NEG_Y, |sun| sun.forward());
                models.sit(
                    &mut commands,
                    game,
                    assets,
                    &mut stage,
                    sitter,
                    greeting,
                    sun,
                );
                for spawned in stage.spawned() {
                    commands
                        .entity(spawned)
                        .insert(DespawnOnExit(Screen::Match));
                }
            }
            None => stage.clear(&mut commands),
        }
        *asked = wanted;
    }
    let Some((portrait, fitted)) = stage.picture() else {
        return;
    };
    // A fitted model's picture keeps its shape, in a band across the box.
    let height = match fitted {
        true => PORTRAIT.x / aaa_render::portrait::FIT_ASPECT,
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

/// Keeps the display's bars, pictures and status up to date.
pub fn refresh(
    sim: Res<Sim>,
    looks: Res<Looks>,
    mut bars: Query<(&Fill, &mut Meter, Option<&mut Hint>)>,
    mut pictures: Query<(&UnitPicture, &mut Shade)>,
    mut statuses: Query<(&Status, &mut Text)>,
) {
    let sim = &sim.0;
    for (fill, mut meter, hint) in &mut bars {
        if let (Fill::Stockpile(b), Some(mut hint)) = (*fill, hint) {
            let max = sim.supply_of(b).map_or(0.0, |s| s.stockpile_max);
            let left = sim.buildings[b].supply.stockpile;
            let amounts = [format!("{left:.0}"), format!("{max:.0}")];
            let words = amounts
                .iter()
                .fold(looks.said(STOCKPILE), |w, a| w.replacen("%.0f", a, 1));
            if hint.0 != words {
                hint.0 = words;
            }
        }
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
            Fill::Stockpile(b) => {
                let max = sim.supply_of(b).map_or(0.0, |s| s.stockpile_max);
                let left = sim.buildings[b].supply.stockpile;
                if max > 0.0 { left / max } else { 0.0 }
            }
            Fill::Progress(b) => {
                let b = &sim.buildings[b];
                b.queue
                    .front()
                    .map_or(0.0, |c| b.progress as f32 / c.ticks as f32)
            }
        };
        if meter.share != share {
            meter.share = share;
        }
    }
    for (picture, mut shade) in &mut pictures {
        shade.set_if_neq(match sim.units[picture.0].alive() {
            true => Shade::Full,
            false => Shade::Dark,
        });
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
        _ if regiment.guarding() => "mind_guarding",
        _ if sim.underway(r) || regiment.unpack_at.is_some() => match regiment.order {
            Order::Run => "mind_retreating",
            Order::Attack | Order::Pursue(_) => "mind_attacking",
            Order::Move => "mind_moving",
        },
        // Weightier than resting, lighter than anything it's doing.
        _ if sim.lacks_supply(r) => "out_of_supply",
        _ => "mind_resting",
    })
}

/// What a building's doing, as the game's actor state ids; nothing for one
/// at rest.
fn building_state(sim: &World, b: usize) -> Option<&'static str> {
    let building = &sim.buildings[b];
    // A queue outweighs being out of supply, which outweighs disabled,
    // even while either holds the queue up.
    let queue = match building.queue.front().map(|c| &c.work) {
        Some(aaa_sim::Work::Research(_)) => Some("structure_researching"),
        Some(aaa_sim::Work::Raise(_)) => Some("structure_recruiting"),
        None => None,
    };
    match () {
        _ if building.packing.is_some() => Some("structure_packing"),
        _ if building.unpacking.is_some() => Some("structure_unpacking"),
        _ if queue.is_some() => queue,
        _ if sim.out_of_supply(b) && building.standing() => Some("out_of_supply"),
        _ if building.state == State::Disabled => Some("disabled"),
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

/// Whether a regiment is a single unit, such as a truck.
/// A regiment's name as the game gives it: its template's, numbered if
/// it's one its side numbers. `names` are the templates' names, by
/// template, and `strings` the game's text, for the numbering.
pub(crate) fn regiment_name(
    sim: &World,
    names: &HashMap<Id, String>,
    strings: &aaa_data::Strings,
    r: usize,
) -> String {
    let regiment = &sim.regiments[r];
    numbered(
        strings,
        regiment.number,
        template_name(names, regiment.template.as_str()),
    )
}

/// A building's name, as [`regiment_name`]: numbered as the organization
/// it stands in is.
pub(crate) fn building_name(
    sim: &World,
    names: &HashMap<Id, String>,
    strings: &aaa_data::Strings,
    b: usize,
) -> String {
    let building = &sim.buildings[b];
    numbered(
        strings,
        building.number,
        template_name(names, building.template.as_str()),
    )
}

fn template_name(names: &HashMap<Id, String>, template: &str) -> String {
    (names.get(&Id::from(template)).cloned()).unwrap_or_else(|| template.to_string())
}

/// `name` with `number` before it, as "1st Infantry Regiment", in the
/// game's words for each ending where it has them.
pub(crate) fn numbered(strings: &aaa_data::Strings, number: Option<u32>, name: String) -> String {
    let Some(n) = number else {
        return name;
    };
    let key = match (n % 100, n % 10) {
        (11..=20, _) => "actor_name_numbered|ds",
        (_, 1) => "actor_name_numbered_st|ds",
        (_, 2) => "actor_name_numbered_nd|ds",
        (_, 3) => "actor_name_numbered_rd|ds",
        _ => "actor_name_numbered|ds",
    };
    match strings.get(key) {
        Some(said) => said
            .replacen("%d", &n.to_string(), 1)
            .replacen("%s", &name, 1),
        None => format!("{} {name}", ordinal(n)),
    }
}

/// Whether a regiment is a unit on its own, such as a truck.
pub(crate) fn lone(sim: &World, r: usize) -> bool {
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
        .find(|&u| is_officer(sim.units[u].template.as_str()));
    officer.unwrap_or(units[0])
}

/// Whether a unit template is a regiment's commanding officer, which the
/// game names `..._co`.
fn is_officer(template: &str) -> bool {
    template.to_ascii_lowercase().ends_with("_co")
}

fn ordinal(n: u32) -> String {
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

/// A bar in `node`, filled as `fill` says.
fn bar(parent: &mut ChildSpawnerCommands, node: Node, fill: Fill) {
    let gauge = match fill {
        Fill::Progress(_) => Gauge::Progress,
        Fill::Stockpile(_) => Gauge::Stockpile,
        _ => Gauge::Health,
    };
    parent.spawn((meter(node, gauge), fill));
}

/// A regiment's card clicked: selects it alone, in place of any building.
fn pick(
    activate: On<Activate>,
    cards: Query<&Pick>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
) {
    if let Ok(card) = cards.get(activate.entity) {
        selected.0 = vec![card.0];
        chosen.0 = None;
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
    fn property_icons_keep_their_order_and_the_newest_go_without_a_place() {
        let ids = |of: &[&str]| -> Vec<Id> { of.iter().map(|&p| Id::from(p)).collect() };
        // What's shown stays as it stands, what's gone leaves, the rest follow.
        let now = ids(&["forest", "guarding", "supplied"]);
        let shown = ids(&["guarding", "city", "forest"]);
        assert_eq!(
            ordered(now, &shown),
            ids(&["guarding", "forest", "supplied"])
        );
        let many = ids(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        assert_eq!(ordered(many.clone(), &[]), many[..MAX_PROPERTIES]);
        assert_eq!(ordered(many.clone(), &many[7..]).first(), many.last());
    }

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

    #[test]
    fn names_are_numbered_in_the_games_words() {
        let strings = aaa_data::Strings::from_iter([
            ("actor_name_numbered|ds".to_string(), "%dth %s".to_string()),
            (
                "actor_name_numbered_st|ds".to_string(),
                "%dst %s".to_string(),
            ),
        ]);
        let name = |n| numbered(&strings, n, "Regiment".into());
        assert_eq!(name(None), "Regiment");
        assert_eq!(name(Some(21)), "21st Regiment");
        assert_eq!(name(Some(11)), "11th Regiment");
        assert_eq!(name(Some(14)), "14th Regiment");
        // Where the game has no words for the ending, English.
        assert_eq!(name(Some(2)), "2nd Regiment");
    }

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
