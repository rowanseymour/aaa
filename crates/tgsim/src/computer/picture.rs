//! What a computer player makes of the match when it plans: its own
//! things, the enemy things it knows of, and how strong each side is in
//! each region.

use std::collections::HashMap;

use tgmap::{Id, Relation};

use crate::{Piece, Seen, State, Target, World};

/// How far a unit's strength is felt, in seconds of travel at its speed.
const INFLUENCE_SECONDS: f32 = 45.0;

/// The fastest speed a unit's strength is spread at.
const SPEED_CAP: f32 = 10.0;

/// How a thing's strength weakens out to its reach.
const INFLUENCE_EXPONENT: f32 = 1.5;

/// The share of the weakening left at the edge of a thing's reach,
/// before the exponent.
const INFLUENCE_EDGE: f32 = 0.2;

/// How much further than its range a building's strength is weighed as
/// reaching, so that it's still strong at the edge of its range.
const BUILDING_FUDGE: f32 = 1.7;

/// How far a building's strength is felt for the border.
const BORDER_REACH: f32 = 300.0;

/// An enemy thing a computer player knows of: one it sees, or saw.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Known {
    pub piece: Piece,
    /// Where it is, or was last seen.
    pub at: [f32; 2],
    pub seen: bool,
}

/// One planning think's view of the match for side `player`.
pub(super) struct Picture<'w> {
    pub world: &'w World,
    pub player: u32,
    /// Its own organizations still in the match that it may order.
    pub organizations: Vec<usize>,
    /// Its own buildings standing.
    pub buildings: Vec<usize>,
    /// The enemy things it knows of.
    pub known: Vec<Known>,
    /// Each region's strength, of its side and allies, and of its enemies:
    /// that of units, and of buildings.
    pub friendly: Vec<f32>,
    pub enemy: Vec<f32>,
    pub friendly_static: Vec<f32>,
    pub enemy_static: Vec<f32>,
    /// Each region's strength of buildings felt further off, which marks
    /// borders.
    pub enemy_border: Vec<f32>,
    /// Where the side's economy stands.
    pub balance: crate::Balance,
    /// Region-to-region travel, worked out as it's asked for, and that over
    /// land alone.
    travel: std::cell::RefCell<HashMap<usize, std::rc::Rc<Vec<f32>>>>,
    land: std::cell::RefCell<HashMap<usize, std::rc::Rc<Vec<f32>>>>,
}

impl<'w> Picture<'w> {
    /// The match as `player` sees it, with `memory` the enemy things it saw
    /// before, which this brings up to date.
    pub fn new(world: &'w World, player: u32, memory: &mut Vec<Known>) -> Picture<'w> {
        let organizations: Vec<usize> = (0..world.organizations.len())
            .filter(|&r| {
                let o = &world.organizations[r];
                o.owner == player
                    && !o.arriving
                    && !world.destroyed(r)
                    && o.unpacked_into.is_none()
                    && world.computer_may_order(r)
            })
            .collect();
        let buildings: Vec<usize> = (0..world.buildings.len())
            .filter(|&b| {
                let building = &world.buildings[b];
                building.owner == player && building.state == State::Standing
            })
            .collect();
        let regions = world.regions.regions.len();
        let mut picture = Picture {
            world,
            player,
            organizations,
            buildings,
            known: Vec::new(),
            friendly: vec![0.0; regions],
            enemy: vec![0.0; regions],
            friendly_static: vec![0.0; regions],
            enemy_static: vec![0.0; regions],
            enemy_border: vec![0.0; regions],
            balance: world.balance(player),
            travel: Default::default(),
            land: Default::default(),
        };
        picture.weigh();
        picture.know(memory);
        picture
    }

    pub fn enemy_of(&self, other: u32) -> bool {
        self.world.sides.relation(self.player, other) == Relation::Enemy
    }

    fn friend_of(&self, other: u32) -> bool {
        matches!(
            self.world.sides.relation(self.player, other),
            Relation::Itself | Relation::Ally
        )
    }

    /// Spreads every unit's and building's strength over the regions
    /// around it, seen or not.
    fn weigh(&mut self) {
        let world = self.world;
        for r in 0..world.organizations.len() {
            let o = &world.organizations[r];
            if o.arriving || o.unpacked_into.is_some() || world.destroyed(r) {
                continue;
            }
            let (friend, enemy) = (self.friend_of(o.owner), self.enemy_of(o.owner));
            let value = world.full_value(r);
            let region = world.regions.at(world.centre(r));
            let (Some(region), true, true) = (region, friend || enemy, value > 0.0) else {
                continue;
            };
            let reach = o.speed.min(SPEED_CAP) * INFLUENCE_SECONDS;
            let travel = self.land_travel(region);
            let into = if friend {
                &mut self.friendly
            } else {
                &mut self.enemy
            };
            spread(into, &travel, value, reach, reach);
        }
        for b in 0..world.buildings.len() {
            let building = &world.buildings[b];
            if building.state != State::Standing {
                continue;
            }
            let (friend, enemy) = (
                self.friend_of(building.owner),
                self.enemy_of(building.owner),
            );
            let value = world.target_value(Target::Building(b));
            let region = world.regions.at(building.position);
            let (Some(region), true, true) = (region, friend || enemy, value > 0.0) else {
                continue;
            };
            let range = (world.rules.guard_zones.get(&building.template))
                .copied()
                .unwrap_or(0.0);
            let travel = self.land_travel(region);
            let into = if friend {
                &mut self.friendly_static
            } else {
                &mut self.enemy_static
            };
            spread(into, &travel, value, range, range * BUILDING_FUDGE);
            if enemy {
                spread(
                    &mut self.enemy_border,
                    &travel,
                    value,
                    BORDER_REACH,
                    BORDER_REACH,
                );
            }
        }
    }

    /// Travel from region `from` over land only, which strength spreads
    /// across.
    fn land_travel(&self, from: usize) -> std::rc::Rc<Vec<f32>> {
        let mut cache = self.land.borrow_mut();
        let regions = &self.world.regions;
        let out = cache.entry(from).or_insert_with(|| {
            let land = |r: usize| regions.regions[r].island.is_some();
            regions.travel_through(from, land).into()
        });
        out.clone()
    }

    /// Brings `memory` up to date with what the side sees now, and takes
    /// what it knows from it.
    fn know(&mut self, memory: &mut Vec<Known>) {
        let world = self.world;
        let seen_units = world.sees_units(self.player);
        let mut now = Vec::new();
        for r in 0..world.organizations.len() {
            let o = &world.organizations[r];
            if o.arriving || o.unpacked_into.is_some() || !self.enemy_of(o.owner) {
                continue;
            }
            if world.destroyed(r) {
                continue;
            }
            if world.survivors(r).any(|u| seen_units[u]) {
                now.push(Known {
                    piece: Piece::Organization(r),
                    at: world.centre(r),
                    seen: true,
                });
            }
        }
        for b in 0..world.buildings.len() {
            let building = &world.buildings[b];
            if building.state != State::Standing || !self.enemy_of(building.owner) {
                continue;
            }
            // One on ground it has explored, as it shows a person under
            // the fog.
            if world.building_seen(self.player, b) != Seen::Unexplored {
                now.push(Known {
                    piece: Piece::Building(b),
                    at: building.position,
                    seen: world.sees_building(self.player, b),
                });
            }
        }
        // What it saw before and doesn't now, while it's still there and
        // still an enemy's.
        for old in memory.iter() {
            if now.iter().any(|k| k.piece == old.piece) || !self.still_enemy(old.piece) {
                continue;
            }
            now.push(Known {
                seen: false,
                ..old.clone()
            });
        }
        *memory = now.clone();
        self.known = now;
    }

    fn still_enemy(&self, piece: Piece) -> bool {
        let world = self.world;
        match piece {
            Piece::Organization(r) => {
                !world.destroyed(r) && self.enemy_of(world.organizations[r].owner)
            }
            Piece::Building(b) => {
                let building = &world.buildings[b];
                building.state == State::Standing && self.enemy_of(building.owner)
            }
        }
    }

    /// Travel from region `from` to each region.
    pub fn travel(&self, from: usize) -> std::rc::Rc<Vec<f32>> {
        let mut cache = self.travel.borrow_mut();
        let out = cache
            .entry(from)
            .or_insert_with(|| self.world.regions.travel(from).into());
        out.clone()
    }

    /// The region `at` is in.
    pub fn region(&self, at: [f32; 2]) -> Option<usize> {
        self.world.regions.at(at)
    }

    /// Where a piece stands.
    pub fn position(&self, piece: Piece) -> [f32; 2] {
        match piece {
            Piece::Organization(r) => self.world.centre(r),
            Piece::Building(b) => self.world.buildings[b].position,
        }
    }

    /// The templates a piece is made of, whose properties are its own: an
    /// organization's and its members', but not what it unpacks into; a
    /// building's.
    pub fn templates(&self, piece: Piece) -> Vec<&'w Id> {
        let world = self.world;
        match piece {
            Piece::Organization(r) => {
                let o = &world.organizations[r];
                let mut out = vec![&o.template];
                for u in world.survivors(r) {
                    out.push(&world.units[u].template);
                }
                out
            }
            Piece::Building(b) => vec![&world.buildings[b].template],
        }
    }

    /// What a piece unpacks into, for a packed building.
    pub fn unpacks(&self, piece: Piece) -> Option<&'w Id> {
        match piece {
            Piece::Organization(r) => {
                let o = &self.world.organizations[r];
                o.unpacks.as_ref().map(|u| &u.building.template)
            }
            Piece::Building(_) => None,
        }
    }

    /// Whether an organization fights: it has strength and isn't a packed
    /// building.
    pub fn fights(&self, r: usize) -> bool {
        let o = &self.world.organizations[r];
        o.unpacks.is_none() && self.world.full_value(r) > 0.0
    }

    /// An organization's share of its health left, over its survivors.
    pub fn health(&self, r: usize) -> f32 {
        let world = self.world;
        let (have, most) = world.survivors(r).fold((0.0, 0.0), |(h, m), u| {
            (h + world.units[u].health, m + world.max_health(u))
        });
        if most > 0.0 { have / most } else { 1.0 }
    }

    /// An organization's share of its morale left, for one that has any.
    pub fn morale(&self, r: usize) -> f32 {
        let o = &self.world.organizations[r];
        if o.morale_max > 0.0 {
            o.morale / o.morale_max
        } else {
            1.0
        }
    }

    /// The side's net money a minute.
    pub fn money_rate(&self) -> f32 {
        let money = crate::economy::money(&self.world.rules.economy);
        self.balance.net.get(money)
    }
}

/// Adds `value` to its own region, the one at no distance, and to each
/// region less than `reach` away weakened as if its reach were `weighed`.
fn spread(into: &mut [f32], travel: &[f32], value: f32, reach: f32, weighed: f32) {
    for (region, &d) in travel.iter().enumerate() {
        let share = match d {
            0.0 => 1.0,
            d if d < reach && weighed > 0.0 => libm::powf(
                1.0 - (1.0 - INFLUENCE_EDGE) * d / weighed,
                INFLUENCE_EXPONENT,
            ),
            _ => continue,
        };
        into[region] += value * share;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn strength_weakens_to_the_edge_of_its_reach() {
        let mut into = [0.0; 4];
        super::spread(
            &mut into,
            &[0.0, 50.0, 100.0, f32::INFINITY],
            100.0,
            100.0,
            100.0,
        );
        assert_eq!(into[0], 100.0);
        assert!((into[1] - 100.0 * libm::powf(0.6, 1.5)).abs() < 1e-3);
        assert_eq!(&into[2..], [0.0, 0.0]);
        // A building's is weighed as reaching further than it does.
        let mut into = [0.0; 2];
        super::spread(&mut into, &[0.0, 10.0], 100.0, 10.0, 17.0);
        assert_eq!(into[1], 0.0);
    }
}
