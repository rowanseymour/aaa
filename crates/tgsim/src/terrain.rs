//! What the terrain things stand on does to them: the properties it gives
//! them and the speed it holds them to.

use tgmap::{Id, TerrainEffect, TerrainIndex, Zone};

use crate::{Target, World};

/// The property everything that can move has, whatever its template says.
const MOBILE: &str = "mobile";

impl World {
    /// Looks at the terrain under every organization that can move and under
    /// its members. An organization's is that under its leader, and a member
    /// with it shares that: only one straggling outside its control zone
    /// has that of its own tile. What an organization can't move for, such as
    /// being unpacked, keeps what it had.
    pub(crate) fn stand(&mut self) {
        for r in 0..self.organizations.len() {
            let organization = &self.organizations[r];
            if organization.unpacked_into.is_some()
                || organization.speed <= 0.0
                || self.destroyed(r)
            {
                continue;
            }
            let at = self.leader_position(r);
            let under = self.tile_terrains.at(at);
            // Again every time: who's left to qualify changes too.
            let gained = self.gained(r, under);
            let organization = &mut self.organizations[r];
            organization.terrain = under;
            if organization.terrain_properties != gained {
                organization.terrain_properties = gained;
            }
            let mut zone: Option<Zone> = None;
            for p in 0..self.organizations[r].units.len() {
                let organization = &self.organizations[r];
                let u = organization.units[p];
                if !self.units[u].alive() {
                    continue;
                }
                let position = self.units[u].position;
                let own = self.tile_terrains.at(position);
                let with = own == under || Some(p) == organization.leader || {
                    let reach = organization.control(&self.rules.guard_rules);
                    let zone = zone.get_or_insert_with(|| self.pathing.zone(at, reach));
                    zone.holds(position)
                };
                self.units[u].terrain = if with { under } else { own };
            }
        }
    }

    /// The properties an organization gains from terrain: those any member
    /// left, or the organization itself, has what they need.
    fn gained(&self, r: usize, terrain: TerrainIndex) -> Vec<Id> {
        let listed = &self.tile_terrains.get(terrain).properties;
        if listed.is_empty() {
            return Vec::new();
        }
        let members = self.survivors(r).map(|u| &self.units[u].template);
        let templates: Vec<&Id> = (members.chain([&self.organizations[r].template])).collect();
        let gained = listed
            .iter()
            .filter(|p| templates.iter().any(|t| self.gains(t, p)));
        gained.cloned().collect()
    }

    /// Whether things of a template that can move have what a terrain's
    /// property needs.
    fn gains(&self, template: &Id, property: &Id) -> bool {
        let Some(effect) = self.rules.terrain_rules.effects.get(property) else {
            return false;
        };
        let kind = self.rules.kinds.get(template);
        let has = |p: &Id| {
            p.as_str().eq_ignore_ascii_case(MOBILE)
                || kind.is_some_and(|k| k.properties.contains(p))
        };
        effect.requires.iter().all(has)
    }

    /// What shields `t` from harm: its terrain properties' effects, and
    /// `shelter`, what else it holds (from [`World::shelter`]).
    pub(crate) fn protection<'a>(
        &'a self,
        t: Target,
        shelter: &'a TerrainEffect,
    ) -> Vec<&'a TerrainEffect> {
        let mut out = self.terrain_effects(t);
        out.push(shelter);
        out
    }

    /// What the properties `t` holds for a time or by its stance, and its
    /// side's technologies on its kind, add to its resistance, past its
    /// template's own, which its fighter has already.
    pub(crate) fn shelter(&self, t: Target) -> TerrainEffect {
        let effects = &self.rules.combat_effects;
        let stance = match t {
            Target::Unit(u) => self.stance_property(u).and_then(|p| effects.get(p)),
            Target::Building(_) => None,
        };
        let borne = self.borne(effects, |e| &e.requires, t).chain(stance);
        TerrainEffect {
            resists: borne.flat_map(|e| e.resists.iter().cloned()).collect(),
            ..TerrainEffect::default()
        }
    }

    /// What the terrain properties a target holds do: for a unit, those it
    /// has what they need of its organization's terrain and its own.
    pub(crate) fn terrain_effects(&self, t: Target) -> Vec<&TerrainEffect> {
        let Target::Unit(u) = t else {
            return Vec::new();
        };
        let unit = &self.units[u];
        let mut held: Vec<&Id> = Vec::new();
        for terrain in [self.organizations[unit.organization].terrain, unit.terrain] {
            for p in &self.tile_terrains.get(terrain).properties {
                if !held.contains(&p) && self.gains(&unit.template, p) {
                    held.push(p);
                }
            }
        }
        let effects = &self.rules.terrain_rules.effects;
        held.into_iter().filter_map(|p| effects.get(p)).collect()
    }

    /// The fastest the terrain a unit is on lets it go, where it holds it back
    /// at all: the limit of the last of the terrain's properties to have one,
    /// whoever that property is for. None holds back a unit whose template or
    /// organization's is free of them, or whose organization has, for a time or
    /// by a technology on it, a property that frees it.
    pub(crate) fn speed_limit(&self, u: usize) -> Option<f32> {
        let unit = &self.units[u];
        let rules = &self.rules.terrain_rules;
        let terrain = self.tile_terrains.get(unit.terrain);
        let limits = terrain.properties.iter().rev();
        let limit = match terrain.mountain {
            true => rules.slowest,
            false => limits
                .filter_map(|p| rules.effects.get(p)?.speed_limit)
                .next()?,
        };
        let organization = &self.organizations[unit.organization];
        let timed = (organization.properties.iter())
            .chain(&unit.properties)
            .map(|h| &h.property);
        let technologies = self.technologies_of(organization.owner);
        let things = self.weighed(crate::Target::Unit(u));
        let on = |p: &&Id| {
            (self.gains(&organization.template, p) || self.gains(&unit.template, p))
                && self.keeps_side_copy(organization.owner, &things, p)
        };
        let free = rules.free.contains(&unit.template)
            || rules.free.contains(&organization.template)
            || (timed.chain(technologies.filter(on))).any(|p| rules.unlimited.contains(p));
        (!free).then_some(limit.max(rules.slowest))
    }
}

#[cfg(test)]
mod tests {

    use crate::combat::damage;
    use crate::testing::*;
    use std::collections::HashMap;

    use tgmap::{Domain, Id, Pathing, TerrainEffect, Tile};

    use crate::{Command, Held, Target, World};

    /// Forest, whose first property is for infantry and whose second for
    /// vehicles, and the rules those properties work by.
    fn forest() -> (tgmap::TileTerrain, tgmap::TerrainRules) {
        let forest = tgmap::TileTerrain {
            properties: vec![Id::from("forest_infantry"), Id::from("forest_vehicles")],
            mountain: false,
            water: false,
        };
        let infantry = TerrainEffect {
            requires: vec![Id::from("infantry")],
            defense: 4.0,
            resists: vec![(Id::from("gunfire"), 0.5)],
            speed_limit: Some(1.5),
        };
        let vehicles = TerrainEffect {
            requires: vec![Id::from("mobile"), Id::from("vehicle")],
            defense: 0.0,
            resists: vec![(Id::from("gunfire"), -0.2)],
            speed_limit: Some(1.0),
        };
        let rules = tgmap::TerrainRules {
            effects: HashMap::from([
                (Id::from("forest_infantry"), infantry),
                (Id::from("forest_vehicles"), vehicles),
            ]),
            unlimited: vec![Id::from("trailblazing")],
            ..tgmap::TerrainRules::default()
        };
        (forest, rules)
    }

    #[test]
    fn terrain_holds_everyone_on_it_to_its_last_propertys_speed_limit() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let (forest, rules) = forest();
        (w.tile_terrains, w.rules.terrain_rules) =
            (tgmap::TileTerrains::uniform(20, forest), rules);
        w.rules
            .kinds
            .insert(Id::from("soldier"), kind(&["infantry"]));
        let to = [10.0, 30.0];
        w.apply(
            7,
            Command::Move {
                organization: 0,
                to,
            },
        )
        .unwrap();
        run(&mut w, 1.0);
        // The vehicles' limit, though they're infantry.
        assert!(w.units.iter().all(|u| u.speed == 1.0));
        // Never below the lowest there is.
        w.rules.terrain_rules.slowest = 1.25;
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 1.25));
        // A mountain's is the lowest there is.
        let mountain = tgmap::TileTerrain {
            properties: Vec::new(),
            mountain: true,
            water: false,
        };
        (w.tile_terrains, w.rules.terrain_rules.slowest) =
            (tgmap::TileTerrains::uniform(20, mountain), 0.5);
        run(&mut w, 0.2);
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        // Unless its organization has, for a time, a property free of limits,
        w.organizations[0].properties.push(Held {
            property: Id::from("trailblazing"),
            until: u64::MAX,
            by: None,
        });
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
        w.organizations[0].properties.clear();
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        // its side a technology that is, on those it's for,
        w.grant(7, Id::from("trailblazing"));
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        let anyone = TerrainEffect::default();
        (w.rules.terrain_rules.effects).insert(Id::from("trailblazing"), anyone);
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
        w.technologies.clear();
        // or its template is.
        w.rules.terrain_rules.free.insert(Id::from("squad"));
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
    }

    #[test]
    fn members_share_their_organizations_terrain_unless_they_straggle() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let (forest, rules) = forest();
        w.rules.terrain_rules = rules;
        w.tile_terrains = tgmap::TileTerrains::uniform(20, tgmap::TileTerrain::default());
        w.rules
            .kinds
            .insert(Id::from("soldier"), kind(&["infantry"]));
        // Under the first only, who with no control range is on his own.
        w.tile_terrains
            .set(Tile::at(w.units[0].position), forest.clone());
        w.stand();
        let held = |w: &World, u| w.terrain_effects(Target::Unit(u)).len();
        assert_eq!((held(&w, 0), held(&w, 1)), (1, 0));
        // What it holds adds to its defence.
        for unit in &mut w.units {
            unit.fighter = Some(soldier(vec![]));
        }
        let raised = |u| (w.defense(Target::Unit(u))).map(|(now, plain)| now - plain);
        assert_eq!((raised(0), raised(1)), (Some(4.0), Some(0.0)));
        assert!(w.organizations[0].terrain_properties.is_empty());
        assert_eq!((w.speed_limit(0), w.speed_limit(1)), (Some(1.0), None));
        w.organizations[0].control_range = 10.0;
        w.stand();
        assert_eq!((held(&w, 0), w.speed_limit(0)), (0, None));
        // Under the organization, which gains only what a member can.
        w.tile_terrains.set(Tile::at(w.leader_position(0)), forest);
        w.stand();
        assert_eq!((held(&w, 0), held(&w, 1)), (1, 1));
        assert_eq!(
            w.organizations[0].terrain_properties,
            [Id::from("forest_infantry")]
        );
        // Which follows who's left in it.
        w.rules
            .kinds
            .insert(Id::from("soldier"), kind(&["vehicle"]));
        w.stand();
        assert_eq!(
            w.organizations[0].terrain_properties,
            [Id::from("forest_vehicles")]
        );
        w.rules
            .kinds
            .insert(Id::from("soldier"), kind(&["infantry"]));
        w.stand();
        // Unpacked, it keeps what it had wherever that leaves it.
        w.organizations[0].unpacked_into = Some(0);
        w.tile_terrains = tgmap::TileTerrains::uniform(20, tgmap::TileTerrain::default());
        w.stand();
        assert_eq!(
            w.organizations[0].terrain_properties,
            [Id::from("forest_infantry")]
        );
    }

    #[test]
    fn terrain_properties_add_to_defence_and_resistance() {
        let (_, rules) = forest();
        let effect = |id: &str| &rules.effects[&Id::from(id)];
        let hit = |ground: &[&TerrainEffect]| {
            damage(
                &main_damage(&rifle(12.0, 12.0)),
                &soldier(vec![]),
                ground,
                1.0,
                Default::default(),
            )
        };
        assert_eq!(hit(&[]), 12.0);
        assert!((hit(&[effect("forest_infantry")]) - 4.0).abs() < 1e-5);
        assert!((hit(&[effect("forest_vehicles")]) - 14.4).abs() < 1e-5);
        let both = [effect("forest_infantry"), effect("forest_vehicles")];
        assert!((hit(&both) - 5.6).abs() < 1e-5);
    }
}
