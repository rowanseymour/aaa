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
//!   banner between its efficiency and morale, its captain's health, and
//!   a small bar for each of its [`small_bars`]; clicking one selects it
//!   alone.
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

use bevy::ecs::system::SystemParam;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use tgdata::{GUARDING, GameData, Id, PropertyLook};
use tgrender::MapAssets;
use tgrender::overlay::BannerImages;
use tgrender::portrait::{Sitter, Stage};
use tgsim::{Ledger, Pause, Piece, Relation, State, Target, World};

use crate::banners::{self, Badges};
use crate::marks::tint;
use crate::ui::{self, Hint, Ink, Meter, Shade, visible};
use crate::{
    Drawn, Leaders, Loaded, Local, Orders, Screen, Selected, SelectedBuilding, Sim, TINTS,
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

/// How many properties' icons the display has places for: the rest are
/// gathered behind a marker.
pub(crate) const MAX_PROPERTIES: usize = 7;

/// How much brighter a banner's plate shows in a portrait than its owner's
/// colour: each channel times this, held to 1, then times the plate's
/// image. Measured from the game's screens, where blue (0, 0.3, 1) shows
/// as (0, 0.6, 1) and green (0, 0.73, 0.1) as (0, 1, 0.2) over the image.
const PORTRAIT_LIGHT: f32 = 2.0;

/// How many of a building's commissions the display has places for when
/// its layout doesn't say: the rest are shown only to be there.
pub(crate) const MAX_QUEUED: usize = 6;

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
    /// The properties whose icons show, those held longest first, each
    /// with what tints its icon.
    pub(crate) properties: Vec<(Id, Tint)>,
    /// The units in each place of the organizations shown, which a
    /// replacement changes.
    pub(crate) members: Vec<usize>,
    /// Whether the building shown has a supply stockpile bar: a relay's,
    /// once it's done unpacking.
    pub(crate) stockpile: bool,
    /// The thing shown alone, as the fog tells whether it's seen now.
    pub(crate) seen_as: Option<Target>,
}

/// What a property's icon is tinted by: where its holder has it from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Tint {
    /// Its own, or its activity's: untinted.
    Plain,
    /// Its side's, such as a technology, or its own research.
    Side,
    /// What a building hands its side.
    Support,
    /// An ability's, used by its holder's side or an ally.
    Friendly,
    /// An ability's, used by an enemy.
    Enemy,
    /// An ability's, used by a side that's neither: untinted.
    Neutral,
}

/// The actor state a building is in while under siege, whose properties
/// it holds then.
const UNDER_SIEGE: &str = "under_siege";

/// Part of the display that shows only while the thing shown is seen now,
/// not only remembered: the property icons and the supply stockpile bar.
#[derive(Component, Clone, Copy)]
pub struct WhileSeen(pub(crate) Target);

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
    /// That of several organizations, in the order they were selected.
    /// Its name is the first's, which has been selected longest, and its
    /// cards go newest first: see [`newest_first`].
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
    /// One unit of the selected organization, on its own: the
    /// organization's index, then the unit's.
    Unit(usize, usize),
}

/// What of the selected organization the display shows on its own, if
/// anything: the organization's index, then the unit, or a Division HQ's
/// building. It holds while that organization, or that building, alone
/// stays selected.
#[derive(Resource, Default)]
pub struct ShownAlone(pub(crate) Option<(usize, Target)>);

/// A part that turns the display to a unit, or a Division HQ's
/// building, on its own, with what's selected left as it is.
#[derive(Component, Clone, Copy)]
pub struct ShowAlone(pub(crate) Target);

/// Makes `part` the button called `id`, which shows `of` on its own, or
/// with none, does nothing, as clicking what's already the whole
/// selection does.
pub(crate) fn shows_alone(part: &mut EntityCommands, of: Option<Target>, id: &str) {
    part.insert((Button, ui::called(id), Pickable::default()));
    if let Some(of) = of {
        part.insert(ShowAlone(of)).observe(show_alone);
    }
}

/// A portrait or a unit's card clicked with the left button: shows its
/// unit or building on its own, if that's still its organization's and
/// the organization, or the building, alone is selected.
pub(crate) fn show_alone(
    activate: On<Activate>,
    parts: Query<&ShowAlone>,
    mouse: Res<ButtonInput<MouseButton>>,
    sim: Res<Sim>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    mut alone: ResMut<ShownAlone>,
) {
    if !left(&mouse) {
        return;
    }
    let Ok(&ShowAlone(of)) = parts.get(activate.entity) else {
        return;
    };
    let Some(r) = owning(&sim.0, of) else {
        return;
    };
    let building = match of {
        Target::Building(b) => Some(b),
        Target::Unit(_) => None,
    };
    if selected.0 == [r] || (selected.0.is_empty() && building.is_some() && chosen.0 == building) {
        alone.0 = Some((r, of));
    }
}

/// Whether a button's activation now is the left mouse button's, or
/// the console's or a key's: a button activates on any mouse button, and
/// the display's act on the left alone.
fn left(mouse: &ButtonInput<MouseButton>) -> bool {
    !mouse.any_just_released([MouseButton::Right, MouseButton::Middle])
}

/// A part that gives Command All for the organization at its index: the
/// selection becomes every organization of its HQ, or of it as one.
#[derive(Component, Clone, Copy)]
pub struct CommandsAll(pub(crate) usize);

/// Makes `part` the button called `object.banner`, which gives Command
/// All for the organization at `r`.
pub(crate) fn commands_all(part: &mut EntityCommands, r: usize) {
    let button = (Button, ui::called("object.banner"), Pickable::default());
    part.insert((button, CommandsAll(r))).observe(command_all);
}

/// A banner of the player's own clicked with the left button: selects
/// every organization attached to the HQ its organization is, or is
/// attached to; one with no HQ is left as it is. Another side's isn't
/// commanded, and would give away where its others stand.
fn command_all(
    activate: On<Activate>,
    parts: Query<&CommandsAll>,
    mouse: Res<ButtonInput<MouseButton>>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
) {
    let Ok(&CommandsAll(r)) = parts.get(activate.entity) else {
        return;
    };
    if !left(&mouse) {
        return;
    }
    if local.0 != Some(sim.0.organizations[r].owner) {
        return;
    }
    let Some(hq) = hq_for(&sim.0, r) else {
        return;
    };
    selected.0 = sim.0.attached(hq).collect();
    chosen.0 = None;
}

/// The HQ the organization at `r` is, packed or unpacked, or else the
/// one it's attached to.
fn hq_for(sim: &World, r: usize) -> Option<u32> {
    let organization = &sim.organizations[r];
    let own = match organization.unpacked_into {
        Some(b) => sim.hq_of(b),
        None => sim.packed_hq(r),
    };
    own.or(organization.hq)
}

/// The organization `of` is part of, while it stands: a living unit's, or
/// the one a building unpacked from while it's still that one's.
fn owning(sim: &World, of: Target) -> Option<usize> {
    match of {
        Target::Unit(u) => (sim.units.get(u).filter(|unit| unit.alive())).map(|u| u.organization),
        Target::Building(b) => {
            let building = sim.buildings.get(b).filter(|b| !b.gone())?;
            let r = building.unpacked_from?;
            (sim.organizations[r].unpacked_into == Some(b)).then_some(r)
        }
    }
}

/// What a right click on a part of the display centres the camera on.
#[derive(Component, Clone, Copy)]
pub struct Centres(pub(crate) Says);

/// Makes a right click on `part` centre the camera on `of`.
pub(crate) fn centres(part: &mut EntityCommands, of: Says) {
    part.insert((Centres(of), Pickable::default()))
        .observe(centre);
}

/// Centres the camera on what a right-clicked part stands for.
fn centre(
    click: On<PointerClick>,
    parts: Query<&Centres>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    mut cameras: Query<&mut tgrender::camera::RtsCamera>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Some([x, y]) = parts
        .get(click.entity)
        .ok()
        .and_then(|c| centred(&sim.0, c.0))
    else {
        return;
    };
    let height = loaded.map.ground.height_at([x, y]);
    for mut camera in &mut cameras {
        camera.focus = tgrender::space::world(x, y, height);
    }
}

/// Where centring on `of` looks: a unit while it lives, else its
/// organization, which is its building while it's unpacked into one.
fn centred(sim: &World, of: Says) -> Option<[f32; 2]> {
    let r = match of {
        Says::Unit(u) if sim.units[u].alive() => return Some(sim.position(Target::Unit(u))),
        Says::Unit(u) => sim.units[u].organization,
        Says::Organization(r) => r,
        Says::Building(b) => return Some(sim.position(Target::Building(b))),
        Says::Delivery(d) => return sim.deliveries.get(d).map(|d| d.position),
        Says::Queued { .. } => return None,
    };
    Some(match sim.organizations[r].unpacked_into {
        Some(b) => sim.position(Target::Building(b)),
        None => sim.centre(r),
    })
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
    /// The health of a position in an organization that no one fills.
    Vacant,
    /// An organization's attack efficiency.
    Efficiency(usize),
    /// An organization's morale.
    Morale(usize),
}

/// A marker on one of an organization's bars, by the organization's index.
#[derive(Component, Clone, Copy)]
pub enum Marker {
    /// On its efficiency bar, at the efficiency it's easing towards.
    Efficiency(usize),
    /// On its morale bar, at its breakpoint, the morale it breaks at.
    Breakpoint(usize),
}

impl Marker {
    /// The organization whose bar it's on.
    pub(crate) fn organization(self) -> usize {
        match self {
            Marker::Efficiency(r) | Marker::Breakpoint(r) => r,
        }
    }
}

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
    /// An aircraft bringing something in, by index into the sim's
    /// deliveries.
    Delivery(usize),
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
    /// A building, which says what its organization's doing if it's an
    /// HQ unpacked from one.
    Building(usize),
    /// A Division HQ's building shown on its own, which says what it's
    /// doing itself.
    Structure(usize),
    /// A unit shown on its own, which counts for nothing by itself.
    Unit(usize),
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
    /// The paths of our own interface's icons for resources, by the
    /// resource's id, drawn in place of their glyphs.
    pub(crate) icons: HashMap<Id, String>,
    pub(crate) strings: Arc<tgdata::Strings>,
    /// The map's own text, for the names it gives things and sides.
    pub(crate) map: Arc<tgdata::Strings>,
    /// How each property that has an icon shows, by id: the shared ones,
    /// and under ids of their own those of `formations`.
    pub(crate) properties: HashMap<Id, PropertyLook>,
    /// The properties an organization in each formation holds, by the
    /// formation's id.
    pub(crate) formations: HashMap<Id, Vec<Id>>,
    /// The property an organization of each template holds while its
    /// formation is broken, by template, for those with one that shows.
    pub(crate) broken: HashMap<Id, Id>,
    /// The properties a building holds while it's under siege.
    pub(crate) besieged: Vec<Id>,
    /// What tints the icons of properties from a side and from a
    /// building, by [`Tint::Side`] and [`Tint::Support`].
    pub(crate) side_tint: Color,
    pub(crate) support_tint: Color,
    /// What tints the icons of properties abilities gave, by
    /// [`Tint::Friendly`] and [`Tint::Enemy`].
    pub(crate) friendly_tint: Color,
    pub(crate) enemy_tint: Color,
    /// How each formation shows on its button, by its id.
    pub(crate) formation_looks: Arc<HashMap<Id, tgdata::FormationLook>>,
    /// Each technology's research button icon, by id, for what's being
    /// researched.
    pub(crate) technologies: HashMap<Id, String>,
    /// The template in each organization's primary slot, by the
    /// organization's; none for one with no such slot.
    pub(crate) primaries: HashMap<Id, Option<Id>>,
    /// The unit template that stands for each organization's, as a control
    /// group's picture: see [`standing_for`].
    pub(crate) standing_for: HashMap<Id, Id>,
    /// Each member's slot property, e.g. `front`, in member order, for
    /// an organization of each template that's whole.
    pub(crate) kinds: HashMap<Id, Vec<Id>>,
    /// Which member, in member order, holds the primary position of an
    /// organization of each template that's whole, for those with one.
    pub(crate) primary_members: HashMap<Id, usize>,
    /// Every position of each organization template's layout, in order,
    /// with the unit template its own template names for it, and which
    /// is the primary one, for those with one.
    pub(crate) positions: HashMap<Id, (Vec<Option<Id>>, Option<usize>)>,
    /// The names of experience levels, by their properties' ids.
    pub(crate) levels: HashMap<Id, String>,
    /// The game's words for speeds, each with the speeds it's for.
    pub(crate) speeds: Vec<([f32; 2], String)>,
    /// What colours a tooltip's figure above, below and at its plain value.
    pub(crate) modifiers: [Color; 3],
    /// The templates that can be recruited, in the order the data has them.
    pub(crate) recruitables: Vec<Id>,
    /// What each building can be rebuilt into, by template.
    pub(crate) upgrades: HashMap<Id, Vec<Id>>,
    /// The properties each template gives others, by template.
    pub(crate) supports: HashMap<Id, Vec<Id>>,
    /// The archive path of the picture marking a point on an
    /// organization's bars, if the layouts have one: the efficiency
    /// bar's, which the morale bar shares.
    pub(crate) marker: Option<String>,
}

impl Looks {
    /// The icon, tooltip and tint of each of `properties` that has a
    /// look, as many as the display has places for, and a tooltip listing
    /// the rest if there are more.
    pub(crate) fn property_tips(
        &self,
        properties: &[(Id, Tint)],
    ) -> (Vec<(String, String, Color)>, Option<String>) {
        let mut tips: Vec<_> = (properties.iter())
            .filter_map(|(id, tint)| Some((self.properties.get(id)?, tint)))
            .map(|(look, tint)| {
                let said = std::iter::once(look.name.clone()).chain(look.lines.iter().cloned());
                (
                    look.icon.clone(),
                    said.collect::<Vec<_>>().join("\n"),
                    self.tint(*tint),
                )
            })
            .collect();
        let more = tips.split_off(tips.len().min(MAX_PROPERTIES));
        let more = (!more.is_empty()).then(|| {
            let tip = std::iter::once(self.said("additional_properties"));
            tip.chain(more.into_iter().map(|(_, tip, _)| tip))
                .collect::<Vec<_>>()
                .join("\n")
        });
        (tips, more)
    }

    /// The colour a property's icon is in, by where it came from.
    pub(crate) fn tint(&self, tint: Tint) -> Color {
        match tint {
            Tint::Plain | Tint::Neutral => Color::WHITE,
            Tint::Side => self.side_tint,
            Tint::Support => self.support_tint,
            Tint::Friendly => self.friendly_tint,
            Tint::Enemy => self.enemy_tint,
        }
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
    mut cache: ResMut<ui::ImageCache>,
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
    let standing_for = components.iter().filter_map(|(id, o)| {
        let layout = layouts.get(&o.layout)?;
        Some((id.clone(), standing_for(layout, o)?))
    });
    let kinds = components.iter().filter_map(|(id, o)| {
        let layout = layouts.get(&o.layout)?;
        let start = layout.formations.get(layout.default)?;
        let filled = o.slots.iter().enumerate().filter(|(_, e)| e.is_some());
        let kinds = filled.flat_map(|(i, _)| {
            let kind = layout.kinds.get(i).cloned().unwrap_or_else(|| Id::from(""));
            std::iter::repeat_n(kind, start.slots.get(i).map_or(1, Vec::len))
        });
        Some((id.clone(), kinds.collect()))
    });
    let primary_members = (components.iter())
        .filter_map(|(id, o)| Some((id.clone(), primary_member_of(layouts.get(&o.layout)?, o)?)));
    let positions = (components.iter())
        .filter_map(|(id, o)| Some((id.clone(), positions_of(layouts.get(&o.layout)?, o)?)));
    let (technologies, listings) = (game.technologies(), game.listings());
    let names = (technologies
        .iter()
        .map(|(id, t)| (id.clone(), t.name.clone())))
    .chain((listings.iter()).map(|(id, l)| (id.clone(), l.name.clone())));
    let glyphs = crate::ui::glyphs(game, &mut images, &mut cache);
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
    let mut broken = HashMap::new();
    for (template, look) in game.broken_looks() {
        let id = Id::from(format!("{template}/broken"));
        properties.insert(id.clone(), look);
        broken.insert(template, id);
    }
    let colour = |c: Option<[f32; 3]>, or: Color| c.map_or(or, |[r, g, b]| Color::srgb(r, g, b));
    commands.insert_resource(Looks {
        primaries: primaries.collect(),
        standing_for: standing_for.collect(),
        kinds: kinds.collect(),
        primary_members: primary_members.collect(),
        positions: positions.collect(),
        levels: game.level_names(),
        speeds: game.speed_words(),
        modifiers: {
            let [up, down, level] = game.modifier_colours();
            [
                colour(up, Color::srgb(0.0, 1.0, 0.0)),
                colour(down, Color::srgb(1.0, 0.0, 0.0)),
                colour(level, Color::WHITE),
            ]
        },
        recruitables: game.recruitables(),
        upgrades: game.upgrades(),
        supports: game.support_properties(),
        names: names.collect(),
        listings: listings.clone(),
        glyphs,
        icons: crate::ui::resource_icons(game),
        strings: game.strings(),
        map: loaded.map.strings.clone(),
        properties,
        formations,
        broken,
        besieged: game.state_properties(UNDER_SIEGE),
        side_tint: colour(
            game.technology_property_colour(),
            Color::srgb(1.0, 1.0, 0.0),
        ),
        support_tint: colour(game.support_property_colour(), Color::srgb(0.0, 0.0, 1.0)),
        friendly_tint: colour(game.friendly_property_colour(), Color::srgb(0.0, 1.0, 0.0)),
        enemy_tint: colour(game.enemy_property_colour(), Color::srgb(1.0, 0.0, 0.0)),
        formation_looks: game.formation_looks(),
        technologies: (technologies.iter())
            .filter_map(|(id, t)| Some((id.clone(), t.icon.clone()?)))
            .collect(),
        marker: efficiency_target(&game.game_widgets()),
    });
    commands.insert_resource(Showing::default());
    commands.insert_resource(ShownAlone::default());
}

/// The archive path of the picture marking where an efficiency bar is
/// heading, from the layouts.
fn efficiency_target(widgets: &[tgdata::widgets::Widget]) -> Option<String> {
    let bar = widgets.iter().find_map(|w| w.find(EFFICIENCY_BAR))?;
    let marker = bar.find(EFFICIENCY_TARGET)?;
    (marker.parts.iter()).find_map(|p| p.texture.clone())
}

/// How far along its bar `marker` stands, its right edge there.
pub(crate) fn marked(sim: &World, marker: Marker) -> Val {
    let share = match marker {
        Marker::Efficiency(r) => sim.efficiency_target(r),
        Marker::Breakpoint(r) => {
            let max = sim.organizations[r].morale_max;
            match max > 0.0 {
                true => sim.breakpoint(r) / max,
                false => 0.0,
            }
        }
    };
    percent(100.0 * share.clamp(0.0, 1.0))
}

/// The mode the selection calls for the display to show in.
fn wanted(
    sim: &World,
    hqs: &[tgsim::Hq],
    local: Option<u32>,
    selected: &[usize],
    chosen: Option<usize>,
    alone: Option<(usize, Target)>,
) -> Option<Mode> {
    let building = |b: usize| {
        // Only its side and their allies see what it's commissioning.
        let queue =
            (sim.buildings[b].queue.iter()).filter(|_| ours(sim, local, sim.buildings[b].owner));
        Actor::Building(b, queue.map(|c| c.template().to_string()).collect())
    };
    let actor = match (selected, chosen) {
        // A Division HQ's building, selected, shows on its own once asked.
        ([], Some(b)) if alone.is_some_and(|(_, of)| of == Target::Building(b)) => {
            return Some(Mode::SimpleActor(building(b)));
        }
        ([], Some(b)) => building(b),
        ([], None) => return None,
        ([r], _) => match alone {
            Some((of, Target::Unit(u))) if of == *r => {
                return Some(Mode::SimpleActor(Actor::Unit(of, u)));
            }
            _ => Actor::Organization(*r),
        },
        (rs, _) => {
            return Some(Mode::Group(rs.iter().take(MAX_SELECTED).copied().collect()));
        }
    };
    let company = match actor {
        Actor::Organization(r) => !lone(sim, r) || hq(sim, hqs, &actor).is_some(),
        Actor::Building(..) => hq(sim, hqs, &actor).is_some(),
        Actor::Unit(..) => false,
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
        Actor::Unit(..) => return None,
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

/// Whose properties show.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Holder {
    Piece(Piece),
    /// A unit shown on its own, by its index.
    Unit(usize),
}

/// Whose properties show: the organization selected alone, the one that
/// unpacked into the building, or else the building itself; or the unit
/// shown on its own.
fn holder(sim: &World, mode: &Mode) -> Option<Holder> {
    Some(match *mode.actor()? {
        Actor::Organization(r) => Holder::Piece(Piece::Organization(r)),
        Actor::Building(b, _) => {
            let unpacked = (sim.organizations.iter()).position(|r| r.unpacked_into == Some(b));
            Holder::Piece(unpacked.map_or(Piece::Building(b), Piece::Organization))
        }
        Actor::Unit(_, u) => Holder::Unit(u),
    })
}

/// The properties an organization or building holds that have an icon,
/// each with its tint: its template's own, its side's technologies, then
/// for an organization its formation's (or its broken formation's), its
/// terrain's, its activity's, its fight's and its entrenchment's, for a
/// building its siege's, then those it has for a time. Those in `shown`
/// come first, as they stand there, so that icons keep the order their
/// properties came in while their holder stays selected.
fn held(sim: &World, looks: &Looks, holder: Holder, shown: &[(Id, Tint)]) -> Vec<(Id, Tint)> {
    ordered(holding(sim, looks, holder), shown, |(p, _)| p)
}

/// What [`held`] gives before what's shown has its say: each property
/// once, at its first copy's place, with that copy's tint.
fn holding(sim: &World, looks: &Looks, holder: Holder) -> Vec<(Id, Tint)> {
    // A thing is made with its template's properties last listed first,
    // so its own before those it inherits.
    let own = |template: &Id| {
        let kind = sim.rules().kinds.get(template);
        kind.into_iter()
            .flat_map(|k| k.properties.iter().rev())
            .cloned()
    };
    let side = match holder {
        Holder::Piece(on) => sim.technologies_on(on),
        Holder::Unit(u) => sim.unit_technologies(u),
    };
    let side = side.into_iter().map(|(p, source)| {
        let tint = match source {
            tgsim::Source::Building(_) => Tint::Support,
            _ => Tint::Side,
        };
        (p.clone(), tint)
    });
    // Research a building does for itself counts as its side's, so it's
    // tinted as a technology is.
    let timed = |held: &[tgsim::Held]| -> Vec<(Id, Tint)> {
        let tint = |p: &Id| match looks.technologies.contains_key(p) {
            true => Tint::Side,
            false => Tint::Plain,
        };
        (held.iter())
            .map(|h| (h.property.clone(), tint(&h.property)))
            .collect()
    };
    let plain = |p: Id| (p, Tint::Plain);
    let all: Vec<(Id, Tint)> = match holder {
        Holder::Piece(Piece::Organization(r)) => {
            let organization = &sim.organizations[r];
            // One that's unpacked stands in no formation, and guards nothing.
            let afield = organization.unpacked_into.is_none();
            let formation = (organization
                .formations
                .get(organization.current_formation()))
            .filter(|_| afield)
            .and_then(|f| looks.formations.get(&Id::from(f.id.as_str())));
            let broken = looks.broken.get(&organization.template);
            let formation: Vec<Id> = match (afield && organization.broken(), broken) {
                (true, Some(b)) => vec![b.clone()],
                (true, None) => Vec::new(),
                (false, _) => formation.into_iter().flatten().cloned().collect(),
            };
            let fight = sim.fight_state(r);
            // It does one thing at a time, so engaging stops its guarding.
            let guards = afield && organization.guarding() && fight != Some(tgmap::ENGAGED);
            let guarding = guards.then(|| Id::from(GUARDING));
            let mind = sim.state(r).map(Id::from);
            let entrenched = organization.entrenched.iter().flatten().cloned();
            let level = sim.level_property(r).cloned();
            (own(&organization.template).chain(level).map(plain))
                .chain(side)
                .chain(formation.into_iter().map(plain))
                .chain(organization.terrain_properties.iter().cloned().map(plain))
                .chain(guarding.map(plain))
                .chain(mind.map(plain))
                .chain(fight.map(Id::from).map(plain))
                .chain(entrenched.map(plain))
                .chain(timed(&organization.properties))
                .collect()
        }
        Holder::Piece(Piece::Building(b)) => {
            let building = &sim.buildings[b];
            let besieged = (looks.besieged.iter())
                .filter(|_| building.besieged.is_some())
                .filter(|p| sim.may_hold(Piece::Building(b), p))
                .cloned();
            (own(&building.template).map(plain))
                .chain(side)
                .chain(besieged.map(plain))
                .chain(timed(&building.properties))
                .collect()
        }
        Holder::Unit(u) => {
            let unit = &sim.units[u];
            let r = unit.organization;
            // It has what its organization hands down that fits it, with
            // the tint it has there.
            let kept = &sim.rules().kept_from_members;
            let handed = holding(sim, looks, Holder::Piece(Piece::Organization(r)))
                .into_iter()
                .filter(|(p, _)| !kept.contains(p) && sim.unit_may_hold(u, p));
            let engaged =
                (sim.unit_states(u).contains(&UNIT_ENGAGED)).then(|| Id::from(tgmap::ENGAGED));
            let stance = sim.stance_property(u).cloned();
            // An ability's by how its holder's side stands towards the
            // side that used it.
            let owner = sim.organizations[r].owner;
            let given = (unit.properties.iter()).map(|h| {
                let tint = match h.by.map(|by| sim.sides.relation(owner, by)) {
                    Some(Relation::Itself | Relation::Ally) => Tint::Friendly,
                    Some(Relation::Enemy) => Tint::Enemy,
                    _ => Tint::Neutral,
                };
                (h.property.clone(), tint)
            });
            (own(&unit.template).map(plain))
                .chain(side)
                .chain(handed)
                .chain(engaged.map(plain))
                .chain(stance.map(plain))
                .chain(given)
                .collect()
        }
    };
    let mut now: Vec<(Id, Tint)> = Vec::new();
    for (p, tint) in all {
        if looks.properties.contains_key(&p) && !now.iter().any(|(q, _)| *q == p) {
            now.push((p, tint));
        }
    }
    now
}

/// The state a unit is in while it attacks a target of its own, as the
/// sim names it.
const UNIT_ENGAGED: &str = "mind_engaged";

/// The properties held `now` as their icons show: those already `shown`
/// as they stand there, then the rest, each matched by its `key`. What a
/// match carries comes from `now`, so a technology whose tint changes, as
/// when one of the side's grants of it goes, keeps its place.
fn ordered<T, K: PartialEq>(mut now: Vec<T>, shown: &[T], key: impl Fn(&T) -> &K) -> Vec<T> {
    let mut out = Vec::with_capacity(now.len());
    for s in shown {
        if let Some(i) = now.iter().position(|p| key(p) == key(s)) {
            out.push(now.remove(i));
        }
    }
    out.extend(now);
    out
}

fn detail(
    sim: &World,
    hqs: &[tgsim::Hq],
    badges: &Badges,
    looks: &Looks,
    mode: &Mode,
    shown: &[(Id, Tint)],
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
    // An HQ's building shows the banner of the organization it unpacked
    // from.
    let unpacked = match mode.actor() {
        Some(Actor::Building(b, _)) => sim.buildings[*b].unpacked_from,
        _ => None,
    };
    let flying = own.iter().copied().chain(unpacked).chain(attached);
    let wears = |r| banners::wears(sim, hqs, r).map(|hq| (hq, badges.side(hq)));
    let banners = flying.map(|r| (r, wears(r), sim.lacks_supply(r))).collect();
    let properties = holder(sim, mode).map_or(Vec::new(), |of| held(sim, looks, of, shown));
    let stockpile = match mode.actor() {
        Some(&Actor::Building(b, _)) => shows_stockpile(sim, b),
        _ => false,
    };
    let seen_as = match mode.actor() {
        Some(&Actor::Organization(r)) => Some(Target::Unit(captain(sim, looks, r))),
        Some(&Actor::Building(b, _)) => Some(Target::Building(b)),
        Some(&Actor::Unit(_, u)) => Some(Target::Unit(u)),
        None => None,
    };
    Detail {
        slots,
        banners,
        properties,
        members,
        stockpile,
        seen_as,
    }
}

/// Works out what the display should show, for either interface to draw.
#[allow(clippy::too_many_arguments)]
pub fn choose(
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    sim: Res<Sim>,
    local: Res<Local>,
    badges: Res<Badges>,
    looks: Res<Looks>,
    mut alone: ResMut<ShownAlone>,
    mut showing: ResMut<Showing>,
) {
    let hqs = sim.0.hqs();
    // Any change to the selection turns the display back, even to the
    // same organization again, as does the unit's death, or its swap for
    // the building it unpacks into or the truck it packs back into.
    if let Some((r, of)) = alone.0 {
        let in_it = owning(&sim.0, of) == Some(r);
        if selected.is_changed() || chosen.is_changed() || !in_it {
            alone.0 = None;
        }
    }
    let mode = wanted(&sim.0, &hqs, local.0, &selected.0, chosen.0, alone.0);
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
                a == b
                    && was.slots == now.slots
                    && was.banners == now.banners
                    && was.stockpile == now.stockpile
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
    looks: &Looks,
    game: &GameData,
    (cache, images): (&mut ui::ImageCache, &mut Assets<Image>),
    template: &str,
) -> Option<Handle<Image>> {
    let path = (drawn.0.icon(template).map(str::to_string))
        .or_else(|| looks.technologies.get(&Id::from(template)).cloned())?;
    cache.get(game, images, &path)
}

/// The colour an owner's name is in, by how the player stands towards it.
pub(crate) fn owner_colour(sim: &World, local: Option<u32>, owner: u32) -> Color {
    match local.is_some_and(|p| sim.sides.relation(p, owner) == Relation::Neutral) {
        true => NEUTRAL,
        false => TINTS[tint(sim, local, owner)],
    }
}

/// The line saying whose a thing is.
pub(crate) struct Owner {
    pub name: String,
    /// What the name's in, by how the player stands towards the owner.
    pub colour: Color,
    /// The owner's gem, in its own colour whoever's looking, as the lobby
    /// marks it.
    pub gem: Option<[f32; 3]>,
}

/// What one of an HQ's places shows.
pub(crate) enum Holding {
    /// The organization at this index, with its banner.
    Organization(usize),
    /// The commission at `place` in `building`'s queue, by its picture.
    Queued {
        building: usize,
        place: usize,
        picture: Option<Handle<Image>>,
    },
    /// An organization on its way by air: its picture, and the aircraft
    /// bringing it, by index into the sim's deliveries.
    Flown(Option<Handle<Image>>, usize),
    /// Nothing, saying so.
    Empty(String),
}

impl Holding {
    /// What the `i`th place's card does: an organization's selects it, a
    /// commission's calls it off, and the others say what they hold.
    pub(crate) fn hook(&self, card: &mut EntityCommands, i: usize) {
        let id = ui::called(format!("object.slot.{}", i + 1));
        match self {
            &Holding::Organization(r) => {
                let of = says(Says::Organization(r));
                card.insert((Button, id, Pick(r), of, Pickable::default()))
                    .observe(pick);
                centres(card, Says::Organization(r));
            }
            &Holding::Queued {
                building, place, ..
            } => {
                let of = Says::Queued {
                    building,
                    place,
                    held: true,
                };
                card.insert((Button, id, Queued(place), says(of), Pickable::default()))
                    .observe(cancel);
            }
            &Holding::Flown(_, d) => {
                card.insert((Button, id, says(Says::Delivery(d)), Pickable::default()));
                centres(card, Says::Delivery(d));
            }
            Holding::Empty(said) => {
                card.insert((Hint(said.clone()), Pickable::default()));
            }
        }
    }
}

/// What either interface's object display reads of the match and the
/// game to draw what's shown.
pub(crate) struct Shows<'a> {
    pub sim: &'a World,
    pub local: Option<u32>,
    pub leaders: &'a Leaders,
    pub gem: &'a ui::Gem,
    pub drawn: &'a Drawn,
    pub badges: &'a Badges,
    pub detail: &'a Detail,
    pub game: &'a GameData,
    pub looks: &'a mut Looks,
    pub banners: &'a mut BannerImages,
    pub cache: &'a mut ui::ImageCache,
    pub images: &'a mut Assets<Image>,
}

/// The resources [`Shows`] borrows, for a system that lays a display out.
#[derive(SystemParam)]
pub(crate) struct ShowsFrom<'w> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    leaders: Res<'w, Leaders>,
    gem: Res<'w, ui::Gem>,
    drawn: Res<'w, Drawn>,
    badges: Res<'w, Badges>,
    loaded: Res<'w, Loaded>,
    looks: ResMut<'w, Looks>,
    banners: ResMut<'w, BannerImages>,
    cache: ResMut<'w, ui::ImageCache>,
    images: ResMut<'w, Assets<Image>>,
}

impl ShowsFrom<'_> {
    /// What a display reads to draw `detail`.
    pub(crate) fn shows<'a>(&'a mut self, detail: &'a Detail) -> Shows<'a> {
        Shows {
            sim: &self.sim.0,
            local: self.local.0,
            leaders: &self.leaders,
            gem: &self.gem,
            drawn: &self.drawn,
            badges: &self.badges,
            detail,
            game: &self.loaded.game,
            looks: &mut self.looks,
            banners: &mut self.banners,
            cache: &mut self.cache,
            images: &mut self.images,
        }
    }
}

impl Shows<'_> {
    /// The same, borrowed for a shorter while, such as to draw a card with.
    pub(crate) fn reborrow(&mut self) -> Shows<'_> {
        Shows {
            looks: &mut *self.looks,
            banners: &mut *self.banners,
            cache: &mut *self.cache,
            images: &mut *self.images,
            ..*self
        }
    }

    pub(crate) fn picture(&mut self, template: &str) -> Option<Handle<Image>> {
        let loading = (&mut *self.cache, &mut *self.images);
        picture(self.drawn, self.looks, self.game, loading, template)
    }

    /// The picture at `path` in the game's data, for one there's a path to.
    pub(crate) fn image(&mut self, path: Option<&str>) -> Option<Handle<Image>> {
        self.cache.get(self.game, self.images, path?)
    }

    pub(crate) fn organization_name(&self, r: usize) -> String {
        let looks = &self.looks;
        organization_name(self.sim, &looks.names, &looks.strings, &looks.map, r)
    }

    pub(crate) fn building_name(&self, b: usize) -> String {
        let looks = &self.looks;
        building_name(self.sim, &looks.names, &looks.strings, &looks.map, b)
    }

    /// What `template` says of itself, if anything.
    pub(crate) fn described(&self, template: &Id) -> Option<String> {
        self.looks.listings.get(template)?.description.clone()
    }

    /// The owner line of what `owner` holds, for a side in the match.
    pub(crate) fn owner(&self, owner: u32) -> Option<Owner> {
        let player = self.sim.sides.player(owner)?;
        let name = (self.leaders).owner(player, &self.sim.sides, &self.looks.map);
        Some(Owner {
            name,
            colour: owner_colour(self.sim, self.local, owner),
            // An independent side's things show no gem.
            gem: (!player.independent).then_some(player.rgb),
        })
    }

    /// The shield icon `template` gives its defence.
    pub(crate) fn defense_icon(&mut self, template: &Id) -> Option<Handle<Image>> {
        let icon = self.defense_icon_path(template);
        self.image(icon.as_deref())
    }

    /// The path of the shield icon `template` gives its defence.
    pub(crate) fn defense_icon_path(&self, template: &Id) -> Option<String> {
        (self.looks.listings.get(template)).and_then(|l| l.defense_icon.clone())
    }

    /// The icons of the first `most` of `target`'s weapons.
    pub(crate) fn weapon_icons(
        &mut self,
        target: Target,
        most: usize,
    ) -> Vec<Option<Handle<Image>>> {
        let icons = self.weapon_icon_paths(target, most);
        (icons.iter())
            .map(|icon| self.image(icon.as_deref()))
            .collect()
    }

    /// The paths of the icons of the first `most` of `target`'s weapons.
    pub(crate) fn weapon_icon_paths(&self, target: Target, most: usize) -> Vec<Option<String>> {
        (self.sim.weapons(target).iter())
            .take(most)
            .map(|w| w.shown.icon.clone())
            .collect()
    }

    /// What an HQ's place shows, holding `place`, of `building`'s queue.
    pub(crate) fn held(&mut self, place: Place, building: Option<usize>) -> Holding {
        match (place, building) {
            (Place::Organization(r), _) => Holding::Organization(r),
            (Place::Queued(n), Some(b)) => {
                let queue = &self.sim.buildings[b].queue;
                let template = queue.get(n).map(|c| c.template().to_string());
                let picture = template.and_then(|t| self.picture(&t));
                Holding::Queued {
                    building: b,
                    place: n,
                    picture,
                }
            }
            (Place::Flown(d), _) => {
                let template = self.sim.deliveries[d].template.to_string();
                let picture = self.picture(&template);
                Holding::Flown(picture, d)
            }
            _ => Holding::Empty(self.looks.said(EMPTY_SLOT)),
        }
    }

    /// Where the organization at `r`'s banner shows in a portrait of its
    /// model, as [`BannerImages::framing`] gives it.
    ///
    /// [`BannerImages::framing`]: tgrender::overlay::BannerImages::framing
    pub(crate) fn banner_framing(&mut self, r: usize) -> Option<Rect> {
        let organization = &self.sim.organizations[r];
        let faction = self.leaders.banner(organization.owner);
        let template = organization.template.as_str();
        self.banners.framing(self.game, template, faction)
    }

    /// The organization at `r`'s banner, as it flies over it: its layers,
    /// bottom first, each in its colour, with none where it has no such
    /// layer.
    pub(crate) fn banner(&mut self, r: usize) -> [(Option<Handle<Image>>, Color); 5] {
        let (sim, game, banners, images) =
            (self.sim, self.game, &mut *self.banners, &mut *self.images);
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
        let lit = |c: f32| (c * PORTRAIT_LIGHT).min(1.0);
        [
            (
                looks.plate.clone(),
                Color::srgb(lit(red), lit(green), lit(blue)),
            ),
            (icon, Color::WHITE),
            (looks.frame, Color::WHITE),
            (out_of_supply, Color::WHITE),
            (badge, Color::WHITE),
        ]
    }
}

/// The parts of an object display, each drawn its interface's own way;
/// which of them show, and in what order, is the same for both.
pub(crate) trait Parts<'a> {
    fn shows(&mut self) -> &mut Shows<'a>;
    /// The name over the owner, and where it has `properties`, its icons.
    fn title(&mut self, name: String, owner: u32, properties: bool);
    /// The portrait of `template`, over a bar filled as `fill` says, both
    /// giving `of`'s tooltip, a click on the portrait showing `alone` on
    /// its own.
    fn portrait(&mut self, template: &str, fill: Fill, of: Says, alone: Option<Target>);
    /// `target`'s weapons, each its icon with its main damage by it.
    fn attacks(&mut self, target: Target);
    /// The shield, with `target`'s defence by it, whose `template` gives
    /// the icon; only called for a target with a defence.
    fn defense(&mut self, target: Target, template: &Id);
    /// What a lone thing's template says of it.
    fn description(&mut self, says: String);
    /// What the building at `b` is commissioning.
    fn queue(&mut self, b: usize);
    /// The relay at `b`'s stockpile, under its portrait.
    fn stockpile(&mut self, b: usize);
    /// The organization at `r`'s banner, between its efficiency and morale.
    fn flag(&mut self, r: usize);
    /// An HQ's places, with what `building`'s been asked for.
    fn places(&mut self, building: Option<usize>);
    /// A card for each of the organization at `r`'s units but `captain`.
    fn units(&mut self, r: usize, captain: usize);
    /// A card for each of the organizations at `rs`.
    fn cards(&mut self, rs: &[usize]);
    fn formation_buttons(&mut self);
    /// What `status`'s thing counts for in its side's economy, and what
    /// it's doing.
    fn status(&mut self, status: Status);

    /// Draws what's shown in `mode`.
    fn draw(&mut self, mode: &Mode) {
        match mode {
            Mode::SimpleActor(actor) | Mode::Company(actor @ Actor::Unit(..)) => {
                self.simple_actor(actor)
            }
            Mode::Company(Actor::Organization(r)) => self.organization(*r),
            Mode::Company(Actor::Building(b, _)) => {
                let b = *b;
                // A Division HQ's building, its captain, shows on its own
                // from its portrait.
                let sim = self.shows().sim;
                let captain = sim.buildings[b].unpacked_from.filter(|&r| !lone(sim, r));
                self.structure(b, captain.map(|_| Target::Building(b)));
                // An HQ unpacked from an organization shows its banner.
                if let Some(r) = self.shows().sim.buildings[b].unpacked_from {
                    self.flag(r);
                }
                self.places(Some(b));
                self.status(Status::Building(b));
            }
            Mode::Group(rs) => {
                let shows = self.shows();
                let name = shows.organization_name(rs[0]);
                let owner = shows.sim.organizations[rs[0]].owner;
                self.title(name, owner, false);
                self.cards(rs);
            }
            Mode::Settlement => {}
        }
        if matches!(mode, Mode::Company(Actor::Organization(_)) | Mode::Group(_)) {
            self.formation_buttons();
        }
    }

    /// The simple actor display: a building with what it's commissioning,
    /// or a unit on its own.
    fn simple_actor(&mut self, actor: &Actor) {
        let sim = self.shows().sim;
        let (target, template, own, status) = match *actor {
            Actor::Organization(r) => {
                let captain = self.heading(r);
                let unit = sim.units[captain].template.clone();
                let own = Some(&sim.organizations[r].template);
                (Target::Unit(captain), unit, own, Status::Organization(r))
            }
            Actor::Building(b, _) => {
                self.structure(b, None);
                let template = sim.buildings[b].template.clone();
                // Selected, an HQ's building is its organization's; a
                // Division HQ's is on its own only from its portrait.
                let status = match sim.buildings[b].unpacked_from {
                    Some(r) if !lone(sim, r) => Status::Structure(b),
                    _ => Status::Building(b),
                };
                (Target::Building(b), template, None, status)
            }
            Actor::Unit(r, u) => {
                let shows = self.shows();
                let template = sim.units[u].template.clone();
                let name = shows
                    .looks
                    .names
                    .get(&template)
                    .cloned()
                    .unwrap_or_default();
                self.title(name, sim.organizations[r].owner, true);
                self.portrait(template.as_str(), Fill::Unit(u), Says::Unit(u), None);
                (Target::Unit(u), template, None, Status::Unit(u))
            }
        };
        self.attacks(target);
        if sim.defense(target).is_some() {
            self.defense(target, &template);
        }
        match *actor {
            // What it's commissioning shows where its description would.
            Actor::Building(b, ref queue) if !queue.is_empty() => self.queue(b),
            // An organization's template says more of it than its captain's.
            _ => {
                let shows = self.shows();
                let says = own
                    .into_iter()
                    .chain([&template])
                    .find_map(|t| shows.described(t));
                if let Some(says) = says {
                    self.description(says);
                }
            }
        }
        self.status(status);
    }

    /// The organization at `r`'s name and owner, and its captain's
    /// portrait over the captain's health; gives the captain.
    fn heading(&mut self, r: usize) -> usize {
        let shows = self.shows();
        let name = shows.organization_name(r);
        let owner = shows.sim.organizations[r].owner;
        let captain = captain(shows.sim, shows.looks, r);
        let template = shows.sim.units[captain].template.clone();
        // An organization's shows its captain on its own; a lone unit's is
        // already the whole selection, so clicking it does nothing.
        let alone = (!lone(shows.sim, r)).then_some(Target::Unit(captain));
        self.title(name, owner, true);
        let of = Says::Unit(captain);
        self.portrait(template.as_str(), Fill::Unit(captain), of, alone);
        captain
    }

    /// The company display for the organization at `r`: its units, or as
    /// an HQ its places, and its banner.
    fn organization(&mut self, r: usize) {
        let captain = self.heading(r);
        let shows = self.shows();
        // An HQ shows its places where an organization shows its units.
        if !lone(shows.sim, r) && shows.detail.slots.is_empty() {
            self.units(r, captain);
        }
        // A lone thing has this display only as an HQ, which still shows
        // its banner.
        self.flag(r);
        self.places(None);
        self.status(Status::Organization(r));
    }

    /// A building's name and owner, and its picture over its health and,
    /// for a relay, its stockpile; a click on the picture shows `alone`
    /// on its own.
    fn structure(&mut self, b: usize, alone: Option<Target>) {
        let shows = self.shows();
        let building = &shows.sim.buildings[b];
        let (template, owner) = (building.template.clone(), building.owner);
        let name = shows.building_name(b);
        let stockpile = shows.detail.stockpile;
        self.title(name, owner, true);
        self.portrait(
            template.as_str(),
            Fill::Building(b),
            Says::Building(b),
            alone,
        );
        if stockpile {
            self.stockpile(b);
        }
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
        Actor::Unit(r, u) => Some((sim.units[*u].template.as_str(), sim.organizations[*r].owner)),
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

/// What kind of thing a display of a thing on its own shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lone {
    Building,
    Organization,
    Unit,
}

/// The one thing a display of a thing on its own shows: its kind, and
/// its index.
type Alone = Option<(Lone, usize)>;

/// The map's sun: not the stage's own light, nor the menu backdrop's, still
/// up as the match starts.
type MapSun = (
    With<DirectionalLight>,
    Without<bevy::camera::visibility::RenderLayers>,
    Without<crate::backdrop::Backdrop>,
);

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
    suns: Query<&Transform, MapSun>,
) {
    let mode = showing.now.as_ref().map(|(mode, _)| mode);
    let wanted = mode.and_then(|mode| {
        let (template, owner) = portrayed(&sim.0, &looks, mode)?;
        // Each thing shown on its own gets a portrait afresh, where a
        // organization keeps one that's already up.
        let alone = match mode {
            Mode::SimpleActor(Actor::Building(b, _)) => Some((Lone::Building, *b)),
            Mode::SimpleActor(Actor::Organization(r)) => Some((Lone::Organization, *r)),
            Mode::SimpleActor(Actor::Unit(_, u)) => Some((Lone::Unit, *u)),
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

/// Whether the building at `b` shows a supply stockpile bar: a relay does
/// once it's done unpacking.
fn shows_stockpile(sim: &World, b: usize) -> bool {
    let relay = sim.supply_of(b).is_some_and(|s| s.relay);
    relay && sim.buildings[b].unpacking.is_none()
}

/// Hides what shows only while the thing shown is seen now, as the fog
/// covers it.
pub fn hide_remembered(
    sim: Res<Sim>,
    fog: Option<Res<crate::fog::Fog>>,
    mut parts: Query<(&WhileSeen, &mut Visibility)>,
) {
    for (of, mut shown) in &mut parts {
        shown.set_if_neq(visible(seen_now(&sim.0, fog.as_deref(), of.0)));
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
    mut markers: Query<(&Marker, &mut Node)>,
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
    for (&marker, mut node) in &mut markers {
        let at = marked(sim, marker);
        if node.left != at {
            node.left = at;
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
            // Blank for a building only remembered under the fog.
            Status::Building(b) | Status::Structure(b)
                if !seen_now(sim, fog.as_deref(), Target::Building(b)) =>
            {
                None
            }
            Status::Building(b) => hq_state(sim, b).unwrap_or_else(|| building_state(sim, b)),
            Status::Structure(b) => building_state(sim, b),
            Status::Unit(u) => unit_state(sim, u),
        };
        let words = ids.map(|i| looks.state(i)).unwrap_or_default();
        if text.0 != words {
            text.0 = words;
        }
    }
}

/// What a unit of an organization is doing on its own account, as the
/// game's actor state id: the weightiest of its states, engaged winning
/// over under siege on their tie.
fn unit_state(sim: &World, u: usize) -> Option<&'static str> {
    let unit = &sim.units[u];
    if !unit.alive() {
        return None;
    }
    let states = sim.unit_states(u);
    Some(match () {
        _ if states.contains(&UNIT_ENGAGED) => UNIT_ENGAGED,
        _ if states.contains(&"under_siege") => "under_siege",
        _ if states.contains(&"falling") => "falling",
        _ => sim.unit_activity(u),
    })
}

/// What an organization's doing, as the game's actor state ids.
fn organization_state(sim: &World, r: usize) -> Option<&'static str> {
    (!sim.destroyed(r)).then(|| doing(sim, r))
}

/// What an organization's doing, as the game's actor state id, whether or
/// not it has members standing, as an HQ unpacked into its building has
/// none.
fn doing(sim: &World, r: usize) -> &'static str {
    match sim.activity(r) {
        // Weightier than resting, lighter than anything it's doing.
        "mind_resting" if sim.lacks_supply(r) => "out_of_supply",
        "mind_resting" if sim.entrenched(r) => tgdata::ENTRENCHED,
        activity => activity,
    }
}

/// What an HQ's building is doing, where that's its organization's state:
/// all but packing and unpacking, which are the building's own, and not
/// once another side has taken the building from it.
fn hq_state(sim: &World, b: usize) -> Option<Option<&'static str>> {
    let building = &sim.buildings[b];
    let r = building.unpacked_from?;
    let moving = building.packing.is_some() || building.unpacking.is_some();
    let held = sim.organizations[r].owner == building.owner;
    (held && !moving).then(|| Some(doing(sim, r)))
}

/// What a building's doing, as the game's actor state ids. One that packs
/// up rests as a regiment does when nothing else is going on; any other is
/// just normal.
fn building_state(sim: &World, b: usize) -> Option<&'static str> {
    let building = &sim.buildings[b];
    let structure = crate::actions::structure_state(building);
    // A queue outweighs being out of supply, which outweighs resting, which
    // outweighs disabled, even while either holds the queue up.
    match structure {
        "structure_packing" | "structure_unpacking" => Some(structure),
        _ if building.besieged.is_some() => Some("under_siege"),
        _ if building.constructing => Some("construction"),
        "structure_researching" | "structure_recruiting" => Some(structure),
        _ if sim.out_of_supply(b) && building.standing() => Some("out_of_supply"),
        _ if sim.rules().packing.contains_key(&building.template) => Some("mind_resting"),
        _ if building.state == State::Disabled => Some("disabled"),
        _ => Some(structure),
    }
}

/// How full a bar filled as `fill` says is, from 0 to 1.
pub(crate) fn share(sim: &World, fill: Fill) -> f32 {
    let (now, full) = measure(sim, fill);
    match (fill, full > 0.0) {
        (_, true) => (now / full).clamp(0.0, 1.0),
        // What has no health to lose shows whole.
        (Fill::Unit(_) | Fill::Organization(_) | Fill::Building(_) | Fill::Flight, false) => 1.0,
        (
            Fill::Progress(_)
            | Fill::Stockpile(_)
            | Fill::Efficiency(_)
            | Fill::Morale(_)
            | Fill::Vacant,
            false,
        ) => 0.0,
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
        Fill::Vacant => (0.0, 1.0),
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
    /// The lines of a thing's tooltip, in the game's order: its template's
    /// name and health, then, for one the player sees now, its stockpile,
    /// efficiency and morale, experience, speed, cargo, economy and lists;
    /// then its faction, and its template's description. An organization
    /// has no health of its own to give.
    fn tip(&self, says: Says) -> Vec<String> {
        let (sim, looks) = (self.sim, self.looks);
        let name = |template: &Id| template_name(&looks.names, template.as_str());
        let row = |key: &str, value: String| self.row(key, value);
        let of = |(now, full): (f32, f32)| format!("{now:.0}/{full:.0}");
        let (template, mut lines) = match says {
            Says::Unit(u) => {
                let unit = &sim.units[u];
                let r = unit.organization;
                let mut lines = vec![name(&unit.template)];
                if let Some(plain) = unit.fighter.as_ref().map(|f| f.health) {
                    let (now, full) = measure(sim, Fill::Unit(u));
                    lines.push(self.health(now, full, plain));
                }
                // One that's dead says only what its template would.
                if !unit.alive() {
                    lines.extend(self.speed(unit.speed_max, unit.speed_max));
                    lines.extend(self.given(&unit.template));
                } else if self.fog.is_none_or(|f| f.lifted || f.shows_unit(u)) {
                    // Its regiment's, as units keep none of their own.
                    lines.extend(self.experience(r));
                    lines.extend(self.speed(sim.unit_speed(u), unit.speed_max));
                    lines.extend(self.economy(Status::Organization(r)));
                    lines.extend(self.given(&unit.template));
                    lines.extend(self.properties(Holder::Unit(u)));
                }
                if unit.alive() {
                    lines.extend(self.faction(&unit.template, sim.organizations[r].owner));
                }
                // A regiment's unit has no description; a lone one's is
                // its own.
                if !lone(sim, r) {
                    return lines;
                }
                (unit.template.clone(), lines)
            }
            Says::Building(b) => {
                let building = &sim.buildings[b];
                let remembered = self.fog.and_then(|f| f.remembered_health(b));
                let (now, full) = remembered.unwrap_or_else(|| measure(sim, Fill::Building(b)));
                let mut lines = vec![
                    name(&building.template),
                    self.health(now, full, building.fighter.health),
                ];
                let seen = self
                    .fog
                    .is_none_or(|f| f.building(b) == crate::fog::Look::Shown);
                if seen {
                    if sim.supply_of(b).is_some_and(|s| s.relay) {
                        let stockpile = of(measure(sim, Fill::Stockpile(b)));
                        lines.push(row("ui_tooltip_item_title_supply_stockpile", stockpile));
                    }
                    lines.extend(self.economy(Status::Building(b)));
                    lines.extend(self.recruits(b));
                    let upgrades = looks.upgrades.get(&building.template);
                    let upgrades = upgrades.into_iter().flatten().rev();
                    lines.extend(self.list("ui_tooltip_item_title_upgrades", upgrades));
                    lines.extend(self.researches(b));
                    lines.extend(self.given(&building.template));
                    let unpacked =
                        (sim.organizations.iter()).position(|r| r.unpacked_into == Some(b));
                    lines.extend(self.properties(Holder::Piece(
                        unpacked.map_or(Piece::Building(b), Piece::Organization),
                    )));
                }
                lines.extend(self.faction(&building.template, building.owner));
                (building.template.clone(), lines)
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
                    lines.extend(self.experience(r));
                    if organization.unpacked_into.is_none() {
                        let plain = (organization.units.iter())
                            .map(|&u| sim.units[u].speed_max)
                            .fold(f32::INFINITY, f32::min);
                        lines.extend(self.speed(sim.unhurried_speed(r), plain));
                    }
                    lines.extend(self.economy(Status::Organization(r)));
                    lines.extend(self.given(&organization.template));
                    lines.extend(self.properties(Holder::Piece(Piece::Organization(r))));
                }
                (organization.template.clone(), lines)
            }
            Says::Delivery(d) => {
                let Some(delivery) = sim.deliveries.get(d) else {
                    return Vec::new();
                };
                let mut lines = vec![name(&delivery.template)];
                let dropping = || Some(&sim.organizations[delivery.organization?].template);
                if let Some(cargo) = delivery.cargo().or_else(dropping) {
                    lines.push(row("ui_tooltip_item_title_transport_cargo", name(cargo)));
                }
                (delivery.template.clone(), lines)
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
                return lines;
            }
        };
        lines.extend(self.described(&template));
        lines
    }

    /// `figure` in the colour for how `now` stands to its `plain` value:
    /// above, below or level.
    fn modified(&self, figure: String, now: f32, plain: f32) -> String {
        let [up, down, level] = self.looks.modifiers;
        let colour = match now - plain {
            d if d > 0.001 => up,
            d if d < -0.001 => down,
            _ => level,
        };
        ui::tinted(&figure, colour)
    }

    /// The Health row: `now` over `full`, tinted by `full` against the
    /// template's `plain` most.
    fn health(&self, now: f32, full: f32, plain: f32) -> String {
        let figure = self.modified(format!("{now:.0}/{full:.0}"), full, plain);
        self.row("ui_tooltip_item_title_health", figure)
    }

    /// The Speed row of what goes at `speed` now, in the game's word for
    /// it, tinted by it against its `plain` speed; none for what can't
    /// move at all.
    fn speed(&self, speed: f32, plain: f32) -> Option<String> {
        if !(plain > 0.0 && plain.is_finite()) {
            return None;
        }
        let paces = self.looks.speeds.iter();
        let (_, word) = paces
            .into_iter()
            .find(|(r, _)| (r[0]..r[1]).contains(&speed))?;
        let word = self.modified(word.clone(), speed, plain);
        Some(self.row("ui_tooltip_item_title_speed", word))
    }

    /// The [`economy_rows`] of what `status`'s thing counts for in its
    /// side's economy.
    fn economy(&self, status: Status) -> Vec<String> {
        let Some(now) = counted(self.sim, self.local, status) else {
            return Vec::new();
        };
        // Only a building ever counts for less than it would, and a city
        // is taken to count for all it could.
        let full = match status {
            Status::Building(b) | Status::Structure(b) if self.sim.buildings[b].zone.is_some() => {
                now.clone()
            }
            Status::Building(b) | Status::Structure(b) => self.sim.building_full_ledger(b),
            Status::Organization(r) => match self.sim.organizations[r].unpacked_into {
                Some(b) => self.sim.building_full_ledger(b),
                None => now.clone(),
            },
            Status::Unit(_) => now.clone(),
        };
        let rows = economy_rows(&now, &full, &self.sim.rules().economy.resources);
        (rows.into_iter())
            .map(|(title, said)| self.row(title, said))
            .collect()
    }

    /// A list row: its title and the names of `ids`, those with names,
    /// joined by ", "; none when there are none.
    fn list<'i>(&self, key: &str, ids: impl Iterator<Item = &'i Id>) -> Option<String> {
        let names: Vec<String> = ids.filter_map(|id| self.named(id)).collect();
        (!names.is_empty()).then(|| self.row(key, names.join(", ")))
    }

    /// What the game calls a template, technology or property.
    fn named(&self, id: &Id) -> Option<String> {
        let looks = self.looks;
        (looks.names.get(id).cloned())
            .or_else(|| looks.properties.get(id).map(|l| l.name.clone()))
            .filter(|n| !n.is_empty())
    }

    /// The Recruits row of the building at `b`: the recruitable templates
    /// that need it, those its side may make by its technologies, each
    /// with the other buildings it needs.
    fn recruits(&self, b: usize) -> Option<String> {
        let (sim, looks) = (self.sim, self.looks);
        let building = &sim.buildings[b];
        let owner = building.owner;
        let needs_it = |group: &Vec<Id>| group.contains(&building.template);
        let names: Vec<String> = (looks.recruitables.iter())
            .filter_map(|template| {
                let requires = sim.rules().requires.get(template)?;
                let allowed = !sim.barred(owner, template)
                    && requires.properties.iter().all(|p| sim.holds(owner, p));
                if !requires.structures.iter().any(needs_it) || !allowed {
                    return None;
                }
                let name = self.named(template)?;
                let others: Vec<String> = (requires.structures.iter())
                    .filter(|g| !needs_it(g))
                    .filter_map(|g| self.named(g.first()?))
                    .collect();
                if others.is_empty() {
                    return Some(name);
                }
                let with = looks.said("ui_tooltip_other_requirements|s");
                Some(name + &crate::messages::filled(&with, &[others.join(", ")]))
            })
            .collect();
        (!names.is_empty()).then(|| self.row("ui_tooltip_item_title_recruits", names.join(", ")))
    }

    /// The Researches row of the building at `b`: what its research menu
    /// offers its side, done or under way included.
    fn researches(&self, b: usize) -> Option<String> {
        let sim = self.sim;
        let owner = sim.buildings[b].owner;
        let offered = (sim.research_list(b).iter())
            .zip(sim.researchables(owner, b))
            .filter(|(_, r)| *r != tgsim::Researchable::Hidden)
            .map(|(item, _)| &item.technology);
        self.list("ui_tooltip_item_title_researches", offered)
    }

    /// The Provides and Technology rows of `template`: what it gives
    /// others, and, for a building, the technologies it gives its side.
    fn given(&self, template: &Id) -> Vec<String> {
        let supports = self.looks.supports.get(template).into_iter().flatten();
        let granted = self.sim.rules().granted.get(template).into_iter().flatten();
        [
            self.list("ui_tooltip_item_title_properties_provided", supports),
            self.list("ui_tooltip_item_title_properties_technology", granted),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// The Properties row: the names of the properties `holder` holds from
    /// its side, a building or an ability, in its row of icons' order,
    /// each in its icon's tint, as is the comma after it.
    fn properties(&self, holder: Holder) -> Option<String> {
        let looks = self.looks;
        let held = holding(self.sim, looks, holder);
        let mut seen: Vec<&Id> = Vec::new();
        let mut named: Vec<(String, Color)> = Vec::new();
        for (id, tint) in held.iter().filter(|(_, tint)| *tint != Tint::Plain) {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            if let Some(look) = looks.properties.get(id) {
                named.push((look.name.clone(), looks.tint(*tint)));
            }
        }
        let last = named.len().checked_sub(1)?;
        let said: String = (named.iter().enumerate())
            .map(|(i, (name, colour))| match i == last {
                true => ui::tinted(name, *colour),
                false => ui::tinted(&format!("{name}, "), *colour),
            })
            .collect();
        Some(self.row("ui_tooltip_item_title_properties", said))
    }

    /// What `template` says of itself, if anything.
    fn described(&self, template: &Id) -> Option<String> {
        let listing = self.looks.listings.get(template)?;
        listing.description.clone().filter(|d| !d.is_empty())
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
        Status::Unit(_) => return None,
        Status::Building(b) | Status::Structure(b) => {
            let building = &sim.buildings[b];
            let ledger = match building.zone {
                Some(_) => sim.city_ledger(b),
                None => sim.building_ledger(b),
            };
            (building.owner, ledger)
        }
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

/// The tooltip's economy rows, by title, for a thing that counts for `now`
/// and would in full for `full`: Production and Upkeep, each with what it
/// would be under it where that differs.
fn economy_rows(
    now: &Ledger,
    full: &Ledger,
    resources: &[tgdata::ResourceKind],
) -> Vec<(&'static str, String)> {
    // Each resource's production less its upkeep, in resource order.
    let net = |l: &Ledger| -> Vec<f32> {
        (resources.iter())
            .map(|r| l.production.get(r.id.as_str()) - l.upkeep.get(r.id.as_str()))
            .collect()
    };
    let (now, full) = (net(now), net(full));
    let listed = |nets: &[f32], made: bool| {
        let each = resources.iter().zip(nets).filter_map(|(r, &n)| {
            let n = (n * 1000.0).round() / 1000.0;
            if n == 0.0 || (n > 0.0) != made {
                return None;
            }
            Some(match r.role == tgdata::ResourceRole::LimitedConsumed {
                true => format!("{} {}", n.abs().round(), r.name),
                false => format!("{n:+} {}", r.name),
            })
        });
        each.collect::<Vec<_>>().join(" ")
    };
    let sides = [
        (
            true,
            "ui_tooltip_item_title_production",
            "ui_tooltip_item_title_production_potential",
        ),
        (
            false,
            "ui_tooltip_item_title_upkeep",
            "ui_tooltip_item_title_upkeep_potential",
        ),
    ];
    let mut rows = Vec::new();
    for (made, title, maximum) in sides {
        let (said, most) = (listed(&now, made), listed(&full, made));
        if !said.is_empty() || said != most {
            rows.push((title, said.clone()));
        }
        if said != most {
            rows.push((maximum, most));
        }
    }
    rows
}

/// The economy line's figures for the thing a status line is of: none
/// for what [`counted`] doesn't say.
pub(crate) fn economy_line(sim: &World, local: Option<u32>, status: Status) -> Vec<(Id, String)> {
    let resources = &sim.rules().economy.resources;
    counted(sim, local, status).map_or_else(Vec::new, |l| economy(&l, resources))
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

/// The group display's cards' organizations, left to right: each that
/// joins the selection goes first, pushing the rest along.
pub(crate) fn newest_first(rs: &[usize]) -> impl Iterator<Item = usize> + '_ {
    rs.iter().rev().copied()
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
    if let Some(p) = primary_member(looks, organization) {
        return organization.units[p];
    }
    let primary = (looks.primaries.get(&organization.template)).and_then(|p| p.as_ref());
    let unit = primary.and_then(|template| {
        let is = |&&u: &&usize| sim.units[u].template == *template;
        organization.units.iter().find(is).copied()
    });
    unit.unwrap_or_else(|| leader(sim, r))
}

/// The organization at `r`'s units that have cards, all but an officer
/// `captain`, who has the portrait: grouped by their slots' property,
/// the groups in the order the layout first has them.
pub(crate) fn card_groups(sim: &World, looks: &Looks, r: usize, captain: usize) -> Vec<Vec<usize>> {
    let organization = &sim.organizations[r];
    let officer = is_officer(sim.units[captain].template.as_str());
    let kinds = looks.kinds.get(&organization.template).map(Vec::as_slice);
    grouped(&organization.units, kinds, officer.then_some(captain))
}

/// `units` but `without`, grouped by their `kinds`, given in the same
/// order; all in one group where the kinds don't account for them.
fn grouped(units: &[usize], kinds: Option<&[Id]>, without: Option<usize>) -> Vec<Vec<usize>> {
    let kinds = kinds.filter(|k| k.len() == units.len());
    let mut groups: Vec<(Option<&Id>, Vec<usize>)> = Vec::new();
    for (n, &u) in units.iter().enumerate() {
        if without == Some(u) {
            continue;
        }
        let kind = kinds.map(|k| &k[n]);
        match groups.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, group)) => group.push(u),
            None => groups.push((kind, vec![u])),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// Where each card of `groups` goes from the corner of the space they
/// share, `size` big: each group a block as many cards high as fit,
/// filled across then down, `gap` apart, and the blocks side by side
/// `separation` further apart. Blocks too wide for it all go in one,
/// wrapping across.
pub(crate) fn card_places(
    groups: &[Vec<usize>],
    card: Vec2,
    gap: Vec2,
    separation: f32,
    size: Vec2,
) -> Vec<(usize, Vec2)> {
    let step = card + gap;
    let rows = (((size.y + gap.y) / step.y).floor() as usize).max(1);
    let fits = (((size.x + gap.x) / step.x).floor() as usize).max(1);
    let blocks: Vec<usize> = groups.iter().map(|g| g.len().div_ceil(rows)).collect();
    let wide = blocks
        .iter()
        .map(|&a| a as f32 * step.x + separation)
        .sum::<f32>();
    let all;
    let (groups, blocks) = match wide - gap.x - separation <= size.x {
        true => (groups, blocks),
        false => {
            all = [groups.concat()];
            (&all[..], vec![fits])
        }
    };
    let mut left = 0.0;
    let mut places = Vec::new();
    for (group, across) in groups.iter().zip(blocks) {
        for (n, &u) in group.iter().enumerate() {
            let (column, row) = (n % across, n / across);
            places.push((
                u,
                Vec2::new(left + column as f32 * step.x, row as f32 * step.y),
            ));
        }
        left += across as f32 * step.x + separation;
    }
    places
}

/// The unit template that stands for an organization made as `component`
/// says: that of its first locked slot with one, else its primary slot's,
/// else its first slot's with one.
fn standing_for(layout: &tgdata::Layout, component: &tgdata::OrganizationComponent) -> Option<Id> {
    let filled = |i: usize| component.slots.get(i).cloned().flatten();
    let locked = (0..component.slots.len()).filter(|&i| layout.locked.get(i) == Some(&true));
    (locked.filter_map(filled).next())
        .or_else(|| layout.primary.and_then(filled))
        .or_else(|| component.slots.iter().flatten().next().cloned())
}

/// Which member, in member order, of a whole organization made as
/// `component` says holds `layout`'s primary position: the last of its
/// primary slot's, after those of the filled slots before it.
fn primary_member_of(
    layout: &tgdata::Layout,
    component: &tgdata::OrganizationComponent,
) -> Option<usize> {
    let (primary, start) = (layout.primary?, layout.formations.get(layout.default)?);
    component.slots.get(primary)?.as_ref()?;
    let count = |i: usize| start.slots.get(i).map_or(1, Vec::len);
    let through = (component.slots.iter().enumerate().take(primary + 1))
        .filter(|(_, e)| e.is_some())
        .map(|(i, _)| count(i));
    through.sum::<usize>().checked_sub(1)
}

/// Every position of `layout`, in order, with the unit template
/// `component` names for it, and which is the primary one: the last of
/// the primary slot's.
fn positions_of(
    layout: &tgdata::Layout,
    component: &tgdata::OrganizationComponent,
) -> Option<(Vec<Option<Id>>, Option<usize>)> {
    let start = layout.formations.get(layout.default)?;
    let mut positions = Vec::new();
    let mut primary = None;
    for (i, slot) in start.slots.iter().enumerate() {
        let template = component.slots.get(i).cloned().flatten();
        positions.extend(std::iter::repeat_n(template, slot.len()));
        if layout.primary == Some(i) {
            primary = positions.len().checked_sub(1);
        }
    }
    Some((positions, primary))
}

/// Which of `organization`'s members holds its primary position, where
/// its members can be told apart by position.
fn primary_member(looks: &Looks, organization: &tgsim::Organization) -> Option<usize> {
    let kinds = looks.kinds.get(&organization.template)?;
    let p = *looks.primary_members.get(&organization.template)?;
    (kinds.len() == organization.units.len()).then_some(p)
}

/// The small bars on the organization at `r`'s card in the group display,
/// in order: one for each of its first nine positions that its template
/// names a unit for, but the primary one, whose unit has the wide bar.
/// Each has the unit standing in that position, or none while it's empty.
pub(crate) fn small_bars(sim: &World, looks: &Looks, r: usize) -> Vec<Option<usize>> {
    let organization = &sim.organizations[r];
    let captain = captain(sim, looks, r);
    let Some((positions, primary)) = looks.positions.get(&organization.template) else {
        // Without its layout, a bar for each of its units but the captain,
        // where it has a primary slot.
        let primary = looks.primaries.get(&organization.template);
        let wide = primary.is_some_and(Option::is_some).then_some(captain);
        let units = organization.units.iter().copied().take(MOST_SMALL_BARS);
        return units.filter(|&u| Some(u) != wide).map(Some).collect();
    };
    // The captain has the primary position, and the wide bar, whatever
    // its template.
    let mut open = positions.clone();
    if let Some(p) = *primary {
        open[p] = None;
    }
    let units = (organization.units.iter())
        .filter(|&&u| primary.is_none() || u != captain)
        .map(|&u| (u, &sim.units[u].template));
    small(positions, &standing(&open, units), *primary)
}

/// Which of `units`, each with its template, stands in each of
/// `positions`: the first not yet placed of the position's template, then
/// any left over in the positions still empty, as one whose template has
/// since changed.
fn standing<'a>(
    positions: &[Option<Id>],
    units: impl Iterator<Item = (usize, &'a Id)>,
) -> Vec<Option<usize>> {
    let mut left: Vec<_> = units.collect();
    let mut held: Vec<Option<usize>> = (positions.iter())
        .map(|template| {
            let at = left
                .iter()
                .position(|(_, t)| Some(*t) == template.as_ref())?;
            Some(left.remove(at).0)
        })
        .collect();
    let mut left = left.into_iter();
    for (place, template) in held.iter_mut().zip(positions) {
        if place.is_none() && template.is_some() {
            *place = left.next().map(|(u, _)| u);
        }
    }
    held
}

/// Who `held` has in each of the first [`MOST_SMALL_BARS`] of
/// `positions`, but `primary` and any with no unit template.
fn small(
    positions: &[Option<Id>],
    held: &[Option<usize>],
    primary: Option<usize>,
) -> Vec<Option<usize>> {
    (positions.iter().zip(held).take(MOST_SMALL_BARS).enumerate())
        .filter(|&(i, (template, _))| template.is_some() && Some(i) != primary)
        .map(|(_, (_, &u))| u)
        .collect()
}

/// How many of an organization's positions the group display's card
/// looks at for its small bars.
const MOST_SMALL_BARS: usize = 9;

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

/// Where either interface shows the portrait: a picture filling the width
/// of a box that centres it, holding the flat picture until there's a
/// portrait drawn to show.
#[derive(Component)]
pub struct Portrait;

/// Shows the portrait, once it's drawn, in each [`Portrait`]: a face
/// filling the box whatever shape that is, a fitted model keeping its
/// shape, in a band across it.
pub fn show_portrait(
    stage: Res<Stage>,
    mut boxes: Query<(&mut ImageNode, &mut Node), With<Portrait>>,
) {
    let Some((portrait, fitted)) = stage.picture() else {
        return;
    };
    for (mut shown, mut node) in &mut boxes {
        if shown.image == portrait {
            continue;
        }
        shown.image = portrait.clone();
        shown.image_mode = bevy::ui::widget::NodeImageMode::Stretch;
        let (height, aspect) = match fitted {
            true => (Val::Auto, Some(tgrender::portrait::FIT_ASPECT)),
            false => (percent(100), None),
        };
        node.height = height;
        node.aspect_ratio = aspect;
    }
}

/// An organization's card clicked: selects it alone, in place of any building.
pub(crate) fn pick(
    activate: On<Activate>,
    cards: Query<&Pick>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut selected: ResMut<Selected>,
    mut chosen: ResMut<SelectedBuilding>,
) {
    if let (Ok(card), true) = (cards.get(activate.entity), left(&mouse)) {
        selected.0 = vec![card.0];
        chosen.0 = None;
    }
}

/// One of a building's commissions clicked: calls it off.
pub(crate) fn cancel(
    activate: On<Activate>,
    places: Query<&Queued>,
    mouse: Res<ButtonInput<MouseButton>>,
    chosen: Res<SelectedBuilding>,
    local: Res<Local>,
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
) {
    if !left(&mouse) {
        return;
    }
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
    fn a_unit_shows_on_its_own_only_while_its_organization_alone_is_selected() {
        let sim = crate::orders::testing::fresh();
        let u = sim.organizations[0].units[0];
        let shown = |selected: &[usize], alone| wanted(&sim, &[], Some(1), selected, None, alone);
        let alone = Some((0, Target::Unit(u)));
        assert_eq!(
            shown(&[0], alone),
            Some(Mode::SimpleActor(Actor::Unit(0, u)))
        );
        // Another organization's unit doesn't take its place.
        let other = sim.organizations[1].units[0];
        assert_eq!(
            shown(&[0], Some((1, Target::Unit(other)))),
            Some(Mode::Company(Actor::Organization(0)))
        );
        assert!(matches!(shown(&[0, 1], alone), Some(Mode::Group(_))));
    }

    #[test]
    fn a_division_hqs_building_shows_on_its_own_while_its_unpacked() {
        let mut sim = unpacked_hq();
        sim.organizations[0].unpacked_into = Some(0);
        assert_eq!(owning(&sim, Target::Building(0)), Some(0));
        let alone = Some((0, Target::Building(0)));
        assert!(matches!(
            wanted(&sim, &[], Some(1), &[], Some(0), alone),
            Some(Mode::SimpleActor(Actor::Building(0, _)))
        ));
        // Packed back into its truck, it's the organization's no more.
        sim.organizations[0].unpacked_into = None;
        assert_eq!(owning(&sim, Target::Building(0)), None);
    }

    #[test]
    fn centring_on_an_organization_looks_at_its_building_while_unpacked() {
        let mut sim = unpacked_hq();
        let u = sim.organizations[0].units[0];
        assert_eq!(
            centred(&sim, Says::Unit(u)),
            Some(sim.position(Target::Unit(u)))
        );
        assert_eq!(centred(&sim, Says::Organization(0)), Some(sim.centre(0)));
        sim.organizations[0].unpacked_into = Some(0);
        let building = Some(sim.position(Target::Building(0)));
        assert_eq!(centred(&sim, Says::Organization(0)), building);
        assert_eq!(centred(&sim, Says::Building(0)), building);
    }

    /// A match whose player 1 has a relay, building 0, unpacked from its
    /// organization 0.
    fn unpacked_hq() -> World {
        let mut sim = hq_by(tgsim::Rules::default());
        sim.buildings[0].unpacked_from = Some(0);
        sim
    }

    /// A match played by `rules` whose player 1 has a relay, building 0.
    fn hq_by(mut rules: tgsim::Rules) -> World {
        let supply = tgdata::Supply {
            range: 30.0,
            relay: true,
            stockpile_max: 60.0,
        };
        rules.supplies.insert(Id::from("hq"), supply);
        let mut sim = crate::orders::testing::fresh_by(rules);
        let hq = tgmap::Building {
            id: 9,
            template: "hq".into(),
            owner: 1,
            position: [20.0, 20.0],
            facing: [0.0, -1.0],
            fighter: Default::default(),
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: None,
            deploy: None,
            organization: None,
            awake: false,
        };
        sim.add_buildings(&[hq]);
        sim
    }

    #[test]
    fn a_building_that_packs_rests_and_any_other_is_normal() {
        let mut sim = hq_by(tgsim::Rules::default());
        sim.buildings[0].state = State::Standing;
        assert_eq!(building_state(&sim, 0), Some("structure_normal"));
        let truck = tgmap::Organization {
            id: 0,
            template: "truck".into(),
            owner: 0,
            facing: [1.0, 0.0],
            members: Vec::new(),
            formations: Vec::new(),
            formation: 0,
            combat: None,
            operational_range: None,
            control_range: None,
            primary: None,
            unpacks: None,
            hq: None,
            morale: 1.0,
            experience: 0.0,
            efficiency: None,
        };
        let packs = tgmap::Packs {
            truck,
            offset: [0.0, 0.0],
            facing: [1.0, 0.0],
            seconds: 7.0,
            health_min: 0.5,
        };
        let mut rules = tgsim::Rules::default();
        rules.packing.insert(Id::from("hq"), packs);
        let mut sim = hq_by(rules);
        sim.buildings[0].state = State::Standing;
        assert_eq!(building_state(&sim, 0), Some("mind_resting"));
        sim.buildings[0].state = State::Disabled;
        assert_eq!(
            building_state(&sim, 0),
            Some("mind_resting"),
            "ties, and wins"
        );
    }

    #[test]
    fn a_building_under_siege_says_so_over_going_up() {
        let mut sim = hq_by(tgsim::Rules::default());
        sim.buildings[0].state = State::Standing;
        sim.buildings[0].constructing = true;
        assert_eq!(building_state(&sim, 0), Some("construction"));
        sim.buildings[0].besieged = Some(u64::MAX);
        assert_eq!(building_state(&sim, 0), Some("under_siege"));
    }

    #[test]
    fn command_all_takes_the_hq_an_organization_is_attached_to() {
        let mut sim = crate::orders::testing::fresh();
        assert_eq!(hq_for(&sim, 0), None);
        sim.organizations[0].hq = Some(77);
        assert_eq!(hq_for(&sim, 0), Some(77));
    }

    #[test]
    fn an_hq_says_what_its_organization_is_doing_but_while_it_moves_in_or_out() {
        let mut sim = unpacked_hq();
        assert_eq!(hq_state(&sim, 0), Some(Some("mind_resting")));
        sim.buildings[0].unpacking = Some((0, 10));
        assert_eq!(hq_state(&sim, 0), None);
        sim.buildings[0].unpacking = None;
        sim.buildings[0].packing = Some((0, 10));
        assert_eq!(hq_state(&sim, 0), None);
        // Once taken, it's the building's own.
        sim.buildings[0].packing = None;
        sim.buildings[0].owner = 2;
        assert_eq!(hq_state(&sim, 0), None);
    }

    #[test]
    fn a_relay_shows_its_stockpile_once_its_unpacked() {
        let mut sim = unpacked_hq();
        assert!(shows_stockpile(&sim, 0));
        sim.buildings[0].unpacking = Some((0, 10));
        assert!(!shows_stockpile(&sim, 0));
    }

    #[test]
    fn property_icons_keep_their_order_and_the_newest_come_last() {
        let ids = |of: &[&str]| -> Vec<Id> { of.iter().map(|&p| Id::from(p)).collect() };
        // What's shown stays as it stands, what's gone leaves, the rest follow.
        let now = ids(&["forest", "guarding", "supplied"]);
        let shown = ids(&["guarding", "city", "forest"]);
        assert_eq!(
            ordered(now, &shown, |p| p),
            ids(&["guarding", "forest", "supplied"])
        );
        let many = ids(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        assert_eq!(ordered(many.clone(), &[], |p| p), many);
        let last = ordered(many.clone(), &many[7..], |p| p);
        assert_eq!(last.first(), many.last());
        // One whose tint changes stays where it was, in its new tint.
        let tinted = |of: &[(&str, Tint)]| -> Vec<(Id, Tint)> {
            of.iter().map(|&(p, t)| (Id::from(p), t)).collect()
        };
        let now = tinted(&[("tech", Tint::Side), ("guarding", Tint::Plain)]);
        let shown = tinted(&[("guarding", Tint::Plain), ("tech", Tint::Support)]);
        assert_eq!(
            ordered(now, &shown, |(p, _)| p),
            tinted(&[("guarding", Tint::Plain), ("tech", Tint::Side)])
        );
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
    fn a_tooltip_gives_the_maximum_under_what_counts_for_less() {
        use tgdata::ResourceKind as R;
        let resources = [
            R::upkeep("oil", Default::default()),
            R::limited("building_limit_consumed", "building_limit_provided"),
        ];
        let ledger = |oil: f32| {
            let mut ledger = Ledger::default();
            ledger.production.set("oil", oil);
            ledger.upkeep.set("building_limit_consumed", 1.0);
            ledger
        };
        let rows = |now: f32, full: f32| -> Vec<String> {
            let rows = economy_rows(&ledger(now), &ledger(full), &resources).into_iter();
            rows.map(|(title, said)| format!("{title}: {said}"))
                .collect()
        };
        // A relay cut off from supply earns only what its truck would.
        assert_eq!(
            rows(10.0, 20.0),
            [
                "ui_tooltip_item_title_production: +10 oil",
                "ui_tooltip_item_title_production_potential: +20 oil",
                "ui_tooltip_item_title_upkeep: 1 building_limit_consumed",
            ]
        );
        // One that earns nothing still says so, over what it would.
        assert_eq!(
            rows(0.0, 20.0)[..2],
            [
                "ui_tooltip_item_title_production: ",
                "ui_tooltip_item_title_production_potential: +20 oil",
            ]
        );
        // In full, no Maximum.
        assert_eq!(rows(20.0, 20.0).len(), 2);
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
    fn each_kind_of_card_has_a_block_of_its_own() {
        let groups = [(0..6).collect(), vec![6, 7]];
        let (card, gap) = (Vec2::new(42.0, 47.0), Vec2::splat(2.0));
        let places = card_places(&groups, card, gap, 5.0, Vec2::new(203.0, 101.0));
        let at = |u: usize| places.iter().find(|p| p.0 == u).unwrap().1;
        // Three across and two down, then the other two in a column.
        assert_eq!(at(2), Vec2::new(88.0, 0.0));
        assert_eq!(at(3), Vec2::new(0.0, 49.0));
        assert_eq!(at(6), Vec2::new(137.0, 0.0));
        assert_eq!(at(7), Vec2::new(137.0, 49.0));
    }

    #[test]
    fn too_many_cards_for_blocks_wrap_across_in_one() {
        let groups = [(0..8).collect(), vec![8, 9]];
        // Wider than the space, as one group of ten would be too.
        let (card, gap) = (Vec2::new(42.0, 47.0), Vec2::splat(2.0));
        let places = card_places(&groups, card, gap, 5.0, Vec2::new(203.0, 101.0));
        let at = |u: usize| places.iter().find(|p| p.0 == u).unwrap().1;
        assert_eq!(at(3), Vec2::new(132.0, 0.0));
        assert_eq!(at(4), Vec2::new(0.0, 49.0));
        assert!(places.iter().all(|(_, p)| p.x + card.x <= 203.0));
    }

    #[test]
    fn cards_are_grouped_by_slot_unless_the_slots_dont_fit() {
        let (front, support) = (Id::from("front"), Id::from("support"));
        let kinds = [
            front.clone(),
            support.clone(),
            front.clone(),
            Id::from("captain"),
        ];
        assert_eq!(
            grouped(&[5, 6, 7, 8], Some(&kinds), Some(8)),
            [vec![5, 7], vec![6]]
        );
        // One lost, so the slots no longer say which is which.
        assert_eq!(grouped(&[5, 6, 7], Some(&kinds), None), [vec![5, 6, 7]]);
    }

    #[test]
    fn the_primary_member_is_the_last_of_its_slot_after_the_filled_ones() {
        let positions = |n: usize| vec![[0.0, 0.0]; n];
        let layout = tgdata::Layout {
            formations: vec![tgdata::Formation {
                id: Id::from("line"),
                slots: vec![positions(3), positions(1), positions(2)],
                aggressive: false,
                effect: Default::default(),
                types: Vec::new(),
            }],
            default: 0,
            combat: None,
            primary: Some(2),
            locked: Vec::new(),
            kinds: Vec::new(),
            broken: Default::default(),
            broken_types: Vec::new(),
        };
        let component = |slots: [Option<&str>; 3]| tgdata::OrganizationComponent {
            layout: Id::from("squad"),
            slots: slots.map(|s| s.map(Id::from)).to_vec(),
        };
        let whole = component([Some("rifle"), Some("rifle"), Some("captain")]);
        assert_eq!(primary_member_of(&layout, &whole), Some(5));
        // An empty slot before it has no members.
        let short = component([Some("rifle"), None, Some("captain")]);
        assert_eq!(primary_member_of(&layout, &short), Some(4));
        let leaderless = component([Some("rifle"), Some("rifle"), None]);
        assert_eq!(primary_member_of(&layout, &leaderless), None);
    }

    #[test]
    fn a_regiment_is_stood_for_by_its_first_locked_slot_else_its_leader() {
        let layout = |locked: Vec<bool>| tgdata::Layout {
            formations: Vec::new(),
            default: 0,
            combat: None,
            primary: Some(2),
            locked,
            kinds: Vec::new(),
            broken: Default::default(),
            broken_types: Vec::new(),
        };
        let component = |slots: [Option<&str>; 3]| tgdata::OrganizationComponent {
            layout: Id::from("squad"),
            slots: slots.map(|s| s.map(Id::from)).to_vec(),
        };
        let whole = component([Some("rifle"), Some("mortar"), Some("captain")]);
        let locked = layout(vec![false, true, true]);
        assert_eq!(standing_for(&locked, &whole), Some(Id::from("mortar")));
        // A locked slot with no unit type is passed over.
        let gapped = component([Some("rifle"), None, Some("captain")]);
        assert_eq!(standing_for(&locked, &gapped), Some(Id::from("captain")));
        let unlocked = layout(vec![false; 3]);
        assert_eq!(standing_for(&unlocked, &whole), Some(Id::from("captain")));
        // Without a leader either, the first slot with a unit type.
        let leaderless = component([None, Some("mortar"), None]);
        assert_eq!(
            standing_for(&unlocked, &leaderless),
            Some(Id::from("mortar"))
        );
    }

    #[test]
    fn small_bars_are_the_first_nine_positions_but_the_primary() {
        let named = |slots: [(Option<&str>, usize); 6]| -> Vec<Option<Id>> {
            (slots.into_iter())
                .flat_map(|(t, n)| std::iter::repeat_n(t.map(Id::from), n))
                .collect()
        };
        // AA's infantry: eight soldiers, then the captain, all named.
        let infantry = named([
            (Some("rifle"), 3),
            (Some("mg"), 1),
            (Some("mortar"), 1),
            (Some("at"), 1),
            (Some("sniper"), 2),
            (Some("captain"), 1),
        ]);
        let held: Vec<_> = (10..19).map(Some).collect();
        let bars = small(&infantry, &held, Some(8));
        assert_eq!(bars, (10..18).map(Some).collect::<Vec<_>>());
        // Its recon regiment names no unit for three slots.
        let recon = named([
            (Some("rifle"), 3),
            (None, 1),
            (Some("mortar"), 1),
            (None, 1),
            (None, 2),
            (Some("captain"), 1),
        ]);
        assert_eq!(small(&recon, &held, Some(8)).len(), 4);
        // The primary one first, as a tank regiment's lead tank.
        let tanks = vec![Some(Id::from("tank")); 4];
        let held = [Some(4), Some(5), Some(6), Some(7)];
        assert_eq!(small(&tanks, &held, Some(0)), [Some(5), Some(6), Some(7)]);
        // No primary position: one each, nine at most.
        let herd = vec![Some(Id::from("cow")); 10];
        assert_eq!(small(&herd, &[None; 10], None).len(), 9);
    }

    #[test]
    fn units_stand_in_the_positions_of_their_templates() {
        let (rifle, captain) = (Id::from("rifle"), Id::from("captain"));
        let positions = [
            Some(rifle.clone()),
            Some(rifle.clone()),
            None,
            Some(captain.clone()),
        ];
        // An organization that's arrived as its captain and one rifleman.
        let units = [(7, &captain), (8, &rifle)];
        let held = standing(&positions, units.into_iter());
        assert_eq!(held, [Some(8), None, None, Some(7)]);
        // One whose template no position names takes the first empty one.
        let other = Id::from("other");
        let units = [(7, &captain), (9, &other)];
        let held = standing(&positions, units.into_iter());
        assert_eq!(held, [Some(9), None, None, Some(7)]);
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
