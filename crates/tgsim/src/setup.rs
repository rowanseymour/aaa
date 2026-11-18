//! Setting a match up from its map, and adding to it as it's played.

use std::collections::{BTreeMap, VecDeque};

use tgmap::{Domain, Id, Map, Pathing, Role, TerrainIndex, Tile};

use crate::{
    Building, Fate, Fog, Mind, Order, Organization, Outcome, Rules, Sides, State, Supplying,
    Trailer, Treasury, TurretAim, Unit, World, normalised, wrap,
};

/// [`World::holdings`] from each placed object's role, owner and template.
/// Organizations and buildings count in their own right, and what the map
/// left out for lack of room counts for nothing.
pub(crate) fn holdings<'a>(
    things: impl IntoIterator<Item = (Role, Option<u32>, &'a str)>,
) -> Vec<(u32, Id)> {
    (things.into_iter())
        .filter(|(role, ..)| !matches!(role, Role::Crowded | Role::Organization | Role::Building))
        .filter_map(|(_, owner, template)| Some((owner?, Id::from(template))))
        .collect()
}

impl World {
    pub fn new(map: &Map, rules: Rules) -> World {
        let sides = Sides {
            names: BTreeMap::new(),
            computers: BTreeMap::new(),
            players: map.players.clone(),
            teams: map.teams.clone(),
        };
        let mut world = World::build(sides, map.pathing.clone(), rules, &map.organizations);
        world.add_buildings(&map.buildings);
        world.detach_from_missing();
        world.tile_terrains = map.tile_terrains.clone();
        world.scenery = map.scenery.clone();
        world.clearable = map.clearable.clone();
        world.holdings = holdings(
            map.roles()
                .map(|(t, role)| (role, tgmap::owner(t), t.template.as_str())),
        );
        world.settlements = map.settlements.clone();
        world.triggers = crate::Triggers::new(map.triggers.as_ref());
        world.triggers.names = map.named().map(|(n, id)| (n.to_string(), id)).collect();
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
        world.next_id = world.next_id.max(map.next_id());
        world
    }

    /// A world of `organizations` played by the default [`Rules`].
    pub fn from_organizations(
        sides: Sides,
        pathing: Pathing,
        organizations: &[tgmap::Organization],
    ) -> World {
        World::build(sides, pathing, Rules::default(), organizations)
    }

    /// As [`World::from_organizations`], played by `rules`.
    pub fn from_organizations_by(
        sides: Sides,
        pathing: Pathing,
        rules: Rules,
        organizations: &[tgmap::Organization],
    ) -> World {
        World::build(sides, pathing, rules, organizations)
    }

    /// What the match is played by, fixed once it's begun.
    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    fn build(
        sides: Sides,
        pathing: Pathing,
        rules: Rules,
        organizations: &[tgmap::Organization],
    ) -> World {
        // Nothing's hidden until the match sets its fog.
        let fog = Fog::new(pathing.side, tgmap::FogSetting::None);
        let fate = |p: &tgmap::Player| Fate {
            outcome: p.outcome,
            default_defeat: p.default_defeat,
            by_cities: p.won_by_cities,
            resigned: p.resigned,
            resigning: false,
        };
        let fates: BTreeMap<u32, Fate> = sides.players.iter().map(|p| (p.id, fate(p))).collect();
        // A match is over once a side has won or every side has lost.
        let decided = |of: Outcome| fates.values().filter(move |f| f.outcome == of);
        let over = decided(Outcome::Won).next().is_some()
            || !fates.is_empty() && decided(Outcome::Playing).next().is_none();
        let rng = rules.seed;
        let mut world = World {
            sides,
            pathing,
            rules,
            tile_terrains: tgmap::TileTerrains::default(),
            organizations: Vec::new(),
            units: Vec::new(),
            buildings: Vec::new(),
            deliveries: Vec::new(),
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
            pools: BTreeMap::new(),
            triggers: crate::Triggers::default(),
            fates,
            over,
            rng,
            holdings: Vec::new(),
            settlements: Vec::new(),
            treasuries: BTreeMap::new(),
            commanders: BTreeMap::new(),
            operated: Vec::new(),
            reveals: BTreeMap::new(),
            incoming: Vec::new(),
            watches: Vec::new(),
            afflicted: Vec::new(),
            summoned: Vec::new(),
            main_land: std::sync::OnceLock::new(),
            notices: Vec::new(),
            fog,
            numbers: BTreeMap::new(),
        };
        for organization in organizations {
            world.add_organization(organization);
        }
        world
    }

    /// Adds an organization as the map has it, standing where its members do,
    /// returning its index.
    pub fn add_organization(&mut self, organization: &tgmap::Organization) -> usize {
        self.push_organization(organization, true)
    }

    /// Adds an organization raised during the match, giving it and its members
    /// ids no object has, returning its index.
    pub fn add_new_organization(&mut self, mut organization: tgmap::Organization) -> usize {
        // A lone unit is its own organization, so takes one id.
        let ids = organization.members.len() as u32 + u32::from(!organization.lone());
        organization.renumber(self.take_ids(ids));
        self.add_organization(&organization)
    }

    /// Adds an organization of `template`, one of [`Rules::raisable`], owned by
    /// `owner` and standing in its starting formation around `at` facing
    /// `facing`, returning its index. `None`, with nothing added, for a
    /// template that isn't, an owner the match hasn't got, a place off
    /// the map or a facing that's no direction.
    pub fn raise(
        &mut self,
        template: &Id,
        owner: u32,
        at: [f32; 2],
        facing: [f32; 2],
    ) -> Option<usize> {
        let size = self.pathing.size();
        let on_map = at.iter().all(|c| (0.0..=size).contains(c));
        let facing = normalised(facing).filter(|f| f.iter().all(|c| c.is_finite()))?;
        self.sides.player(owner).filter(|_| on_map)?;
        let mut organization = self.rules.raisable.get(template)?.clone();
        organization.owner = owner;
        organization.place(at, facing);
        Some(self.add_new_organization(organization))
    }

    /// Adds an organization, numbering it if `numbered`.
    pub(crate) fn push_organization(
        &mut self,
        organization: &tgmap::Organization,
        numbered: bool,
    ) -> usize {
        let r = self.organizations.len();
        let first = self.units.len();
        for m in &organization.members {
            let unit = self.new_unit(m, r);
            self.units.push(unit);
        }
        let units: Vec<usize> = (first..self.units.len()).collect();
        let position = match organization.primary {
            Some(p) => self.units[first + p].position,
            None => self.average(&units),
        };
        let template = organization.template.clone();
        let number = match numbered {
            true => self.next_number(organization.owner, &template),
            false => None,
        };
        self.organizations.push(Organization {
            id: organization.id,
            template,
            number,
            owner: organization.owner,
            units,
            speed: organization
                .members
                .iter()
                .map(|m| m.speed)
                .fold(f32::INFINITY, f32::min),
            position,
            facing: normalised(organization.facing).unwrap_or([0.0, -1.0]),
            formations: organization.formations.clone(),
            formation: organization.formation,
            combat: organization.combat,
            operational_range: organization.operational_range.unwrap_or(0.0),
            control_range: organization.control_range.unwrap_or(0.0),
            order: Order::Move,
            engaged: Vec::new(),
            resume: None,
            unpacks: organization.unpacks.clone(),
            unpack_at: None,
            unpack_again: None,
            unpacked_into: None,
            rally: None,
            leader: organization.primary,
            hq: organization.hq,
            // Until its first look, so it doesn't start out marked.
            in_supply: true,
            replacing: (organization.members.iter())
                .map(|m| m.lost.unwrap_or(0.0))
                .collect(),
            bombarding: false,
            bombarded: Vec::new(),
            properties: Vec::new(),
            guard: None,
            arriving: false,
            terrain: TerrainIndex::default(),
            terrain_properties: Vec::new(),
            morale: 0.0,
            morale_max: 0.0,
            mind: Mind::Steady,
            engaged_before: false,
            experience: 0.0,
            level: 0,
            engaging_before: false,
            efficiency: organization.efficiency.unwrap_or(1.0),
            struck: None,
            broken_places: Vec::new(),
        });
        self.set_experience(r, organization.experience);
        self.organizations[r].morale_max = self.maximum_morale(r);
        // A map keeps its share; what's raised starts full.
        let share = organization.morale.clamp(0.0, 1.0);
        self.organizations[r].morale = self.organizations[r].morale_max * share;
        let ids = organization.members.iter().map(|m| m.id);
        self.next_id = ids
            .chain([organization.id])
            .fold(self.next_id, |n, id| n.max(id + 1));
        r
    }

    /// The next number `owner` gives an organization of `template`, if it's
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

    /// A unit of the organization at `r`, standing as `m` does.
    pub(crate) fn new_unit(&self, m: &tgmap::Member, r: usize) -> Unit {
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
        Unit {
            health: fighter.as_ref().map_or(0.0, |f| f.health * m.health),
            health_factor: 1.0,
            fired_ago: fighter
                .as_ref()
                .map_or(Vec::new(), |f| vec![f32::INFINITY; f.weapons.len()]),
            fighter,
            target: None,
            shot: None,
            died: None,
            death: None,
            besieged: None,
            struck: None,
            left: None,
            // One its organization has lost comes back as a recruit's do.
            awaited: m.lost.is_some(),
            descent: None,
            terrain: TerrainIndex::default(),
            id: m.id,
            template: m.template.clone(),
            organization: r,
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
            speed_max: m.speed,
            acceleration: m.acceleration,
            trailer: m.trailer.as_ref().map(|t| Trailer::new(t, [x, y], facing)),
            closing: None,
            busy: 0,
            decided: 0,
            met: None,
        }
    }

    /// Takes `n` ids no object has, returning the first.
    pub(crate) fn take_ids(&mut self, n: u32) -> u32 {
        let first = self.next_id;
        self.next_id += n;
        first
    }

    /// Adds buildings as the map has them, those out of health already
    /// disabled. Their ground is taken to be blocked already.
    pub fn add_buildings(&mut self, buildings: &[tgmap::Building]) {
        self.push_buildings(buildings, true);
    }

    /// Adds buildings, numbering them if `numbered`.
    pub(crate) fn push_buildings(&mut self, buildings: &[tgmap::Building], numbered: bool) {
        for b in buildings {
            let health = b.fighter.health * b.health;
            self.buildings.push(Building {
                id: b.id,
                template: b.template.clone(),
                number: None,
                owner: b.owner,
                position: b.position,
                facing: b.facing,
                fired_ago: vec![f32::INFINITY; b.fighter.weapons.len()],
                turrets: (b.fighter.turrets.iter())
                    .map(|t| TurretAim {
                        yaw: wrap(t.start),
                        idle: 0.0,
                    })
                    .collect(),
                fighter: b.fighter.clone(),
                health,
                health_factor: 1.0,
                target: None,
                noticed: Vec::new(),
                awake: b.awake,
                inside: BTreeMap::new(),
                besieged: None,
                struck: None,
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
                remembered: Vec::new(),
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
    fn a_match_draws_its_chances_from_its_seed() {
        let drawn = |seed| {
            let rules = Rules {
                seed,
                ..Default::default()
            };
            let mut w = World::build(
                Sides::default(),
                Pathing::uniform(4, Domain::Land),
                rules,
                &[],
            );
            [w.chance(), w.chance(), w.chance()]
        };
        assert_eq!(drawn(7), drawn(7));
        assert_ne!(drawn(7), drawn(8));
    }

    #[test]
    fn what_a_map_leaves_out_for_lack_of_room_adds_nothing() {
        let mut w = economy();
        w.rules.economies.insert(
            Id::from("start_loc"),
            tgmap::Economy {
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
    fn numbered_organizations_count_per_side_and_template_never_reusing_one() {
        let mut w = headquarters();
        w.rules.numbered = HashSet::from([Id::from("truck"), Id::from("tank")]);
        let mut t = truck();
        let mut add = |w: &mut World, owner, template: &str| {
            (t.owner, t.template) = (owner, template.into());
            let r = w.add_organization(&t);
            w.organizations[r].number
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
