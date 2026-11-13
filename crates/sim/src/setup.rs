//! Setting a match up from its map, and adding to it as it's played.

use std::collections::{BTreeMap, HashMap, VecDeque};

use aaa_map::{Domain, Id, Map, Pathing, Role, TerrainIndex, Tile};

use crate::{
    Building, Fog, Order, Regiment, Rules, Sides, State, Supplying, Trailer, Treasury, TurretAim,
    Unit, World, normalised, wrap,
};

/// [`World::holdings`] from each placed object's role, owner and template.
/// Regiments and buildings count in their own right, and what the map
/// left out for lack of room counts for nothing.
pub(crate) fn holdings<'a>(
    things: impl IntoIterator<Item = (Role, Option<u32>, &'a str)>,
) -> Vec<(u32, Id)> {
    (things.into_iter())
        .filter(|(role, ..)| !matches!(role, Role::Crowded | Role::Regiment | Role::Building))
        .filter_map(|(_, owner, template)| Some((owner?, Id::from(template))))
        .collect()
}

impl World {
    pub fn new(map: &Map, rules: Rules) -> World {
        let sides = Sides {
            players: map.players.clone(),
            teams: map.teams.clone(),
        };
        let mut world = World::build(sides, map.pathing.clone(), rules, &map.regiments);
        world.add_buildings(&map.buildings);
        world.detach_from_missing();
        world.tile_terrains = map.tile_terrains.clone();
        world.scenery = map.scenery.clone();
        world.clearable = map.clearable.clone();
        world.holdings = holdings(
            map.roles()
                .map(|(t, role)| (role, aaa_map::owner(t), t.template.as_str())),
        );
        for p in &map.players {
            for t in &p.technologies {
                world.grant(p.id, t.clone());
            }
            world.treasuries.insert(
                p.id,
                Treasury {
                    stockpile: p.stockpile,
                    shortage: None,
                },
            );
        }
        let members = map
            .things
            .iter()
            .flat_map(|t| t.organization())
            .flat_map(|o| &o.members);
        let ids = map.things.iter().chain(members).map(|t| t.id);
        world.next_id = world.next_id.max(ids.max().map_or(1, |id| id + 1));
        world
    }

    /// A world of `regiments` played by the default [`Rules`].
    pub fn from_regiments(
        sides: Sides,
        pathing: Pathing,
        regiments: &[aaa_map::Regiment],
    ) -> World {
        World::build(sides, pathing, Rules::default(), regiments)
    }

    /// What the match is played by, fixed once it's begun.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    fn build(
        sides: Sides,
        pathing: Pathing,
        rules: Rules,
        regiments: &[aaa_map::Regiment],
    ) -> World {
        // Nothing's hidden until the match sets its fog.
        let fog = Fog::new(pathing.side, aaa_map::FogSetting::None);
        let mut world = World {
            sides,
            pathing,
            rules,
            tile_terrains: aaa_map::TileTerrains::default(),
            regiments: Vec::new(),
            units: Vec::new(),
            buildings: Vec::new(),
            scenery: Vec::new(),
            clearable: Vec::new(),
            cleared: Vec::new(),
            ticks: 0,
            loaded: false,
            next_id: 1,
            next_commission: 0,
            flying: Vec::new(),
            landings: Vec::new(),
            technologies: BTreeMap::new(),
            objectives: BTreeMap::new(),
            rng: 0x9e37_79b9_7f4a_7c15,
            holdings: Vec::new(),
            treasuries: BTreeMap::new(),
            commanders: BTreeMap::new(),
            notices: Vec::new(),
            fog,
            numbers: HashMap::new(),
        };
        for regiment in regiments {
            world.add_regiment(regiment);
        }
        world
    }

    /// Adds a regiment as the map has it, standing where its members do,
    /// returning its index.
    pub fn add_regiment(&mut self, regiment: &aaa_map::Regiment) -> usize {
        self.push_regiment(regiment, true)
    }

    /// Adds a regiment, numbering it if `numbered`.
    pub(crate) fn push_regiment(&mut self, regiment: &aaa_map::Regiment, numbered: bool) -> usize {
        let r = self.regiments.len();
        let first = self.units.len();
        for m in &regiment.members {
            let [x, y] = m.position;
            let facing = normalised(m.facing).unwrap_or([0.0, -1.0]);
            let tile = Tile::at([x, y]);
            // Units start on what they travel on.
            let domain = match self.pathing.passable(tile, Domain::Land)
                || !self.pathing.passable(tile, Domain::Naval)
            {
                true => Domain::Land,
                false => Domain::Naval,
            };
            let fighter = m.fighter.clone();
            self.units.push(Unit {
                health: fighter.as_ref().map_or(0.0, |f| f.health * m.health),
                reloads: fighter
                    .as_ref()
                    .map_or(Vec::new(), |f| vec![0.0; f.weapons.len()]),
                fighter,
                target: None,
                shot: None,
                died: None,
                besieged: None,
                left: None,
                // One its regiment has lost comes back as a recruit's do.
                awaited: m.lost.is_some(),
                terrain: TerrainIndex::default(),
                id: m.id,
                template: m.template.clone(),
                regiment: r,
                domain,
                position: [x, y],
                previous: [x, y],
                facing,
                shape: m.shape,
                end_facing: facing,
                path: Vec::new(),
                from: [x, y],
                leg: None,
                goal: None,
                exact: false,
                ways: 0,
                look: 0,
                turn_radius: match m.turn_rate > 0.0 {
                    true => m.speed / m.turn_rate,
                    false => 0.0,
                },
                speed: 0.0,
                acceleration: m.acceleration,
                turn_rate: m.turn_rate,
                trailer: m.trailer.as_ref().map(|t| Trailer::new(t, [x, y], facing)),
                closing: false,
            });
        }
        let units: Vec<usize> = (first..self.units.len()).collect();
        let position = match regiment.primary {
            Some(p) => self.units[first + p].position,
            None => self.average(&units),
        };
        let template = regiment.template.clone();
        let number = match numbered {
            true => self.next_number(regiment.owner, &template),
            false => None,
        };
        self.regiments.push(Regiment {
            id: regiment.id,
            template,
            number,
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
            combat: regiment.combat,
            operational_range: regiment.operational_range.unwrap_or(0.0),
            control_range: regiment.control_range.unwrap_or(0.0),
            order: Order::Move,
            engaged: Vec::new(),
            resume: None,
            unpacks: regiment.unpacks.clone(),
            unpack_at: None,
            unpack_again: None,
            unpacked_into: None,
            rally: None,
            leader: regiment.primary,
            hq: regiment.hq,
            // Until its first look, so it doesn't start out marked.
            in_supply: true,
            replacing: (regiment.members.iter())
                .map(|m| m.lost.unwrap_or(0.0))
                .collect(),
            bombarding: false,
            bombarded: Vec::new(),
            properties: Vec::new(),
            guard: None,
            terrain: TerrainIndex::default(),
            terrain_properties: Vec::new(),
        });
        let ids = regiment.members.iter().map(|m| m.id);
        self.next_id = ids
            .chain([regiment.id])
            .fold(self.next_id, |n, id| n.max(id + 1));
        r
    }

    /// The next number `owner` gives a regiment of `template`, if it's
    /// numbered and a side's.
    fn next_number(&mut self, owner: u32, template: &Id) -> Option<u32> {
        if !self.rules.numbered.contains(template) || self.sides.player(owner).is_none() {
            return None;
        }
        let n = self.numbers.entry((owner, template.clone())).or_insert(0);
        *n += 1;
        Some(*n)
    }

    /// The next number for the organization the building at `b` stands in.
    fn building_number(&mut self, b: usize) -> Option<u32> {
        let building = &self.buildings[b];
        let template = building.organization.as_ref()?.template.clone();
        self.next_number(building.owner, &template)
    }

    /// Takes `n` ids no object has, returning the first.
    pub fn take_ids(&mut self, n: u32) -> u32 {
        let first = self.next_id;
        self.next_id += n;
        first
    }

    /// Adds buildings as the map has them, those out of health already
    /// disabled. Their ground is taken to be blocked already.
    pub fn add_buildings(&mut self, buildings: &[aaa_map::Building]) {
        self.push_buildings(buildings, true);
    }

    /// Adds buildings, numbering them if `numbered`.
    pub(crate) fn push_buildings(&mut self, buildings: &[aaa_map::Building], numbered: bool) {
        for b in buildings {
            let health = b.fighter.health * b.health;
            self.buildings.push(Building {
                id: b.id,
                template: b.template.clone(),
                number: None,
                owner: b.owner,
                position: b.position,
                facing: b.facing,
                reloads: vec![0.0; b.fighter.weapons.len()],
                turrets: (b.fighter.turrets.iter())
                    .map(|t| TurretAim {
                        yaw: wrap(t.start),
                        idle: 0.0,
                    })
                    .collect(),
                fighter: b.fighter.clone(),
                health,
                target: None,
                noticed: Vec::new(),
                awake: b.awake,
                inside: HashMap::new(),
                besieged: None,
                shot: None,
                state: match health > 0.0 {
                    true => State::Standing,
                    false => State::Disabled,
                },
                captured: None,
                unpacked_from: None,
                zone: b.zone,
                members: b.members.clone(),
                obstacle: b.obstacle.clone(),
                unpacking: None,
                packing: None,
                queue: VecDeque::new(),
                progress: 0,
                paused: None,
                deploy: b.deploy,
                organization: b.organization.clone(),
                rally: None,
                supply: Supplying::default(),
                properties: Vec::new(),
                created: self.ticks,
            });
            self.next_id = self.next_id.max(b.id + 1);
            if numbered {
                let at = self.buildings.len() - 1;
                self.buildings[at].number = self.building_number(at);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;

    use std::collections::HashSet;

    #[test]
    fn what_a_map_leaves_out_for_lack_of_room_adds_nothing() {
        let mut w = economy();
        w.rules.economies.insert(
            Id::from("start_loc"),
            aaa_map::Economy {
                production: amounts(&[("building_limit_provided", 20.0)]),
                ..Default::default()
            },
        );
        let before = w.balance(1);
        w.holdings = holdings([
            (Role::Crowded, Some(1), "start_loc"),
            (Role::Other, None, "start_loc"),
        ]);
        assert!(w.holdings.is_empty());
        assert_eq!(w.balance(1), before);
        w.holdings = holdings([(Role::Other, Some(1), "start_loc")]);
        assert_ne!(w.balance(1), before);
    }

    #[test]
    fn numbered_regiments_count_per_side_and_template_never_reusing_one() {
        let mut w = headquarters();
        w.rules.numbered = HashSet::from([Id::from("truck"), Id::from("tank")]);
        let mut t = truck();
        let mut add = |w: &mut World, owner, template: &str| {
            (t.owner, t.template) = (owner, template.into());
            let r = w.add_regiment(&t);
            w.regiments[r].number
        };
        assert_eq!(add(&mut w, 1, "truck"), Some(1));
        assert_eq!(add(&mut w, 1, "truck"), Some(2));
        assert_eq!(add(&mut w, 2, "truck"), Some(1));
        assert_eq!(add(&mut w, 1, "tank"), Some(1));
        assert_eq!(add(&mut w, 1, "jeep"), None);
        // Nobody's aren't numbered.
        assert_eq!(add(&mut w, 0, "truck"), None);
        w.destroy(1);
        assert_eq!(add(&mut w, 1, "truck"), Some(3));
    }
}
