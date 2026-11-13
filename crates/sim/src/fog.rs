//! Fog of war: which tiles each side sees now, and which it has ever
//! seen.

use std::collections::{BTreeMap, BTreeSet};

use aaa_map::{FogSetting, Id, Relation, Tile};

use crate::{Enemy, Target, World, distance};

/// How a tile stands for a side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Seen {
    /// Never seen.
    Unexplored,
    /// Seen before, not now.
    Explored,
    /// Seen now.
    Visible,
}

/// Each side's view of the map, kept by tile.
#[derive(Debug, Clone)]
pub struct Fog {
    pub setting: FogSetting,
    /// Tiles per side of the map.
    side: usize,
    /// By player id.
    sides: BTreeMap<u32, SideFog>,
}

#[derive(Debug, Clone)]
struct SideFog {
    /// Row-major, whether something of the side sees each tile.
    now: Vec<bool>,
    /// Whether the side has ever seen each tile.
    ever: Vec<bool>,
    /// Whether the side sees each tile through what it fights, which
    /// doesn't explore it.
    remote: Vec<bool>,
}

impl SideFog {
    fn new(tiles: usize, explored: bool) -> Self {
        SideFog {
            now: vec![false; tiles],
            ever: vec![explored; tiles],
            remote: vec![false; tiles],
        }
    }

    fn seen(&self, i: usize) -> Seen {
        match (self.now[i] || self.remote[i], self.ever[i]) {
            (true, _) => Seen::Visible,
            (false, true) => Seen::Explored,
            (false, false) => Seen::Unexplored,
        }
    }
}

/// A radius pushed out to a quarter tile past the whole tiles it spans,
/// unless it's already that far past them.
fn padded(radius: f32) -> f32 {
    const TILE: f32 = aaa_map::pathing::CELL_SIZE;
    let tiles = radius / TILE;
    match tiles.fract() < 0.25 {
        true => (tiles.floor() + 0.25) * TILE,
        false => radius,
    }
}

impl Fog {
    pub(crate) fn new(side: usize, setting: FogSetting) -> Self {
        Fog {
            setting,
            side,
            sides: BTreeMap::new(),
        }
    }

    fn side_mut(&mut self, player: u32) -> &mut SideFog {
        let tiles = self.side * self.side;
        let explored = self.setting == FogSetting::Explored;
        (self.sides)
            .entry(player)
            .or_insert_with(|| SideFog::new(tiles, explored))
    }

    /// Marks the tiles whose centres lie within `radius` of the centre of
    /// the tile under `at`, in `layer` of `player`'s view.
    fn stamp(&mut self, player: u32, at: [f32; 2], radius: f32, remote: bool) {
        const TILE: f32 = aaa_map::pathing::CELL_SIZE;
        let reach = padded(radius) / TILE;
        let centre = Tile::at(at);
        let n = reach.floor() as i32;
        let side = self.side as i32;
        let fog = self.side_mut(player);
        for dy in -n..=n {
            let y = centre.y + dy;
            if !(0..side).contains(&y) {
                continue;
            }
            let across = (reach * reach - (dy * dy) as f32).max(0.0).sqrt().floor() as i32;
            for x in (centre.x - across).max(0)..=(centre.x + across).min(side - 1) {
                let i = y as usize * side as usize + x as usize;
                match remote {
                    true => fog.remote[i] = true,
                    false => {
                        fog.now[i] = true;
                        fog.ever[i] = true;
                    }
                }
            }
        }
    }

    /// How a tile stands for `player` and the sides it shares sight with,
    /// out of `allies`.
    fn seen_by(&self, allies: &[u32], i: usize) -> Seen {
        if self.setting == FogSetting::None {
            return Seen::Visible;
        }
        let explored = self.setting == FogSetting::Explored;
        let seen = allies
            .iter()
            .filter_map(|p| self.sides.get(p))
            .map(|f| f.seen(i));
        let floor = match explored {
            true => Seen::Explored,
            false => Seen::Unexplored,
        };
        seen.max().unwrap_or(floor).max(floor)
    }
}

impl World {
    /// Starts the fog over with `setting`, every side seeing what its
    /// things see now.
    pub fn set_fog(&mut self, setting: FogSetting) {
        self.fog = Fog::new(self.pathing.side, setting);
        self.look();
    }

    /// The players whose sight `player` shares: itself and its allies.
    pub fn sharing_sight(&self, player: u32) -> Vec<u32> {
        let mut sharing = vec![player];
        let players = self.sides.players.iter().map(|p| p.id);
        sharing.extend(
            players.filter(|&p| p != player && self.sides.relation(player, p) == Relation::Ally),
        );
        sharing
    }

    /// How the tile under `at` stands for `player`.
    pub fn seen(&self, player: u32, at: [f32; 2]) -> Seen {
        self.seen_with(&self.sharing_sight(player), at)
    }

    /// How the tile under `at` stands for the sides `allies`, sharing sight.
    fn seen_with(&self, allies: &[u32], at: [f32; 2]) -> Seen {
        let index = Tile::at(at).index(self.fog.side);
        index.map_or(Seen::Unexplored, |i| self.fog.seen_by(allies, i))
    }

    /// How every tile stands for `player`, row-major from the south-west
    /// corner.
    pub fn view(&self, player: u32) -> Vec<Seen> {
        let allies = self.sharing_sight(player);
        let tiles = self.fog.side * self.fog.side;
        (0..tiles).map(|i| self.fog.seen_by(&allies, i)).collect()
    }

    /// Whether `player` sees the unit at `u`: its own and its allies'
    /// always, others' while it's, or its regiment is, on ground it sees.
    pub fn sees_unit(&self, player: u32, u: usize) -> bool {
        let r = self.units[u].regiment;
        let members = self.members_seen(&self.sharing_sight(player), r);
        let place = self.regiments[r].units.iter().position(|&m| m == u);
        place.is_some_and(|i| members[i])
    }

    /// Whether `player` sees each unit, as [`World::sees_unit`] has it.
    pub fn sees_units(&self, player: u32) -> Vec<bool> {
        let allies = self.sharing_sight(player);
        let mut seen = vec![false; self.units.len()];
        for r in 0..self.regiments.len() {
            let members = self.members_seen(&allies, r);
            for (&u, shown) in self.regiments[r].units.iter().zip(members) {
                seen[u] = shown;
            }
        }
        seen
    }

    /// Whether the sides `allies` see each member of the regiment at `r`,
    /// in its order.
    fn members_seen(&self, allies: &[u32], r: usize) -> Vec<bool> {
        let regiment = &self.regiments[r];
        if self.fog.setting == FogSetting::None || allies.contains(&regiment.owner) {
            return vec![true; regiment.units.len()];
        }
        let from = self.leader_position(r);
        let strayed: Vec<bool> = (regiment.units.iter())
            .map(|&u| self.strays(r, u, from))
            .collect();
        let visible: Vec<bool> = (regiment.units.iter())
            .map(|&u| {
                let unit = &self.units[u];
                unit.alive() && self.seen_with(allies, unit.position) == Seen::Visible
            })
            .collect();
        // Those with the regiment show while any of them does.
        let with = (strayed.iter().zip(&visible)).any(|(&strayed, &visible)| !strayed && visible);
        (strayed.iter().zip(&visible))
            .map(|(&strayed, &visible)| visible || (!strayed && with))
            .collect()
    }

    /// Whether `player` sees the building at `b`: its own and its allies'
    /// always, others' while any ground under it is seen.
    pub fn sees_building(&self, player: u32, b: usize) -> bool {
        let building = &self.buildings[b];
        if self.fog.setting == FogSetting::None
            || self.sharing_sight(player).contains(&building.owner)
        {
            return true;
        }
        self.building_seen(player, b) == Seen::Visible
    }

    /// How the best-seen tile under the building at `b` stands for
    /// `player`.
    pub fn building_seen(&self, player: u32, b: usize) -> Seen {
        let building = &self.buildings[b];
        let allies = self.sharing_sight(player);
        let tiles: Vec<Tile> = match &building.obstacle {
            Some(o) => o.footprint().collect(),
            None => vec![Tile::at(building.position)],
        };
        let seen = tiles.into_iter().filter_map(|t| t.index(self.fog.side));
        seen.map(|i| self.fog.seen_by(&allies, i))
            .max()
            .unwrap_or(Seen::Unexplored)
    }

    fn sight_of(&self, template: &str) -> Option<&aaa_map::Sight> {
        self.sights.get(&Id::from(template))
    }

    /// Whether the unit at `u` has left the zone of control of its
    /// regiment, at `r` and led `from`, and so sees, and is seen, on its
    /// own. Without a zone there's nothing to stray from.
    pub(crate) fn strays(&self, r: usize, u: usize, from: [f32; 2]) -> bool {
        let regiment = &self.regiments[r];
        let leader = regiment.leader.map(|p| regiment.units[p]);
        if regiment.units.len() < 2
            || leader == Some(u)
            || regiment.control(&self.guard_rules) <= 0.0
        {
            return false;
        }
        distance(self.units[u].position, from) > regiment.control(&self.guard_rules)
    }

    /// Lays each side's sight afresh from what its things see now.
    pub(crate) fn look(&mut self) {
        if self.fog.setting == FogSetting::None {
            return;
        }
        for p in self.sides.players.iter().map(|p| p.id) {
            self.fog.side_mut(p);
        }
        for fog in self.fog.sides.values_mut() {
            fog.now.fill(false);
            fog.remote.fill(false);
        }
        let mut discs: Vec<(u32, [f32; 2], f32)> = Vec::new();
        let mut remote: Vec<(u32, [f32; 2], f32)> = Vec::new();
        for r in 0..self.regiments.len() {
            if self.destroyed(r) {
                continue;
            }
            let regiment = &self.regiments[r];
            let owner = regiment.owner;
            let at = self.leader_position(r);
            let present = |u: &&usize| self.units[**u].alive();
            if !regiment.units.iter().any(|u| present(&u)) {
                continue;
            }
            let sight = self.sight_of(&regiment.template);
            if let Some(s) = sight {
                discs.push((owner, at, s.range));
            }
            // Those that have strayed from it see for themselves.
            for &u in regiment.units.iter().filter(present) {
                if self.strays(r, u, at)
                    && let Some(s) = self.sight_of(&self.units[u].template)
                {
                    discs.push((owner, self.units[u].position, s.range));
                }
            }
            if let Some(s) = sight.filter(|s| s.remote > 0.0) {
                for p in self.regiment_shown_to(r) {
                    remote.push((p, at, s.remote));
                }
            }
        }
        for b in 0..self.buildings.len() {
            let building = &self.buildings[b];
            if building.gone() || building.unpacking.is_some() {
                continue;
            }
            let Some(s) = self.sight_of(&building.template) else {
                continue;
            };
            discs.push((building.owner, building.position, s.range));
            if s.remote > 0.0 {
                for p in self.building_shown_to(b) {
                    remote.push((p, building.position, s.remote));
                }
            }
        }
        for (p, at, radius) in discs {
            self.fog.stamp(p, at, radius, false);
        }
        for (p, at, radius) in remote {
            self.fog.stamp(p, at, radius, true);
        }
    }

    /// The owner of what a regiment or building engages.
    fn enemy_owner(&self, e: Enemy) -> u32 {
        match e {
            Enemy::Regiment(r) => self.regiments[r].owner,
            Enemy::Building(b) => self.buildings[b].owner,
        }
    }

    fn target_owner(&self, t: Target) -> u32 {
        match t {
            Target::Unit(u) => self.regiments[self.units[u].regiment].owner,
            Target::Building(b) => self.buildings[b].owner,
        }
    }

    /// The sides other than its own fighting the regiment at `r`: those
    /// engaging or bombarding it, and those it engages or fires on.
    fn regiment_shown_to(&self, r: usize) -> BTreeSet<u32> {
        let regiment = &self.regiments[r];
        let mut sides = BTreeSet::new();
        let engaging = self
            .regiments
            .iter()
            .filter(|o| o.engaged.contains(&Enemy::Regiment(r)));
        sides.extend(engaging.map(|o| o.owner));
        let bombarding = regiment
            .bombarded
            .iter()
            .filter(|b| b.tick + b.length >= self.ticks);
        sides.extend(bombarding.map(|b| self.enemy_owner(b.by)));
        sides.extend(regiment.engaged.iter().map(|&e| self.enemy_owner(e)));
        let targets = regiment.units.iter().filter_map(|&u| self.units[u].target);
        sides.extend(targets.map(|t| self.target_owner(t)));
        sides.remove(&regiment.owner);
        sides
    }

    /// The sides other than its own fighting the building at `b`: those
    /// engaging it, and those it's aimed at.
    fn building_shown_to(&self, b: usize) -> BTreeSet<u32> {
        let building = &self.buildings[b];
        let mut sides = BTreeSet::new();
        let engaging = self
            .regiments
            .iter()
            .filter(|o| o.engaged.contains(&Enemy::Building(b)));
        sides.extend(engaging.map(|o| o.owner));
        sides.extend(building.target.map(|t| self.target_owner(t)));
        sides.remove(&building.owner);
        sides
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use aaa_map::{Domain, Fighter, Pathing};

    /// Players 1 and 2 at war, as `battle` has them, seeing `range` round
    /// themselves and showing those fighting them `remote` round them.
    fn sighted(fighters: Vec<Fighter>, range: f32, remote: f32, fog: aaa_map::FogSetting) -> World {
        let mut w = battle(&[1, 2], fighters, at_war());
        w.sights
            .insert(Id::from("squad"), aaa_map::Sight { range, remote });
        w.set_fog(fog);
        w
    }

    fn put(w: &mut World, u: usize, at: [f32; 2]) {
        w.units[u].position = at;
        w.units[u].previous = at;
    }

    #[test]
    fn radii_reach_a_quarter_tile_past_whole_tiles() {
        assert_eq!(padded(18.0), 18.5);
        assert_eq!(padded(26.0), 26.5);
        assert_eq!(padded(65.0), 65.0);
        assert_eq!(padded(18.4), 18.5);
        assert_eq!(padded(18.6), 18.6);
    }

    #[test]
    fn sides_and_their_allies_see_round_their_things_and_remember_it() {
        let mut w = sighted(
            vec![soldier(vec![]), soldier(vec![])],
            6.0,
            3.0,
            aaa_map::FogSetting::Full,
        );
        put(&mut w, 1, [70.0, 70.0]);
        w.tick();
        // Tile centres within 6.5 of the centre of the tile under it.
        assert_eq!(w.seen(1, [10.0, 10.0]), Seen::Visible);
        assert_eq!(w.seen(1, [10.0, 17.0]), Seen::Visible);
        assert_eq!(w.seen(1, [10.0, 19.0]), Seen::Unexplored);
        assert_eq!(w.seen(3, [10.0, 10.0]), Seen::Visible);
        assert_eq!(w.seen(2, [10.0, 10.0]), Seen::Unexplored);
        assert!(w.sees_unit(1, 0) && w.sees_unit(3, 0) && !w.sees_unit(2, 0));
        assert!(!w.sees_unit(1, 1));

        put(&mut w, 0, [40.0, 10.0]);
        w.tick();
        assert_eq!(w.seen(1, [10.0, 10.0]), Seen::Explored);
        assert_eq!(w.seen(1, [40.0, 10.0]), Seen::Visible);
        // Until the second comes near enough to be seen.
        put(&mut w, 1, [44.0, 10.0]);
        w.tick();
        assert!(w.sees_unit(1, 1) && w.sees_unit(2, 0));
    }

    #[test]
    fn the_setting_decides_what_starts_explored_or_is_never_hidden() {
        let w = sighted(
            vec![soldier(vec![]), soldier(vec![])],
            6.0,
            3.0,
            aaa_map::FogSetting::Explored,
        );
        assert_eq!(w.seen(1, [10.0, 10.0]), Seen::Visible);
        assert_eq!(w.seen(1, [70.0, 70.0]), Seen::Explored);
        let w = sighted(
            vec![soldier(vec![]), soldier(vec![])],
            6.0,
            3.0,
            aaa_map::FogSetting::None,
        );
        assert_eq!(w.seen(2, [70.0, 70.0]), Seen::Visible);
        assert!(w.sees_unit(2, 0));
    }

    #[test]
    fn what_fights_a_side_is_shown_it_without_exploring() {
        // Too short-sighted to see each other 10 apart, but firing on each
        // other shows each to the other.
        let mut w = sighted(
            vec![
                soldier(vec![rifle(12.0, 1.0)]),
                soldier(vec![rifle(12.0, 1.0)]),
            ],
            2.0,
            3.0,
            aaa_map::FogSetting::Full,
        );
        assert!(!w.sees_unit(2, 0));
        w.tick();
        assert!(w.sees_unit(2, 0) && w.sees_unit(1, 1));
        // Once they stop, it's as if never seen.
        for u in &mut w.units {
            u.fighter = None;
            u.target = None;
        }
        w.regiments.iter_mut().for_each(|r| r.engaged.clear());
        w.tick();
        assert_eq!(w.seen(2, [10.0, 10.0]), Seen::Unexplored);
    }

    #[test]
    fn units_stray_only_from_a_zone_of_control() {
        let mut w = world(Pathing::uniform(40, Domain::Land));
        put(&mut w, 1, [60.0, 60.0]);
        let from = w.units[0].position;
        assert!(!w.strays(0, 1, from));
        w.regiments[0].control_range = 10.0;
        assert!(w.strays(0, 1, from) && !w.strays(0, 0, from));
    }
}
