//! How objects stand, move and fight: their bodies, blocking shapes,
//! weapons and what those can hit.

use std::collections::HashMap;

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
    /// What its properties make it for weapons' targets to match, e.g.
    /// `infantry`.
    pub flags: Vec<Id>,
    pub weapons: Vec<Weapon>,
    /// How high above it its attacks start, where they don't say.
    pub attack_height: f32,
    /// For one that defends itself, how near something must come for it to
    /// notice it, and so ever fire on it.
    pub notice: Option<f32>,
    /// The parts that turn to aim its weapons.
    pub turrets: Vec<Turret>,
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
    pub range: f32,
    pub min_range: f32,
    /// Seconds between shots.
    pub recharge: f32,
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
    pub damage: Vec<Damage>,
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

/// Which flags a target must have, and must not, for a weapon to fire on
/// it. Besides a fighter's own [`Fighter::flags`], it has `enemy`, `friend`
/// or `neutral` by how its side stands toward the firer's.
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
            if !flag.is_empty() {
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
    pub fn bodies(&mut self) -> HashMap<Id, Body> {
        self.by_template(|t| Some(body(t)))
    }

    /// How the objects of every template that can be hurt fight, by
    /// template id, including the ids the game has since renamed.
    pub fn fighters(&mut self) -> HashMap<Id, Fighter> {
        let definitions = self.definitions();
        let flights = self.by_template(|t| flight(t.projectile_component.as_ref()?));
        let mut out = self.by_template(|t| fighter(t, &definitions.properties));
        for w in out.values_mut().flat_map(|f| &mut f.weapons) {
            w.flight = w.projectile.as_ref().and_then(|p| flights.get(p)).copied();
        }
        out
    }

    /// How far up their targets projectiles aim.
    pub fn aim_heights(&mut self) -> AimHeights {
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
    fighter.attack_height = (thing.performer_component.iter())
        .find_map(|p| p.attack_height)
        .unwrap_or(1.45);
    let turrets = thing.performer_component.iter().flat_map(|p| &p.turret);
    let name = thing.ids.as_ref().map_or("?", Id::as_str);
    let turrets: Vec<(Option<&Id>, Turret)> = turrets
        .map(|t| (t.turret_ids.as_ref(), turret(name, t)))
        .collect();
    fighter.turrets = turrets.iter().map(|(_, t)| t.clone()).collect();
    let performs = thing.performer_component.iter().flat_map(|p| &p.ability);
    let defends = thing.defender_component.iter().flat_map(|d| &d.ability);
    let abilities = performs.chain(defends);
    for (a, d) in abilities.filter_map(|a| Some((a, a.ability_definition.as_ref()?))) {
        let attack = d
            .ability_type
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("attack"));
        let damage: Vec<Damage> = d
            .effect
            .iter()
            .filter(|e| {
                e.kind
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case("MAIN_DAMAGE"))
            })
            .filter_map(|e| {
                Some(Damage {
                    kind: e.damage_type_ids.clone()?,
                    amount: e.amount?,
                })
            })
            .collect();
        let Some(range) = d.range.filter(|_| attack && !damage.is_empty()) else {
            continue;
        };
        fighter.weapons.push(Weapon {
            range,
            min_range: d.min_range,
            recharge: d.recharge_time,
            area: d.area,
            max_targets: d.max_targets.unwrap_or(0),
            full_within: d.attenuation_range_min.unwrap_or(1.0),
            edge_damage: d.attenuation_factor_max.unwrap_or(1.0),
            targets: Targets::parse(d.targets.as_deref().unwrap_or("")),
            damage,
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
        });
    }
    Some(fighter)
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
            gun.damage,
            [Damage {
                kind: Id::from("gunfire"),
                amount: 12.0
            }]
        );
        let enemy = Id::from("enemy");
        assert!(gun.targets.allow([&enemy].into_iter()));
        assert!(!gun.targets.allow([&enemy, &Id::from("AIR")].into_iter()));
        assert!(!gun.targets.allow(tank.flags.iter()));
    }
}
