//! What the data says of each state things can be in, beyond what other
//! subjects read of them.

use std::collections::HashSet;
use std::sync::Arc;

use crate::{GameData, Id, defs};

/// The ids of `states` whose type is `busy`, as the first to define each
/// has it.
fn busy(states: impl IntoIterator<Item = defs::ActorState>) -> HashSet<Id> {
    let mut seen = HashSet::new();
    let mut out = HashSet::new();
    for state in states {
        let Some(id) = state.ids else { continue };
        if seen.insert(id.clone()) && state.kind.is_some_and(|k| k.eq_ignore_ascii_case("busy")) {
            out.insert(id);
        }
    }
    out
}

/// The ids of `states` whose `formation_required` is `BROKEN`, as the
/// first to define each has it.
fn formationless(states: impl IntoIterator<Item = defs::ActorState>) -> HashSet<Id> {
    let mut seen = HashSet::new();
    let mut out = HashSet::new();
    for state in states {
        let Some(id) = state.ids else { continue };
        let broken =
            (state.formation_required.as_deref()).is_some_and(|f| f.eq_ignore_ascii_case("broken"));
        if seen.insert(id.clone()) && broken {
            out.insert(id);
        }
    }
    out
}

impl GameData {
    /// The states whose type is `busy`, which changes what a building in
    /// one offers.
    pub fn busy_states(&self) -> HashSet<Id> {
        busy(self.actor_states().iter().cloned())
    }

    /// The states a regiment has no formation in, whose
    /// `formation_required` is `BROKEN`, as the first to define each has it.
    pub fn formationless_states(&self) -> HashSet<Id> {
        formationless(self.actor_states().iter().cloned())
    }

    /// Every state the state files define, in the order they load, those
    /// defined twice included.
    pub(crate) fn actor_states(&self) -> Arc<Vec<defs::ActorState>> {
        self.kept("actor_states", || {
            self.read_all(|n| n.starts_with("game/actor_states"), "ActorState")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_states_typed_busy_are_busy_and_the_first_definition_wins() {
        let parsed = tgi::parse(
            "[ActorState]\n{\n\tIDS = structure_packing\n\ttype = busy\n}\n\
             [ActorState]\n{\n\tIDS = structure_recruiting\n\ttype = queue\n}\n\
             [ActorState]\n{\n\tIDS = mind_resting\n}\n\
             [ActorState]\n{\n\tIDS = structure_razing\n\ttype = BUSY\n}\n\
             [ActorState]\n{\n\tIDS = structure_packing\n\ttype = none\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let states =
            (parsed.root.blocks_of("ActorState")).map(|b| templates.deserialize(b).unwrap());
        let mut got: Vec<_> = busy(states).into_iter().collect();
        got.sort();
        assert_eq!(
            got,
            [Id::from("structure_packing"), Id::from("structure_razing")]
        );
    }

    #[test]
    fn states_whose_formation_required_is_broken_have_no_formation() {
        let parsed = tgi::parse(
            "[ActorState]\n{\n\tIDS = mind_routing\n\tformation_required = BROKEN\n}\n\
             [ActorState]\n{\n\tIDS = mind_moving\n\tformation_required = DESIRED\n}\n\
             [ActorState]\n{\n\tIDS = mind_razing\n\tformation_required = broken\n}\n\
             [ActorState]\n{\n\tIDS = mind_resting\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let states =
            (parsed.root.blocks_of("ActorState")).map(|b| templates.deserialize(b).unwrap());
        let mut got: Vec<_> = formationless(states).into_iter().collect();
        got.sort();
        assert_eq!(got, [Id::from("mind_razing"), Id::from("mind_routing")]);
    }
}
