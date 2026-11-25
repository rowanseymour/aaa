//! Special operations that do damage round where they're aimed.

use std::collections::HashMap;

use crate::supply::is;
use crate::{Damage, Flight, GameData, Id, Targets, defs};

/// A special operation that does damage round where it lands, as Carpet
/// Bomb does.
#[derive(Debug, Clone, PartialEq)]
pub struct Strike {
    /// What it can be given on and hits.
    pub targets: Targets,
    /// The radius it hits within, to the edge of what it hits.
    pub area: f32,
    /// Within this distance of its centre it does full damage...
    pub full_within: f32,
    /// ...falling to this fraction of it at the area's edge.
    pub edge_damage: f32,
    pub damage: Vec<Damage>,
    /// Damage it adds to what each one's own targets match, with no
    /// falloff, defence or least.
    pub extra: Vec<(Targets, Damage)>,
    /// Seconds from its use to its landing.
    pub delay: f32,
    /// What it shows its user round its centre from its use.
    pub sight: Option<Lookout>,
    /// What it leaves on each unit it hits.
    pub affliction: Option<Affliction>,
}

/// A property a strike leaves on each unit it hits for a while, such as
/// one that hurts it each second.
#[derive(Debug, Clone, PartialEq)]
pub struct Affliction {
    pub property: Id,
    pub seconds: f32,
    /// The property's enchantment type, which immunities name.
    pub kind: Option<Id>,
}

/// Ground a special operation shows its user while something it places
/// there wears out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lookout {
    pub range: f32,
    /// The once-a-second looks it lasts, the first within a second of its
    /// placing.
    pub looks: u32,
}

/// The strike an operation is, for one whose main damage falls on all it
/// hits round an area, rather than on one thing or on only some of what
/// it reaches: landing when its projectile's flight ends, at once without
/// one.
pub(crate) fn strike(
    a: &defs::AbilityDefinition,
    flights: &HashMap<Id, Flight>,
    lookouts: &HashMap<Id, Lookout>,
) -> Option<Strike> {
    let mains: Vec<&defs::Effect> = (a.effect.iter())
        .filter(|e| is(&e.kind, "MAIN_DAMAGE"))
        .collect();
    let own = |e: &&defs::Effect| {
        let targets = Targets::of(e.targets.as_deref());
        targets != Targets::default()
    };
    if a.area <= 0.0 || mains.is_empty() || mains.iter().any(own) {
        return None;
    }
    let main = mains.into_iter().filter_map(Damage::of).collect();
    let extra = (a.effect.iter())
        .filter(|e| is(&e.kind, "ADD_DAMAGE"))
        .filter_map(|e| {
            let targets = Targets::of(e.targets.as_deref());
            Some((targets, Damage::of(e)?))
        })
        .collect();
    let projectile = a.projectile_ids.as_ref().and_then(|p| flights.get(p));
    let sight = (a.effect.iter())
        .filter(|e| is(&e.kind, "SUMMON"))
        .find_map(|e| lookouts.get(e.object_ids.as_ref()?))
        .copied();
    let affliction = a.effect.iter().find_map(|e| {
        let p = e.enchantment_property.as_ref()?;
        Some(Affliction {
            property: p.ids.clone()?,
            seconds: p.duration.unwrap_or(0.0),
            kind: p.enchantment_type.clone(),
        })
    });
    Some(Strike {
        targets: Targets::of(a.targets.as_deref()),
        area: a.area,
        full_within: a.attenuation_range_min.unwrap_or(1.0),
        edge_damage: a.attenuation_factor_max.unwrap_or(1.0),
        damage: main,
        extra,
        delay: projectile.map_or(0.0, |f| f.life),
        sight,
        affliction,
    })
}

impl GameData {
    /// The ground things that see and wear out show while they last, by
    /// template id: for as many looks as their health lasts against their
    /// own properties' loss at each.
    pub(crate) fn lookouts(&self) -> HashMap<Id, Lookout> {
        let sights = self.sights();
        let decays = self.decays();
        let fighters = self.fighters();
        (sights.into_iter())
            .filter_map(|(id, sight)| {
                let rate = decays.get(&id).copied()?;
                let health = fighters.get(&id)?.health;
                let lookout = Lookout {
                    range: sight.range,
                    looks: (health / rate).ceil() as u32,
                };
                Some((id, lookout))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition(text: &str) -> defs::AbilityDefinition {
        let parsed = tgi::parse(text);
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("AbilityDefinition").next().unwrap();
        templates.deserialize(block).unwrap()
    }

    #[test]
    fn a_strike_lands_when_its_projectile_does_with_its_damage_and_sight() {
        let bomb = definition(
            "[AbilityDefinition]\n{\n\tIDS = bomb\n\ttargets = +ACTOR -AIR\n\tarea = 16\n\
             \tattenuation_range_min = 4\n\tattenuation_factor_max = 0.25\n\
             \tprojectile_ids = shell\n\
             \t[Effect]\n\t{\n\t\ttype = MAIN_DAMAGE\n\t\tdamage_type_IDS = explosive\n\t\tamount = 400\n\t}\n\
             \t[Effect]\n\t{\n\t\ttype = ADD_DAMAGE\n\t\tdamage_type_IDS = explosive\n\
             \t\ttargets = building\n\t\tamount = 1200\n\t}\n\
             \t[Effect]\n\t{\n\t\ttype = SUMMON\n\t\tobject_ids = eye\n\t}\n}\n",
        );
        let flight = Flight {
            kind: crate::FlightKind::Straight,
            speed: 100.0,
            life: 3.0,
        };
        let flights = HashMap::from([(Id::from("shell"), flight)]);
        let eye = Lookout {
            range: 30.0,
            looks: 5,
        };
        let lookouts = HashMap::from([(Id::from("eye"), eye)]);
        let s = strike(&bomb, &flights, &lookouts).unwrap();
        let explosive = |amount| Damage {
            kind: Id::from("explosive"),
            amount,
        };
        assert_eq!(s.damage, [explosive(400.0)]);
        assert_eq!(s.extra, [(Targets::parse("building"), explosive(1200.0))]);
        assert_eq!((s.area, s.full_within, s.edge_damage), (16.0, 4.0, 0.25));
        assert_eq!((s.delay, s.sight), (3.0, Some(eye)));
        assert_eq!(s.targets, Targets::parse("+ACTOR -AIR"));
        // Without a projectile it lands at once; without damage it's none.
        let mut now = bomb.clone();
        now.projectile_ids = None;
        assert_eq!(strike(&now, &flights, &lookouts).unwrap().delay, 0.0);
        let fear = definition("[AbilityDefinition]\n{\n\tIDS = fear\n}\n");
        assert_eq!(strike(&fear, &flights, &lookouts), None);
    }

    #[test]
    fn only_damage_on_all_round_an_area_is_a_strike() {
        let none = (HashMap::new(), HashMap::new());
        let strikes = |text: &str| strike(&definition(text), &none.0, &none.1).is_some();
        let main = "\t[Effect]\n\t{\n\t\ttype = MAIN_DAMAGE\n\t\tdamage_type_IDS = flame\n\t\tamount = 9\n";
        // Damage on one thing, with no area, as Industrial Sabotage's.
        assert!(!strikes(&format!(
            "[AbilityDefinition]\n{{\n{main}\t}}\n}}\n"
        )));
        // Damage only on what its own targets match, as Purge's.
        let only = format!(
            "[AbilityDefinition]\n{{\n\tarea = 20\n{main}\t\ttargets = commanding_officer\n\t}}\n}}\n"
        );
        assert!(!strikes(&only));
        assert!(strikes(&format!(
            "[AbilityDefinition]\n{{\n\tarea = 20\n{main}\t}}\n}}\n"
        )));
        // Templates give every effect targets of `null`, which is none.
        assert!(strikes(&format!(
            "[AbilityDefinition]\n{{\n\tarea = 20\n{main}\t\ttargets = null\n\t}}\n}}\n"
        )));
    }

    #[test]
    fn a_strike_that_gives_a_property_leaves_it_on_what_it_hits() {
        let bomb = definition(
            "[AbilityDefinition]\n{\n\tarea = 20\n\
             \t[Effect]\n\t{\n\t\ttype = MAIN_DAMAGE\n\t\tdamage_type_IDS = flame\n\t\tamount = 500\n\t}\n\
             \t[Effect]\n\t{\n\t\ttype = ENCHANTMENT\n\t\t[EnchantmentProperty]\n\t\t{\n\
             \t\t\tIDS = radiation\n\t\t\tduration = 10\n\t\t\tenchantment_type = rays\n\
             \t\t\t[Modifier]\n\t\t\t{\n\t\t\t\ttype = PERIODIC_DAMAGE\n\
             \t\t\t\tdamage_type = flame\n\t\t\t\tamount = 15\n\t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let s = strike(&bomb, &HashMap::new(), &HashMap::new()).unwrap();
        let radiation = Affliction {
            property: Id::from("radiation"),
            seconds: 10.0,
            kind: Some(Id::from("rays")),
        };
        assert_eq!(s.affliction, Some(radiation));
    }
}
