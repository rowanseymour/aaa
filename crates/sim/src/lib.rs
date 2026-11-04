//! The rules of the game, played out with no rendering so that it can run
//! headless.
//!
//! A *match* is one play of a map: a *match setup* (the map, its players and
//! their nations, teams and rules) once it's running. Each *player* is a side
//! in it, controlled by a person or an AI.

pub mod path;

use std::collections::HashMap;

use aaa_map::pathing::tile_at;
use aaa_map::{Domain, Fighter, Id, Map, Pathing, Weapon};

pub use aaa_map::{Player, Relation, Team};

/// Seconds of play one [`World::tick`] advances.
pub const TICK: f32 = 0.1;

/// A match in progress.
pub struct World {
    pub sides: Sides,
    /// The owning [`Player::id`] of each owned object that isn't a
    /// regiment, such as a building, by object id.
    pub owners: HashMap<u32, u32>,
    pub pathing: Pathing,
    pub regiments: Vec<Regiment>,
    pub units: Vec<Unit>,
    /// Ticks played.
    pub ticks: u64,
}

pub struct Regiment {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`], the only one it takes orders from.
    pub owner: u32,
    /// Indices into [`World::units`].
    pub units: Vec<usize>,
    /// Its slowest member's, which they all keep to so as to stay together.
    pub speed: f32,
    /// Where it stands, or is headed: its members take their places in the
    /// formation around it.
    pub position: [f32; 2],
    /// The way it faces, or will once there.
    pub facing: [f32; 2],
    pub formations: Vec<aaa_map::Formation>,
    /// Index into `formations` of the one it's in.
    pub formation: usize,
}

pub struct Unit {
    /// Its object's id in the map.
    pub id: u32,
    pub template: String,
    /// Index into [`World::regiments`].
    pub regiment: usize,
    pub domain: Domain,
    /// In world units.
    pub position: [f32; 2],
    /// Where it was a tick ago, to draw it in between.
    pub previous: [f32; 2],
    /// The way it faces, a unit vector.
    pub facing: [f32; 2],
    /// The way to face once it arrives.
    end_facing: [f32; 2],
    /// Waypoints still ahead, the next last.
    path: Vec<[f32; 2]>,
    /// What it can take and deal; `None` for what can't be hurt.
    pub fighter: Option<Fighter>,
    pub health: f32,
    /// Seconds until each of its fighter's weapons can fire again.
    reloads: Vec<f32>,
    /// Index into [`World::units`] of what it last fired on.
    pub target: Option<usize>,
    /// The tick it last fired on, and the index of the weapon it fired.
    pub shot: Option<(u64, usize)>,
    /// The tick it died on.
    pub died: Option<u64>,
}

impl Unit {
    pub fn moving(&self) -> bool {
        !self.path.is_empty()
    }

    pub fn alive(&self) -> bool {
        self.died.is_none()
    }
}

impl World {
    pub fn new(map: &Map) -> World {
        let sides = Sides {
            players: map.players.clone(),
            teams: map.teams.clone(),
        };
        let mut world = World::from_regiments(sides, map.pathing.clone(), &map.regiments);
        world.owners = map
            .things
            .iter()
            .filter_map(|t| Some((t.id, aaa_map::owner(t)?)))
            .collect();
        world
    }

    pub fn from_regiments(
        sides: Sides,
        pathing: Pathing,
        regiments: &[aaa_map::Regiment],
    ) -> World {
        let mut world = World {
            sides,
            owners: HashMap::new(),
            pathing,
            regiments: Vec::new(),
            units: Vec::new(),
            ticks: 0,
        };
        for (r, regiment) in regiments.iter().enumerate() {
            let first = world.units.len();
            for m in &regiment.members {
                let [x, y, _] = m.thing.position;
                let facing =
                    normalised([m.thing.facing[0], m.thing.facing[1]]).unwrap_or([0.0, -1.0]);
                let (tx, ty) = tile_at([x, y]);
                // Units start on what they travel on.
                let domain = match world.pathing.passable(tx, ty, Domain::Land)
                    || !world.pathing.passable(tx, ty, Domain::Naval)
                {
                    true => Domain::Land,
                    false => Domain::Naval,
                };
                let fighter = m.fighter.clone();
                world.units.push(Unit {
                    health: fighter.as_ref().map_or(0.0, |f| f.health),
                    reloads: fighter
                        .as_ref()
                        .map_or(Vec::new(), |f| vec![0.0; f.weapons.len()]),
                    fighter,
                    target: None,
                    shot: None,
                    died: None,
                    id: m.thing.id,
                    template: m.thing.template.clone(),
                    regiment: r,
                    domain,
                    position: [x, y],
                    previous: [x, y],
                    facing,
                    end_facing: facing,
                    path: Vec::new(),
                });
            }
            let units: Vec<usize> = (first..world.units.len()).collect();
            let position = match regiment.primary {
                Some(p) => world.units[first + p].position,
                None => world.average(&units),
            };
            world.regiments.push(Regiment {
                id: regiment.id,
                template: regiment.template.clone(),
                owner: regiment.owner,
                units,
                speed: regiment
                    .members
                    .iter()
                    .map(|m| m.speed)
                    .fold(f32::INFINITY, f32::min),
                position,
                facing: normalised(regiment.facing).unwrap_or([0.0, -1.0]),
                formations: regiment.formations.clone(),
                formation: regiment.formation,
            });
        }
        world
    }

    /// Where a regiment's surviving members stand on average.
    pub fn centre(&self, regiment: usize) -> [f32; 2] {
        let survivors: Vec<usize> = self.survivors(regiment).collect();
        self.average(&survivors)
    }

    /// A regiment's members still alive.
    pub fn survivors(&self, regiment: usize) -> impl Iterator<Item = usize> + '_ {
        let units = &self.regiments[regiment].units;
        units.iter().copied().filter(|&u| self.units[u].alive())
    }

    /// Whether a regiment has lost every member.
    pub fn destroyed(&self, regiment: usize) -> bool {
        self.survivors(regiment).next().is_none()
    }

    fn average(&self, units: &[usize]) -> [f32; 2] {
        let n = units.len().max(1) as f32;
        let [x, y] = units.iter().fold([0.0, 0.0], |[x, y], &u| {
            let [ux, uy] = self.units[u].position;
            [x + ux, y + uy]
        });
        [x / n, y / n]
    }

    /// Whether `player` may give `regiment` orders.
    pub fn commands(&self, player: u32, regiment: usize) -> bool {
        self.regiments[regiment].owner == player && !self.destroyed(regiment)
    }

    /// Sends a regiment to `target`, turned to face the way it's going,
    /// each member making for its place in the formation there.
    pub fn order_move(
        &mut self,
        player: u32,
        regiment: usize,
        target: [f32; 2],
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let r = &mut self.regiments[regiment];
        let from = r.position;
        if let Some(heading) = normalised([target[0] - from[0], target[1] - from[1]]) {
            r.facing = heading;
        }
        r.position = target;
        self.take_places(regiment);
        Ok(())
    }

    /// Switches a regiment to another of its formations, its members
    /// taking their new places where it stands, or is headed.
    pub fn set_formation(
        &mut self,
        player: u32,
        regiment: usize,
        formation: usize,
    ) -> Result<(), NotYours> {
        if !self.commands(player, regiment) {
            return Err(NotYours);
        }
        let r = &mut self.regiments[regiment];
        if formation < r.formations.len() && formation != r.formation {
            r.formation = formation;
            self.take_places(regiment);
        }
        Ok(())
    }

    /// Sends each of a regiment's members to its place in the formation.
    fn take_places(&mut self, regiment: usize) {
        let r = &self.regiments[regiment];
        let [px, py] = r.position;
        let [fx, fy] = r.facing;
        let offsets = &r.formations[r.formation].offsets;
        for (&u, &[right, ahead]) in r.units.iter().zip(offsets) {
            let place = [px + right * fy + ahead * fx, py - right * fx + ahead * fy];
            let unit = &mut self.units[u];
            if !unit.alive() {
                continue;
            }
            let mut way = path::find(&self.pathing, unit.domain, unit.position, place);
            way.reverse();
            unit.path = way;
            unit.end_facing = r.facing;
            if unit.path.is_empty() {
                unit.facing = r.facing;
            }
        }
    }

    /// Plays one [`TICK`].
    pub fn tick(&mut self) {
        self.travel();
        self.fight();
        self.ticks += 1;
    }

    fn travel(&mut self) {
        for unit in &mut self.units {
            unit.previous = unit.position;
            let mut left = self.regiments[unit.regiment].speed * TICK;
            while let Some(&next) = unit.path.last() {
                let (dx, dy) = (next[0] - unit.position[0], next[1] - unit.position[1]);
                let distance = dx.hypot(dy);
                if let Some(f) = normalised([dx, dy]) {
                    unit.facing = f;
                }
                if distance > left {
                    unit.position[0] += dx / distance * left;
                    unit.position[1] += dy / distance * left;
                    break;
                }
                unit.position = next;
                left -= distance;
                unit.path.pop();
                if unit.path.is_empty() {
                    unit.facing = unit.end_facing;
                }
            }
        }
    }

    /// Fires every weapon that's ready at what it can hit, then takes the
    /// dead out of the fight, so that units hit each other at once.
    fn fight(&mut self) {
        let mut hits: Vec<(usize, f32)> = Vec::new();
        for u in 0..self.units.len() {
            let unit = &self.units[u];
            let Some(fighter) = unit.fighter.as_ref().filter(|_| unit.alive()) else {
                continue;
            };
            let mut reloads = unit.reloads.clone();
            let mut target = unit.target;
            let mut shot = unit.shot;
            for (w, weapon) in fighter.weapons.iter().enumerate() {
                reloads[w] = (reloads[w] - TICK).max(0.0);
                if reloads[w] > 0.0 {
                    continue;
                }
                let Some(t) = self.aim(u, weapon, target) else {
                    continue;
                };
                target = Some(t);
                shot = Some((self.ticks, w));
                reloads[w] = weapon.recharge;
                self.strike(weapon, self.units[t].position, t, &mut hits);
            }
            let unit = &mut self.units[u];
            unit.reloads = reloads;
            unit.target = target;
            unit.shot = shot;
        }
        for (t, amount) in hits {
            let unit = &mut self.units[t];
            if unit.alive() {
                unit.health -= amount;
                if unit.health <= 0.0 {
                    unit.health = 0.0;
                    unit.died = Some(self.ticks);
                    unit.path.clear();
                    unit.target = None;
                }
            }
        }
        for u in 0..self.units.len() {
            if self.units[u].target.is_some_and(|t| !self.units[t].alive()) {
                self.units[u].target = None;
            }
        }
    }

    /// What unit `u` fires `weapon` at: the one it fired on last if it
    /// still can, else the nearest it can.
    fn aim(&self, u: usize, weapon: &Weapon, last: Option<usize>) -> Option<usize> {
        if let Some(t) = last.filter(|&t| self.can_hit(u, weapon, t)) {
            return Some(t);
        }
        let from = self.units[u].position;
        (0..self.units.len())
            .filter(|&t| self.can_hit(u, weapon, t))
            .min_by(|&a, &b| {
                distance(from, self.units[a].position)
                    .total_cmp(&distance(from, self.units[b].position))
            })
    }

    fn can_hit(&self, u: usize, weapon: &Weapon, t: usize) -> bool {
        let (unit, target) = (&self.units[u], &self.units[t]);
        let Some(fighter) = target.fighter.as_ref().filter(|_| target.alive() && t != u) else {
            return false;
        };
        let d = distance(unit.position, target.position);
        if d > weapon.range || d < weapon.min_range {
            return false;
        }
        let side = match self.sides.relation(
            self.regiments[unit.regiment].owner,
            self.regiments[target.regiment].owner,
        ) {
            Relation::Enemy => "enemy",
            Relation::Ally | Relation::Itself => "friend",
            _ => "neutral",
        };
        let side = Id::from(side);
        weapon
            .targets
            .allow(fighter.flags.iter().chain(std::iter::once(&side)))
    }

    /// Records the damage a shot landing at `at`, aimed at unit `t`, does
    /// to what it hits.
    fn strike(&self, weapon: &Weapon, at: [f32; 2], t: usize, hits: &mut Vec<(usize, f32)>) {
        if weapon.area <= 0.0 {
            if let Some(f) = &self.units[t].fighter {
                hits.push((t, damage(weapon, f, 1.0)));
            }
            return;
        }
        for (i, unit) in self.units.iter().enumerate() {
            let Some(f) = unit.fighter.as_ref().filter(|_| unit.alive()) else {
                continue;
            };
            let d = distance(at, unit.position);
            if d > weapon.area {
                continue;
            }
            hits.push((i, damage(weapon, f, falloff(weapon, d))));
        }
    }
}

/// The fraction of an area shot's damage that reaches `d` from where it
/// lands: full within [`Weapon::full_within`], falling
/// evenly to [`Weapon::edge_damage`] at the edge.
fn falloff(weapon: &Weapon, d: f32) -> f32 {
    let full = weapon.full_within.min(weapon.area);
    if d <= full || weapon.area <= full {
        return 1.0;
    }
    let along = (d - full) / (weapon.area - full);
    1.0 + (weapon.edge_damage - 1.0) * along
}

/// Health a shot of `weapon` takes from `target`, its damage scaled by
/// `scale`: defence comes off first, then resistance takes its share, and
/// every hit does at least 1.
fn damage(weapon: &Weapon, target: &Fighter, scale: f32) -> f32 {
    weapon
        .damage
        .iter()
        .map(|d| {
            let resist = target.resists.get(&d.kind).copied().unwrap_or(0.0);
            ((d.amount * scale - target.defense.max(0.0)) * (1.0 - resist)).max(1.0)
        })
        .sum()
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// An order to a regiment from a player that doesn't own it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotYours;

/// The players in a match and how they stand toward each other.
#[derive(Debug, Clone, Default)]
pub struct Sides {
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
}

impl Sides {
    pub fn player(&self, id: u32) -> Option<&Player> {
        self.players.iter().find(|p| p.id == id)
    }

    /// How the player `from` stands toward the player `to`, by their ids;
    /// neutral toward a player the match doesn't have.
    pub fn relation(&self, from: u32, to: u32) -> Relation {
        if from == to {
            return Relation::Itself;
        }
        let index = |id| self.players.iter().position(|p| p.id == id);
        let (Some(a), Some(b)) = (index(from), index(to)) else {
            return Relation::Neutral;
        };
        let (pa, pb) = (&self.players[a], &self.players[b]);
        // Players put in a team keep their own stored relations at war
        // with everyone, so the team's speak for them.
        let stance = match (pa.team, pb.team) {
            (Some(ta), Some(tb)) => self.teams[ta].team_relations.get(tb),
            (Some(ta), None) => self.teams[ta].relations.get(b),
            (None, Some(tb)) => pa.team_relations.get(tb),
            (None, None) => pa.relations.get(b),
        };
        stance.copied().unwrap_or(Relation::Neutral)
    }

    /// The player a person plays by default: the first a person may.
    pub fn first_playable(&self) -> Option<&Player> {
        self.players.iter().find(|p| p.playable && !p.independent)
    }
}

fn normalised([x, y]: [f32; 2]) -> Option<[f32; 2]> {
    let length = x.hypot(y);
    (length > 1e-6).then(|| [x / length, y / length])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tgck::map::{Detail, Thing};

    fn member(id: u32, position: [f32; 2], speed: f32) -> aaa_map::Member {
        aaa_map::Member {
            thing: Thing {
                template: "soldier".into(),
                class: tgck::map::ACTOR,
                id,
                name: String::new(),
                position: [position[0], position[1], 0.0],
                facing: [0.0, 1.0, 0.0],
                flags: 0,
                scale: 1.0,
                variation: -1,
                seen_by: 0,
                detail: Detail::Raw(Vec::new()),
            },
            speed,
            fighter: None,
        }
    }

    /// Two soldiers side by side facing +y, the slower one on the right,
    /// who can also line up one behind the other.
    fn world(pathing: Pathing) -> World {
        let formation = |id: &str, offsets| aaa_map::Formation {
            id: id.into(),
            offsets,
        };
        let regiment = aaa_map::Regiment {
            id: 1,
            template: "squad".into(),
            owner: 7,
            facing: [0.0, 1.0],
            members: vec![member(2, [9.0, 10.0], 3.0), member(3, [11.0, 10.0], 2.0)],
            formations: vec![
                formation("line", vec![[-1.0, 0.0], [1.0, 0.0]]),
                formation("file", vec![[0.0, 1.0], [0.0, -1.0]]),
            ],
            formation: 0,
            primary: None,
        };
        World::from_regiments(Sides::default(), pathing, &[regiment])
    }

    fn run(world: &mut World, seconds: f32) {
        for _ in 0..(seconds / TICK).round() as usize {
            world.tick();
        }
    }

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-3
    }

    #[test]
    fn regiments_move_at_their_slowest_members_speed() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 1.0);
        assert!(near(w.units[0].position, [9.0, 12.0]));
        assert!(near(w.units[1].position, [11.0, 12.0]));
        assert!(w.units.iter().all(Unit::moving));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 20.0]));
        assert!(!w.units.iter().any(Unit::moving));
    }

    #[test]
    fn formations_turn_to_face_the_way_they_go() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        // Facing +x now, so the left-hand man ends up on the +y side.
        w.order_move(7, 0, [30.0, 10.0]).unwrap();
        run(&mut w, 20.0);
        assert!(
            near(w.units[0].position, [30.0, 11.0]),
            "{:?}",
            w.units[0].position
        );
        assert!(near(w.units[1].position, [30.0, 9.0]));
        assert!(near(w.units[0].facing, [1.0, 0.0]));
    }

    #[test]
    fn members_go_back_to_their_places_after_being_kept_from_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        // The right-hand man's place at 11,20 is blocked.
        pathing.set_blocked(5, 10);
        let mut w = world(pathing);
        w.order_move(7, 0, [10.0, 20.0]).unwrap();
        run(&mut w, 10.0);
        assert!(!near(w.units[1].position, [11.0, 20.0]));
        w.order_move(7, 0, [10.0, 30.0]).unwrap();
        run(&mut w, 10.0);
        assert!(near(w.units[0].position, [9.0, 30.0]));
        assert!(near(w.units[1].position, [11.0, 30.0]));
    }

    #[test]
    fn changing_formation_rearranges_them_where_they_stand() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        w.set_formation(7, 0, 1).unwrap();
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [10.0, 11.0]));
        assert!(near(w.units[1].position, [10.0, 9.0]));
        assert!(near(w.units[1].facing, [0.0, 1.0]));
    }

    #[test]
    fn units_go_around_what_blocks_them() {
        let mut pathing = Pathing::uniform(20, Domain::Land);
        for x in 0..15 {
            pathing.set_blocked(x, 8);
        }
        let mut w = world(pathing.clone());
        w.order_move(7, 0, [10.0, 30.0]).unwrap();
        for _ in 0..400 {
            w.tick();
            for u in &w.units {
                let (x, y) = tile_at(u.position);
                assert!(pathing.passable(x, y, Domain::Land), "{:?}", u.position);
            }
        }
        assert!(near(w.units[0].position, [9.0, 30.0]));
    }

    #[test]
    fn regiments_only_take_orders_from_their_owner() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        assert_eq!(w.order_move(8, 0, [10.0, 20.0]), Err(NotYours));
        assert_eq!(w.set_formation(8, 0, 1), Err(NotYours));
        run(&mut w, 5.0);
        assert!(near(w.units[0].position, [9.0, 10.0]));
        assert_eq!(w.regiments[0].formation, 0);
    }

    fn rifle(range: f32, amount: f32) -> Weapon {
        Weapon {
            range,
            min_range: 0.0,
            recharge: 2.0,
            area: 0.0,
            full_within: 1.0,
            edge_damage: 1.0,
            targets: aaa_map::Targets {
                required: vec![Id::from("enemy")],
                excluded: vec![Id::from("tank")],
            },
            damage: vec![aaa_map::Damage {
                kind: Id::from("gunfire"),
                amount,
            }],
            animation: None,
        }
    }

    fn soldier(weapons: Vec<Weapon>) -> Fighter {
        Fighter {
            health: 100.0,
            defense: 0.0,
            resists: HashMap::new(),
            flags: vec![Id::from("infantry")],
            weapons,
        }
    }

    /// A regiment for each of `owners`, of one fighter each, the first at
    /// 10,10 and each next 10 further along y.
    fn battle(owners: &[u32], fighters: Vec<Fighter>, sides: Sides) -> World {
        let regiments = owners
            .iter()
            .zip(fighters)
            .enumerate()
            .map(|(i, (&owner, fighter))| {
                let mut m = member(i as u32 * 2 + 2, [10.0, 10.0 + 10.0 * i as f32], 2.0);
                m.fighter = Some(fighter);
                aaa_map::Regiment {
                    id: i as u32 * 2 + 1,
                    template: "squad".into(),
                    owner,
                    facing: [0.0, 1.0],
                    members: vec![m],
                    formations: vec![aaa_map::Formation {
                        id: "line".into(),
                        offsets: vec![[0.0, 0.0]],
                    }],
                    formation: 0,
                    primary: None,
                }
            })
            .collect::<Vec<_>>();
        World::from_regiments(sides, Pathing::uniform(40, Domain::Land), &regiments)
    }

    /// Players 1 and 2 at war, and 3 allied to 1.
    fn at_war() -> Sides {
        Sides {
            players: vec![
                player(1, None, &[0, 3, 1], &[]),
                player(2, None, &[3, 0, 3], &[]),
                player(3, None, &[1, 3, 0], &[]),
            ],
            teams: Vec::new(),
        }
    }

    #[test]
    fn enemies_in_range_fire_on_each_other_until_one_dies() {
        let mut w = battle(
            &[1, 2],
            vec![
                soldier(vec![rifle(12.0, 30.0)]),
                soldier(vec![rifle(12.0, 20.0)]),
            ],
            at_war(),
        );
        w.tick();
        assert_eq!(w.units[0].target, Some(1));
        assert_eq!(w.units[0].health, 80.0);
        assert_eq!(w.units[1].health, 70.0);
        // Every 2s: the second is down to 10 after three volleys and dies
        // to the fourth, while the first has taken 80.
        run(&mut w, 6.0);
        assert!(w.units[0].alive() && !w.units[1].alive());
        assert_eq!(w.units[0].health, 20.0);
        assert!(w.destroyed(1) && !w.destroyed(0));
        assert_eq!(w.units[0].target, None);
        assert_eq!(w.order_move(2, 1, [0.0, 0.0]), Err(NotYours));
    }

    #[test]
    fn weapons_hold_fire_on_friends_what_they_exclude_and_what_is_out_of_range() {
        let mut tank = soldier(vec![rifle(12.0, 30.0)]);
        tank.flags = vec![Id::from("tank")];
        // 1 and 3 are allies, 2 a tank the rifles can't hurt, and 4 out of
        // the rifles' range.
        let mut w = battle(
            &[1, 3, 2, 2],
            vec![
                soldier(vec![rifle(12.0, 30.0)]),
                soldier(vec![rifle(12.0, 30.0)]),
                tank,
                soldier(Vec::new()),
            ],
            at_war(),
        );
        run(&mut w, 10.0);
        assert_eq!(w.units[2].health, 100.0);
        assert_eq!(w.units[3].health, 100.0);
        // The tank's own rifle has no such limit.
        assert!(!w.units[1].alive());
    }

    #[test]
    fn defence_then_resistance_soften_hits() {
        let mut tank = soldier(Vec::new());
        tank.defense = 6.0;
        tank.resists = [(Id::from("gunfire"), 0.6)].into();
        // (12 - 6) * 0.4
        assert!((damage(&rifle(12.0, 12.0), &tank, 1.0) - 2.4).abs() < 1e-5);
        assert_eq!(damage(&rifle(12.0, 5.0), &tank, 1.0), 1.0);
        assert!((damage(&rifle(12.0, 50.0), &tank, 0.5) - 7.6).abs() < 1e-5);
    }

    #[test]
    fn area_shots_fall_off_toward_their_edge() {
        let shell = Weapon {
            area: 6.0,
            full_within: 2.0,
            edge_damage: 0.5,
            ..rifle(30.0, 40.0)
        };
        assert_eq!(falloff(&shell, 1.0), 1.0);
        assert_eq!(falloff(&shell, 4.0), 0.75);
        assert_eq!(falloff(&shell, 6.0), 0.5);
    }

    fn player(id: u32, team: Option<usize>, relations: &[u32], team_relations: &[u32]) -> Player {
        let stored = |r: &[u32]| r.iter().map(|&r| Relation::from_u32(r).unwrap()).collect();
        Player {
            id,
            name: format!("kingdom_{id}"),
            color: String::new(),
            rgb: [1.0; 3],
            team,
            independent: team.is_none(),
            playable: team.is_some(),
            relations: stored(relations),
            team_relations: stored(team_relations),
        }
    }

    #[test]
    fn teams_decide_relations_for_their_players() {
        let team = |id, relations: [Relation; 4], team_relations| Team {
            id,
            name: String::new(),
            relations: relations.into(),
            team_relations,
        };
        use Relation::{Ally, Enemy, Neutral};
        // Two players a team, at odds with each other as they're stored,
        // and civilians neutral to everyone.
        let sides = Sides {
            players: vec![
                player(1, Some(0), &[0, 3, 3, 3], &[3, 3]),
                player(2, Some(0), &[3, 0, 3, 3], &[3, 3]),
                player(3, Some(1), &[3, 3, 0, 3], &[3, 3]),
                player(4, None, &[2, 2, 2, 0], &[2, 2]),
            ],
            teams: vec![
                team(10, [Enemy, Enemy, Enemy, Neutral], vec![Ally, Enemy]),
                team(11, [Enemy, Enemy, Enemy, Neutral], vec![Enemy, Ally]),
            ],
        };
        assert_eq!(sides.relation(1, 1), Relation::Itself);
        assert_eq!(sides.relation(1, 2), Ally);
        assert_eq!(sides.relation(2, 3), Enemy);
        assert_eq!(sides.relation(3, 4), Neutral);
        assert_eq!(sides.relation(4, 1), Neutral);
        assert_eq!(sides.relation(1, 99), Neutral);
        assert_eq!(sides.first_playable().map(|p| p.id), Some(1));
    }

    #[test]
    fn players_without_teams_keep_their_own_relations() {
        let sides = Sides {
            players: vec![
                player(1, None, &[0, 1, 3], &[]),
                player(2, None, &[1, 0, 3], &[]),
                player(3, None, &[2, 3, 0], &[]),
            ],
            teams: Vec::new(),
        };
        assert_eq!(sides.relation(1, 2), Relation::Ally);
        assert_eq!(sides.relation(1, 3), Relation::Enemy);
        assert_eq!(sides.relation(3, 1), Relation::Neutral);
    }
}
