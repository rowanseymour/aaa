//! The object display along the bottom, between the minimap and the action
//! buttons, describing what's selected, laid out as the game's own but
//! without its chrome. Its title names it and its owner, and a line along
//! the bottom says what it earns and takes to keep, and what it's doing.
//! It shows in one of the game's four
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
//! Beside the title of a regiment or a building is an icon for each
//! property it holds that has one, a building one unpacked into showing
//! the regiment's. What the pointer's
//! on says what it is in a box above the display: a part that stands for
//! a thing, such as a unit's card, says of that thing what [`Says`] has.
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
use aaa_sim::{Ledger, Order, Pause, Piece, Relation, State, World};
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
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

/// The strings' key for what an HQ's empty place says under the pointer.
const EMPTY_SLOT: &str = "object_display_CompanyDisplay_HQAttached_HQAttachedOrganization_ToolTip";

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
    /// The aircraft bringing the regiment the place is held for, by index
    /// into the sim's deliveries.
    Flown(usize),
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
    /// A regiment's units' health, evened out over them.
    Regiment(usize),
    Building(usize),
    /// How far along a building's first commission is.
    Progress(usize),
    /// How much of a relay's supply stockpile is left.
    Stockpile(usize),
    /// The health of a delivery's aircraft, which nothing hurts.
    Flight,
}

/// Whose tooltip a part of the display gives under the pointer.
#[derive(Component, Clone, Copy)]
pub enum Says {
    Unit(usize),
    Building(usize),
    Regiment(usize),
    /// What a building is commissioning, by place in its queue, and
    /// whether it shows in an HQ's place, where one that waits still says
    /// how far along it is.
    Queued {
        building: usize,
        place: usize,
        held: bool,
    },
}

/// What makes a part of the display give `of`'s tooltip.
fn says(of: Says) -> impl Bundle {
    (of, Hint(String::new()))
}

/// The line saying what a regiment or building counts for in its side's
/// economy, with what it says now: see [`economy`].
#[derive(Component, Default)]
pub struct EconomyLine(Vec<(&'static str, String)>);

/// The line saying what a regiment or building is doing.
#[derive(Component, Clone, Copy)]
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
    /// Templates' and technologies' names, by id.
    names: HashMap<Id, String>,
    /// Each resource's glyph, by the resource's field name in the data.
    glyphs: HashMap<&'static str, Handle<Image>>,
    strings: Arc<aaa_data::Strings>,
    /// How each property that has an icon shows, by id: the shared ones,
    /// and under ids of their own those of `formations`.
    properties: HashMap<Id, PropertyLook>,
    /// The properties a regiment in each formation holds, by the
    /// formation's id.
    formations: HashMap<Id, Vec<Id>>,
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
    let (technologies, listings) = (game.technologies(), game.listings());
    let names = (technologies
        .iter()
        .map(|(id, t)| (id.clone(), t.name.clone())))
    .chain((listings.iter()).map(|(id, l)| (id.clone(), l.name.clone())));
    let glyphs = crate::actions::glyphs(game, &mut images, formats.0);
    let mut properties: HashMap<Id, PropertyLook> = (*game.property_looks()).clone();
    let mut formations = HashMap::new();
    for (formation, look) in game.formation_looks().iter() {
        let held = look.properties.iter().enumerate().map(|(n, look)| {
            let id = formation_property(formation, n);
            properties.insert(id.clone(), look.clone());
            id
        });
        formations.insert(formation.clone(), held.collect());
    }
    commands.insert_resource(Looks {
        primaries: primaries.collect(),
        names: names.collect(),
        glyphs,
        strings: game.strings(),
        properties,
        formations,
        technologies: (technologies.iter())
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

/// The id that stands for the `n`th property of a formation, which the
/// data gives none: one no property of the data's own can have.
fn formation_property(formation: &Id, n: usize) -> Id {
    Id::from(format!("{formation}/{n}"))
}

/// Whose properties show: the regiment selected alone, the one that
/// unpacked into the building, or else the building itself.
fn holder(sim: &World, mode: &Mode) -> Option<Piece> {
    Some(match *mode.actor()? {
        Actor::Regiment(r) => Piece::Regiment(r),
        Actor::Building(b, _) => {
            let unpacked = (sim.regiments.iter()).position(|r| r.unpacked_into == Some(b));
            unpacked.map_or(Piece::Building(b), Piece::Regiment)
        }
    })
}

/// The properties a regiment or building holds that have an icon: its
/// template's own, then for a regiment its formation's, its terrain's
/// and its stance's, then those it has for a time. Those in `shown` come
/// first, as they stand there, so that icons keep the order their
/// properties came in while their holder stays selected.
fn held(sim: &World, looks: &Looks, holder: Piece, shown: &[Id]) -> Vec<Id> {
    let own = |template: &Id| {
        let kind = sim.rules().kinds.get(template);
        kind.into_iter().flat_map(|k| &k.properties).cloned()
    };
    let all: Vec<Id> = match holder {
        Piece::Regiment(r) => {
            let regiment = &sim.regiments[r];
            // One that's unpacked stands in no formation, and guards nothing.
            let afield = regiment.unpacked_into.is_none();
            let formation = regiment.formations.get(regiment.current_formation());
            let formation = formation
                .filter(|_| afield)
                .and_then(|f| looks.formations.get(&Id::from(f.id.as_str())));
            let guarding = (afield && regiment.guarding()).then(|| Id::from(GUARDING));
            let timed = regiment.properties.iter().map(|h| h.property.clone());
            own(&regiment.template)
                .chain(formation.into_iter().flatten().cloned())
                .chain(regiment.terrain_properties.iter().cloned())
                .chain(guarding)
                .chain(timed)
                .collect()
        }
        Piece::Building(b) => {
            let building = &sim.buildings[b];
            let timed = building.properties.iter().map(|h| h.property.clone());
            own(&building.template).chain(timed).collect()
        }
    };
    let mut now: Vec<Id> = Vec::new();
    for p in all {
        if looks.properties.contains_key(&p) && !now.contains(&p) {
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
            .chain(sim.bound_for(hq.id).map(Place::Flown))
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
    let properties = holder(sim, mode).map_or(Vec::new(), |of| held(sim, looks, of, shown));
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
                // The owner's own colour, whoever's looking, as the lobby marks it.
                let [red, green, blue] = p.rgb;
                display.spawn((
                    at(9.0, 22.0, 3.0, 11.0),
                    BackgroundColor(Color::srgb(red, green, blue)),
                    Pickable::IGNORE,
                ));
                display.spawn((
                    at(18.0, 21.0, 290.0, 14.0),
                    ui::coloured(self.leaders.owner(p, self.local), Font::Hud, colour),
                ));
            }
        });
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
                    card.insert((Button, Pick(r), says(Says::Regiment(r))));
                    card.observe(pick);
                    card.with_children(|card| {
                        self.banner(card, inside, r);
                        let [left, top, width, height] = SLOT_HEALTH;
                        bar(card, at(left, top, width, height), Fill::Regiment(r), None);
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
                    card.insert((Button, Queued(n), says(of))).observe(cancel);
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

    /// A regiment's name and owner, and its captain's portrait over the
    /// captain's health.
    fn heading(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        let regiment = &self.sim.regiments[r];
        self.title(commands, root, self.regiment_name(r), regiment.owner);
        let captain = captain(self.sim, self.looks, r);
        let template = self.sim.units[captain].template.clone();
        let (fill, of) = (Fill::Unit(captain), Says::Unit(captain));
        self.portrait(commands, root, template.as_str(), fill, of);
    }

    fn regiment(&mut self, commands: &mut Commands, root: Entity, r: usize) {
        self.heading(commands, root, r);
        let captain = captain(self.sim, self.looks, r);
        // An HQ shows its places where a regiment shows its units.
        let is_hq = !self.detail.slots.is_empty();
        if !lone(self.sim, r) && !is_hq {
            // Cards across, wrapping, as the game's has them,
            // for all but an officer, who has the portrait.
            let officer = is_officer(self.sim.units[captain].template.as_str());
            let mut units = self.sim.regiments[r].units.clone();
            units.retain(|&u| !(officer && u == captain));
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
                            let card = framed(Node {
                                width: px(42.0 * SCALE),
                                height: px(47.0 * SCALE),
                                flex_direction: FlexDirection::Column,
                                ..default()
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
                    .spawn((framed(at(366.0, 44.0, 63.0, 77.0)), says(Says::Regiment(r))))
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
            let captain = captain(self.sim, self.looks, r);
            // A thing on its own answers for itself.
            let of = match lone(self.sim, r) {
                true => Says::Unit(captain),
                false => Says::Regiment(r),
            };
            let mut card = commands.spawn((
                framed(at(left, 40.0, CARD, 138.0)),
                Button,
                Pick(r),
                says(of),
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
                // The wide bar is the captain's, then one for each unit.
                let wide = at(2.0, 50.0, CARD - 6.0, 5.0);
                bar(card, wide, Fill::Unit(captain), Some(Says::Unit(captain)));
                let half = (CARD - 8.0) / 2.0;
                for (n, &u) in units.iter().enumerate() {
                    let (col, row) = ((n % 2) as f32, (n / 2) as f32);
                    let node = at(2.0 + col * (half + 2.0), 58.0 + row * 5.0, half, 3.0);
                    bar(card, node, Fill::Unit(u), Some(Says::Unit(u)));
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
                display.spawn((card, says(of))).observe(cancel);
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
                None => match looks.primaries.get(&regiment.template) {
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

/// Keeps the display's bars, pictures, tooltips and status up to date.
#[allow(clippy::too_many_arguments)]
pub fn refresh(
    sim: Res<Sim>,
    looks: Res<Looks>,
    drawn: Res<Drawn>,
    leaders: Res<Leaders>,
    local: Res<Local>,
    mut bars: Query<(&Fill, &mut Meter)>,
    mut tips: Query<(&Says, &mut Hint, &Hovered)>,
    mut pictures: Query<(&UnitPicture, &mut Shade)>,
    mut statuses: Query<(&Status, &mut Text)>,
    mut lines: Query<(Entity, &Status, &mut EconomyLine)>,
    mut commands: Commands,
) {
    let sim = &sim.0;
    for (fill, mut meter) in &mut bars {
        let (now, full) = measure(sim, *fill);
        let share = match (*fill, full > 0.0) {
            (_, true) => now / full,
            // What has no health to lose shows whole.
            (Fill::Unit(_) | Fill::Regiment(_) | Fill::Building(_) | Fill::Flight, false) => 1.0,
            (Fill::Progress(_) | Fill::Stockpile(_), false) => 0.0,
        };
        if meter.share != share {
            meter.share = share;
        }
    }
    let teller = Teller {
        sim,
        looks: &looks,
        drawn: &drawn,
        leaders: &leaders,
        local: local.0,
    };
    // Only what the pointer's on has anyone to tell.
    for (says, mut hint, _) in tips.iter_mut().filter(|(.., on)| on.get()) {
        let words = teller.tip(*says).join("\n");
        if hint.0 != words {
            hint.0 = words;
        }
    }
    for (row, status, mut line) in &mut lines {
        let now = counted(sim, local.0, *status).map_or_else(Vec::new, |l| economy(&l));
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

/// How much a bar has of how much it could: health, a commission's
/// progress out of 100, or supply. Nothing of nothing for what has no
/// such measure.
fn measure(sim: &World, fill: Fill) -> (f32, f32) {
    let unit = |u: usize| {
        let unit = &sim.units[u];
        let full = unit.fighter.as_ref().map_or(0.0, |f| f.health);
        match unit.alive() {
            true => (unit.health, full),
            false => (0.0, full),
        }
    };
    match fill {
        Fill::Unit(u) => unit(u),
        Fill::Regiment(r) => {
            let units = sim.regiments[r].units.iter().map(|&u| unit(u));
            let shares: Vec<f32> = units
                .filter(|&(_, full)| full > 0.0)
                .map(|(now, full)| now / full)
                .collect();
            match shares.len() {
                0 => (0.0, 0.0),
                n => (shares.iter().sum::<f32>() / n as f32, 1.0),
            }
        }
        Fill::Building(b) => {
            let b = &sim.buildings[b];
            (b.health, b.fighter.health)
        }
        Fill::Progress(b) => {
            let b = &sim.buildings[b];
            b.queue.front().map_or((0.0, 0.0), |c| {
                (100.0 * b.progress as f32 / c.ticks as f32, 100.0)
            })
        }
        Fill::Stockpile(b) => {
            let max = sim.supply_of(b).map_or(0.0, |s| s.stockpile_max);
            (sim.buildings[b].supply.stockpile, max)
        }
        Fill::Flight => (1.0, 1.0),
    }
}

/// What the tooltips of the display's things read.
struct Teller<'a> {
    sim: &'a World,
    looks: &'a Looks,
    drawn: &'a Drawn,
    leaders: &'a Leaders,
    local: Option<u32>,
}

impl Teller<'_> {
    /// The lines of a thing's tooltip: its template's name, then what
    /// applies to it of its health, its stockpile and its general. A
    /// regiment has no health of its own to give.
    fn tip(&self, says: Says) -> Vec<String> {
        let (sim, looks) = (self.sim, self.looks);
        let name = |template: &Id| template_name(&looks.names, template.as_str());
        let row = |key: &str, value: String| self.row(key, value);
        let of = |(now, full): (f32, f32)| format!("{now:.0}/{full:.0}");
        match says {
            Says::Unit(u) => {
                let unit = &sim.units[u];
                let mut lines = vec![name(&unit.template)];
                if unit.fighter.is_some() {
                    let health = of(measure(sim, Fill::Unit(u)));
                    lines.push(row("ui_tooltip_item_title_health", health));
                }
                // One that's dead says only what its template would.
                if unit.alive() {
                    let owner = sim.regiments[unit.regiment].owner;
                    lines.extend(self.general(&unit.template, owner));
                }
                lines
            }
            Says::Building(b) => {
                let building = &sim.buildings[b];
                let health = of(measure(sim, Fill::Building(b)));
                let mut lines = vec![
                    name(&building.template),
                    row("ui_tooltip_item_title_health", health),
                ];
                if sim.supply_of(b).is_some_and(|s| s.relay) {
                    let stockpile = of(measure(sim, Fill::Stockpile(b)));
                    lines.push(row("ui_tooltip_item_title_supply_stockpile", stockpile));
                }
                lines.extend(self.general(&building.template, building.owner));
                lines
            }
            Says::Regiment(r) => vec![name(&sim.regiments[r].template)],
            Says::Queued {
                building: b,
                place,
                held,
            } => {
                let building = &sim.buildings[b];
                let Some(commission) = building.queue.get(place) else {
                    return Vec::new();
                };
                let mut lines = vec![name(commission.template())];
                // Only the first is under way.
                let (now, _) = match place {
                    0 => measure(sim, Fill::Progress(b)),
                    _ => (0.0, 100.0),
                };
                if place == 0 || held {
                    let done = format!("{}%", now as u32);
                    lines.push(row("ui_tooltip_item_title_production_percentage", done));
                }
                if let (0, Some(pause)) = (place, building.paused) {
                    let why = match pause {
                        Pause::Limit(_) => "ui_tooltip_queue_paused_unit_limit",
                        Pause::Disabled => "ui_tooltip_queue_paused_disabled",
                        Pause::Supply => "ui_tooltip_queue_paused_oos",
                    };
                    lines.push(String::new());
                    lines.push(row("ui_tooltip_item_title_queue_paused", looks.said(why)));
                }
                lines.push(looks.said("ui_tooltip_production_queue_item_cancel"));
                lines
            }
        }
    }

    /// What a template that shows its owner's general says of them: who,
    /// and to their own side and its allies, their experience.
    fn general(&self, template: &Id, owner: u32) -> Vec<String> {
        let general = (self.drawn.0.shows_faction_portrait(template.as_str()))
            .then(|| self.leaders.0.get(&owner))
            .flatten();
        let Some(general) = general else {
            return Vec::new();
        };
        let mut lines = vec![self.row("ui_tooltip_item_general", general.name.clone())];
        if let (true, Some(experience)) = (
            ours(self.sim, self.local, owner),
            self.sim.experience(owner),
        ) {
            lines.push(self.row("ui_tooltip_item_experience", format!("{experience:.0}")));
        }
        lines
    }

    /// A line of a tooltip: the game's words for what it tells, then that.
    fn row(&self, key: &str, value: String) -> String {
        format!("{} {value}", self.looks.said(key))
    }
}

/// Whether `owner` is the player's own side or an ally of it.
fn ours(sim: &World, local: Option<u32>, owner: u32) -> bool {
    local.is_some_and(|p| {
        matches!(
            sim.sides.relation(p, owner),
            Relation::Itself | Relation::Ally
        )
    })
}

/// What the thing a status line is of counts for in its side's economy
/// now, which only the player's own side's and its allies' say.
fn counted(sim: &World, local: Option<u32>, status: Status) -> Option<Ledger> {
    let (owner, ledger) = match status {
        Status::Building(b) => (sim.buildings[b].owner, sim.building_ledger(b)),
        Status::Regiment(r) => {
            let regiment = &sim.regiments[r];
            let ledger = match regiment.unpacked_into {
                Some(b) => sim.building_ledger(b),
                None => sim.regiment_ledger(r),
            };
            (regiment.owner, ledger)
        }
    };
    ours(sim, local, owner).then_some(ledger)
}

/// What a thing counts for in its side's economy, as the display's line
/// has it, each with its resource's field name in the data: of each
/// resource what it earns less what it takes to keep, signed, but for
/// the limits it takes up, which are bare. Nothing for what comes to
/// nothing.
fn economy(ledger: &Ledger) -> Vec<(&'static str, String)> {
    // As the game orders them: a limit made room in before one taken up.
    const ORDER: [&str; 7] = [
        "money",
        "ammo",
        "oil",
        "unit_limit_provided",
        "unit_limit_consumed",
        "building_limit_provided",
        "building_limit_consumed",
    ];
    let entries = ORDER.into_iter().filter_map(|kind| {
        let net = ledger.production.get(kind) - ledger.upkeep.get(kind);
        // To few enough figures that sums' rounding doesn't show.
        let (net, figure) = match kind.ends_with("_consumed") {
            true => (net.abs().round(), format!("{}", net.abs().round())),
            false => {
                let net = (net * 1000.0).round() / 1000.0;
                (net, format!("{net:+}"))
            }
        };
        (net != 0.0).then_some((kind, figure))
    });
    entries.collect()
}

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

/// A regiment's captain, whose portrait stands for it: the unit in its
/// primary slot, else its leader.
fn captain(sim: &World, looks: &Looks, r: usize) -> usize {
    let regiment = &sim.regiments[r];
    let primary = (looks.primaries.get(&regiment.template)).and_then(|p| p.as_ref());
    let unit = primary.and_then(|template| {
        let is = |&&u: &&usize| sim.units[u].template == *template;
        regiment.units.iter().find(is).copied()
    });
    unit.unwrap_or_else(|| leader(sim, r))
}

/// A regiment's commanding officer, else its first unit.
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

/// A bar in `node`, filled as `fill` says, giving `of`'s tooltip where
/// it has one of its own rather than leaving that to what it's on.
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
    fn the_economy_line_signs_all_but_the_limits_taken_up() {
        let ledger = |earned: &[(&str, f32)], kept: &[(&str, f32)]| {
            let mut ledger = Ledger::default();
            earned
                .iter()
                .for_each(|&(k, a)| ledger.production.set(k, a));
            kept.iter().for_each(|&(k, a)| ledger.upkeep.set(k, a));
            ledger
        };
        let line = |ledger: Ledger| -> Vec<String> {
            let line = economy(&ledger).into_iter();
            line.map(|(kind, figure)| format!("{kind} {figure}"))
                .collect()
        };
        // An HQ's truck: what it earns, the regiments it makes room for,
        // and the building it counts as.
        let truck = ledger(
            &[("money", 12.0), ("ammo", 5.0), ("unit_limit_provided", 6.0)],
            &[("building_limit_consumed", 1.0)],
        );
        assert_eq!(
            line(truck),
            [
                "money +12",
                "ammo +5",
                "unit_limit_provided +6",
                "building_limit_consumed 1"
            ]
        );
        // A regiment: what it takes to keep, and the one it counts as.
        let tanks = ledger(
            &[("ammo", 2.5)],
            &[("ammo", 22.5), ("oil", 20.0), ("unit_limit_consumed", 1.0)],
        );
        assert_eq!(
            line(tanks),
            ["ammo -20", "oil -20", "unit_limit_consumed 1"]
        );
        // One that both makes room and takes it up says each.
        let both = ledger(
            &[("unit_limit_provided", 3.0)],
            &[("unit_limit_consumed", 1.0)],
        );
        assert_eq!(
            line(both),
            ["unit_limit_provided +3", "unit_limit_consumed 1"]
        );
        // What sums leave over doesn't show, nor a limit barely taken up.
        let residue = ledger(
            &[("money", 0.1 + 0.2), ("ammo", 12.000001)],
            &[("money", 0.3), ("unit_limit_consumed", 0.4)],
        );
        assert_eq!(line(residue), ["ammo +12"]);
        assert!(line(Ledger::default()).is_empty());
    }

    #[test]
    fn a_formations_properties_have_ids_of_their_own() {
        let column = Id::from("regiment_column");
        assert_ne!(formation_property(&column, 0), column);
        assert_ne!(
            formation_property(&column, 0),
            formation_property(&column, 1)
        );
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
