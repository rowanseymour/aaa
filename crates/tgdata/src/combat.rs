//! How objects stand, move and fight: their bodies, blocking shapes,
//! weapons and what those can hit.

use std::collections::HashMap;
use std::sync::Arc;

use crate::strings::localized;
use crate::supply::{is, listed};
use crate::{Cue, GameData, Id, WeaponEffects, audio::cues, defs, effects::vfx_cues, not_null};

/// How a template's objects take part in movement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Body {
    /// The ground it keeps movers off; `None` if it doesn't block.
    pub blocking: Option<Shape>,
    /// Its blocking shape, which it takes up the ground with whether or
    /// not it blocks.
    pub shape: Shape,
    /// Its soft shape, which holds its blocking shape.
    pub soft: Shape,
    /// It floats, so it stands on water rather than land.
    pub water_based: bool,
    /// It stands on the half-tile grid.
    pub tile_locked: bool,
    /// Nothing gets in its way, so it can stand anywhere on the map.
    pub ethereal: bool,
    /// It can belong to a side.
    pub ownable: bool,
    /// It never moves: it has no mover with a top speed, and is neither an
    /// organization nor a projectile. Only such things get in the way of
    /// what's placed as a map loads or a building unpacks.
    pub fixed: bool,
    /// Top speed in world units a second; `None` if it can't move.
    pub speed: Option<f32>,
    /// World units a second it gains each second, speeding up; 0 for
    /// reaching top speed at once. It slows down at once.
    pub acceleration: f32,
    /// Radians a second it turns while moving; 0 for facing its way at
    /// once.
    pub turn_rate: f32,
    /// Where it's coupled to what tows it or what it tows: x to its right
    /// and y ahead.
    pub hitch: Option<[f32; 2]>,
    /// The template of the trailer it tows.
    pub trailer: Option<Id>,
}

/// How a template's objects fight: what they can take and what they deal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fighter {
    pub health: f32,
    pub defense: f32,
    /// Whoever brings its health to nothing can take it over.
    pub capturable: bool,
    /// It's destroyed when its health runs out; otherwise it's disabled.
    pub destructible: bool,
    /// The fraction of each damage type's damage its properties turn
    /// aside, by damage type; negative where they add to it.
    pub resists: HashMap<Id, f32>,
    /// The target types its template gives it, e.g. `infantry` from its
    /// properties or `invulnerable` from its flags.
    pub flags: Vec<Id>,
    /// The templates it can build, which aren't `cannot_build` to it.
    pub builds: Vec<Id>,
    pub weapons: Vec<Weapon>,
    /// What it sets off where it dies.
    pub on_death: Option<Weapon>,
    /// The abilities that put health back into what they're used on, such
    /// as an engineer's for raising a building or mending one.
    pub repairs: Vec<Repair>,
    /// Added to each of its weapons' scores, by how the weapon rates.
    pub bonus: Ratings,
    /// Whether it scores weapons by how little the target resists their
    /// main damage, as well as by their own [`Choice`].
    pub check_resistance: bool,
    /// How high above it its attacks start, where they don't say.
    pub attack_height: f32,
    /// For one that defends itself, how near something must come for it to
    /// notice it, and so ever fire on it.
    pub notice: Option<f32>,
    /// The parts that turn to aim its weapons.
    pub turrets: Vec<Turret>,
    /// The mana it starts with and holds at most, which its weapons'
    /// [`Weapon::mana_cost`] is paid from.
    pub mana: f32,
    /// Its soft shape: area shots reach as far as the circle round it.
    pub soft: Shape,
    /// The outline weapons' ranges run to and from.
    pub target: Shape,
    /// Whether that's a square, which gives way to a rectangle where
    /// ranges keep only one of two as it is.
    pub target_square: bool,
}

/// A part of a fighter that turns to aim its weapons, without turning the
/// fighter. Angles are radians from the fighter's facing, counter-clockwise
/// seen from above.
#[derive(Debug, Clone, PartialEq)]
pub struct Turret {
    /// Radians a second.
    pub turn_rate: f32,
    /// The whole sweep it aims within, centred on `start`; a full turn or
    /// more aims anywhere.
    pub sweep: f32,
    /// Where it points as the fighter appears, and goes back to.
    pub start: f32,
    /// Seconds without turning to anything before it goes back, if it does.
    pub reset: Option<f32>,
    /// Where it stands from its fighter: ahead, then to the left.
    pub offset: [f32; 2],
}

/// An attack a fighter makes by itself on what comes in range.
#[derive(Debug, Clone, PartialEq)]
pub struct Weapon {
    /// The id of its ability definition: the template it's made from, or
    /// else its own id.
    pub definition: Option<Id>,
    /// It's one of its fighter's `[PerformerComponent]` abilities, which a
    /// building never chooses to use itself.
    pub performed: bool,
    pub range: f32,
    pub min_range: f32,
    /// Seconds between shots.
    pub recharge: f32,
    /// The mana each shot takes from its fighter's [`Fighter::mana`].
    pub mana_cost: f32,
    /// Radius of the area a shot hits, 0 for only its target.
    pub area: f32,
    /// The most an area shot hits, the nearest where it lands; 0 for all
    /// it reaches.
    pub max_targets: u32,
    /// Within this distance of where a shot lands it does full damage...
    pub full_within: f32,
    /// ...falling to this fraction of it at the area's edge.
    pub edge_damage: f32,
    pub targets: Targets,
    /// What each use does, in the order its ability lists its effects.
    pub hits: Vec<WeaponHit>,
    /// Its area is round its user where it stands as it fires, whatever
    /// it's aimed at.
    pub centred_on_user: bool,
    /// The ids of the animation its user plays to fire it.
    pub animation: Option<String>,
    /// It's on a turret, which aims it without turning its user.
    pub turret: bool,
    /// The id of the model point of that turret.
    pub turret_point: Option<Id>,
    /// The fighter's turret that must point at a target before it fires,
    /// by index into [`Fighter::turrets`].
    pub aimed_by: Option<usize>,
    /// Where its projectiles appear from its user, in the user's model's
    /// axes, forward along -y.
    pub spawn_position: [f32; 3],
    /// Played each shot.
    pub sounds: Vec<Cue>,
    pub effects: WeaponEffects,
    /// The template of the projectile a shot fires, if it fires one.
    pub projectile: Option<Id>,
    /// How that projectile flies, if its template says.
    pub flight: Option<Flight>,
    /// An order to bombard can use it, and its hits are bombardment when
    /// it fires other than by its user's own choice of shot.
    pub bombard: bool,
    /// Its main damage ignores defense.
    pub vorpal: bool,
    /// Its main damage ignores its user's attack efficiency.
    pub spell: bool,
    pub choice: Choice,
    pub shown: WeaponShown,
}

/// What the selected thing's panel shows of a weapon besides its damage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeaponShown {
    /// Its ability's icon, as an archive path.
    pub icon: Option<String>,
    pub name: Option<String>,
    /// The game's words for how often it fires and how wide it hits.
    pub rate: Option<String>,
    pub area: Option<String>,
    /// The ability's own words, until [`GameData::fighters`] reads them.
    name_key: Option<String>,
    volley_area: f32,
}

/// An ability that adds health to what it's used on each time, up to its
/// most.
#[derive(Debug, Clone, PartialEq)]
pub struct Repair {
    pub range: f32,
    /// Seconds between uses.
    pub recharge: f32,
    /// The health each use adds, before its user's properties add to it.
    pub amount: f32,
    pub targets: Targets,
    /// The ids of the animation its user plays to use it.
    pub animation: Option<String>,
    /// Played each use.
    pub sounds: Vec<Cue>,
    /// Seconds a use takes, during which its user does nothing else.
    pub lasts: f32,
}

/// What decides whether a unit picks a weapon over its others.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub priority: f32,
    /// What the priority is multiplied by, by how the weapon rates.
    pub weight: Ratings,
    /// What's added to that.
    pub bonus: Ratings,
    /// The properties its user must all hold to have it.
    pub requires: Vec<Id>,
    /// The lowest and highest level its user's organization may be at to
    /// have it, by index into its levels.
    pub levels: [Option<usize>; 2],
    /// Seconds a use takes, its animation and reloading after, during
    /// which its user does nothing else.
    pub lasts: f32,
}

impl Default for Choice {
    fn default() -> Self {
        Choice {
            priority: 0.0,
            weight: Ratings {
                move_into_range: 1.0,
                use_now: 1.0,
                wait: 1.0,
            },
            bonus: Ratings::default(),
            requires: Vec::new(),
            levels: [None; 2],
            lasts: 0.0,
        }
    }
}

/// An amount for each way a weapon can rate against a target it can be
/// used on.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ratings {
    pub move_into_range: f32,
    pub use_now: f32,
    pub wait: f32,
}

impl Weapon {
    /// Its main damage, as it lands on what it's fired at.
    pub fn main_damage(&self) -> impl Iterator<Item = &Damage> {
        (self.hits.iter())
            .filter(|h| !h.on_user)
            .filter_map(|h| match &h.does {
                Does::Main(d) => Some(d),
                _ => None,
            })
    }
}

/// One of the effects of a weapon's use.
#[derive(Debug, Clone, PartialEq)]
pub struct WeaponHit {
    /// Which of the things the use hits it reaches.
    pub targets: Targets,
    /// It goes to the weapon's user instead, once a use, as it's used.
    pub on_user: bool,
    pub does: Does,
}

/// What an effect of a weapon's use does to what it reaches.
#[derive(Debug, Clone, PartialEq)]
pub enum Does {
    /// Damage past defence and attack modifiers.
    Main(Damage),
    /// Damage that only resistance lessens.
    Add(Damage),
    /// Morale taken from the organization of the unit it reaches.
    Morale(f32),
}

impl WeaponHit {
    /// A weapon's main damage, for whatever it's fired at.
    pub fn main(damage: Damage) -> WeaponHit {
        WeaponHit {
            targets: Targets::default(),
            on_user: false,
            does: Does::Main(damage),
        }
    }
}

/// How a projectile flies to what it's fired at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Flight {
    pub kind: FlightKind,
    /// World units a second.
    pub speed: f32,
    /// Seconds it flies, whatever the distance, if above 0.
    pub life: f32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlightKind {
    /// Straight at its target.
    #[default]
    Straight,
    /// In an arc.
    Lob,
    /// Dropped from above.
    Bomb,
}

/// How far up their targets projectiles aim, as shares of the targets'
/// heights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AimHeights {
    pub straight: f32,
    /// Lobbed and dropped.
    pub arcing: f32,
}

/// The target types a thing must have, and must not, for a filter to
/// match it. What it has besides its own [`Fighter::flags`] depends on the
/// match, as the simulation works it out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Targets {
    pub required: Vec<Id>,
    pub excluded: Vec<Id>,
}

impl Targets {
    pub(crate) fn parse(text: &str) -> Targets {
        let mut out = Targets::default();
        for word in text.split(|c: char| c == ',' || c.is_whitespace()) {
            let (list, flag) = match word.strip_prefix(['-', '!']) {
                Some(flag) => (&mut out.excluded, flag),
                None => (&mut out.required, word.strip_prefix('+').unwrap_or(word)),
            };
            // `null` is no restriction, as templates default to.
            if !flag.is_empty() && !flag.eq_ignore_ascii_case("null") {
                list.push(Id::from(flag));
            }
        }
        out
    }

    pub fn allow<'a>(&self, mut flags: impl Iterator<Item = &'a Id> + Clone) -> bool {
        self.required.iter().all(|r| flags.clone().any(|f| f == r))
            && !flags.any(|f| self.excluded.contains(f))
    }
}

/// The target types a property gives whatever holds it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropertyTypes {
    pub types: Vec<Id>,
    /// For a technology, the properties a thing of the side needs for it
    /// to be on it.
    pub requires: Vec<Id>,
}

fn property_types(p: &defs::Property) -> Option<PropertyTypes> {
    let types = listed(p.enabled_target_flags.as_deref());
    (!types.is_empty()).then(|| PropertyTypes {
        types,
        requires: listed(p.required_properties.as_deref()),
    })
}

/// `ids` by what the game looks each up by, and by every old id renamed
/// to one of them.
fn with_renames(ids: &[Id], renames: &HashMap<Id, Id>) -> Vec<Id> {
    let current: Vec<Id> = ids.iter().map(|i| crate::renamed(renames, i)).collect();
    let old = (renames.iter())
        .filter(|(_, new)| current.contains(new))
        .map(|(old, _)| old.clone());
    let mut out: Vec<Id> = current.iter().cloned().chain(old).collect();
    out.sort();
    out.dedup();
    out
}

/// The target types `files` define, in the order they're read.
fn defined_types(files: &[crate::Scoped]) -> Vec<Id> {
    (files.iter())
        .flat_map(|f| f.read::<defs::TargetType>("TargetType"))
        .filter_map(|t| t.ids)
        .collect()
}

/// Damage a shot does, of one type.
#[derive(Debug, Clone, PartialEq)]
pub struct Damage {
    /// E.g. `gunfire`.
    pub kind: Id,
    pub amount: f32,
}

/// An outline centred on an object, in world units: an object's own scale
/// doesn't change it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Shape {
    #[default]
    Point,
    Circle {
        radius: f32,
    },
    /// Width across the object's facing and height along it.
    Rect {
        width: f32,
        height: f32,
    },
}

impl Shape {
    /// The shape grown by `by` on every side; a point grows into a circle.
    pub fn padded(self, by: f32) -> Shape {
        match self {
            Shape::Point => Shape::Circle { radius: by },
            Shape::Circle { radius } => Shape::Circle {
                radius: radius + by,
            },
            Shape::Rect { width, height } => Shape::Rect {
                width: width + 2.0 * by,
                height: height + 2.0 * by,
            },
        }
    }
}

impl GameData {
    /// How the objects of every template block and move, by template id,
    /// including the ids the game has since renamed.
    pub fn bodies(&self) -> HashMap<Id, Body> {
        self.by_template(|t| Some(body(t)))
    }

    /// The target types the data defines, in the order the game loads
    /// them, those defined again included.
    pub fn target_types(&self) -> Vec<Id> {
        defined_types(&crate::scoped(&self.parse_tgis(|_| true)))
    }

    /// The target types each property that gives any gives, by property
    /// id, experience levels' included.
    pub fn property_types(&self) -> HashMap<Id, PropertyTypes> {
        let mut out = self.property_effects(property_types);
        for (id, p) in self.level_properties() {
            if let Some(types) = property_types(&p) {
                out.insert(id, types);
            }
        }
        out
    }

    /// How the objects of every template that can be hurt fight, by
    /// template id, including the ids the game has since renamed.
    pub fn fighters(&self) -> Arc<HashMap<Id, Fighter>> {
        self.kept("fighters", || self.read_fighters())
    }

    fn read_fighters(&self) -> HashMap<Id, Fighter> {
        let definitions = self.definitions();
        let flights = self.flights();
        let mut out = self.by_template(|t| fighter(t, &definitions.properties));
        // Maps may place what a builder lists under an id since renamed.
        let renames = crate::renames(&self.translations(), "Thing");
        for f in out.values_mut().filter(|f| !f.builds.is_empty()) {
            f.builds = with_renames(&f.builds, &renames);
        }
        let strings = self.strings();
        let words = |category: &str, amount: f32| {
            crate::special_ops::word_for(&strings, &definitions.descriptions, category, amount)
        };
        for w in out.values_mut().flat_map(|f| &mut f.weapons) {
            w.flight = w.projectile.as_ref().and_then(|p| flights.get(p)).copied();
            let shown = &mut w.shown;
            shown.name = localized(&strings, shown.name_key.as_deref());
            shown.rate = words("RECHARGE_RATE", w.recharge);
            let (category, radius) = match shown.volley_area > w.area {
                true => ("AREA_VOLLEY", shown.volley_area),
                false => ("AREA", w.area),
            };
            shown.area = (radius > 0.0).then(|| words(category, radius)).flatten();
        }
        out
    }

    /// How each projectile template's projectiles fly, by template id.
    pub(crate) fn flights(&self) -> Arc<HashMap<Id, Flight>> {
        self.kept("flights", || {
            self.by_template(|t| flight(t.projectile_component.as_ref()?))
        })
    }

    /// How far up their targets projectiles aim.
    pub fn aim_heights(&self) -> AimHeights {
        let svars = self.svars();
        AimHeights {
            straight: (svars.iter())
                .find_map(|v| v.projectile_target_height_ratio)
                .unwrap_or(0.65),
            arcing: (svars.iter())
                .find_map(|v| v.projectile_arcing_target_height_ratio)
                .unwrap_or(0.35),
        }
    }
}

fn body(thing: &defs::Thing) -> Body {
    let flag =
        |f: fn(&defs::Flags) -> Option<bool>| thing.flags.iter().rev().find_map(f) == Some(true);
    let shape = blocking_shape(thing);
    let mover = thing.mover_component.as_ref();
    let speed = mover.and_then(|m| m.speed_max).filter(|&s| s > 0.0);
    // Turning takes time only for turn types that name a radius to turn
    // round, at top speed.
    let turns = mover
        .and_then(|m| m.turn_type.as_deref())
        .is_some_and(|t| !t.eq_ignore_ascii_case("INSTANT"));
    let radius = mover.and_then(|m| m.turn_radius).filter(|&r| r > 0.0);
    let turn_rate = match (turns, speed, radius) {
        (true, Some(s), Some(r)) => s / r,
        _ => 0.0,
    };
    let hitch = mover.and_then(|m| m.trailer_hitch.as_ref()?.node_offset);
    Body {
        blocking: flag(|f| f.blocking).then_some(shape),
        shape,
        soft: soft_shape(thing),
        water_based: flag(|f| f.water_based),
        tile_locked: flag(|f| f.tile_locked),
        ethereal: flag(|f| f.ethereal),
        ownable: thing.flags.iter().rev().find_map(|f| f.ownable) != Some(false),
        fixed: speed.is_none()
            && thing.organization_component.is_none()
            && thing.projectile_component.is_none(),
        speed,
        acceleration: mover.and_then(|m| m.acceleration).unwrap_or(0.0).max(0.0),
        turn_rate,
        hitch: hitch.map(|[x, y, _]| [x, y]),
        trailer: mover
            .and_then(|m| m.trailer_actor.clone())
            .filter(|t| !t.as_str().eq_ignore_ascii_case("null")),
    }
}

pub(crate) fn conform(thing: &defs::Thing) -> Option<Shape> {
    let conforms = thing.flags.iter().rev().find_map(|f| f.conform_to_terrain) == Some(true);
    conforms.then(|| soft_shape(thing))
}

fn blocking_shape(thing: &defs::Thing) -> Shape {
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.blocking_shape.as_deref());
    shape(thing, kind, |s| s.blocking_radius, 0.2)
}

/// The soft shape, which where the template leaves it `null` takes the
/// blocking shape's kind, but still its own radius as a circle.
pub(crate) fn soft_shape(thing: &defs::Thing) -> Shape {
    let kind = thing
        .size
        .iter()
        .rev()
        .find_map(|s| s.soft_shape.as_deref())
        .filter(|k| !k.trim().is_empty() && !k.trim().eq_ignore_ascii_case("null"));
    let kind = kind.or_else(|| {
        thing
            .size
            .iter()
            .rev()
            .find_map(|s| s.blocking_shape.as_deref())
    });
    shape(thing, kind, |s| s.soft_radius, 0.5)
}

/// The target shape, which where the template leaves it `null` is the
/// soft shape.
fn target_shape(thing: &defs::Thing) -> Shape {
    let size = |f: fn(&defs::Size) -> Option<&str>| thing.size.iter().rev().find_map(f);
    match size(|s| s.target_shape.as_deref()).and_then(named) {
        Some(kind) => shape(thing, Some(kind), |s| s.target_radius, 0.5),
        None => soft_shape(thing),
    }
}

/// The kind of shape the target shape ends up as.
fn target_kind(thing: &defs::Thing) -> Option<&str> {
    let size = |f: fn(&defs::Size) -> Option<&str>| thing.size.iter().rev().find_map(f);
    (size(|s| s.target_shape.as_deref()).and_then(named))
        .or_else(|| size(|s| s.soft_shape.as_deref()).and_then(named))
        .or_else(|| size(|s| s.blocking_shape.as_deref()))
        .map(str::trim)
}

/// A shape's kind, unless it's left `null`.
fn named(kind: &str) -> Option<&str> {
    let kind = kind.trim();
    (!kind.is_empty() && !kind.eq_ignore_ascii_case("null")).then_some(kind)
}

/// A shape of the template's `[Size]`, which its shapes share all but the
/// radius of, `default` where it sets none.
fn shape(
    thing: &defs::Thing,
    kind: Option<&str>,
    radius: fn(&defs::Size) -> Option<f32>,
    default: f32,
) -> Shape {
    let size = |f: fn(&defs::Size) -> Option<f32>| thing.size.iter().rev().find_map(f);
    match kind.unwrap_or("CIRCLE").to_ascii_uppercase().as_str() {
        "POINT" => Shape::Point,
        "SQUARE" => {
            let side = size(|s| s.square_size).unwrap_or(4.0);
            Shape::Rect {
                width: side,
                height: side,
            }
        }
        "RECT" => Shape::Rect {
            width: size(|s| s.rect_width).unwrap_or(4.0),
            height: size(|s| s.rect_height).unwrap_or(4.0),
        },
        _ => Shape::Circle {
            radius: size(radius).unwrap_or(default),
        },
    }
}

pub(crate) fn fighter(
    thing: &defs::Thing,
    shared: &HashMap<Id, defs::Property>,
) -> Option<Fighter> {
    let body = thing.body_component.as_ref()?;
    let health = body.health.filter(|&h| h > 0.0)?;
    let mut fighter = Fighter {
        health,
        defense: body.defense.unwrap_or(0.0),
        capturable: body.captureable.unwrap_or(false),
        destructible: body.destructible.unwrap_or(true),
        notice: (thing.defender_component.as_ref())
            .map(|d| d.control_range.unwrap_or(0.0).max(0.0)),
        soft: soft_shape(thing),
        target: target_shape(thing),
        target_square: target_kind(thing).is_some_and(|k| k.eq_ignore_ascii_case("SQUARE")),
        ..Fighter::default()
    };
    let properties = thing.properties_component.iter().flat_map(|c| {
        let refs = c
            .property
            .iter()
            .filter_map(|r| shared.get(r.property_ids.as_ref()?));
        refs.chain(&c.intrinsic_property)
    });
    for p in properties {
        let flags = p.enabled_target_flags.as_deref().unwrap_or("");
        for flag in flags.split(',').map(str::trim) {
            let flag = Id::from(flag);
            if !flag.as_str().is_empty()
                && flag != Id::from("null")
                && !fighter.flags.contains(&flag)
            {
                fighter.flags.push(flag);
            }
        }
        for m in &p.modifier {
            let resist = m
                .kind
                .as_deref()
                .is_some_and(|k| k.eq_ignore_ascii_case("RESIST_DAMAGE"));
            if let (true, Some(kind), Some(amount)) = (resist, &m.damage_type, m.amount) {
                *fighter.resists.entry(kind.clone()).or_default() += amount;
            }
        }
    }
    // What it is, whatever it holds.
    let invulnerable = thing.flags.iter().rev().find_map(|f| f.invulnerable) == Some(true);
    for (is, flag) in [
        (invulnerable, "invulnerable"),
        (thing.construction_component.is_some(), "constructable"),
    ] {
        if is {
            fighter.flags.push(Id::from(flag));
        }
    }
    fighter.builds = (thing.builder_component.iter())
        .flat_map(|b| &b.build_actor)
        .filter_map(|a| a.actor_ids.clone())
        .collect();
    fighter.attack_height = (thing.performer_component.iter())
        .find_map(|p| p.attack_height)
        .unwrap_or(1.45);
    let turrets = thing.performer_component.iter().flat_map(|p| &p.turret);
    let name = thing.ids.as_ref().map_or("?", Id::as_str);
    let turrets: Vec<(Option<&Id>, Turret)> = turrets
        .map(|t| (t.turret_ids.as_ref(), turret(name, t)))
        .collect();
    fighter.turrets = turrets.iter().map(|(_, t)| t.clone()).collect();
    if let Some(p) = &thing.performer_component {
        fighter.check_resistance = p.check_damage_resistance;
        fighter.mana = p.mana_max.unwrap_or(0.0);
        fighter.bonus = Ratings {
            move_into_range: p.move_bonus.unwrap_or(0.0),
            use_now: p.use_bonus.unwrap_or(0.0),
            wait: p.wait_bonus.unwrap_or(0.0),
        };
    }
    let animations = (thing.character_component.as_ref()).map_or(&[][..], |c| &c.animation);
    let performs = thing.performer_component.iter().flat_map(|p| &p.ability);
    let defends = thing.defender_component.iter().flat_map(|d| &d.ability);
    let abilities = (performs.map(|a| (a, true))).chain(defends.map(|a| (a, false)));
    // The weapon an ability is, if it does damage and has a range.
    let weapon_of = |a: &defs::Ability, d: &defs::AbilityDefinition, range: Option<f32>| {
        let hits = weapon_hits(&d.effect);
        let damages = hits.iter().any(|h| !matches!(h.does, Does::Morale(_)));
        let range = range.filter(|_| damages)?;
        Some(Weapon {
            definition: not_null(d.template.clone().or_else(|| d.ids.clone())),
            performed: false,
            range,
            min_range: d.min_range,
            recharge: d.recharge_time,
            mana_cost: d.mana_cost,
            area: d.area,
            max_targets: d.max_targets.unwrap_or(0),
            full_within: d.attenuation_range_min.unwrap_or(1.0),
            edge_damage: d.attenuation_factor_max.unwrap_or(1.0),
            targets: Targets::parse(d.targets.as_deref().unwrap_or("")),
            hits,
            centred_on_user: d.flags.iter().any(|f| f.center_on_source),
            animation: a
                .animation1
                .clone()
                .filter(|ids| !ids.eq_ignore_ascii_case("null")),
            turret: a
                .turret_ids
                .as_deref()
                .is_some_and(|ids| !ids.eq_ignore_ascii_case("null")),
            turret_point: not_null(a.turret_ids.as_deref().map(Id::from)),
            aimed_by: not_null(a.turret_ids.as_deref().map(Id::from)).and_then(|ids| {
                let found = turrets.iter().position(|(t, _)| *t == Some(&ids));
                if found.is_none() {
                    log::warn!("{name}: no turret {ids} to aim its ability");
                }
                found
            }),
            spawn_position: a.spawn_position.unwrap_or_default(),
            sounds: cues(&d.when_do),
            effects: WeaponEffects {
                start: vfx_cues(&d.when_start),
                shot: vfx_cues(&d.when_do),
                per_target: vfx_cues(&d.when_do_per_target),
                end: vfx_cues(&d.when_end),
            },
            projectile: not_null(d.projectile_ids.clone()),
            flight: None,
            bombard: d.flags.iter().any(|f| f.bombard),
            vorpal: d.flags.iter().any(|f| f.vorpal),
            spell: d.flags.iter().any(|f| f.spell),
            choice: choice(a, animations),
            shown: WeaponShown {
                icon: d.icon.as_deref().and_then(crate::ui_path),
                name_key: d.name.clone(),
                volley_area: d.volley_area,
                ..Default::default()
            },
        })
    };
    let abilities = abilities.filter_map(|(a, p)| Some((a, a.ability_definition.as_ref()?, p)));
    for (a, d, performed) in abilities {
        let heals = d.effect.iter().filter(|e| is(&e.kind, "HEAL"));
        let heal: Option<f32> = heals.map(|e| e.amount.unwrap_or(0.0)).reduce(|a, b| a + b);
        if let (Some(amount), Some(range)) = (heal, d.range) {
            fighter.repairs.push(Repair {
                range,
                recharge: d.recharge_time,
                amount,
                targets: Targets::parse(d.targets.as_deref().unwrap_or("")),
                animation: (a.animation1.clone()).filter(|ids| !ids.eq_ignore_ascii_case("null")),
                sounds: cues(&d.when_do),
                lasts: choice(a, animations).lasts,
            });
            continue;
        }
        // Whatever kind of ability it is, if it does damage.
        if let Some(weapon) = weapon_of(a, d, d.range) {
            fighter.weapons.push(Weapon {
                performed,
                ..weapon
            });
        }
    }
    let dies_with = body.on_death_ability.as_ref();
    let dies_with = dies_with.and_then(|a| Some((a, a.ability_definition.as_ref()?)));
    // It's set off where its user is, so needs no range.
    fighter.on_death = dies_with.and_then(|(a, d)| weapon_of(a, d, Some(0.0)));
    Some(fighter)
}

/// How a unit weighs ability `a` against its others, used by something
/// animated by `animations`.
fn choice(a: &defs::Ability, animations: &[defs::Animation]) -> Choice {
    let base = Choice::default();
    let played = (a.animation1.as_deref())
        .and_then(|ids| animations.iter().find(|n| crate::supply::is(&n.ids, ids)))
        .and_then(|n| n.length)
        .unwrap_or(0.0);
    let level = |l: Option<i32>| l.and_then(|l| usize::try_from(l).ok());
    Choice {
        priority: a.priority.unwrap_or(base.priority),
        weight: Ratings {
            move_into_range: a.move_weight.unwrap_or(base.weight.move_into_range),
            use_now: a.use_weight.unwrap_or(base.weight.use_now),
            wait: a.wait_weight.unwrap_or(base.weight.wait),
        },
        bonus: Ratings {
            move_into_range: a.move_bonus.unwrap_or(0.0),
            use_now: a.use_bonus.unwrap_or(0.0),
            wait: a.wait_bonus.unwrap_or(0.0),
        },
        requires: (a.required_properties.as_deref().unwrap_or(""))
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty() && !p.eq_ignore_ascii_case("null"))
            .map(Id::from)
            .collect(),
        levels: [
            level(a.required_experience_min),
            level(a.required_experience_max),
        ],
        lasts: played + a.reload_time.unwrap_or(0.0).max(0.0),
    }
}

/// What an ability's effects do as a weapon fires it, those that damage or
/// take morale, in the order it lists them.
fn weapon_hits(effects: &[defs::Effect]) -> Vec<WeaponHit> {
    let damage = |e: &defs::Effect| {
        Some(Damage {
            kind: e.damage_type_ids.clone()?,
            amount: e.amount?,
        })
    };
    let hit = |e: &defs::Effect| {
        let does = match () {
            _ if is(&e.kind, "MAIN_DAMAGE") => Does::Main(damage(e)?),
            _ if is(&e.kind, "ADD_DAMAGE") => Does::Add(damage(e)?),
            _ if is(&e.kind, "MORALE_LOSS") => Does::Morale(e.amount?),
            _ => return None,
        };
        let targets = Targets::parse(e.targets.as_deref().unwrap_or(""));
        let on_user = targets.required.contains(&Id::from("self"));
        Some(WeaponHit {
            targets,
            on_user,
            does,
        })
    };
    effects.iter().filter_map(hit).collect()
}

/// A turret's yaw fields must each be 0 to 360 degrees; those outside are
/// taken to the nearer end.
fn turret(thing: &str, t: &defs::Turret) -> Turret {
    let degrees = |d: f32| {
        if !(0.0..=360.0).contains(&d) {
            log::warn!("{thing}: turret yaw field {d} isn't within 0 to 360 degrees");
        }
        d.clamp(0.0, 360.0).to_radians()
    };
    Turret {
        turn_rate: degrees(t.yaw_rotation_per_sec_degrees),
        sweep: degrees(t.yaw_rotation_range_degrees),
        start: degrees(t.yaw_start_degrees),
        reset: t.can_reset.then_some(t.reset_time),
        offset: t.offset.unwrap_or_default(),
    }
}

fn flight(p: &defs::ProjectileComponent) -> Option<Flight> {
    let kind = match p.projectile_type.as_deref().map(str::to_ascii_uppercase) {
        Some(k) if k == "LOB" => FlightKind::Lob,
        Some(k) if k == "BOMB" => FlightKind::Bomb,
        _ => FlightKind::Straight,
    };
    Some(Flight {
        kind,
        speed: p.projectile_speed.unwrap_or(0.0),
        life: p.projectile_life.unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_filters_require_and_exclude_flags() {
        let ids = |flags: &[&str]| flags.iter().copied().map(Id::from).collect::<Vec<_>>();
        let t = Targets::parse("+enemy, vehicle -air\t!mine,+ - é");
        assert_eq!(t.required, ids(&["enemy", "vehicle", "é"]));
        assert_eq!(t.excluded, ids(&["air", "mine"]));
        assert_eq!(Targets::parse(""), Targets::default());
    }

    #[test]
    fn vehicles_conform_by_their_soft_shape() {
        let parsed = tgi::parse(
            "[Template Vehicle]\n{\n\t[Flags]\n\tconform_to_terrain = true\n\
             \t[Size]\n\t{\n\t\tblocking_shape = CIRCLE\n\t\tblocking_radius = 0.7\n\
             \t\tsoft_shape = RECT\n\t\trect_width = 1.5\n\t\trect_height = 2.5\n\t}\n}\n\
             [Thing Template=Vehicle]\n{\n\tIDS = tank\n}\n\
             [Thing]\n{\n\tIDS = soldier\n\t[Size]\n\tsoft_shape = CIRCLE\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let conform = |i| {
            let block = parsed.root.blocks_of("Thing").nth(i).unwrap();
            conform(&templates.deserialize(block).unwrap())
        };
        let footprint = Shape::Rect {
            width: 1.5,
            height: 2.5,
        };
        assert_eq!(conform(0), Some(footprint));
        assert_eq!(conform(1), None);
    }

    #[test]
    fn target_shapes_left_null_are_the_soft_shape() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = tank\n\t[Size]\n\tblocking_shape = CIRCLE\n\
             \tsoft_radius = 2\n\ttarget_shape = null\n\ttarget_radius = 3\n}\n\
             [Thing]\n{\n\tIDS = ship\n\t[Size]\n\tblocking_shape = RECT\n\trect_width = 3\n\
             \trect_height = 5\n\ttarget_shape = CIRCLE\n\ttarget_radius = 4\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let shapes: Vec<Shape> = parsed
            .root
            .blocks_of("Thing")
            .map(|b| target_shape(&templates.deserialize(b).unwrap()))
            .collect();
        let circle = |radius| Shape::Circle { radius };
        assert_eq!(shapes, [circle(2.0), circle(4.0)]);
        let square =
            tgi::parse("[Thing]\n{\n\tIDS = hut\n\t[Size]\n\tblocking_shape = SQUARE\n}\n");
        let templates = tgi::Templates::new([&square.root]);
        let hut = square.root.blocks_of("Thing").next().unwrap();
        assert_eq!(
            target_kind(&templates.deserialize(hut).unwrap()),
            Some("SQUARE")
        );
    }

    #[test]
    fn soft_shapes_left_null_take_the_blocking_kind() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = hut\n\t[Size]\n\tblocking_shape = RECT\n\trect_width = 3\n\
             \trect_height = 5\n\tsoft_shape = null\n}\n\
             [Thing]\n{\n\tIDS = tree\n\t[Size]\n\tblocking_shape = CIRCLE\n\
             \tblocking_radius = 2\n\tsoft_shape = null\n}\n\
             [Thing]\n{\n\tIDS = rock\n\t[Size]\n\tblocking_shape = CIRCLE\n\
             \tsoft_shape = CIRCLE\n\tsoft_radius = 3\n}\n\
             [Thing]\n{\n\tIDS = flag\n\t[Size]\n\tblocking_shape = POINT\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let bodies: Vec<(Shape, Shape)> = parsed
            .root
            .blocks_of("Thing")
            .map(|b| body(&templates.deserialize(b).unwrap()))
            .map(|b| (b.shape, b.soft))
            .collect();
        let rect = Shape::Rect {
            width: 3.0,
            height: 5.0,
        };
        let circle = |radius| Shape::Circle { radius };
        assert_eq!(
            bodies,
            [
                (rect, rect),
                (circle(2.0), circle(0.5)),
                (circle(0.2), circle(3.0)),
                (Shape::Point, Shape::Point)
            ]
        );
    }

    /// The fighter `text`'s one Thing makes.
    fn read_fighter(text: &str) -> Fighter {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        fighter(&thing, &HashMap::new()).unwrap()
    }

    #[test]
    fn any_ability_that_does_damage_is_a_weapon_its_effects_in_order() {
        let charge = read_fighter(
            "[Thing]
{
    IDS = banzai
    [BodyComponent]
    health = 175
    [PerformerComponent]
    {
        [Ability]
        {
            [AbilityDefinition]
            {
                ability_type = null
                range = 0.5
                area = 1
                [Flags]
                {
                    center_on_source = true
                }
                [Effect]
                {
                    type = MAIN_DAMAGE
                    damage_type_IDS = ap
                    amount = 200
                }
                [Effect]
                {
                    type = ADD_DAMAGE
                    damage_type_IDS = explosive
                    targets = SELF
                    amount = 900
                }
                [Effect]
                {
                    type = MORALE_LOSS
                    targets = infantry
                    amount = 3
                }
            }
        }
    }
}
",
        );
        let weapon = &charge.weapons[0];
        assert!(weapon.centred_on_user);
        let damage = |kind: &str, amount| Damage {
            kind: Id::from(kind),
            amount,
        };
        let on_self = WeaponHit {
            targets: Targets::parse("SELF"),
            on_user: true,
            does: Does::Add(damage("explosive", 900.0)),
        };
        let morale = WeaponHit {
            targets: Targets::parse("infantry"),
            on_user: false,
            does: Does::Morale(3.0),
        };
        assert_eq!(
            weapon.hits,
            [WeaponHit::main(damage("ap", 200.0)), on_self, morale]
        );
        assert_eq!(weapon.main_damage().count(), 1);
    }

    #[test]
    fn fighters_are_invulnerable_and_constructable_as_their_templates_say() {
        let plain = "[Thing]\n{\n\tIDS = a\n\t[BodyComponent]\n\thealth = 1\n}\n";
        assert!(read_fighter(plain).flags.is_empty());
        let both = read_fighter(
            "[Thing]
{
    IDS = a
    [BodyComponent]
    health = 1
    [Flags]
    {
        invulnerable = true
    }
    [ConstructionComponent]
}
",
        );
        assert_eq!(
            both.flags,
            [Id::from("invulnerable"), Id::from("constructable")]
        );
    }

    #[test]
    fn fighters_build_what_their_templates_list() {
        let engineer = read_fighter(
            "[Thing]
{
    IDS = a
    [BodyComponent]
    health = 1
    [BuilderComponent]
    {
        [BuildActor]
        actor_ids = bunker

        [BuildActor]
        actor_ids = bunker_aa
    }
}
",
        );
        assert_eq!(engineer.builds, [Id::from("bunker"), Id::from("bunker_aa")]);
    }

    #[test]
    fn abilities_that_heal_are_repairs_not_weapons() {
        let engineer = read_fighter(
            "[Thing]
{
    IDS = a
    [BodyComponent]
    health = 1
    [PerformerComponent]
    {
        [Ability]
        {
            animation1 = build1
            [AbilityDefinition]
            {
                range = 1
                recharge_time = 1.5
                targets = +constructable -under_construction +friend
                [Effect]
                {
                    type = HEAL
                    amount = 5
                }
            }
        }
    }
}
",
        );
        assert!(engineer.weapons.is_empty());
        let [repair] = &engineer.repairs[..] else {
            panic!("{:?}", engineer.repairs);
        };
        assert_eq!(
            (repair.range, repair.recharge, repair.amount),
            (1.0, 1.5, 5.0)
        );
        assert_eq!(repair.animation.as_deref(), Some("build1"));
        let ids = |l: &[&str]| l.iter().map(|&t| Id::from(t)).collect::<Vec<_>>();
        assert_eq!(repair.targets.required, ids(&["constructable", "friend"]));
        assert_eq!(repair.targets.excluded, ids(&["under_construction"]));
    }

    #[test]
    fn what_a_builder_lists_counts_under_its_old_and_new_ids() {
        let renames = HashMap::from([
            (Id::from("old_bunker"), Id::from("bunker")),
            (Id::from("old_gun"), Id::from("gun")),
        ]);
        let ids = [Id::from("bunker"), Id::from("old_gun")];
        let expected = ["bunker", "gun", "old_bunker", "old_gun"].map(Id::from);
        assert_eq!(with_renames(&ids, &renames), expected);
    }

    #[test]
    fn target_types_are_read_file_by_file_in_order() {
        let file = |text: &str| (String::new(), tgi::parse(text));
        let files = [
            file("[TargetType]\n{\n\tIDS = air\n}\n[TargetType]\n{\n\tIDS = mine\n}\n"),
            file("[TargetType]\n{\n\tIDS = friend\n}\n[TargetType]\n{\n\tIDS = air\n}\n"),
        ];
        let types = defined_types(&crate::scoped(&files));
        assert_eq!(types, ["air", "mine", "friend", "air"].map(Id::from));
    }

    #[test]
    fn fighters_take_weapons_from_defenders() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\tIDS = hq\n\t[BodyComponent]\n\thealth = 3500\n\
             \t[DefenderComponent]\n\t{\n\t\tcontrol_range = 20\n\t\t[Ability]\n\t\t{\n\
             \t\t\tturret_ids = Turret\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\
             \t\t\t\tability_type = attack\n\t\t\t\trange = 20\n\t\t\t\trecharge_time = 2\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\t\t\t\tdamage_type_IDS = pierce\n\
             \t\t\t\tamount = 20\n\t\t\t}\n\t\t}\n\t}\n             \t[PerformerComponent]\n\t{\n\t\t[Turret]\n\t\t{\n\t\t\tturret_ids = Other\n\t\t}\n             \t\t[Turret]\n\t\t{\n\t\t\tturret_ids = Turret\n\t\t\tyaw_rotation_per_sec_degrees = 60\n             \t\t\tyaw_rotation_range_degrees = 70\n\t\t\tyaw_start_degrees = 400\n             \t\t\tcan_reset = false\n\t\t\toffset = 1, -2\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let hq = fighter(&thing, &HashMap::new()).unwrap();
        assert_eq!(hq.notice, Some(20.0));
        let gun = &hq.weapons[0];
        assert_eq!((gun.range, gun.recharge), (20.0, 2.0));
        assert!(gun.turret);
        assert_eq!(gun.aimed_by, Some(1));
        let full = std::f32::consts::TAU;
        let other = Turret {
            turn_rate: full,
            sweep: full,
            start: 0.0,
            reset: Some(10.0),
            offset: [0.0; 2],
        };
        let turret = Turret {
            turn_rate: 60f32.to_radians(),
            sweep: 70f32.to_radians(),
            start: full,
            reset: None,
            offset: [1.0, -2.0],
        };
        assert_eq!(hq.turrets, [other, turret]);
    }

    #[test]
    fn weapons_are_named_by_their_definitions_and_know_whose_they_are() {
        let parsed = tgi::parse(
            "[Template Shell]
{
    range = 40
    [Effect]
    {
        type = MAIN_DAMAGE
        damage_type_IDS = pierce
        amount = 10
    }
}
[Thing]
{
    IDS = bunker
    [BodyComponent]
    health = 100
    [DefenderComponent]
    {
        [Ability]
        {
            [AbilityDefinition]
            {
                IDS = gun
                range = 25
                [Effect]
                {
                    type = MAIN_DAMAGE
                    damage_type_IDS = pierce
                    amount = 5
                }
            }
        }
    }
    [PerformerComponent]
    {
        [Ability]
        {
            [AbilityDefinition Template=Shell]
            {
                range = 50
            }
        }
    }
}
",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let bunker = fighter(&thing, &HashMap::new()).unwrap();
        let named: Vec<_> = (bunker.weapons.iter())
            .map(|w| (w.definition.as_ref().map(Id::as_str), w.performed, w.range))
            .collect();
        assert_eq!(
            named,
            [(Some("Shell"), true, 50.0), (Some("gun"), false, 25.0)]
        );
    }

    #[test]
    fn weapons_take_how_their_user_chooses_them_from_their_abilities() {
        let parsed = tgi::parse(
            "[Template Ability]\n{\n\tpriority = 0\n\tmove_bonus = 0\n\tuse_bonus = 0\n}\n\
             [Template Gun inherit=Ability]\n{\n\tpriority = 1\n\twait_weight = 0\n\
             \tmove_bonus = 3\n\tuse_bonus = 50\n}\n\
             [Thing]\n{\n\tIDS = banzai\n\t[BodyComponent]\n\thealth = 100\n\
             \t[CharacterComponent]\n\t{\n\t\t[Animation]\n\t\t{\n\t\t\tids = charge\n\
             \t\t\tlength = 0.75\n\t\t}\n\t}\n\
             \t[PerformerComponent]\n\t{\n\t\tcheck_damage_resistance = true\n\t\tuse_bonus = 2\n\
             \t\t[Ability Template=Gun]\n\t\t{\n\t\t\tanimation1 = charge\n\t\t\treload_time = 0.5\n\
             \t\t\tuse_bonus = 110\n\t\t\trequired_properties = tech_napalm, veteran\n\
             \t\t\trequired_experience_min = 1\n\t\t\trequired_experience_max = -1\n\
             \t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\trange = 0.5\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\t\t\t\tdamage_type_IDS = armor_piercing\n\
             \t\t\t\tamount = 200\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let man = fighter(&thing, &HashMap::new()).unwrap();
        assert!(man.check_resistance);
        assert_eq!(man.bonus.use_now, 2.0);
        let choice = &man.weapons[0].choice;
        assert_eq!(choice.priority, 1.0);
        let weight = &choice.weight;
        assert_eq!(
            [weight.move_into_range, weight.use_now, weight.wait],
            [1.0, 1.0, 0.0]
        );
        let bonus = &choice.bonus;
        assert_eq!(
            [bonus.move_into_range, bonus.use_now, bonus.wait],
            [3.0, 110.0, 0.0]
        );
        assert_eq!(
            choice.requires,
            [Id::from("tech_napalm"), Id::from("veteran")]
        );
        assert_eq!(choice.levels, [Some(1), None]);
        assert_eq!(choice.lasts, 1.25);
    }

    #[test]
    fn fighters_take_resistance_and_flags_from_properties() {
        let parsed = tgi::parse(
            "[Template Property]\n{\n\t[list Modifier]\n}\n\
             [Template Resist]\n{\n\ttype = RESIST_DAMAGE\n}\n\
             [Template Gunfire]\n{\n\ttype = MAIN_DAMAGE\n\tdamage_type_IDS = gunfire\n}\n\
             [Property Template=Property]\n{\n\tIDS = vehicle\n\tenabled_target_flags = vehicle\n\
             \t[Modifier Template=Resist]\n\tdamage_type = gunfire\n\tamount = 0.4\n}\n\
             [Property Template=Property]\n{\n\tIDS = tank\n\tenabled_target_flags = tank, vehicle\n\
             \t[Modifier Template=Resist]\n\tdamage_type = gunfire\n\tamount = 0.2\n}\n\
             [Thing]\n{\n\tIDS = tank\n\t[BodyComponent]\n\thealth = 400\n\tdefense = 6\n\
             \t[PropertiesComponent]\n\t{\n\t\t[Property]\n\t\tproperty_IDS = vehicle\n\
             \t\t[Property]\n\t\tproperty_IDS = tank\n\t}\n\
             \t[PerformerComponent]\n\t{\n\t\t[Ability]\n\t\t{\n\t\t\tanimation1 = attack1\n\
             \t\t\tturret_ids = turret\n\t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\tability_type = attack\n\t\t\t\trange = 12\n\
             \t\t\t\trecharge_time = 3\n\t\t\t\ttargets = +enemy, -air\n\
             \t\t\t\t[Effect Template=Gunfire]\n\t\t\t\tamount = 12\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let properties = parsed
            .root
            .blocks_of("Property")
            .filter_map(|b| templates.deserialize::<defs::Property>(b).ok())
            .filter_map(|p| Some((p.ids.clone()?, p)))
            .collect();
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let tank = fighter(&thing, &properties).unwrap();
        assert_eq!((tank.health, tank.defense), (400.0, 6.0));
        assert!((tank.resists[&Id::from("gunfire")] - 0.6).abs() < 1e-6);
        assert_eq!(tank.flags, [Id::from("vehicle"), Id::from("tank")]);
        let gun = &tank.weapons[0];
        assert_eq!((gun.range, gun.recharge), (12.0, 3.0));
        assert_eq!(gun.animation.as_deref(), Some("attack1"));
        assert!(gun.turret);
        assert_eq!(
            gun.hits,
            [WeaponHit::main(Damage {
                kind: Id::from("gunfire"),
                amount: 12.0
            })]
        );
        let enemy = Id::from("enemy");
        assert!(gun.targets.allow([&enemy].into_iter()));
        assert!(!gun.targets.allow([&enemy, &Id::from("AIR")].into_iter()));
        assert!(!gun.targets.allow(tank.flags.iter()));
    }
}
