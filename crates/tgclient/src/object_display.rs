//! The object display: what's selected, worked out as [`Showing`] for
//! either interface to draw, with the markers their parts share so that
//! one set of systems keeps both up to date. It shows in one of the game's
//! four [`Mode`]s:
//!
//! - Simple actor, for a building or a unit on its own: its portrait over
//!   its health, and what a building's been asked to commission, the first
//!   with its progress; clicking one calls it off.
//! - Company, for an organization: its officer's portrait over the
//!   organization's health, a card per other unit with the unit's health, and
//!   its banner under its attack efficiency, marked where that's heading. An
//!   HQ, packed or not, has instead of the cards a place for each
//!   organization it takes, with the organization attached there or the one
//!   it's been asked for.
//! - Group, for several: a card each, up to [`MAX_SELECTED`], with the
//!   banner, its efficiency, the organization's health and each unit's;
//!   clicking one selects it alone.
//! - Settlement, for a settlement (see [`tgdata::defs::CityComponent`]):
//!   unused, since a city shows as a simple actor.
//!
//! Beside the title of an organization or a building is an icon for each
//! property it holds that has one, a building one unpacked into showing
//! the organization's. A part that stands for a thing, such as a unit's
//! card, says of that thing what [`Says`] has under the pointer.
//!
//! The portrait of one thing is drawn from a model: the face its template
//! names, an organization's being its commanding officer's, its owner's faction
//! where its template says so, else all of its own model. Its flat picture
//! stands in only where none of those can be drawn.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgdata::{GUARDING, GameData, Id, PropertyLook};
use tgrender::MapAssets;
use tgrender::overlay::BannerImages;
use tgrender::portrait::{Sitter, Stage};
use tgrender::textures;
use tgsim::{Ledger, Order, Pause, Piece, Relation, State, Target, World};

use crate::banners::{self, Badges};
use crate::ui::{self, Hint, Ink, Meter, Shade};
use crate::{
    Drawn, Leaders, Loaded, Local, Orders, Screen, Selected, SelectedBuilding, Sim, TINTS, tint,
};

/// How many organizations can be selected at once, as many as the group
/// display has cards for.
pub const MAX_SELECTED: usize = 6;
/// How many of an HQ's places the display has room for, and how many go
/// across before the next row.
const MAX_SLOTS: usize = 6;
const SLOTS_ACROSS: usize = 3;
/// How far from the corner of the `area` they share the `i`th of an HQ's
/// `count` places, each `slot` in size, goes: across then down, spread
/// evenly over it.
pub(crate) fn spread(i: usize, count: usize, area: Vec2, slot: Vec2) -> Vec2 {
    let rows = count.div_ceil(SLOTS_ACROSS).max(2);
    let gap = |area: f32, slot: f32, n: usize| (area - n as f32 * slot) / (n - 1) as f32;
    let step = slot + Vec2::new(gap(area.x, slot.x, SLOTS_ACROSS), gap(area.y, slot.y, rows));
    Vec2::new((i % SLOTS_ACROSS) as f32, (i / SLOTS_ACROSS) as f32) * step
}

/// The strings' key for what an HQ's empty place says under the pointer.
pub(crate) const EMPTY_SLOT: &str =
    "object_display_CompanyDisplay_HQAttached_HQAttachedOrganization_ToolTip";

/// How many properties' icons the display has places for.
const MAX_PROPERTIES: usize = 7;

/// The game's layout's box for the portrait, and how far in from its
/// left, top, right and bottom the portrait itself is.
pub(crate) const PORTRAIT_BOX: [f32; 4] = [9.0, 47.0, 129.0, 124.0];
pub(crate) const PORTRAIT_IN: [f32; 4] = [2.0, 2.0, 2.0, 7.0];
/// The portrait's size within its box, in the game's interface units.
pub(crate) const PORTRAIT: Vec2 = Vec2::new(
    PORTRAIT_BOX[2] - PORTRAIT_IN[0] - PORTRAIT_IN[2],
    PORTRAIT_BOX[3] - PORTRAIT_IN[1] - PORTRAIT_IN[3],
);
/// How many pixels the portrait is drawn with for each of the game's
/// interface units it spans: enough for either interface's box to show it
/// sharply on the densest screens.
const PORTRAIT_DENSITY: f32 = 2.1;

/// What the display shows now.
#[derive(Resource, Default)]
pub struct Showing {
    pub(crate) now: Option<(Mode, Detail)>,
    /// Whether all that's changed since it was last different is which
    /// properties' icons show.
    pub(crate) icons_only: bool,
}

/// Has the interface just put up lay its display out afresh, as it would
/// once what's shown changes.
pub fn redraw(mut showing: ResMut<Showing>) {
    showing.icons_only = false;
}

/// What of the display's layout can change while it shows the same thing.
#[derive(Clone, PartialEq, Debug, Default)]
pub(crate) struct Detail {
    /// For an HQ, what's in each of its places.
    pub(crate) slots: Vec<Place>,
    /// For each organization whose banner shows, the badge it wears and
    /// whether it's out of supply.
    pub(crate) banners: Vec<(usize, Option<banners::Worn>, bool)>,
    /// The properties whose icons show, those held longest first.
    pub(crate) properties: Vec<Id>,
    /// The units in each place of the organizations shown, which a
    /// replacement changes.
    pub(crate) members: Vec<usize>,
}

/// What's in one of an HQ's places.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Place {
    Empty,
    /// The organization attached there.
    Organization(usize),
    /// What the place is held for, by where it is in the HQ's queue.
    Queued(usize),
    /// The aircraft bringing the organization the place is held for, by index
    /// into the sim's deliveries.
    Flown(usize),
}

/// Which of the game's displays shows, and what of; a change in it lays
/// the display out afresh.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Mode {
    /// A building's, or that of a unit on its own, such as a truck.
    SimpleActor(Actor),
    /// An organization's, or an HQ's, packed or not.
    Company(Actor),
    /// A settlement's, which no AA thing is.
    #[allow(dead_code)]
    Settlement,
    /// That of several organizations.
    Group(Vec<usize>),
}

impl Mode {
    /// The one thing it shows, if it shows one.
    pub(crate) fn actor(&self) -> Option<&Actor> {
        match self {
            Mode::SimpleActor(actor) | Mode::Company(actor) => Some(actor),
            Mode::Settlement | Mode::Group(_) => None,
        }
    }
}

/// What a display of one thing shows.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Actor {
    Organization(usize),
    /// With the templates of what it's commissioning.
    Building(usize, Vec<String>),
}

/// A bar filled to a share of its width.
#[derive(Component, Clone, Copy)]
pub enum Fill {
    /// A unit's health.
    Unit(usize),
    /// An organization's units' health, evened out over them.
    Organization(usize),
    Building(usize),
    /// How far along a building's first commission is.
    Progress(usize),
    /// How much of a relay's supply stockpile is left.
    Stockpile(usize),
    /// The health of a delivery's aircraft, which nothing hurts.
    Flight,
    /// An organization's attack efficiency.
    Efficiency(usize),
    /// An organization's morale.
    Morale(usize),
}

/// The marker on an organization's efficiency bar at the efficiency it's
/// easing towards.
#[derive(Component)]
pub struct EfficiencyTarget(pub(crate) usize);

/// The widget whose layout the efficiency bar's marker comes from, and
/// the marker's own widget within it.
const EFFICIENCY_BAR: &str = "AttackEfficiency";
const EFFICIENCY_TARGET: &str = "TargetPercentage";

/// Whose tooltip a part of the display gives under the pointer.
#[derive(Component, Clone, Copy)]
pub enum Says {
    Unit(usize),
    Building(usize),
    Organization(usize),
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
pub(crate) fn says(of: Says) -> impl Bundle {
    (of, Hint(String::new()))
}

/// The line saying what an organization or building is doing.
#[derive(Component, Clone, Copy)]
pub enum Status {
    Organization(usize),
    Building(usize),
}

/// The figure by the shield: the defence of what's selected, as it stands,
/// and the colour it's in while that's its plain one, the theme's if none.
#[derive(Component, Clone, Copy)]
pub struct Defends(pub(crate) Target, pub(crate) Option<Color>);

/// The shield and its figure together, which give the defence's tooltip.
#[derive(Component, Clone, Copy)]
pub struct Shield(pub(crate) Target);

/// The figure by a weapon's icon: the main damage of the selected thing's
/// weapon, by its place among the thing's weapons, as it stands, and the
/// colour it's in while that's its plain one, the theme's if none.
#[derive(Component, Clone, Copy)]
pub struct Strikes(
    pub(crate) Target,
    pub(crate) usize,
    pub(crate) Option<Color>,
);

/// A weapon's icon and its figure together, which give the weapon's
/// tooltip.
#[derive(Component, Clone, Copy)]
pub struct Armed(pub(crate) Target, pub(crate) usize);

/// The most weapons the display shows of one thing.
pub(crate) const MOST_WEAPONS: usize = 5;

/// The game's colours for a figure that's above or below its plain one.
const RAISED: Color = Color::srgb(0.0, 1.0, 0.0);
const LOWERED: Color = Color::srgb(1.0, 0.0, 0.0);
/// The game's colour for the name of an owner the player is neutral
/// towards.
const NEUTRAL: Color = Color::srgb(0.8, 0.8, 0.8);

/// A unit's picture, darkened once it's dead.
#[derive(Component)]
pub struct UnitPicture(pub(crate) usize);

/// An organization's card, which selects it alone.
#[derive(Component)]
pub struct Pick(pub(crate) usize);

/// One of what a building is commissioning, by place in its queue, which
/// calls it off.
#[derive(Component)]
pub struct Queued(pub(crate) usize);

/// What the display is drawn with, loaded with the match.
#[derive(Resource)]
pub struct Looks {
    /// Templates' and technologies' names, by id.
    pub(crate) names: HashMap<Id, String>,
    /// What each template says of itself and shows its defence by.
    pub(crate) listings: Arc<HashMap<Id, tgdata::Listing>>,
    /// Each resource's glyph, by the resource's id.
    pub(crate) glyphs: HashMap<Id, Handle<Image>>,
    pub(crate) strings: Arc<tgdata::Strings>,
    /// The map's own text, for the names it gives things and sides.
    pub(crate) map: Arc<tgdata::Strings>,
    /// How each property that has an icon shows, by id: the shared ones,
    /// and under ids of their own those of `formations`.
    pub(crate) properties: HashMap<Id, PropertyLook>,
    /// The properties an organization in each formation holds, by the
    /// formation's id.
    pub(crate) formations: HashMap<Id, Vec<Id>>,
    /// Each technology's research button icon, by id, for what's being
    /// researched.
    pub(crate) technologies: HashMap<Id, String>,
    /// The template in each organization's primary slot, by the
    /// organization's; none for one with no such slot.
    pub(crate) primaries: HashMap<Id, Option<Id>>,
    /// The names of experience levels, by their properties' ids.
    pub(crate) levels: HashMap<Id, String>,
    pub(crate) images: HashMap<String, Option<Handle<Image>>>,
    pub(crate) formats: bevy::image::CompressedImageFormats,
    /// The archive path of the efficiency bar's marker, if the layouts
    /// have one.
    pub(crate) efficiency_target: Option<String>,
}

impl Looks {
    pub(crate) fn image(
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
    pub(crate) fn said(&self, key: &str) -> String {
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

/// Loads what selections are shown with, and the stage portraits are
/// posed on, for either interface.
pub fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    formats: Res<CompressedImageFormatSupport>,
    mut images: ResMut<Assets<Image>>,
) {
    let width = (PORTRAIT.x * PORTRAIT_DENSITY).round() as u32;
    let stage = Stage::new(&mut commands, &mut images, width);
    commands
        .entity(stage.camera())
        .insert(DespawnOnExit(Screen::Match));
    commands.insert_resource(stage);
    let game = &loaded.game;
    let layouts = game.layouts();
    let components = game.organization_components();
    let primaries = components.iter().map(|(id, o)| {
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
        levels: game.level_names(),
        names: names.collect(),
        listings: listings.clone(),
        glyphs,
        strings: game.strings(),
        map: loaded.map.strings.clone(),
        properties,
        formations,
        technologies: (technologies.iter())
            .filter_map(|(id, t)| Some((id.clone(), t.icon.clone()?)))
            .collect(),
        images: HashMap::new(),
        formats: formats.0,
        efficiency_target: efficiency_target(&game.game_widgets()),
    });
    commands.insert_resource(Showing::default());
}

/// The archive path of the picture marking where an efficiency bar is
/// heading, from the layouts.
fn efficiency_target(widgets: &[tgdata::widgets::Widget]) -> Option<String> {
    let bar = widgets.iter().find_map(|w| w.find(EFFICIENCY_BAR))?;
    let marker = bar.find(EFFICIENCY_TARGET)?;
    (marker.parts.iter()).find_map(|p| p.texture.clone())
}

/// How far along its efficiency bar the organization at `r`'s marker
/// stands: at the efficiency it's easing towards.
pub(crate) fn towards(sim: &World, r: usize) -> Val {
    percent(100.0 * sim.efficiency_target(r).clamp(0.0, 1.0))
}

/// The mode the selection calls for the display to show in.
fn wanted(
    sim: &World,
    hqs: &[tgsim::Hq],
    local: Option<u32>,
    selected: &[usize],
    chosen: Option<usize>,
) -> Option<Mode> {
    let actor = match (selected, chosen) {
        ([], Some(b)) => {
            // Only its side and their allies see what it's commissioning.
            let queue = (sim.buildings[b].queue.iter())
                .filter(|_| ours(sim, local, sim.buildings[b].owner));
            Actor::Building(b, queue.map(|c| c.template().to_string()).collect())
        }
        ([], None) => return None,
        ([r], _) => Actor::Organization(*r),
        (rs, _) => {
            return Some(Mode::Group(rs.iter().take(MAX_SELECTED).copied().collect()));
        }
    };
    let company = match actor {
        Actor::Organization(r) => !lone(sim, r) || hq(sim, hqs, &actor).is_some(),
        Actor::Building(..) => hq(sim, hqs, &actor).is_some(),
    };
    Some(match company {
        true => Mode::Company(actor),
        false => Mode::SimpleActor(actor),
    })
}

/// The HQ `actor` is, if it's one: a Division HQ's building, or its truck.
fn hq<'a>(sim: &World, hqs: &'a [tgsim::Hq], actor: &Actor) -> Option<&'a tgsim::Hq> {
    let id = match *actor {
        Actor::Building(b, _) => sim.hq_of(b)?,
        Actor::Organization(r) => sim.organizations[r].id,
    };
    hqs.iter().find(|h| h.id == id)
}

/// The HQ the display shows, if it shows one.
fn shown_hq<'a>(sim: &World, hqs: &'a [tgsim::Hq], mode: &Mode) -> Option<&'a tgsim::Hq> {
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

/// Whose properties show: the organization selected alone, the one that
/// unpacked into the building, or else the building itself.
fn holder(sim: &World, mode: &Mode) -> Option<Piece> {
    Some(match *mode.actor()? {
        Actor::Organization(r) => Piece::Organization(r),
        Actor::Building(b, _) => {
            let unpacked = (sim.organizations.iter()).position(|r| r.unpacked_into == Some(b));
            unpacked.map_or(Piece::Building(b), Piece::Organization)
        }
    })
}

/// The properties an organization or building holds that have an icon: its
/// template's own, then for an organization its formation's, its
/// terrain's, its stance's, its morale's and its entrenchment's, then those
/// it has for a time. Those in `shown` come first, as they stand there, so
/// that icons keep the order their properties came in while their holder
/// stays selected.
fn held(sim: &World, looks: &Looks, holder: Piece, shown: &[Id]) -> Vec<Id> {
    let own = |template: &Id| {
        let kind = sim.rules().kinds.get(template);
        kind.into_iter().flat_map(|k| &k.properties).cloned()
    };
    let all: Vec<Id> = match holder {
        Piece::Organization(r) => {
            let organization = &sim.organizations[r];
            // One that's unpacked stands in no formation, and guards nothing.
            let afield = organization.unpacked_into.is_none();
            let formation = organization
                .formations
                .get(organization.current_formation());
            let formation = formation
                .filter(|_| afield)
                .and_then(|f| looks.formations.get(&Id::from(f.id.as_str())));
            let guarding = (afield && organization.guarding()).then(|| Id::from(GUARDING));
            let mind = sim.state(r).map(Id::from);
            let timed = organization.properties.iter().map(|h| h.property.clone());
            let entrenched = organization.entrenched.iter().flatten().cloned();
            let level = sim.level_property(r).cloned();
            own(&organization.template)
                .chain(level)
                .chain(formation.into_iter().flatten().cloned())
                .chain(organization.terrain_properties.iter().cloned())
                .chain(guarding)
                .chain(mind)
                .chain(entrenched)
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
    hqs: &[tgsim::Hq],
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
        let attached = sim.attached(hq.id).map(Place::Organization);
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
            Some(Actor::Organization(r)) => std::slice::from_ref(r),
            _ => &[],
        },
    };
    let attached = slots.iter().filter_map(|place| match place {
        Place::Organization(r) => Some(*r),
        _ => None,
    });
    let members = (own.iter()).flat_map(|&r| sim.organizations[r].units.iter().copied());
    let members = members.collect();
    let flying = own.iter().copied().chain(attached);
    let wears = |r| banners::wears(sim, hqs, r).map(|hq| (hq, badges.side(hq)));
    let banners = flying.map(|r| (r, wears(r), sim.lacks_supply(r))).collect();
    let properties = holder(sim, mode).map_or(Vec::new(), |of| held(sim, looks, of, shown));
    Detail {
        slots,
        banners,
        properties,
        members,
    }
}

/// Works out what the display should show, for either interface to draw.
pub fn choose(
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    local: Res<Local>,
    badges: Res<Badges>,
    looks: Res<Looks>,
    mut showing: ResMut<Showing>,
) {
    let hqs = sim.0.hqs();
    let mode = wanted(&sim.0, &hqs, local.0, &selected.0, chosen.0);
    let mode = mode.map(|m| {
        let shown = match &showing.now {
            Some((was, detail)) if holder(&sim.0, was) == holder(&sim.0, &m) => {
                detail.properties.as_slice()
            }
            _ => &[],
        };
        let detail = detail(&sim.0, &hqs, &badges, &looks, &m, shown);
        (m, detail)
    });
    // Who plays decides what owners are called.
    if showing.now == mode && !local.is_changed() {
        return;
    }
    let icons_only = !local.is_changed()
        && match (&showing.now, &mode) {
            (Some((a, was)), Some((b, now))) => {
                a == b && was.slots == now.slots && was.banners == now.banners
            }
            _ => false,
        };
    *showing = Showing {
        now: mode,
        icons_only,
    };
}

/// The picture of a template, or of a technology being researched.
pub(crate) fn picture(
    drawn: &Drawn,
    looks: &mut Looks,
    game: &GameData,
    images: &mut Assets<Image>,
    template: &str,
) -> Option<Handle<Image>> {
    let path = (drawn.0.icon(template).map(str::to_string))
        .or_else(|| looks.technologies.get(&Id::from(template)).cloned());
    looks.image(game, images, path.as_deref())
}

/// The colour an owner's name is in, by how the player stands towards it.
pub(crate) fn owner_colour(sim: &World, local: Option<u32>, owner: u32) -> Color {
    match local.is_some_and(|p| sim.sides.relation(p, owner) == Relation::Neutral) {
        true => NEUTRAL,
        false => TINTS[tint(sim, local, owner)],
    }
}

/// What an organization's banner is drawn from.
pub(crate) struct Flown<'a> {
    pub sim: &'a World,
    pub detail: &'a Detail,
    pub leaders: &'a Leaders,
    pub badges: &'a Badges,
    pub game: &'a GameData,
}

impl Flown<'_> {
    /// The organization at `r`'s banner, as it flies over it: its layers,
    /// bottom first, each in its colour, with none where it has no such
    /// layer.
    pub(crate) fn layers(
        &self,
        banners: &mut BannerImages,
        images: &mut Assets<Image>,
        r: usize,
    ) -> [(Option<Handle<Image>>, Color); 5] {
        let (sim, game) = (self.sim, self.game);
        let organization = &sim.organizations[r];
        let [red, green, blue] = (sim.sides.player(organization.owner)).map_or([1.0; 3], |p| p.rgb);
        let template = organization.template.as_str();
        let icon = banners.icon(game, images, template);
        let shown = self.detail.banners.iter().find(|(of, ..)| *of == r);
        let (wears, lacks) = shown.map_or((None, false), |&(_, wears, lacks)| (wears, lacks));
        let faction = self.leaders.banner(organization.owner);
        let looks = banners.layers(game, images, template, faction);
        let badge =
            wears.and_then(|(hq, _)| self.badges.badge(banners, game, images, template, hq));
        let out_of_supply = looks.out_of_supply.filter(|_| lacks);
        [
            (looks.plate.clone(), Color::srgb(red, green, blue)),
            (icon, Color::WHITE),
            (looks.frame, Color::WHITE),
            (out_of_supply, Color::WHITE),
            (badge, Color::WHITE),
        ]
    }
}

/// The template whose portrait stands for what the display shows, and
/// its owner: a building's own, or an organization's primary slot's, which for
/// an HQ is the building once it's unpacked. None for an organization with no
/// such slot, and for several things.
fn portrayed<'a>(sim: &'a World, looks: &'a Looks, mode: &Mode) -> Option<(&'a str, u32)> {
    match mode.actor()? {
        Actor::Building(b, _) => {
            let building = &sim.buildings[*b];
            Some((building.template.as_str(), building.owner))
        }
        Actor::Organization(r) => {
            let organization = &sim.organizations[*r];
            let template = match organization.unpacked_into {
                Some(b) => sim.buildings[b].template.as_str(),
                // A unit on its own is no organization.
                None => match looks.primaries.get(&organization.template) {
                    Some(primary) => primary.as_ref()?.as_str(),
                    None => organization.template.as_str(),
                },
            };
            Some((template, organization.owner))
        }
    }
}

/// Who the portrait of `template`, owned by `owner`, is of: its owner's
/// faction where it says so and there is one, else the face it names, and
/// failing either its own model.
fn sitter(drawn: &Drawn, leaders: &Leaders, template: &str, owner: u32) -> Sitter {
    let faction = (drawn.0.shows_faction_portrait(template))
        .then(|| leaders.0.get(&owner))
        .flatten();
    let face = match faction {
        Some(faction) => faction.portrait.clone(),
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

/// Draws the portrait of what the display shows, for either interface to
/// show once it's there.
#[allow(clippy::too_many_arguments)]
pub fn portray(
    mut commands: Commands,
    showing: Res<Showing>,
    sim: Res<Sim>,
    leaders: Res<Leaders>,
    looks: Res<Looks>,
    mut drawn: ResMut<Drawn>,
    mut assets: MapAssets,
    mut stage: ResMut<Stage>,
    mut asked: bevy::ecs::system::Local<Option<(Sitter, Alone)>>,
    suns: Query<&Transform, With<DirectionalLight>>,
) {
    let mode = showing.now.as_ref().map(|(mode, _)| mode);
    let wanted = mode.and_then(|mode| {
        let (template, owner) = portrayed(&sim.0, &looks, mode)?;
        // Each thing shown on its own gets a portrait afresh, where a
        // organization keeps one that's already up.
        let alone = match mode {
            Mode::SimpleActor(Actor::Building(b, _)) => Some((true, *b)),
            Mode::SimpleActor(Actor::Organization(r)) => Some((false, *r)),
            _ => None,
        };
        Some((sitter(&drawn, &leaders, template, owner), alone))
    });
    // Asked once, as one that can't be drawn still can't the next time.
    if *asked != wanted {
        match &wanted {
            Some((sitter, alone)) => {
                let models = &mut drawn.0;
                let assets = &mut assets.models();
                let greeting = alone.is_some();
                let sun = suns.iter().next().map_or(Dir3::NEG_Y, |sun| sun.forward());
                models.sit(&mut commands, assets, &mut stage, sitter, greeting, sun);
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
}

/// Whether the player sees `of` now, rather than only remembering it.
fn seen_now(sim: &World, fog: Option<&crate::fog::Fog>, of: Target) -> bool {
    fog.is_none_or(|f| {
        f.lifted
            || match of {
                Target::Unit(u) => f.shows_organization(sim, sim.units[u].organization),
                Target::Building(b) => f.building(b) == crate::fog::Look::Shown,
            }
    })
}

fn visible(shown: bool) -> Visibility {
    match shown {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    }
}

/// A weapon's tooltip: its name, its damage now as `figure`, the game's
/// words for its rate and area, and what kind of weapon it is.
fn weapon_tip(looks: &Looks, weapon: &tgdata::Weapon, figure: String) -> Vec<String> {
    let shown = &weapon.shown;
    let row = |key: &str, value: &Option<String>| {
        value.as_ref().map(|v| format!("{} {v}", looks.said(key)))
    };
    // The glyph for its damage's type has nowhere to go in a tooltip.
    let damage = looks.said("ability_effect_desc_main_damage_glyph_long|cf");
    let damage = (!figure.is_empty()).then(|| damage.replace("%lc", "").replacen("%g", &figure, 1));
    let kinds = [
        (weapon.bombard, "ui_tooltip_bombard"),
        (weapon.vorpal, "ui_tooltip_vorpal"),
    ];
    let mut lines = vec![shown.name.clone(), damage];
    lines.push(row("ui_tooltip_item_title_recharge_rate", &shown.rate));
    lines.push(row("ui_tooltip_item_title_area", &shown.area));
    lines.extend(
        kinds
            .iter()
            .filter(|(is, _)| *is)
            .map(|(_, key)| Some(looks.said(key))),
    );
    lines
        .into_iter()
        .flatten()
        .filter(|l| !l.is_empty())
        .collect()
}

/// What a weapon's figure isn't, among the display's other words.
type OtherFigure = (Without<Status>, Without<Defends>);
/// What a weapon isn't, among the display's other tooltips.
type OtherTip = (Without<Says>, Without<Shield>);

/// Keeps the display's bars, pictures, tooltips and status up to date.
#[allow(clippy::too_many_arguments)]
pub fn refresh(
    sim: Res<Sim>,
    looks: Res<Looks>,
    drawn: Res<Drawn>,
    leaders: Res<Leaders>,
    local: Res<Local>,
    mut bars: Query<(&Fill, &mut Meter)>,
    mut targets: Query<(&EfficiencyTarget, &mut Node)>,
    fog: Option<Res<crate::fog::Fog>>,
    mut tips: Query<(&Says, &mut Hint, &Hovered)>,
    mut pictures: Query<(&UnitPicture, &mut Shade)>,
    mut statuses: Query<(&Status, &mut Text)>,
    mut defenses: Query<(&Defends, &mut Text, &mut TextColor), Without<Status>>,
    mut shields: Query<(&Shield, &mut Hint, &Hovered, &mut Visibility), Without<Says>>,
    mut strikes: Query<(&Strikes, &mut Text, &mut TextColor), OtherFigure>,
    mut armed: Query<(&Armed, &mut Hint, &Hovered, &mut Visibility), OtherTip>,
    theme: Res<ui::Theme>,
) {
    let sim = &sim.0;
    for (fill, mut meter) in &mut bars {
        let share = share(sim, *fill);
        if meter.share != share {
            meter.share = share;
        }
    }
    for (target, mut node) in &mut targets {
        let towards = towards(sim, target.0);
        if node.left != towards {
            node.left = towards;
        }
    }
    let teller = Teller {
        sim,
        looks: &looks,
        drawn: &drawn,
        leaders: &leaders,
        local: local.0,
        fog: fog.as_deref(),
    };
    // Only what the pointer's on has anyone to tell.
    for (says, mut hint, _) in tips.iter_mut().filter(|(.., on)| on.get()) {
        let words = teller.tip(*says).join("\n");
        if hint.0 != words {
            hint.0 = words;
        }
    }
    let defense = |of: Target| {
        let (now, plain) = sim.defense(of).unwrap_or_default();
        // Adding nothing keeps what rounds up to nothing from reading "-0".
        (format!("{:.0}", now.round() + 0.0), now.partial_cmp(&plain))
    };
    let tint = |against: Option<std::cmp::Ordering>, plain: Option<Color>| match against {
        Some(std::cmp::Ordering::Greater) => RAISED,
        Some(std::cmp::Ordering::Less) => LOWERED,
        _ => plain.unwrap_or_else(|| theme.ink(Ink::Body)),
    };
    for (of, mut text, mut colour) in &mut defenses {
        let (figure, against) = defense(of.0);
        let tint = tint(against, of.1);
        if text.0 != figure {
            text.0 = figure;
        }
        colour.set_if_neq(TextColor(tint));
    }
    // Neither shows for a thing only remembered.
    let seen = |of: Target| seen_now(sim, fog.as_deref(), of);
    for (of, mut hint, on, mut shown) in &mut shields {
        shown.set_if_neq(visible(seen(of.0)));
        if !on.get() {
            continue;
        }
        let title = looks.said("ui_tooltip_defense_value");
        let explains = looks.said("template_game_DefenseValues_ToolTip");
        let mut words = vec![title, defense(of.0).0, explains];
        words.retain(|line| !line.is_empty());
        let words = words.join("\n");
        if hint.0 != words {
            hint.0 = words;
        }
    }
    // A weapon whose plain main damage is nothing shows no figure.
    let attack = |of: Target, i: usize| {
        let (now, plain) = sim.attack(of, i)?;
        let figure = format!("{:.0}", now.round() + 0.0);
        let figure = if plain == 0.0 { String::new() } else { figure };
        Some((figure, now.partial_cmp(&plain)))
    };
    for (of, mut text, mut colour) in &mut strikes {
        let Some((figure, against)) = attack(of.0, of.1) else {
            continue;
        };
        if text.0 != figure {
            text.0 = figure;
        }
        colour.set_if_neq(TextColor(tint(against, of.2)));
    }
    for (of, mut hint, on, mut shown) in &mut armed {
        shown.set_if_neq(visible(seen(of.0)));
        if !on.get() {
            continue;
        }
        let (Some(weapon), Some((figure, _))) = (sim.weapons(of.0).get(of.1), attack(of.0, of.1))
        else {
            continue;
        };
        let words = weapon_tip(&looks, weapon, figure).join("\n");
        if hint.0 != words {
            hint.0 = words;
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
            Status::Organization(r) => organization_state(sim, r),
            Status::Building(b) => building_state(sim, b),
        };
        let words = ids.map(|i| looks.state(i)).unwrap_or_default();
        if text.0 != words {
            text.0 = words;
        }
    }
}

/// What an organization's doing, as the game's actor state ids.
fn organization_state(sim: &World, r: usize) -> Option<&'static str> {
    let organization = &sim.organizations[r];
    if sim.destroyed(r) {
        return None;
    }
    Some(match organization.mind {
        tgsim::Mind::Routing { .. } => "mind_routing",
        tgsim::Mind::Exhausted => "mind_exhausted",
        _ if !organization.engaged.is_empty() => "mind_engaged",
        tgsim::Mind::Reforming { .. } => "mind_reforming",
        _ if organization.chasing() => "mind_chasing",
        _ if matches!(organization.order, Order::Pursue(_)) => "mind_attacking",
        _ if organization.guarding() => "mind_guarding",
        _ if sim.underway(r) || organization.unpack_at.is_some() => match organization.order {
            Order::Run => "mind_retreating",
            Order::Attack | Order::Pursue(_) => "mind_attacking",
            Order::Move => "mind_moving",
        },
        // Weightier than resting, lighter than anything it's doing.
        _ if sim.lacks_supply(r) => "out_of_supply",
        _ if sim.entrenched(r) => tgdata::ENTRENCHED,
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
        Some(tgsim::Work::Research(_)) => Some("structure_researching"),
        Some(tgsim::Work::Raise(_)) => Some("structure_recruiting"),
        None => None,
    };
    match () {
        _ if building.packing.is_some() => Some("structure_packing"),
        _ if building.unpacking.is_some() => Some("structure_unpacking"),
        _ if building.constructing => Some("construction"),
        _ if queue.is_some() => queue,
        _ if sim.out_of_supply(b) && building.standing() => Some("out_of_supply"),
        _ if building.state == State::Disabled => Some("disabled"),
        _ => None,
    }
}

/// How full a bar filled as `fill` says is, from 0 to 1.
pub(crate) fn share(sim: &World, fill: Fill) -> f32 {
    let (now, full) = measure(sim, fill);
    match (fill, full > 0.0) {
        (_, true) => (now / full).clamp(0.0, 1.0),
        // What has no health to lose shows whole.
        (Fill::Unit(_) | Fill::Organization(_) | Fill::Building(_) | Fill::Flight, false) => 1.0,
        (Fill::Progress(_) | Fill::Stockpile(_) | Fill::Efficiency(_) | Fill::Morale(_), false) => {
            0.0
        }
    }
}

/// How much a bar has of how much it could: health, a commission's
/// progress out of 100, or supply. Nothing of nothing for what has no
/// such measure.
fn measure(sim: &World, fill: Fill) -> (f32, f32) {
    let unit = |u: usize| {
        let unit = &sim.units[u];
        let full = sim.max_health(u);
        match unit.alive() {
            true => (unit.health, full),
            false => (0.0, full),
        }
    };
    match fill {
        Fill::Unit(u) => unit(u),
        Fill::Organization(r) => {
            let units = sim.organizations[r].units.iter().map(|&u| unit(u));
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
            (b.health, b.fighter.health * b.health_factor)
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
        Fill::Efficiency(r) => (sim.organizations[r].efficiency, 1.0),
        Fill::Morale(r) => {
            let organization = &sim.organizations[r];
            (organization.morale, organization.morale_max)
        }
    }
}

/// What the tooltips of the display's things read.
struct Teller<'a> {
    sim: &'a World,
    looks: &'a Looks,
    drawn: &'a Drawn,
    leaders: &'a Leaders,
    local: Option<u32>,
    fog: Option<&'a crate::fog::Fog>,
}

impl Teller<'_> {
    /// The lines of a thing's tooltip: its template's name, then what
    /// applies to it of its health, its stockpile and its faction. A
    /// organization has no health of its own to give.
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
                lines.extend(self.experience(unit.organization));
                // One that's dead says only what its template would.
                if unit.alive() {
                    let owner = sim.organizations[unit.organization].owner;
                    lines.extend(self.faction(&unit.template, owner));
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
                lines.extend(self.faction(&building.template, building.owner));
                lines
            }
            Says::Organization(r) => {
                let organization = &sim.organizations[r];
                let mut lines = vec![name(&organization.template)];
                // What it's like now is told only of one in sight.
                if self.sees(r) {
                    let percent = (100.0 * organization.efficiency).round() as i32;
                    let efficiency = match looks.said("ui_tooltip_attack_efficiency|d") {
                        said if said.is_empty() => format!("{percent}%"),
                        said => crate::messages::filled(&said, &[percent.to_string()]),
                    };
                    lines.push(row("ui_tooltip_item_title_attack_efficiency", efficiency));
                    if organization.morale_max > 0.0 {
                        let morale =
                            format!("{:.0}/{:.0}", organization.morale, organization.morale_max);
                        lines.push(row("ui_tooltip_item_title_morale", morale));
                    }
                }
                lines.extend(self.experience(r));
                lines
            }
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
                if let (0, Some(pause)) = (place, &building.paused) {
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

    /// The experience the organization at `r` has, if it earns any: its
    /// level's name, then its points over the next level's.
    fn experience(&self, r: usize) -> Option<String> {
        let sim = self.sim;
        let level = sim.level_property(r)?;
        let points = sim.organizations[r].experience;
        let held = match sim.next_level(r) {
            Some(next) => format!("{points:.0}/{next:.0}"),
            None => format!("{points:.0}"),
        };
        let said = match self.looks.levels.get(level) {
            Some(name) => format!("{name} {held}"),
            None => held,
        };
        Some(self.row("ui_tooltip_item_title_experience", said))
    }

    /// What a template that shows its owner's faction says of them: who,
    /// and to their own side and its allies, their experience.
    fn faction(&self, template: &Id, owner: u32) -> Vec<String> {
        let faction = (self.drawn.0.shows_faction_portrait(template.as_str()))
            .then(|| self.leaders.0.get(&owner))
            .flatten();
        let Some(faction) = faction else {
            return Vec::new();
        };
        let mut lines = vec![self.row("ui_tooltip_item_general", faction.name.clone())];
        if let (true, Some(experience)) = (
            ours(self.sim, self.local, owner),
            self.sim.experience(owner),
        ) {
            lines.push(self.row("ui_tooltip_item_experience", format!("{experience:.0}")));
        }
        lines
    }

    /// Whether the player sees the organization at `r` now.
    fn sees(&self, r: usize) -> bool {
        self.fog
            .is_none_or(|f| f.lifted || f.shows_organization(self.sim, r))
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
pub(crate) fn counted(sim: &World, local: Option<u32>, status: Status) -> Option<Ledger> {
    let (owner, ledger) = match status {
        Status::Building(b) => (sim.buildings[b].owner, sim.building_ledger(b)),
        Status::Organization(r) => {
            let organization = &sim.organizations[r];
            let ledger = match organization.unpacked_into {
                Some(b) => sim.building_ledger(b),
                None => sim.organization_ledger(r),
            };
            (organization.owner, ledger)
        }
    };
    ours(sim, local, owner).then_some(ledger)
}

/// What a thing counts for in its side's economy, as the display's line
/// has it, by resource in the order `resources` are defined: of each what
/// it earns less what it takes to keep, signed, but for a limited
/// resource taken up, which is bare. Nothing for what comes to nothing.
pub(crate) fn economy(ledger: &Ledger, resources: &[tgdata::ResourceKind]) -> Vec<(Id, String)> {
    let entries = resources.iter().filter_map(|r| {
        let kind = r.id.as_str();
        let net = ledger.production.get(kind) - ledger.upkeep.get(kind);
        // To few enough figures that sums' rounding doesn't show.
        let (net, figure) = match r.role == tgdata::ResourceRole::LimitedConsumed {
            true => (net.abs().round(), format!("{}", net.abs().round())),
            false => {
                let net = (net * 1000.0).round() / 1000.0;
                (net, format!("{net:+}"))
            }
        };
        (net != 0.0).then(|| (r.id.clone(), figure))
    });
    entries.collect()
}

/// An organization's name as the game gives it: the one its map gives it, else
/// its template's, numbered if it's one its side numbers. `names` are the
/// templates' names, by template, `strings` the game's text, for the
/// numbering, and `map` the map's own.
pub(crate) fn organization_name(
    sim: &World,
    names: &HashMap<Id, String>,
    strings: &tgdata::Strings,
    map: &tgdata::Strings,
    r: usize,
) -> String {
    let organization = &sim.organizations[r];
    if let Some(name) = map_name(map, organization.id) {
        return name;
    }
    numbered(
        strings,
        organization.number,
        template_name(names, organization.template.as_str()),
    )
}

/// A building's name, as [`organization_name`]: numbered as the organization
/// it stands in is. A city's buildings go by the name the map gives the
/// city.
pub(crate) fn building_name(
    sim: &World,
    names: &HashMap<Id, String>,
    strings: &tgdata::Strings,
    map: &tgdata::Strings,
    b: usize,
) -> String {
    // A city's name is the map's, where it gives one.
    let city = sim.city_of(b).map(|c| sim.buildings[c].id);
    if let Some(name) = city.and_then(|c| map_name(map, c)) {
        return name;
    }
    let building = &sim.buildings[b];
    if let Some(name) = map_name(map, building.id) {
        return name;
    }
    numbered(
        strings,
        building.number,
        template_name(names, building.template.as_str()),
    )
}

/// The name a map gives the thing with an id, if it gives it one.
fn map_name(map: &tgdata::Strings, id: u32) -> Option<String> {
    map.get(&format!("name_{id}")).cloned()
}

pub(crate) fn template_name(names: &HashMap<Id, String>, template: &str) -> String {
    (names.get(&Id::from(template)).cloned()).unwrap_or_else(|| template.to_string())
}

/// `name` with `number` before it, as "1st Infantry Regiment", in the
/// game's words for each ending where it has them.
pub(crate) fn numbered(strings: &tgdata::Strings, number: Option<u32>, name: String) -> String {
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

/// Whether an organization is a unit on its own, such as a truck.
pub(crate) fn lone(sim: &World, r: usize) -> bool {
    let organization = &sim.organizations[r];
    matches!(&organization.units[..], [u] if sim.units[*u].template == organization.template)
}

/// An organization's captain, whose portrait stands for it: the unit in its
/// primary slot, else its leader.
pub(crate) fn captain(sim: &World, looks: &Looks, r: usize) -> usize {
    let organization = &sim.organizations[r];
    let primary = (looks.primaries.get(&organization.template)).and_then(|p| p.as_ref());
    let unit = primary.and_then(|template| {
        let is = |&&u: &&usize| sim.units[u].template == *template;
        organization.units.iter().find(is).copied()
    });
    unit.unwrap_or_else(|| leader(sim, r))
}

/// An organization's commanding officer, else its first unit.
fn leader(sim: &World, r: usize) -> usize {
    let units = &sim.organizations[r].units;
    let officer = units
        .iter()
        .copied()
        .find(|&u| is_officer(sim.units[u].template.as_str()));
    officer.unwrap_or(units[0])
}

/// Whether a unit template is an organization's commanding officer, which the
/// game names `..._co`.
pub(crate) fn is_officer(template: &str) -> bool {
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

pub(crate) fn picture_node(picture: Option<Handle<Image>>) -> ImageNode {
    ImageNode::new(picture.unwrap_or_default())
}

/// An organization's card clicked: selects it alone, in place of any building.
pub(crate) fn pick(
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
pub(crate) fn cancel(
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
            let cancel = tgsim::Command::CancelCommission {
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
    fn the_efficiency_marker_comes_from_its_bars_layout() {
        use tgdata::widgets::{ViewElement, Widget};
        let widget = |name: &str, parts, children| Widget {
            name: name.into(),
            kind: None,
            visible: true,
            view: None,
            parts,
            children,
            fields: Vec::new(),
        };
        let decal = ViewElement {
            name: "Decal".into(),
            kind: None,
            bounds: Default::default(),
            when: Vec::new(),
            texture: Some("UI/Game/ObjectDisplay/TargetPercentageBar.tga".into()),
            colour: [1.0; 3],
            text: None,
            font: None,
            justify: None,
            tiles: None,
            fields: Vec::new(),
            children: Vec::new(),
        };
        let marker = widget(EFFICIENCY_TARGET, vec![decal], Vec::new());
        let bar = widget(EFFICIENCY_BAR, Vec::new(), vec![marker]);
        let display = widget("CompanyDisplay", Vec::new(), vec![bar]);
        let layouts = [widget("ObjectDisplay", Vec::new(), vec![display])];
        assert_eq!(
            efficiency_target(&layouts).as_deref(),
            Some("UI/Game/ObjectDisplay/TargetPercentageBar.tga")
        );
        assert_eq!(efficiency_target(&layouts[0].children[0].children), None);
    }

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
        // AA's, in its order: a limit made room in before one taken up.
        use tgdata::ResourceKind as R;
        let resources = [
            R::stockpiled("money"),
            R::upkeep("ammo", Default::default()),
            R::upkeep("oil", Default::default()),
            R::upkeep("unit_limit_provided", Default::default()),
            R::limited("unit_limit_consumed", "unit_limit_provided"),
            R::upkeep("building_limit_provided", Default::default()),
            R::limited("building_limit_consumed", "building_limit_provided"),
        ];
        let ledger = |earned: &[(&str, f32)], kept: &[(&str, f32)]| {
            let mut ledger = Ledger::default();
            earned
                .iter()
                .for_each(|&(k, a)| ledger.production.set(k, a));
            kept.iter().for_each(|&(k, a)| ledger.upkeep.set(k, a));
            ledger
        };
        let line = |ledger: Ledger| -> Vec<String> {
            let line = economy(&ledger, &resources).into_iter();
            line.map(|(kind, figure)| format!("{kind} {figure}"))
                .collect()
        };
        // An HQ's truck: what it earns, the organizations it makes room for,
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
        // An organization: what it takes to keep, and the one it counts as.
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
    fn names_are_numbered_in_the_games_words() {
        let strings = tgdata::Strings::from_iter([
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
    fn organizations_are_numbered_in_english() {
        let words: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22, 101].map(ordinal).into();
        assert_eq!(
            words,
            [
                "1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd", "101st"
            ]
        );
    }
}
