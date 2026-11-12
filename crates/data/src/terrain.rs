//! What terrain does to those standing on it.

use std::collections::{HashMap, HashSet};

use crate::supply::{is, listed};
use crate::{GameData, Id, defs};

/// What a property a terrain gives does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TerrainEffect {
    /// The properties a thing needs to gain it.
    pub requires: Vec<Id>,
    /// What it adds to its holder's defence.
    pub defense: f32,
    /// The fraction of each damage type's damage it turns aside from its
    /// holder; negative where it adds to it.
    pub resists: Vec<(Id, f32)>,
    /// The speed limit it gives its terrain, for everyone on it.
    pub speed_limit: Option<f32>,
}

/// The settings terrain bears on those standing on it by.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainRules {
    /// The lowest a terrain's speed limit comes to.
    pub slowest: f32,
    /// What each shared property does when a terrain gives it, by its id.
    pub effects: HashMap<Id, TerrainEffect>,
    /// The properties whose holders no terrain's speed limit holds back.
    pub unlimited: Vec<Id>,
    /// The templates with such a property, shared or their own.
    pub free: HashSet<Id>,
}

impl Default for TerrainRules {
    fn default() -> Self {
        TerrainRules {
            slowest: 0.5,
            effects: HashMap::new(),
            unlimited: Vec::new(),
            free: HashSet::new(),
        }
    }
}

fn trailblazing(p: &defs::Property) -> bool {
    (p.modifier.iter()).any(|m| is(&m.modifier_flag, "TRAILBLAZING"))
}

fn effect(p: &defs::Property) -> TerrainEffect {
    let mut out = TerrainEffect {
        requires: listed(p.required_properties.as_deref()),
        ..TerrainEffect::default()
    };
    for m in &p.modifier {
        let amount = m.amount.unwrap_or(0.0);
        if is(&m.kind, "ADD") && is(&m.attribute, "DV") {
            out.defense += amount;
        }
        if is(&m.kind, "ADD") && is(&m.attribute, "TERRAIN_SPEED_CAP") && amount > 0.0 {
            out.speed_limit = Some(amount);
        }
        if let (true, Some(kind)) = (is(&m.kind, "RESIST_DAMAGE"), &m.damage_type) {
            out.resists.push((kind.clone(), amount));
        }
    }
    out
}

impl GameData {
    pub fn terrain_rules(&mut self) -> TerrainRules {
        let slowest = (self.svars().iter()).find_map(|v| v.terrain_minimum_move_speed);
        let definitions = self.definitions();
        let shared = definitions.properties.iter();
        let effects = shared.map(|(id, p)| (id.clone(), effect(p))).collect();
        let mut unlimited: Vec<Id> = (definitions.properties.values())
            .filter(|p| trailblazing(p))
            .filter_map(|p| p.ids.clone())
            .collect();
        unlimited.sort();
        let free = self.by_template(|t| {
            let c = t.properties_component.as_ref()?;
            let refs = c.property.iter().filter_map(|r| r.property_ids.as_ref());
            let shared = refs.filter_map(|p| definitions.properties.get(p));
            (shared.chain(&c.intrinsic_property))
                .any(trailblazing)
                .then_some(())
        });
        TerrainRules {
            slowest: slowest.unwrap_or(TerrainRules::default().slowest),
            effects,
            unlimited,
            free: free.into_keys().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_properties_cap_speed_and_change_what_hits_do() {
        let parsed = tgi::parse(
            "[template Modifier]\n{\n\tstring type\n\tstring attribute\n\tfixed amount\n\
             \tstring damage_type\n}\n\
             [template AddModifier inherit=Modifier]\n{\n\ttype = ADD\n}\n\
             [template TerrainSpeedModifier template=AddModifier]\n{\n\
             \tattribute = TERRAIN_SPEED_CAP\n}\n\
             [template ResistDamageModifier inherit=Modifier]\n{\n\ttype = RESIST_DAMAGE\n}\n\
             [template Property]\n{\n\tstring IDS\n\tstring required_properties\n\
             \t[list Modifier]\n}\n\
             [template TerrainProperty inherit=Property]\n{\n\trequired_properties = mobile\n}\n\
             [Property template=TerrainProperty]\n{\n\tIDS = forest_infantry\n\
             \trequired_properties = infantry\n\
             \t[Modifier template=TerrainSpeedModifier]\n\tamount = 2.5\n\
             \t[Modifier template=AddModifier]\n\tattribute = DV\n\tamount = 4\n\
             \t[Modifier template=ResistDamageModifier]\n\tdamage_type = armor_piercing\n\
             \tamount = 0.20\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Property").next().unwrap();
        let property = templates.deserialize::<defs::Property>(block).unwrap();
        assert_eq!(
            effect(&property),
            TerrainEffect {
                requires: vec![Id::from("infantry")],
                defense: 4.0,
                resists: vec![(Id::from("armor_piercing"), 0.2)],
                speed_limit: Some(2.5),
            }
        );
    }
}
