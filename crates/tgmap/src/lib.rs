//! A map as the editor and the game work with it, built from the stored form
//! that [`tgck::Map`] reads and the definitions in [`tgdata`].

mod buildings;
#[cfg(test)]
mod fixtures;
mod ground;
mod obstacle;
mod outline;
pub mod pathing;
mod tile;
mod tile_terrains;
mod units;

pub use buildings::Building;
pub use ground::{Ground, Tilt};
pub use obstacle::Obstacle;
pub use outline::Outline;
pub use pathing::{Domain, Pathing, Zone};
pub use tgck::map::Relation;

/// A map's triggers, as its file holds them.
pub mod triggers {
    pub use tgck::map::{Element, Parameter, Trigger, Triggers, Value};
}
pub use tgdata::{
    ANY_DAMAGE, Affliction, Airlift, Amounts, AnimationSet, BLEEDING, CHASING, Choice,
    CombatAbility, CombatEffect, CombatProfile, CombatValueRules, Damage, Death, DeployPoints,
    Does, ENGAGED, ENTRENCHED, EXHAUSTED, Economy, EconomyRules, Efficiency, Enchantment,
    Entrenchment, EntrenchmentRate, ExperienceGains, Fall, Fighter, Flight, FlightKind, FogSetting,
    Grant, GuardRules, Hero, Id, Kind, Level, Levels, Logistics, Lookout, MoraleEffect,
    MoraleRules, Operation, PropertyTypes, REFORMING, RESTING, ROUTING, Ratings, Repair, Requires,
    Research, ResourceKind, RoutRules, Shape, Sight, Stance, Strike, Summon, Supply, SupplyEffect,
    SupplyRules, TARGETED, Targets, TerrainEffect, TerrainRules, Turret, Weapon, WeaponHit,
};
pub use tile::Tile;
pub use tile_terrains::{TerrainIndex, TileTerrain, TileTerrains};
pub use triggers::Triggers;
pub use units::{
    Division, Formation, Member, Organization, Outcome, Packs, Player, Recruits, Team, Trailer,
    UNPACKED_FACING, Unpacks, owner, relation,
};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tgck::map::{Sun, Terrain, Thing, Value};
use tgdata::{GameData, Nation};

pub struct Map {
    /// The file name without its extension, e.g. `(2v2)Berlin`.
    pub title: String,
    /// The map's own text, such as the names it gives its cities and sides.
    pub strings: Arc<tgdata::Strings>,
    pub ground: Ground,
    pub terrain: Terrain,
    pub things: Vec<Thing>,
    /// Which organization members' records depend on.
    pub world_version: u32,
    pub sun: Sun,
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
    pub organizations: Vec<Organization>,
    pub buildings: Vec<Building>,
    /// The settlements that came with the centres put in place of
    /// placeholders. Being invisible, they aren't among [`Map::things`].
    pub settlements: Vec<Settlement>,
    /// The obstacles that aren't buildings, such as trees and rocks.
    pub scenery: Vec<Obstacle>,
    /// The scenery a building going up in play clears from under it, by
    /// id, each with its blocking shape though it blocks nothing.
    pub clearable: Vec<(u32, Obstacle)>,
    pub pathing: Pathing,
    /// The terrain under each tile, for what it does to those on it.
    pub tile_terrains: TileTerrains,
    /// The ids of the things among [`Map::things`] with no room where the
    /// map has them, for the ground under them or something stored before
    /// them, which the game leaves out and the editor keeps. An
    /// organization with a member like that is here whole, members and all.
    pub crowded: HashSet<u32>,
    /// What the map holds that the game can't place, as it loads.
    pub warnings: Vec<String>,
    /// The map's scripting, where it could be read.
    pub triggers: Option<Triggers>,
}

impl Map {
    /// Loads the map at an archive path, as [`GameData::find_map`] gives it,
    /// with `recruits` from the same game data.
    pub fn load(game: &GameData, recruits: &Recruits, name: &str) -> Result<Map, String> {
        Map::load_as(game, recruits, name, &HashMap::new())
    }

    /// [`Map::load`]s a map with sides playing as `nations`, by player id,
    /// each putting its own templates in place of the placeholders it starts
    /// with, such as its Corps HQ.
    pub fn load_as(
        game: &GameData,
        recruits: &Recruits,
        name: &str,
        nations: &HashMap<u32, Nation>,
    ) -> Result<Map, String> {
        let mut map = game.open_map(name)?;
        let err = |e: tgck::Error| format!("{name}: {e}");
        let hm = map.height_map().map_err(err)?;
        let mountain = map.mountain_height_map().map_err(err)?;
        let terrain = map.terrain().map_err(err)?;
        let variations = game.terrain_variations();
        let ground = Ground::new(&hm, &mountain, &terrain, &variations)
            .map_err(|e| format!("{name}: {e}"))?;
        let mut things = map.things().map_err(err)?;
        // Past every id the map has, kept or not, since triggers name things
        // by id.
        let next = next_id(&things);
        let (mut players, teams) = read_sides(game, &mut map).map_err(err)?;
        let independent: HashSet<u32> = (players.iter())
            .filter(|p| p.independent)
            .map(|p| p.id)
            .collect();
        // The centres that bring their settlements, by id.
        let mut centres = Vec::new();
        for thing in &mut things {
            let owner = owner(thing).filter(|o| !independent.contains(o));
            let nation = owner.and_then(|o| nations.get(&o));
            let template = Id::from(thing.template.as_str());
            if let Some(t) = nation.and_then(|n| n.placeholders.get(&template)) {
                thing.template = t.to_string();
                if let Some(settlement) = recruits.settlements.get(t) {
                    centres.push((thing.id, settlement.clone()));
                }
            }
        }
        let world_version = map.world_version().map_err(err)?;
        let mut warnings = Vec::new();
        // Without the triggers there's no telling what they need kept, so
        // nothing is cleared.
        let triggers = match map.triggers() {
            Ok(triggers) => Some(triggers),
            Err(e) => {
                warnings.push(format!("triggers: {e}"));
                None
            }
        };
        let named = triggers.as_ref().map(Named::new);
        let bodies = &recruits.bodies;
        let defined = |t: &Thing| bodies.contains_key(&Id::from(t.template.as_str()));
        retain(&mut things, |t| {
            let keep = defined(t);
            if !keep {
                warnings.push(format!("{} {}: no such template", t.id, t.template));
            }
            keep
        });
        for thing in &mut things {
            snap(thing, bodies);
            for member in members_mut(thing) {
                snap(member, bodies);
            }
        }
        let mut pathing = Pathing::new(&terrain, &variations);
        pathing.set_heights(|at| ground.height_at(at));
        let tile_terrains = TileTerrains::new(&terrain, &variations);
        let Placed {
            off_map,
            crowded,
            warnings: unplaced,
        } = placed(&things, bodies, &pathing);
        retain(&mut things, |t| !off_map.contains(&t.id));
        warnings.extend(unplaced);
        let standing = without(&things, &crowded);
        let obstacles = obstacles(&standing, bodies);
        let clearable = named
            .map(|named| clearable(&standing, bodies, &named))
            .unwrap_or_default();
        for (_, o) in &obstacles {
            pathing.block(o);
        }
        let fighters = &recruits.fighters;
        for player in &mut players {
            if let Some(n) = nations.get(&player.id) {
                player.nation = Some(n.id.to_string());
            }
        }
        let buildings = buildings::buildings(
            &standing,
            bodies,
            fighters,
            &game.city_zones(),
            &obstacles.iter().cloned().collect(),
            &recruits.deploy_points,
            &recruits.ranges,
        );
        let settlements = founded(&centres, &buildings, next);
        let ids: HashSet<u32> = buildings.iter().map(|b| b.id).collect();
        let scenery = obstacles
            .into_iter()
            .filter(|(id, _)| !ids.contains(id))
            .map(|(_, o)| o)
            .collect();
        let mut organizations = units::organizations(
            &standing,
            bodies,
            fighters,
            &recruits.layouts,
            &recruits.ranges,
        );
        for r in &mut organizations {
            recruits.give_unpacks(r);
        }
        // A map with no string table names nothing of its own.
        let strings = match map.string_table() {
            Ok(table) => game.map_strings(&table),
            Err(e) => {
                warnings.push(format!("strings: {e}"));
                Default::default()
            }
        };
        Ok(Map {
            title: tgdata::map_title(name).to_string(),
            strings: Arc::new(strings),
            ground,
            organizations,
            buildings,
            settlements,
            scenery,
            clearable,
            players,
            teams,
            pathing,
            tile_terrains,
            terrain,
            things,
            world_version,
            sun: map.sun().map_err(err)?,
            crowded,
            warnings,
            triggers,
        })
    }
}

impl Map {
    /// The id after every one the map has given out, for what's added to
    /// it.
    pub fn next_id(&self) -> u32 {
        let settlements = self.settlements.iter().map(|s| s.id + 1);
        next_id(&self.things).max(settlements.max().unwrap_or(1))
    }

    /// The objects standing on the map, organizations' members among them:
    /// those of [`Map::things`] that aren't [`Map::crowded`], whose ids
    /// those of organizations' lost members may repeat.
    pub fn standing(&self) -> impl Iterator<Item = &Thing> {
        buildings::standing(&self.things)
            .map(|(t, _)| t)
            .filter(|t| !self.crowded.contains(&t.id))
    }

    /// The actors the map gives a name, with their ids, in its order.
    pub fn named(&self) -> impl Iterator<Item = (&str, u32)> {
        (self.things.iter())
            .filter(|t| t.class == tgck::map::ACTOR && !t.name.is_empty())
            .map(|t| (t.name.as_str(), t.id))
    }

    /// Each of [`Map::things`] with what it is in a match.
    pub fn roles(&self) -> impl Iterator<Item = (&Thing, Role)> {
        roles(
            &self.things,
            &self.organizations,
            &self.buildings,
            &self.crowded,
        )
    }

    /// Regroups the map's sides into `teams`, each a list of player ids, for
    /// a match: sides on a team are allies, at war with the other teams, and
    /// sides on none leave the match with everything they own. Independent
    /// players keep the stance they had toward each side.
    pub fn arrange(&mut self, teams: &[Vec<u32>]) {
        let kept = |id: u32| teams.iter().any(|t| t.contains(&id));
        let leaving: HashSet<u32> = sides(&self.players)
            .map(|p| p.id)
            .filter(|&id| !kept(id))
            .collect();
        let (players, teams) = arranged(&self.players, &self.teams, teams);
        self.players = players;
        self.teams = teams;
        let owned_by_leaver = |t: &Thing| owner(t).is_some_and(|o| leaving.contains(&o));
        self.things.retain(|t| !owned_by_leaver(t));
        self.organizations.retain(|r| !leaving.contains(&r.owner));
        self.settlements.retain(|s| !leaving.contains(&s.owner));
        // A building whose own owner stays still goes with an organization
        // it stands in that leaves.
        let standing: HashSet<u32> = buildings::standing(&self.things)
            .map(|(t, _)| t.id)
            .collect();
        take_buildings(&mut self.buildings, &mut self.pathing, |b| {
            leaving.contains(&b.owner) || !standing.contains(&b.id)
        });
    }
}

/// A settlement, grouping its centre building with the rest of its town.
#[derive(Debug, Clone, PartialEq)]
pub struct Settlement {
    pub id: u32,
    pub template: Id,
    pub owner: u32,
    /// Its centre's id, among [`Map::buildings`].
    pub centre: u32,
}

/// The settlements of `centres`, each a centre's id with its settlement's
/// template, numbered from `next`. A centre that wasn't placed brings none.
fn founded(centres: &[(u32, Id)], buildings: &[Building], next: u32) -> Vec<Settlement> {
    let placed: HashMap<u32, u32> = buildings.iter().map(|b| (b.id, b.owner)).collect();
    (centres.iter())
        .filter_map(|(centre, template)| Some((*centre, template, *placed.get(centre)?)))
        .zip(next..)
        .map(|((centre, template, owner), id)| Settlement {
            id,
            template: template.clone(),
            owner,
            centre,
        })
        .collect()
}

/// The id after the highest of `things` and their organizations' members.
fn next_id(things: &[Thing]) -> u32 {
    let members = things
        .iter()
        .flat_map(|t| t.organization())
        .flat_map(|o| &o.members);
    let ids = things.iter().chain(members).map(|t| t.id);
    ids.max().map_or(1, |id| id + 1)
}

/// [`Map::roles`] of `things`, from what the map made of them.
fn roles<'a>(
    things: &'a [Thing],
    organizations: &[Organization],
    buildings: &[Building],
    crowded: &'a HashSet<u32>,
) -> impl Iterator<Item = (&'a Thing, Role)> {
    let organization_ids: HashSet<u32> = organizations.iter().map(|r| r.id).collect();
    let building_ids: HashSet<u32> = buildings.iter().map(|b| b.id).collect();
    let trailers: HashSet<u32> = (organizations.iter().flat_map(|r| &r.members))
        .filter_map(|m| Some(m.trailer.as_ref()?.placed?.0))
        .collect();
    things.iter().map(move |t| {
        let held = t.organization().map_or(&[][..], |o| &o.members[..]);
        let role = match t.id {
            id if crowded.contains(&id) => Role::Crowded,
            id if organization_ids.contains(&id) => Role::Organization,
            id if building_ids.contains(&id) => Role::Building,
            id if trailers.contains(&id) => Role::Trailer,
            _ if held.iter().any(|m| building_ids.contains(&m.id)) => Role::HoldsBuilding,
            _ => Role::Other,
        };
        (t, role)
    })
}

/// What one of [`Map::things`] is in a match, as [`Map::roles`] finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Without room where the map has it: see [`Map::crowded`].
    Crowded,
    /// One of [`Map::organizations`].
    Organization,
    /// One of [`Map::buildings`].
    Building,
    /// What an organization's member tows, which goes with it.
    Trailer,
    /// An organization that one of [`Map::buildings`] stands in, as an
    /// unpacked Division HQ's does.
    HoldsBuilding,
    /// Anything else, such as scenery and where sides start.
    Other,
}

/// Takes the buildings that `gone` picks off the map, giving back the
/// ground they kept movers off.
fn take_buildings(
    buildings: &mut Vec<Building>,
    pathing: &mut Pathing,
    gone: impl Fn(&Building) -> bool,
) {
    buildings.retain(|b| {
        if !gone(b) {
            return true;
        }
        if let Some(o) = &b.obstacle {
            pathing.unblock(o);
        }
        false
    });
}

/// Moves a thing whose template keeps it to the half-tile grid onto it.
fn snap(thing: &mut Thing, bodies: &HashMap<Id, tgdata::Body>) {
    if bodies
        .get(&Id::from(thing.template.as_str()))
        .is_some_and(|b| b.tile_locked)
    {
        let step = pathing::CELL_SIZE / 2.0;
        for c in &mut thing.position[..2] {
            *c = (*c / step).round() * step;
        }
    }
}

/// The members of a thing that's an organization.
fn members_mut(thing: &mut Thing) -> &mut [Thing] {
    organization_members(thing).map_or(&mut [], |m| &mut m[..])
}

fn organization_members(thing: &mut Thing) -> Option<&mut Vec<Thing>> {
    use tgck::map::{Component, State};
    let actor = thing.actor_mut()?;
    let State::Decoded { components, .. } = &mut actor.state else {
        return None;
    };
    components.iter_mut().find_map(|c| match c {
        Component::Organization(o) => Some(&mut o.members),
        _ => None,
    })
}

/// Keeps the things, organizations' members among them, that `keep` picks.
fn retain(things: &mut Vec<Thing>, mut keep: impl FnMut(&Thing) -> bool) {
    things.retain(|t| keep(t));
    for t in things {
        if let Some(members) = organization_members(t) {
            members.retain(|m| keep(m));
        }
    }
}

/// The things, organizations' members among them, without those `gone`.
fn without(things: &[Thing], gone: &HashSet<u32>) -> Vec<Thing> {
    let mut out = things.to_vec();
    retain(&mut out, |t| !gone.contains(&t.id));
    out
}

/// What [`placed`] found.
#[derive(Debug, Default)]
struct Placed {
    /// The ids of the things off the map.
    off_map: HashSet<u32>,
    /// The ids of the things without room otherwise.
    crowded: HashSet<u32>,
    /// Why each of those went, in the order stored.
    warnings: Vec<String>,
}

/// Places the things in the order stored, each with its plain blocking
/// shape, to find those off the map and those without room otherwise: on
/// ground they can't stand on, or overlapping a static blocking thing
/// placed before them. Things that don't block, and ethereal ones, need
/// only be on the map. An organization's members are placed with it, and
/// one without room takes the organization and all its members with it.
fn placed(things: &[Thing], bodies: &HashMap<Id, tgdata::Body>, pathing: &Pathing) -> Placed {
    enum Fit {
        Room,
        OffMap,
        Crowded,
    }
    let mut out = Placed::default();
    type Near = HashMap<Tile, Vec<usize>>;
    let mut fixed: Vec<Obstacle> = Vec::new();
    // The static blocking things by the tiles their boxes span, to test
    // only those nearby.
    let mut near: Near = HashMap::new();
    // The tiles the group being placed has added to `near`, to take back
    // if the group goes.
    let mut added: Vec<Tile> = Vec::new();
    let place = |t: &Thing, fixed: &mut Vec<Obstacle>, near: &mut Near, added: &mut Vec<_>| {
        let Some(body) = bodies.get(&Id::from(t.template.as_str())) else {
            return Fit::Room;
        };
        let [x, y, _] = t.position;
        let o = Obstacle {
            position: [x, y],
            facing: units::facing(t),
            shape: body.shape,
            soft: body.soft,
        };
        if !pathing.on_map(&o) {
            return Fit::OffMap;
        }
        if body.blocking.is_none() || body.ethereal {
            return Fit::Room;
        }
        let domain = match body.water_based {
            true => Domain::Naval,
            false => Domain::Land,
        };
        let tiles: Vec<Tile> = pathing::spanned(o.bounds(o.shape)).collect();
        let mut others: Vec<usize> = (tiles.iter())
            .flat_map(|tile| near.get(tile).into_iter().flatten().copied())
            .collect();
        others.sort_unstable();
        others.dedup();
        if !pathing.ground_for(&o, domain) || others.iter().any(|&i| fixed[i].overlaps(&o)) {
            return Fit::Crowded;
        }
        if body.fixed {
            for tile in tiles {
                near.entry(tile).or_default().push(fixed.len());
                added.push(tile);
            }
            fixed.push(o);
        }
        Fit::Room
    };
    for t in things {
        let members = t.organization().map_or(&[][..], |o| &o.members[..]);
        let group = std::iter::once(t).chain(members);
        let before = fixed.len();
        added.clear();
        // The editor keeps a member that only lacks room and goes on, so
        // one further on off the map still drops the lot.
        let mut first_crowded = None;
        let mut off_map = None;
        for m in group.clone() {
            match place(m, &mut fixed, &mut near, &mut added) {
                Fit::Room => {}
                Fit::Crowded => {
                    first_crowded.get_or_insert(m);
                }
                Fit::OffMap => {
                    off_map = Some(m);
                    break;
                }
            }
        }
        let Some(cause) = off_map.or(first_crowded) else {
            continue;
        };
        // Nothing of a group that's gone stands in the way of what follows.
        fixed.truncate(before);
        for tile in added.drain(..).rev() {
            near.get_mut(&tile).and_then(Vec::pop);
        }
        let (why, set) = match off_map.is_some() {
            true => ("off the map", &mut out.off_map),
            false => ("no room", &mut out.crowded),
        };
        out.warnings.push(match cause.id == t.id {
            true => format!("{} {}: {why}", t.id, t.template),
            false => format!(
                "{} {}: {why} for member {} {}",
                t.id, t.template, cause.id, cause.template
            ),
        });
        set.extend(group.map(|m| m.id));
    }
    out
}

/// What blocks the ground, by id, of the objects standing on the map:
/// those that never move.
fn obstacles(things: &[Thing], bodies: &HashMap<Id, tgdata::Body>) -> Vec<(u32, Obstacle)> {
    buildings::standing(things)
        .filter_map(|(t, _)| {
            let body = bodies.get(&Id::from(t.template.as_str()))?;
            let shape = body.blocking.filter(|_| body.fixed)?;
            Some((t.id, obstacle_of(t, shape, body.soft)))
        })
        .collect()
}

/// The things a map's triggers name, as they test or act on them.
#[derive(Debug, Default)]
struct Named {
    ids: HashSet<u32>,
    names: HashSet<String>,
}

impl Named {
    fn new(triggers: &Triggers) -> Named {
        let mut named = Named::default();
        let elements = triggers
            .triggers
            .iter()
            .flat_map(|t| t.events.iter().chain(&t.conditions).chain(&t.effects));
        for p in elements.flat_map(|e| &e.parameters) {
            // Any source about actors may point at one, by id or by name;
            // keeping too much costs only scenery left standing.
            if !p.source.contains("actor") {
                continue;
            }
            match &p.value {
                Value::Int(id) => {
                    named.ids.insert(*id);
                }
                Value::String(name) if !name.is_empty() => {
                    named.names.insert(name.clone());
                }
                _ => {}
            }
        }
        named
    }

    fn has(&self, thing: &Thing) -> bool {
        self.ids.contains(&thing.id) || (!thing.name.is_empty() && self.names.contains(&thing.name))
    }
}

/// The [`Map::clearable`] scenery among the objects standing on the map.
fn clearable(
    things: &[Thing],
    bodies: &HashMap<Id, tgdata::Body>,
    named: &Named,
) -> Vec<(u32, Obstacle)> {
    let scenery = [tgck::map::SCENERY, tgck::map::REACTIVE_SCENERY];
    buildings::standing(things)
        .filter(|(t, _)| scenery.contains(&t.class) && !named.has(t))
        .filter_map(|(t, _)| {
            let body = bodies.get(&Id::from(t.template.as_str()))?;
            if body.blocking.is_some() {
                return None;
            }
            Some((t.id, obstacle_of(t, body.shape, body.soft)))
        })
        .collect()
}

/// `thing` as an obstacle of `shape`, where it stands and facing its way.
fn obstacle_of(thing: &Thing, shape: Shape, soft: Shape) -> Obstacle {
    let [x, y, _] = thing.position;
    Obstacle {
        position: [x, y],
        facing: units::facing(thing),
        shape,
        soft,
    }
}

/// The players and teams of [`Map::arrange`].
fn arranged(
    players: &[Player],
    old_teams: &[Team],
    teams: &[Vec<u32>],
) -> (Vec<Player>, Vec<Team>) {
    let team_of = |id: u32| teams.iter().position(|t| t.contains(&id));
    let before = |from: u32, to: u32| relation(players, old_teams, from, to);
    let kept: Vec<&Player> = players
        .iter()
        .filter(|p| p.independent || team_of(p.id).is_some())
        .collect();
    let between = |a: Option<usize>, b: Option<usize>| match a == b {
        true => Relation::Ally,
        false => Relation::Enemy,
    };
    // How a side or a team stands toward a player, the independent keeping
    // the stance taken toward the first of a team's sides.
    let toward = |from: u32, side: Option<usize>, to: &Player| match (side, team_of(to.id)) {
        (Some(_), Some(_)) if from == to.id => Relation::Itself,
        (Some(a), Some(b)) => between(Some(a), Some(b)),
        _ => before(from, to.id),
    };
    let new_players = kept
        .iter()
        .map(|p| {
            let team = team_of(p.id);
            let team_relations = teams
                .iter()
                .enumerate()
                .map(|(t, members)| match team {
                    Some(own) => between(Some(own), Some(t)),
                    None => members
                        .first()
                        .map_or(Relation::Neutral, |&m| before(p.id, m)),
                })
                .collect();
            Player {
                team,
                relations: kept.iter().map(|q| toward(p.id, team, q)).collect(),
                team_relations,
                ..(*p).clone()
            }
        })
        .collect();
    let new_teams = teams
        .iter()
        .enumerate()
        .map(|(t, members)| Team {
            id: t as u32,
            name: format!("team_{t}"),
            relations: kept
                .iter()
                .map(|q| match members.first() {
                    Some(&m) => toward(m, Some(t), q),
                    None => Relation::Neutral,
                })
                .map(|r| match r {
                    Relation::Itself => Relation::Ally,
                    r => r,
                })
                .collect(),
            team_relations: (0..teams.len())
                .map(|u| between(Some(t), Some(u)))
                .collect(),
        })
        .collect();
    (new_players, new_teams)
}

/// What a map's lobby shows of it, read without loading the rest.
pub struct Brief {
    /// Its name as the game shows it, e.g. `Fall of Berlin`; the file's
    /// where the map has none.
    pub name: String,
    pub description: String,
    /// Width and height in world units, where the map records them.
    pub size: Option<[f32; 2]>,
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
}

impl Brief {
    /// Reads the brief of the map at an archive path, as
    /// [`GameData::find_map`] gives it.
    pub fn load(game: &GameData, name: &str) -> Result<Brief, String> {
        let mut map = game.open_map(name)?;
        let err = |e: tgck::Error| format!("{name}: {e}");
        let header = map.header().map_err(err)?;
        let (players, teams) = read_sides(game, &mut map).map_err(err)?;
        Ok(Brief {
            name: map
                .string(&header.name)
                .map_err(err)?
                .unwrap_or_else(|| tgdata::map_title(name).to_string()),
            description: map
                .string(&header.description)
                .map_err(err)?
                .unwrap_or_default(),
            size: header.size,
            players,
            teams,
        })
    }

    /// The players that play to win, as the lobby numbers its sides from 1;
    /// the rest are independent.
    pub fn sides(&self) -> impl Iterator<Item = &Player> {
        sides(&self.players)
    }
}

/// The players that play to win, in the order the lobby numbers its sides.
pub fn sides(players: &[Player]) -> impl Iterator<Item = &Player> {
    players.iter().filter(|p| !p.independent)
}

fn read_sides<R: std::io::Read + std::io::Seek>(
    game: &GameData,
    map: &mut tgck::Map<R>,
) -> tgck::Result<(Vec<Player>, Vec<Team>)> {
    let rules = game.economy_rules();
    let default = map
        .rules()?
        .rules
        .iter()
        .find(|r| r.name == "ResourceStockpileDefault")
        .and_then(|r| match r.value {
            tgck::map::RuleValue::Float(v) => Some(v),
            tgck::map::RuleValue::Int(v) => Some(v as f32),
            _ => None,
        })
        .unwrap_or(rules.stockpile_default)
        .clamp(0.0, rules.stockpile_max);
    Ok(units::players_and_teams(
        &map.kingdoms()?,
        &map.teams()?,
        &map.restriction_groups()?,
        &game.kingdom_colors(),
        default,
        rules.stockpiled().next().map_or("", Id::as_str),
        &handicaps(&map.header()?.parameters.kingdoms),
    ))
}

/// The handicap the map gives each player that has one, by its kingdom's
/// name.
fn handicaps(setups: &[tgck::map::KingdomSetup]) -> HashMap<String, Id> {
    (setups.iter())
        .filter_map(|k| {
            let h = k.handicap.trim();
            (!h.is_empty() && !h.eq_ignore_ascii_case("null"))
                .then(|| (k.name.clone(), Id::from(h)))
        })
        .collect()
}

/// The point `right` and `ahead` of `origin` when facing `facing`, a unit
/// vector.
pub fn from_local(origin: [f32; 2], [fx, fy]: [f32; 2], [right, ahead]: [f32; 2]) -> [f32; 2] {
    let [x, y] = origin;
    [x + right * fy + ahead * fx, y - right * fx + ahead * fy]
}

/// How far `point` is to the right of `origin` and ahead of it when facing
/// `facing`, a unit vector; the inverse of [`from_local`].
pub fn to_local(origin: [f32; 2], [fx, fy]: [f32; 2], point: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (point[0] - origin[0], point[1] - origin[1]);
    [dx * fy - dy * fx, dx * fx + dy * fy]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{placed, stored_organization};

    #[test]
    fn scenery_is_clearable_unless_it_blocks_is_an_actor_or_a_trigger_names_it() {
        use tgck::map::{Detail, Element, Parameter, Trigger};
        let scenery = |id: u32, template: &str, name: &str| Thing {
            class: tgck::map::SCENERY,
            name: name.into(),
            detail: Detail::None,
            ..placed(id, template, 0, None, None)
        };
        let things = vec![
            scenery(1, "tree", ""),
            scenery(2, "rock", ""),
            placed(3, "tree", 0, None, None),
            scenery(4, "tree", ""),
            scenery(5, "tree", "old_oak"),
            Thing {
                class: tgck::map::REACTIVE_SCENERY,
                ..scenery(6, "tree", "")
            },
        ];
        let trunk = Shape::Circle { radius: 0.2 };
        let bodies = HashMap::from([
            (
                Id::from("tree"),
                tgdata::Body {
                    shape: trunk,
                    ..Default::default()
                },
            ),
            (
                Id::from("rock"),
                tgdata::Body {
                    blocking: Some(trunk),
                    fixed: true,
                    ..Default::default()
                },
            ),
        ]);
        let parameter = |source: &str, value| Parameter {
            kind: "ma_actor".into(),
            source: source.into(),
            set: true,
            value,
        };
        let trigger = Trigger {
            id: 1,
            all_conditions: true,
            active: true,
            repeats: false,
            activation_time: 0.0,
            name: String::new(),
            comment: String::new(),
            next_element: 1,
            events: Vec::new(),
            conditions: vec![Element {
                version: 1,
                name: "actor_alive".into(),
                id: 0,
                parameters: vec![
                    parameter("mi_actor_id", Value::Int(4)),
                    parameter("mi_actor_named", Value::String("old_oak".into())),
                ],
            }],
            effects: Vec::new(),
        };
        let triggers = Triggers {
            version: 5,
            next_id: 2,
            triggers: vec![trigger],
            variables: Vec::new(),
            legacy: None,
            unused_strings: None,
        };
        let cleared = clearable(&things, &bodies, &Named::new(&triggers));
        let ids: Vec<u32> = cleared.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, [1, 6]);
        assert_eq!(cleared[0].1.shape, trunk);
    }

    /// Sides 1 and 2 on the map's first team, 3 and 4 on its second, and an
    /// independent 9 at war with 1 and neutral to the rest.
    fn sides() -> (Vec<Player>, Vec<Team>) {
        use Relation::*;
        let side = |id, team| Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [1.0; 3],
            team: Some(team),
            independent: false,
            playable: true,
            default_defeat: true,
            outcome: Default::default(),
            won_by_cities: false,
            resigned: false,
            nation: None,
            faction: None,
            relations: vec![Enemy; 5],
            team_relations: vec![Enemy; 2],
            stockpile: 0.0,
            technologies: Vec::new(),
            handicap: None,
            barred: Vec::new(),
        };
        let mut players = vec![side(1, 0), side(2, 0), side(3, 1), side(4, 1)];
        players.push(Player {
            id: 9,
            team: None,
            independent: true,
            relations: vec![Enemy, Neutral, Neutral, Neutral, Itself],
            team_relations: vec![Enemy, Neutral],
            ..players[0].clone()
        });
        let team = |id, own: usize| Team {
            id,
            name: format!("team_{id}"),
            relations: vec![Neutral; 5],
            team_relations: (0..2)
                .map(|t| if t == own { Ally } else { Enemy })
                .collect(),
        };
        let mut teams = vec![team(0, 0), team(1, 1)];
        teams[0].relations[4] = Enemy;
        (players, teams)
    }

    fn moves(trailer: Option<&str>) -> tgdata::Body {
        tgdata::Body {
            speed: Some(3.0),
            trailer: trailer.map(Id::from),
            ..tgdata::Body::default()
        }
    }

    #[test]
    fn things_without_room_are_crowded_out_as_the_map_loads() {
        let circle = |radius| Shape::Circle { radius };
        let body = |blocking: bool, fixed: bool| tgdata::Body {
            blocking: blocking.then_some(circle(1.0)),
            shape: circle(1.0),
            soft: circle(1.0),
            fixed,
            ..tgdata::Body::default()
        };
        let bodies = HashMap::from([
            (Id::from("hut"), body(true, true)),
            (Id::from("tank"), body(true, false)),
            (Id::from("ripple"), body(false, true)),
        ]);
        let at = |id, template, [x, y]: [f32; 2]| {
            let mut t = placed(id, template, 0, None, None);
            t.position = [x, y, 0.0];
            t
        };
        let things = [
            // A unit stored before the hut it stands in is kept, and so is
            // the hut, as units are in nothing's way.
            at(1, "tank", [5.0, 5.0]),
            at(2, "hut", [5.5, 5.0]),
            at(3, "tank", [6.0, 5.0]),
            at(4, "hut", [6.5, 5.5]),
            // Blocking things need land, the others only the map.
            at(5, "hut", [13.0, 3.0]),
            at(6, "ripple", [13.0, 3.0]),
            at(7, "ripple", [0.5, 3.0]),
            // Members are placed like any other thing, and one without room
            // takes its organization and the other members with it.
            placed(
                8,
                "squad",
                0,
                None,
                Some(stored_organization(
                    "line",
                    vec![at(9, "tank", [9.0, 9.0]), at(10, "hut", [5.0, 6.0])],
                )),
            ),
            // One off the map drops the lot even after one without room.
            placed(
                11,
                "squad",
                0,
                None,
                Some(stored_organization(
                    "line",
                    vec![at(12, "tank", [5.0, 5.0]), at(13, "tank", [-1.0, 5.0])],
                )),
            ),
        ];
        let mut pathing = Pathing::uniform(8, Domain::Land);
        pathing.tiles[6 + 8] = 2;
        let placed = super::placed(&things, &bodies, &pathing);
        assert_eq!(placed.off_map, HashSet::from([7, 11, 12, 13]));
        assert_eq!(placed.crowded, HashSet::from([3, 4, 5, 8, 9, 10]));
        assert_eq!(
            placed.warnings,
            [
                "3 tank: no room",
                "4 hut: no room",
                "5 hut: no room",
                "7 ripple: off the map",
                "8 squad: no room for member 10 hut",
                "11 squad: off the map for member 13 tank",
            ]
        );
    }

    #[test]
    fn members_are_placed_when_their_organization_is() {
        let circle = Shape::Circle { radius: 1.0 };
        let body = |fixed| tgdata::Body {
            blocking: Some(circle),
            shape: circle,
            soft: circle,
            fixed,
            ..tgdata::Body::default()
        };
        let bodies = HashMap::from([
            (Id::from("hut"), body(true)),
            (Id::from("tank"), body(false)),
        ]);
        let at = |id, template, [x, y]: [f32; 2]| {
            let mut t = placed(id, template, 0, None, None);
            t.position = [x, y, 0.0];
            t
        };
        let squad = |id, member| {
            let tank = at(member, "tank", [5.0, 5.0]);
            placed(
                id,
                "squad",
                0,
                None,
                Some(stored_organization("line", vec![tank])),
            )
        };
        // The member's id is higher than the hut's, but its squad comes
        // first, so both stand; the squad stored after the hut doesn't.
        // A hut in an organization that goes is no obstacle to a hut
        // stored after it.
        let camp = stored_organization(
            "line",
            vec![at(5, "hut", [12.0, 12.0]), at(6, "tank", [5.0, 5.0])],
        );
        let things = [
            squad(1, 10),
            at(2, "hut", [5.0, 5.0]),
            squad(3, 11),
            placed(4, "squad", 0, None, Some(camp)),
            at(7, "hut", [12.0, 12.0]),
        ];
        let pathing = Pathing::uniform(8, Domain::Land);
        let placed = super::placed(&things, &bodies, &pathing);
        assert!(placed.off_map.is_empty());
        assert_eq!(placed.crowded, HashSet::from([3, 11, 4, 5, 6]));
    }

    #[test]
    fn only_the_tiles_a_thing_overlaps_need_to_be_its_ground() {
        let rect = Shape::Rect {
            width: 4.0,
            height: 2.0,
        };
        let bunker = tgdata::Body {
            blocking: Some(rect),
            shape: rect,
            soft: rect,
            fixed: true,
            ..tgdata::Body::default()
        };
        let bodies = HashMap::from([(Id::from("bunker"), bunker)]);
        // Turned 45°, each spans two tiles below its centre tile and one
        // above, on x and y.
        let at = |id, [x, y]: [f32; 2]| {
            let mut t = placed(id, "bunker", 0, None, None);
            t.position = [x, y, 0.0];
            t.facing = [0.70710677, 0.70710677, 0.0];
            t
        };
        let things = [at(1, [10.0, 10.0]), at(2, [30.0, 10.0])];
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // Water only in the first's box corners, and where the second's
        // corner reaches into (15, 3).
        for (x, y) in [(3, 3), (6, 3), (3, 6), (6, 6), (15, 3)] {
            pathing.tiles[y * 20 + x] = 2;
        }
        let placed = super::placed(&things, &bodies, &pathing);
        assert!(placed.off_map.is_empty());
        assert_eq!(placed.crowded, HashSet::from([2]));
    }

    #[test]
    fn tile_locked_things_snap_to_the_half_tile_grid() {
        let locked = tgdata::Body {
            tile_locked: true,
            ..tgdata::Body::default()
        };
        let bodies = HashMap::from([(Id::from("wall"), locked)]);
        let at = |template, [x, y]: [f32; 2]| {
            let mut t = placed(1, template, 0, None, None);
            t.position = [x, y, 3.0];
            snap(&mut t, &bodies);
            t.position
        };
        assert_eq!(at("wall", [10.4, 7.6]), [10.0, 8.0, 3.0]);
        assert_eq!(at("wall", [10.6, -0.4]), [11.0, 0.0, 3.0]);
        assert_eq!(at("tree", [10.4, 7.6]), [10.4, 7.6, 3.0]);
    }

    #[test]
    fn owned_units_outside_organizations_stand_as_organizations_of_one() {
        let things = [
            placed(1, "truck", 2, Some(2), None),
            placed(2, "gun", 2, None, None),
            placed(3, "cow", 0, None, None),
            placed(4, "rock", 2, None, None),
        ];
        let bodies = HashMap::from([
            (Id::from("truck"), moves(Some("gun"))),
            (Id::from("gun"), moves(None)),
            (Id::from("cow"), moves(None)),
        ]);
        let none = HashMap::new();
        let organizations =
            units::organizations(&things, &bodies, &HashMap::new(), &none, &HashMap::new());
        // The gun goes with the truck, and the cow is nobody's.
        let [truck] = &organizations[..] else {
            panic!("{organizations:?}");
        };
        assert!(truck.lone());
        assert_eq!((truck.id, truck.owner, truck.primary), (1, 2, Some(0)));
        assert_eq!(truck.members[0].health, 1.0);
        let trailer = truck.members[0].trailer.as_ref().unwrap();
        assert_eq!(trailer.placed.map(|p| p.0), Some(2));
    }

    #[test]
    fn members_keep_the_health_the_map_has_them_at() {
        let things = [crate::fixtures::hurt(placed(1, "gun", 2, None, None), 0.4)];
        let bodies = HashMap::from([(Id::from("gun"), moves(None))]);
        let none = HashMap::new();
        let organizations =
            units::organizations(&things, &bodies, &HashMap::new(), &none, &HashMap::new());
        assert_eq!(organizations[0].members[0].health, 0.4);
    }

    #[test]
    fn things_are_told_apart_by_what_they_are_in_a_match() {
        let hq = stored_organization("regiment_hq", vec![placed(11, "hq", 0, None, None)]);
        let things = [
            placed(1, "truck", 2, Some(2), None),
            placed(2, "gun", 2, None, None),
            placed(10, "hq_unpacked", 2, None, Some(hq)),
            placed(20, "hut", 2, None, None),
            placed(30, "rock", 0, None, None),
            placed(40, "truck", 2, None, None),
        ];
        let bodies = HashMap::from([
            (Id::from("truck"), moves(Some("gun"))),
            (Id::from("gun"), moves(None)),
        ]);
        let fighters = HashMap::from(["hq", "hut"].map(|t| (Id::from(t), Fighter::default())));
        // The last truck had no room, so it's no organization or anything else.
        let crowded = HashSet::from([40]);
        let standing = without(&things, &crowded);
        let organizations = units::organizations(
            &standing,
            &bodies,
            &fighters,
            &HashMap::new(),
            &HashMap::new(),
        );
        let buildings = buildings::buildings(
            &standing,
            &bodies,
            &fighters,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let found: Vec<(u32, Role)> = roles(&things, &organizations, &buildings, &crowded)
            .map(|(t, role)| (t.id, role))
            .collect();
        assert_eq!(
            found,
            [
                (1, Role::Organization),
                (2, Role::Trailer),
                (10, Role::HoldsBuilding),
                (20, Role::Building),
                (30, Role::Other),
                (40, Role::Crowded),
            ]
        );
    }

    #[test]
    fn buildings_standing_in_organizations_are_buildings() {
        // A Division HQ standing unpacked: its building and a bunker that
        // can't be hurt, both owned through the HQ.
        let hq = placed(
            10,
            "hq_unpacked",
            2,
            None,
            Some(stored_organization(
                "regiment_hq",
                vec![
                    placed(11, "hq", 0, None, None),
                    placed(12, "bunker", 0, None, None),
                ],
            )),
        );
        let things = [hq];
        let bodies = HashMap::from([(
            Id::from("hq"),
            tgdata::Body {
                blocking: Some(Shape::Rect {
                    width: 8.0,
                    height: 8.0,
                }),
                fixed: true,
                ..tgdata::Body::default()
            },
        )]);
        let obstacles = obstacles(&things, &bodies);
        assert_eq!(
            obstacles.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            [11]
        );
        let fighters = HashMap::from([(
            Id::from("hq"),
            Fighter {
                health: 2000.0,
                ..Fighter::default()
            },
        )]);
        let buildings = buildings::buildings(
            &things,
            &bodies,
            &fighters,
            &HashMap::new(),
            &obstacles.into_iter().collect(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let [b] = &buildings[..] else {
            panic!("{buildings:?}");
        };
        assert_eq!((b.id, b.owner, b.position), (11, 2, [11.0, 0.0]));
        // It packs into its place in the organization, which keeps its id.
        let o = b.organization.as_ref().unwrap();
        assert_eq!((o.id, o.template.as_str(), o.owner), (10, "hq_unpacked", 2));
        assert!(o.members.is_empty());
        assert!(b.obstacle.is_some());
        assert!(
            units::organizations(
                &things,
                &bodies,
                &fighters,
                &HashMap::new(),
                &HashMap::new()
            )
            .is_empty()
        );
    }

    #[test]
    fn placed_centres_bring_their_settlements_numbered_after_the_map() {
        let centre = |id, owner| Building {
            id,
            template: "human_center_village".into(),
            owner,
            position: [0.0, 0.0],
            facing: [0.0, -1.0],
            fighter: Fighter::default(),
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: None,
            deploy: None,
            organization: None,
            awake: false,
        };
        let settlement = Id::from("human_settlement");
        // The centre at 9 had no room, so wasn't placed.
        let centres = [
            (4, settlement.clone()),
            (9, settlement.clone()),
            (6, settlement.clone()),
        ];
        let found = founded(&centres, &[centre(4, 1), centre(6, 2)], 20);
        let found: Vec<_> = found.iter().map(|s| (s.id, s.owner, s.centre)).collect();
        assert_eq!(found, [(20, 1, 4), (21, 2, 6)]);
    }

    #[test]
    fn buildings_taken_off_the_map_stop_blocking() {
        let wall = |id, owner, x| Building {
            id,
            template: "wall".into(),
            owner,
            position: [x, 8.0],
            facing: [0.0, -1.0],
            fighter: Fighter::default(),
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: Some(Obstacle {
                position: [x, 8.0],
                facing: [0.0, -1.0],
                shape: Shape::Circle { radius: 1.0 },
                soft: Shape::Circle { radius: 1.0 },
            }),
            deploy: None,
            organization: None,
            awake: false,
        };
        let mut buildings = vec![wall(1, 2, 4.0), wall(2, 3, 12.0)];
        let mut pathing = Pathing::uniform(8, Domain::Land);
        for b in &buildings {
            pathing.block(b.obstacle.as_ref().unwrap());
        }
        take_buildings(&mut buildings, &mut pathing, |b| b.owner == 3);
        assert_eq!(buildings.iter().map(|b| b.id).collect::<Vec<_>>(), [1]);
        assert!(!pathing.passable(Tile::new(2, 4), Domain::Land));
        assert!(pathing.passable(Tile::new(6, 4), Domain::Land));
    }

    #[test]
    fn arranging_makes_teammates_allies_and_the_rest_enemies() {
        let (players, teams) = sides();
        // A free-for-all of 1 and 3, who were on different teams, and 2,
        // who was 1's ally.
        let (players, teams) = arranged(&players, &teams, &[vec![1], vec![2], vec![3, 4]]);
        let ids: Vec<u32> = players.iter().map(|p| p.id).collect();
        assert_eq!(ids, [1, 2, 3, 4, 9]);
        let r = |a, b| relation(&players, &teams, a, b);
        assert_eq!(r(1, 2), Relation::Enemy);
        assert_eq!(r(3, 4), Relation::Ally);
        assert_eq!(r(1, 3), Relation::Enemy);
        assert_eq!(r(1, 1), Relation::Itself);
        // The independent keeps its stances, either way round.
        assert_eq!(r(9, 1), Relation::Enemy);
        assert_eq!(r(1, 9), Relation::Enemy);
        assert_eq!(r(9, 3), Relation::Neutral);
        assert_eq!(r(3, 9), Relation::Neutral);
    }

    #[test]
    fn arranging_leaves_out_sides_on_no_team() {
        let (players, teams) = sides();
        let (players, teams) = arranged(&players, &teams, &[vec![1], vec![4]]);
        let ids: Vec<u32> = players.iter().map(|p| p.id).collect();
        assert_eq!(ids, [1, 4, 9]);
        assert_eq!(teams.len(), 2);
        assert_eq!(relation(&players, &teams, 1, 4), Relation::Enemy);
        assert_eq!(relation(&players, &teams, 1, 2), Relation::Neutral);
    }
}
