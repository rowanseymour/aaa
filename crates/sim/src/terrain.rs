//! What the terrain things stand on does to them: the properties it gives
//! them and the speed it holds them to.

use aaa_map::{Id, TerrainEffect, TerrainIndex, Zone};

use crate::{Target, World};

/// The property everything that can move has, whatever its template says.
const MOBILE: &str = "mobile";

impl World {
    /// Looks at the terrain under every regiment that can move and under
    /// its members. A regiment's is that under its leader, and a member
    /// with it shares that: only one straggling outside its control zone
    /// has that of its own tile. What a regiment can't move for, such as
    /// being unpacked, keeps what it had.
    pub(crate) fn stand(&mut self) {
        for r in 0..self.regiments.len() {
            let regiment = &self.regiments[r];
            if regiment.unpacked_into.is_some() || regiment.speed <= 0.0 || self.destroyed(r) {
                continue;
            }
            let at = self.leader_position(r);
            let under = self.tile_terrains.at(at);
            // Again every time: who's left to qualify changes too.
            let gained = self.gained(r, under);
            let regiment = &mut self.regiments[r];
            regiment.terrain = under;
            if regiment.terrain_properties != gained {
                regiment.terrain_properties = gained;
            }
            let mut zone: Option<Zone> = None;
            for p in 0..self.regiments[r].units.len() {
                let regiment = &self.regiments[r];
                let u = regiment.units[p];
                if !self.units[u].alive() {
                    continue;
                }
                let position = self.units[u].position;
                let own = self.tile_terrains.at(position);
                let with = own == under || Some(p) == regiment.leader || {
                    let reach = regiment.control(&self.guard_rules);
                    let zone = zone.get_or_insert_with(|| self.pathing.zone(at, reach));
                    zone.holds(position)
                };
                self.units[u].terrain = if with { under } else { own };
            }
        }
    }

    /// The properties a regiment gains from terrain: those any member
    /// left, or the regiment itself, has what they need.
    fn gained(&self, r: usize, terrain: TerrainIndex) -> Vec<Id> {
        let listed = &self.tile_terrains.get(terrain).properties;
        if listed.is_empty() {
            return Vec::new();
        }
        let members = self.survivors(r).map(|u| &self.units[u].template_id);
        let templates: Vec<&Id> = (members.chain([&self.regiments[r].template_id])).collect();
        let gained = listed
            .iter()
            .filter(|p| templates.iter().any(|t| self.gains(t, p)));
        gained.cloned().collect()
    }

    /// Whether things of a template that can move have what a terrain's
    /// property needs.
    fn gains(&self, template: &Id, property: &Id) -> bool {
        let Some(effect) = self.terrain_rules.effects.get(property) else {
            return false;
        };
        let kind = self.kinds.get(template);
        let has = |p: &Id| {
            p.as_str().eq_ignore_ascii_case(MOBILE)
                || kind.is_some_and(|k| k.properties.contains(p))
        };
        effect.requires.iter().all(has)
    }

    /// What the terrain properties a target holds do: for a unit, those it
    /// has what they need of its regiment's terrain and its own.
    pub(crate) fn terrain_effects(&self, t: Target) -> Vec<&TerrainEffect> {
        let Target::Unit(u) = t else {
            return Vec::new();
        };
        let unit = &self.units[u];
        let mut held: Vec<&Id> = Vec::new();
        for terrain in [self.regiments[unit.regiment].terrain, unit.terrain] {
            for p in &self.tile_terrains.get(terrain).properties {
                if !held.contains(&p) && self.gains(&unit.template_id, p) {
                    held.push(p);
                }
            }
        }
        let effects = &self.terrain_rules.effects;
        held.into_iter().filter_map(|p| effects.get(p)).collect()
    }

    /// The fastest the terrain a unit is on lets it go, where it holds it
    /// back at all: the limit of the last of the terrain's properties to
    /// have one, whoever that property is for. None holds back a unit
    /// whose template or regiment's is free of them, or whose regiment has,
    /// for a time or by a technology on it, a property that frees it.
    pub(crate) fn speed_limit(&self, u: usize) -> Option<f32> {
        let unit = &self.units[u];
        let rules = &self.terrain_rules;
        let terrain = self.tile_terrains.get(unit.terrain);
        let limits = terrain.properties.iter().rev();
        let limit = match terrain.mountain {
            true => rules.slowest,
            false => limits
                .filter_map(|p| rules.effects.get(p)?.speed_limit)
                .next()?,
        };
        let regiment = &self.regiments[unit.regiment];
        let timed = regiment.properties.iter().map(|h| &h.property);
        let technologies = self.technologies.get(&regiment.owner).into_iter().flatten();
        let on = |p: &&Id| self.gains(&regiment.template_id, p) || self.gains(&unit.template_id, p);
        let free = rules.free.contains(&unit.template_id)
            || rules.free.contains(&regiment.template_id)
            || (timed.chain(technologies.filter(on))).any(|p| rules.unlimited.contains(p));
        (!free).then_some(limit.max(rules.slowest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::damage;
    use crate::testing::*;
    use crate::*;

    #[test]
    fn terrain_holds_everyone_on_it_to_its_last_propertys_speed_limit() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let (forest, rules) = forest();
        (w.tile_terrains, w.terrain_rules) = (aaa_map::TileTerrains::uniform(20, forest), rules);
        w.kinds.insert(Id::from("soldier"), kind(&["infantry"]));
        let to = [10.0, 30.0];
        w.apply(7, Command::Move { regiment: 0, to }).unwrap();
        run(&mut w, 1.0);
        // The vehicles' limit, though they're infantry.
        assert!(w.units.iter().all(|u| u.speed == 1.0));
        // Never below the lowest there is.
        w.terrain_rules.slowest = 1.25;
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 1.25));
        // A mountain's is the lowest there is.
        let mountain = aaa_map::TileTerrain {
            properties: Vec::new(),
            mountain: true,
        };
        (w.tile_terrains, w.terrain_rules.slowest) =
            (aaa_map::TileTerrains::uniform(20, mountain), 0.5);
        run(&mut w, 0.2);
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        // Unless its regiment has, for a time, a property free of limits,
        w.regiments[0].properties.push(Held {
            property: Id::from("trailblazing"),
            until: u64::MAX,
        });
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
        w.regiments[0].properties.clear();
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        // its side a technology that is, on those it's for,
        w.technologies.insert(7, vec![Id::from("trailblazing")]);
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 0.5));
        let anyone = TerrainEffect::default();
        (w.terrain_rules.effects).insert(Id::from("trailblazing"), anyone);
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
        w.technologies.clear();
        // or its template is.
        w.terrain_rules.free.insert(Id::from("squad"));
        w.tick();
        assert!(w.units.iter().all(|u| u.speed == 2.0));
    }

    #[test]
    fn members_share_their_regiments_terrain_unless_they_straggle() {
        let mut w = world(Pathing::uniform(20, Domain::Land));
        let (forest, rules) = forest();
        w.terrain_rules = rules;
        w.tile_terrains = aaa_map::TileTerrains::uniform(20, aaa_map::TileTerrain::default());
        w.kinds.insert(Id::from("soldier"), kind(&["infantry"]));
        // Under the first only, who with no control range is on his own.
        w.tile_terrains
            .set(Tile::at(w.units[0].position), forest.clone());
        w.stand();
        let held = |w: &World, u| w.terrain_effects(Target::Unit(u)).len();
        assert_eq!((held(&w, 0), held(&w, 1)), (1, 0));
        assert!(w.regiments[0].terrain_properties.is_empty());
        assert_eq!((w.speed_limit(0), w.speed_limit(1)), (Some(1.0), None));
        w.regiments[0].control_range = 10.0;
        w.stand();
        assert_eq!((held(&w, 0), w.speed_limit(0)), (0, None));
        // Under the regiment, which gains only what a member can.
        w.tile_terrains.set(Tile::at(w.leader_position(0)), forest);
        w.stand();
        assert_eq!((held(&w, 0), held(&w, 1)), (1, 1));
        assert_eq!(
            w.regiments[0].terrain_properties,
            [Id::from("forest_infantry")]
        );
        // Which follows who's left in it.
        w.kinds.insert(Id::from("soldier"), kind(&["vehicle"]));
        w.stand();
        assert_eq!(
            w.regiments[0].terrain_properties,
            [Id::from("forest_vehicles")]
        );
        w.kinds.insert(Id::from("soldier"), kind(&["infantry"]));
        w.stand();
        // Unpacked, it keeps what it had wherever that leaves it.
        w.regiments[0].unpacked_into = Some(0);
        w.tile_terrains = aaa_map::TileTerrains::uniform(20, aaa_map::TileTerrain::default());
        w.stand();
        assert_eq!(
            w.regiments[0].terrain_properties,
            [Id::from("forest_infantry")]
        );
    }

    #[test]
    fn terrain_properties_add_to_defence_and_resistance() {
        let (_, rules) = forest();
        let effect = |id: &str| &rules.effects[&Id::from(id)];
        let hit =
            |ground: &[&TerrainEffect]| damage(&rifle(12.0, 12.0), &soldier(vec![]), ground, 1.0);
        assert_eq!(hit(&[]), 12.0);
        assert!((hit(&[effect("forest_infantry")]) - 4.0).abs() < 1e-5);
        assert!((hit(&[effect("forest_vehicles")]) - 14.4).abs() < 1e-5);
        let both = [effect("forest_infantry"), effect("forest_vehicles")];
        assert!((hit(&both) - 5.6).abs() < 1e-5);
    }
}
