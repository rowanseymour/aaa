//! How long the remains of what dies lie on the map before they're taken
//! off, and the deaths units play before that time starts.

use std::collections::HashMap;

use crate::{GameData, Id, defs};

/// The seconds the remains of a body that sets none rot, then sink.
const ROT_TIME: f32 = 0.0;
const SINK_TIME: f32 = 2.0;

/// The idle loops a view that sets none plays before a fidget: at least
/// the first, fewer than the second.
const IDLE_LOOPS: [u32; 2] = [1, 3];

/// The seconds a death animation that sets no length takes.
const DEATH_LENGTH: f32 = 1.0;

/// A death animation, as what picks one sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Death {
    /// Which of its kind, e.g. `die2`.
    pub ids: Id,
    /// The [`GameData::damage_filters`] value of the last hit it plays
    /// after; [`ANY_DAMAGE`] for any.
    pub filter: i32,
    /// The least damage that hit did, where there are others to choose.
    pub threshold: f32,
    pub weight: f32,
    /// Seconds it takes.
    pub length: f32,
}

/// The [`Death::filter`] that fits every hit.
pub const ANY_DAMAGE: i32 = -1;

/// What a unit no enemy has hit dies of, for [`Death::filter`].
pub const BLEEDING: i32 = 666;

/// The animations a character plays in one of its looks, as the
/// simulation sees them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AnimationSet {
    /// The `[Aspect]` they're for; `None` for the character's own set,
    /// which its plain look plays.
    pub look: Option<Id>,
    /// The deaths it chooses from as it dies in this look.
    pub deaths: Vec<Death>,
    /// Seconds each animation it plays once and then moves on from
    /// takes, such as an attack's, by ids. A look's set has those of the
    /// character's own set it doesn't list itself.
    pub lengths: HashMap<Id, f32>,
    /// Seconds one loop of the idle it plays standing still takes: its
    /// first `IDLE`'s, or for a look without one, the character's own.
    pub idle: f32,
    /// How many idle loops it plays before a fidget: at least the first,
    /// and fewer than the second.
    pub idle_loops: [u32; 2],
    /// The fidgets it picks from after idling, by their ids, each with its
    /// weight; a look's set has those of the character's own set it
    /// doesn't list itself.
    pub fidgets: Vec<(Id, f32)>,
}

/// A template's animation sets, its own first, then each look's in the
/// order it lists them; none for one without a character. Aircraft
/// never play a death.
fn animation_sets(t: &defs::Thing) -> Option<Vec<AnimationSet>> {
    let flying = t.flags.iter().rev().find_map(|f| f.flying) == Some(true);
    let character = t.character_component.as_ref()?;
    let kind =
        |a: &defs::Animation, k: &str| a.kind.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(k));
    let least = t
        .view
        .as_ref()
        .and_then(|v| v.idle_loops_min)
        .unwrap_or(IDLE_LOOPS[0]);
    let most = t
        .view
        .as_ref()
        .and_then(|v| v.idle_loops_max)
        .unwrap_or(IDLE_LOOPS[1]);
    let set = |look: Option<Id>, animations: &[defs::Animation]| {
        let dies = animations.iter().filter(|a| kind(a, "die") && !flying);
        let deaths = dies
            .map(|a| Death {
                ids: Id::from(a.ids.as_deref().unwrap_or("")),
                filter: a.damage_filter.unwrap_or(ANY_DAMAGE),
                threshold: a.damage_threshold.unwrap_or(0.0),
                weight: a.weight.unwrap_or(1.0),
                length: a.length.unwrap_or(DEATH_LENGTH),
            })
            .collect();
        let once = animations
            .iter()
            .filter(|a| !["die", "idle", "move"].iter().any(|k| kind(a, k)));
        let lengths = once
            .filter_map(|a| Some((Id::from(a.ids.as_deref()?), a.length.unwrap_or(1.0))))
            .collect();
        let idle = (animations.iter().find(|a| kind(a, "idle"))).map(|a| a.length.unwrap_or(1.0));
        let fidgets = (animations.iter().filter(|a| kind(a, "fidget")))
            .filter_map(|a| Some((Id::from(a.ids.as_deref()?), a.weight.unwrap_or(1.0))))
            .collect();
        AnimationSet {
            look,
            deaths,
            lengths,
            idle: idle.unwrap_or(0.0),
            idle_loops: [least, most],
            fidgets,
        }
    };
    let own = set(None, &character.animation);
    let looks = character.aspect_animation_set.iter().map(|s| {
        let look = s.aspect_ids.as_deref().map(Id::from);
        let mut set = set(look, &s.animation);
        for (ids, length) in &own.lengths {
            set.lengths.entry(ids.clone()).or_insert(*length);
        }
        let theirs: Vec<(Id, f32)> = (own.fidgets.iter())
            .filter(|(ids, _)| !set.fidgets.iter().any(|(f, _)| f == ids))
            .cloned()
            .collect();
        set.fidgets.extend(theirs);
        if !s.animation.iter().any(|a| kind(a, "idle")) {
            set.idle = own.idle;
        }
        set
    });
    let looks: Vec<AnimationSet> = looks.collect();
    Some(std::iter::once(own).chain(looks).collect())
}

impl GameData {
    /// Seconds each template's things' remains lie rotting once dead, then
    /// sink before they leave the map, by template id, for those with a
    /// body.
    pub fn remains(&self) -> HashMap<Id, [f32; 2]> {
        self.by_template(|t| {
            let body = t.body_component.as_ref()?;
            Some([
                body.rot_time.unwrap_or(ROT_TIME),
                body.sink_time.unwrap_or(SINK_TIME),
            ])
        })
    }

    /// Each template's animation sets, for those with a character.
    pub fn animation_sets(&self) -> HashMap<Id, Vec<AnimationSet>> {
        self.by_template(animation_sets)
    }

    /// Each kind of damage's number for death animations, by its id.
    pub fn damage_filters(&self) -> HashMap<Id, i32> {
        let types: Vec<defs::DamageType> =
            self.read_all(|n| n == "game/damage_types.tgi", "DamageType");
        (types.into_iter())
            .filter_map(|d| Some((d.ids?, d.damage_filter?)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_look_has_its_own_deaths_and_takes_the_rest_of_its_lengths_from_the_plain_set() {
        let parsed = tgi::parse(
            "[Thing]\n{\n\t[CharacterComponent]\n\t{\n\
             \t\t[Animation]\n\t\t{\n\t\t\ttype = DIE\n\t\t\tIDS = die1\n\t\t\tlength = 2\n\t\t}\n\
             \t\t[Animation]\n\t\t{\n\t\t\ttype = ABILITY\n\t\t\tIDS = attack1\n\t\t\tlength = 0.5\n\t\t}\n\
             \t\t[Animation]\n\t\t{\n\t\t\ttype = ABILITY\n\t\t\tIDS = throw\n\t\t\tlength = 1.5\n\t\t}\n\
             \t\t[Animation]\n\t\t{\n\t\t\ttype = IDLE\n\t\t\tIDS = idle\n\t\t\tlength = 3\n\t\t}\n\
             \t\t[Animation]\n\t\t{\n\t\t\ttype = FIDGET\n\t\t\tIDS = fidget1\n\t\t\tweight = 5\n\t\t}\n\
             \t\t[AspectAnimationSet]\n\t\t{\n\t\t\taspect_ids = kneel\n\
             \t\t\t[Animation]\n\t\t\t{\n\t\t\t\ttype = ABILITY\n\t\t\t\tIDS = attack1\n\t\t\t\tlength = 0.8\n\t\t\t}\n\
             \t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let sets = animation_sets(&thing).unwrap();
        assert_eq!(sets.len(), 2);
        assert_eq!((sets[0].look.clone(), sets[0].deaths.len()), (None, 1));
        assert_eq!(sets[0].lengths.len(), 3, "not the idle");
        assert_eq!((sets[0].idle, sets[0].idle_loops), (3.0, [1, 3]));
        let kneel = &sets[1];
        assert_eq!(kneel.look, Some(Id::from("kneel")));
        assert!(kneel.deaths.is_empty());
        assert_eq!(kneel.lengths[&Id::from("attack1")], 0.8);
        assert_eq!(kneel.lengths[&Id::from("throw")], 1.5);
        // It idles and fidgets as the character does.
        assert_eq!(kneel.idle, 3.0);
        assert_eq!(kneel.fidgets, [(Id::from("fidget1"), 5.0)]);
    }
}
