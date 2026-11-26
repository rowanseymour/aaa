//! What a building keeps aircraft for: the planes in its stock, which of
//! them go out together for each job, the runway they use, what an air
//! attack and air recon cost, and how far round it it defends itself.

use std::collections::{BTreeMap, HashMap};

use crate::{GameData, Id, defs};

/// A building with a stock of aircraft, such as an airfield.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Airfield {
    /// The template of the plane each place in its stock holds, a place for
    /// each plane.
    pub stock: Vec<Id>,
    pub groups: Vec<AirGroup>,
    pub runway: Runway,
    /// What sending out an air attack costs, by resource.
    pub attack_cost: BTreeMap<Id, f32>,
    /// What sending out air recon costs, by resource.
    pub recon_cost: BTreeMap<Id, f32>,
    /// How near it an enemy has to come to be noticed as an intruder.
    pub guard_range: f32,
    /// How far from it an intruder has to go to be forgotten.
    pub guard_operational_range: f32,
}

/// Planes that go out together.
#[derive(Debug, Clone, PartialEq)]
pub struct AirGroup {
    /// The templates of the planes it takes.
    pub templates: Vec<Id>,
    /// The fewest that go out, and the most.
    pub fewest: usize,
    pub most: usize,
    /// The jobs it's sent on.
    pub jobs: Vec<AirJob>,
    /// Where each plane after the first flies from the first: across its
    /// way, along it and up.
    pub formation: Vec<[f32; 3]>,
}

/// What a group goes out for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirJob {
    /// An air attack.
    Offensive,
    /// Flying with an air attack.
    Escort,
    Recon,
    /// Seeing off what comes near its building.
    Defensive,
}

impl AirJob {
    fn parse(word: &str) -> Option<AirJob> {
        match word.trim().to_ascii_uppercase().as_str() {
            "AIR_OFFENSIVE" => Some(AirJob::Offensive),
            "AIR_ESCORT" => Some(AirJob::Escort),
            "AIR_RECON" => Some(AirJob::Recon),
            "AIR_DEFENSIVE" => Some(AirJob::Defensive),
            _ => None,
        }
    }
}

/// Where planes take off and land, from their building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Runway {
    /// Where it starts from the building's root node: x across its facing
    /// and y along it.
    pub offset: [f32; 2],
    /// The way it runs in the same axes, a unit vector.
    pub orientation: [f32; 2],
    pub length: f32,
    /// Where planes wait to take off, in the same axes, the way they face
    /// there, a unit vector, and how far they taxi straight out from it.
    pub hangar_offset: [f32; 2],
    pub hangar_orientation: [f32; 2],
    pub hangar_length: f32,
    /// Seconds between planes taking off.
    pub delay: f32,
}

impl Default for Runway {
    fn default() -> Self {
        Runway {
            offset: [0.0; 2],
            orientation: [0.0, 1.0],
            length: 0.0,
            hangar_offset: [0.0; 2],
            hangar_orientation: [0.0, 1.0],
            hangar_length: 0.0,
            delay: 0.0,
        }
    }
}

/// `[x, y]` as a unit vector, or along y for none.
fn unit(v: Option<[f32; 2]>) -> [f32; 2] {
    let [x, y] = v.unwrap_or([0.0, 1.0]);
    let length = (x * x + y * y).sqrt();
    match length > 0.0 {
        true => [x / length, y / length],
        false => [0.0, 1.0],
    }
}

impl Airfield {
    /// The first group sent on `job`.
    pub fn group(&self, job: AirJob) -> Option<&AirGroup> {
        self.groups.iter().find(|g| g.jobs.contains(&job))
    }
}

/// The airfield `denizens` make a template, if it has a stock.
fn airfield(denizens: &defs::DenizenComponent) -> Option<Airfield> {
    let stock: Vec<Id> = (denizens.inventory.iter())
        .filter_map(|i| Some(std::iter::repeat_n(i.actor_ids.clone()?, i.count as usize)))
        .flatten()
        .collect();
    if stock.is_empty() {
        return None;
    }
    let groups = (denizens.denizen_group.iter())
        .map(|g| {
            let [fewest, most] = g.size.unwrap_or([1.0, 1.0]).map(|n| n.max(0.0) as usize);
            AirGroup {
                templates: crate::supply::listed(g.actor_ids.as_deref()),
                fewest,
                most: most.max(fewest),
                jobs: (g.behavior.iter())
                    .flat_map(|b| b.split('+'))
                    .filter_map(AirJob::parse)
                    .collect(),
                formation: (g.formation_positions.iter())
                    .filter_map(|p| p.offset)
                    .collect(),
            }
        })
        .collect();
    let runway = denizens.runway.as_ref().map_or_else(Runway::default, |r| {
        let [x, y, _] = r.offset.unwrap_or([0.0; 3]);
        // Planes wait at the runway's start where it sets no hangar.
        let [hx, hy, _] = r.hangar_offset.unwrap_or([x, y, 0.0]);
        Runway {
            offset: [x, y],
            orientation: unit(r.orientation),
            length: r.length.unwrap_or(0.0),
            hangar_offset: [hx, hy],
            hangar_orientation: unit(r.hangar_orientation.or(r.orientation)),
            hangar_length: r.hangar_length.unwrap_or(0.0),
            delay: r.delay.unwrap_or(0.0),
        }
    });
    let cost = |c: &Option<defs::Resources>| {
        (c.iter())
            .flat_map(|c| c.amounts())
            .map(|(id, amount)| (id.clone(), amount))
            .collect()
    };
    Some(Airfield {
        stock,
        groups,
        runway,
        attack_cost: cost(&denizens.air_attack_cost),
        recon_cost: cost(&denizens.air_recon_cost),
        guard_range: denizens.guard_range.unwrap_or(0.0),
        guard_operational_range: denizens.guard_operational_range.unwrap_or(0.0),
    })
}

/// How a template flies.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aircraft {
    /// How far above the ground it flies.
    pub height: f32,
    /// How far above the ground it sits when down.
    pub taxi_height: f32,
    /// How fast it moves on the ground.
    pub taxi_speed: f32,
    /// What it's drawn at, of its size, on the ground.
    pub taxi_scale: f32,
    /// The radius of the circle it turns in.
    pub turn_radius: f32,
}

impl GameData {
    /// Every template with a stock of aircraft, by template id.
    pub fn airfields(&self) -> HashMap<Id, Airfield> {
        self.by_template(|t| airfield(t.denizen_component.as_ref()?))
    }

    /// How each template that flies flies, by template id.
    pub fn aircraft(&self) -> HashMap<Id, Aircraft> {
        self.by_template(|t| {
            let a = t.aircraft_component.as_ref()?;
            Some(Aircraft {
                height: a.operational_height?,
                taxi_height: a.taxi_height.unwrap_or(0.0),
                taxi_speed: a.taxi_speed.unwrap_or(1.0),
                taxi_scale: a.taxi_scale.unwrap_or(1.0),
                turn_radius: (t.mover_component.as_ref())
                    .and_then(|m| m.turn_radius)
                    .unwrap_or(0.0),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denizens(text: &str) -> defs::DenizenComponent {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("DenizenComponent").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn an_airfield_has_a_place_for_each_plane_and_its_groups_jobs() {
        let found = airfield(&denizens(
            "[DenizenComponent]\n{\n\
             \tguard_range = 30\n\tguard_operational_range = 48\n\
             \t[DenizenGroup]\n\t{\n\t\tactor_ids = bomber\n\t\tsize = 1,1\n\
             \t\tbehavior = AIR_OFFENSIVE\n\
             \t\t[FormationPositions]\n\t\toffset = -9,+15,-2\n\t}\n\
             \t[DenizenGroup]\n\t{\n\t\tactor_ids = fighter\n\t\tsize = 1,2\n\
             \t\tbehavior = AIR_DEFENSIVE + AIR_RECON\n\t}\n\
             \t[Inventory]\n\tactor_ids = bomber\n\tcount = 2\n\n\
             \t[Inventory]\n\tactor_ids = fighter\n\tcount = 4\n\n\
             \t[Runway]\n\t{\n\t\thangar_offset = 2.5, 1, -.75\n\t\thangar_orientation = 0, 1\n\
             \t\thangar_length = 2.45\n\t\toffset = 0, 5, -.75\n\t\torientation = -1 ,0\n\
             \t\tlength = 4.8\n\
             \t\tdelay = 1\n\t}\n\
             \t[AirAttackCost]\n\tmoney = 100\n\
             \t[AirReconCost]\n\tmoney = 25\n}\n",
        ))
        .expect("an airfield");
        let stock: Vec<_> = found.stock.iter().map(Id::as_str).collect();
        assert_eq!(
            stock,
            [
                "bomber", "bomber", "fighter", "fighter", "fighter", "fighter"
            ]
        );
        let attack = found.group(AirJob::Offensive).expect("an attack group");
        assert_eq!(attack.templates, [Id::from("bomber")]);
        assert_eq!((attack.fewest, attack.most), (1, 1));
        assert_eq!(attack.formation, [[-9.0, 15.0, -2.0]]);
        let recon = found.group(AirJob::Recon).expect("a recon group");
        assert_eq!(recon.jobs, [AirJob::Defensive, AirJob::Recon]);
        assert_eq!((recon.fewest, recon.most), (1, 2));
        assert!(found.group(AirJob::Escort).is_none());
        assert_eq!(found.runway.offset, [0.0, 5.0]);
        assert_eq!(found.runway.orientation, [-1.0, 0.0]);
        assert_eq!(found.runway.delay, 1.0);
        assert_eq!(found.runway.length, 4.8);
        assert_eq!(found.runway.hangar_offset, [2.5, 1.0]);
        assert_eq!(found.runway.hangar_orientation, [0.0, 1.0]);
        assert_eq!(found.runway.hangar_length, 2.45);
        assert_eq!(found.attack_cost.get(&Id::from("money")), Some(&100.0));
        assert_eq!(found.recon_cost.get(&Id::from("money")), Some(&25.0));
        assert_eq!(
            (found.guard_range, found.guard_operational_range),
            (30.0, 48.0)
        );
    }

    #[test]
    fn what_has_no_stock_is_no_airfield() {
        let found = airfield(&denizens("[DenizenComponent]\n{\n\tguard_range = 30\n}\n"));
        assert_eq!(found, None);
    }
}
