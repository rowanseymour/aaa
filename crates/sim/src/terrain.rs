//! What the terrain things stand on does to them: the properties it gives
//! them and the speed it holds them to.

use aaa_map::{Id, TerrainEffect, Zone};

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
            let under = self.grounds.at(at);
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
                let own = self.grounds.at(position);
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
    fn gained(&self, r: usize, terrain: u16) -> Vec<Id> {
        let listed = &self.grounds.kind(terrain).properties;
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
            for p in &self.grounds.kind(terrain).properties {
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
        let ground = self.grounds.kind(unit.terrain);
        let limits = ground.properties.iter().rev();
        let limit = match ground.mountain {
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
