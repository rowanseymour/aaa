//! The player's control groups: what's selected saved under a digit and
//! selected again by it, and the Group Behavior switch on the selection's
//! current group, which has orders go to the selection as one group.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use tgrender::camera::RtsCamera;
use tgrender::terrain::world;

use crate::intent::{Asked, Intent};
use crate::{Loaded, Local, Selected, SelectedBuilding, Sim};

/// What the hotkey files ask for to save and select group n, as
/// `SaveGroup group_1`; the last is `group_10`.
const SAVE: &str = "SaveGroup group_";
const SELECT: &str = "SelectGroup group_";

/// How many groups the game's keys save.
const GROUPS: usize = 10;

/// How soon a second press selecting a group still selected looks at it
/// instead, in seconds (`UICenterGroupTimeMS`).
const CENTRE_AGAIN: f64 = 1.0;

/// The commands that turn Group Behavior on and off.
pub const ENABLE: &str = "group_behavior_enable";
pub const DISABLE: &str = "group_behavior_disable";

/// One of the player's saved groups.
#[derive(Clone, Default, PartialEq, Debug)]
struct Saved {
    /// By index into the sim's organizations.
    organizations: Vec<usize>,
    /// A building saved on its own.
    building: Option<usize>,
    /// Its Group Behavior switch.
    behavior: bool,
}

impl Saved {
    fn is_empty(&self) -> bool {
        self.organizations.is_empty() && self.building.is_none()
    }

    /// Whether it holds exactly the organizations `selected`.
    fn is(&self, selected: &[usize]) -> bool {
        self.building.is_none()
            && self.organizations.len() == selected.len()
            && selected.iter().all(|r| self.organizations.contains(r))
    }
}

/// The player's saved groups, and the switch of the one group that stands
/// in for any other selection of two or more.
#[derive(Resource, Clone, Default, Debug)]
pub struct ControlGroups {
    saved: [Saved; GROUPS],
    stand_in: bool,
    /// The group last selected, and when.
    selected: Option<(usize, f64)>,
}

impl ControlGroups {
    /// The Group Behavior switch of `selected`'s current group: the saved
    /// group that's exactly it, else the stand-in; `None` for fewer than
    /// two, which have no group.
    pub fn behavior(&self, selected: &[usize]) -> Option<bool> {
        (selected.len() >= 2).then(|| match self.saved.iter().find(|g| g.is(selected)) {
            Some(g) => g.behavior,
            None => self.stand_in,
        })
    }

    /// Whether orders to `selected` go to it as one group.
    pub fn grouping(&self, selected: &[usize]) -> bool {
        self.behavior(selected) == Some(true)
    }

    /// Flips the switch of `selected`'s current group.
    pub fn toggle(&mut self, selected: &[usize]) {
        if selected.len() < 2 {
            return;
        }
        match self.saved.iter_mut().find(|g| g.is(selected)) {
            Some(g) => g.behavior = !g.behavior,
            None => self.stand_in = !self.stand_in,
        }
    }

    /// Saves the player's own `organizations`, or else their `building`,
    /// as group `n`: whatever it held goes, and what's saved leaves any
    /// other group, a group left empty going. It takes the switch of the
    /// group it was saved from.
    fn save(&mut self, n: usize, organizations: Vec<usize>, building: Option<usize>) {
        let behavior = self.behavior(&organizations).unwrap_or(false);
        let building = building.filter(|_| organizations.is_empty());
        for g in &mut self.saved {
            g.organizations.retain(|r| !organizations.contains(r));
            if building.is_some() && g.building == building {
                g.building = None;
            }
            if g.is_empty() {
                *g = Saved::default();
            }
        }
        self.saved[n] = Saved {
            organizations,
            building,
            behavior,
        };
    }
}

/// What saving and selecting groups reads and changes.
#[derive(SystemParam)]
pub struct Recall<'w, 's> {
    groups: ResMut<'w, ControlGroups>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    time: Res<'w, Time<Real>>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

/// The group, from 0, that `intent` names after `prefix`.
fn numbered(intent: &Intent, prefix: &str) -> Option<usize> {
    let n: usize = intent.0.strip_prefix(prefix)?.parse().ok()?;
    (1..=GROUPS).contains(&n).then_some(n - 1)
}

/// Saves and selects groups as the keys ask.
pub fn recall(mut intents: MessageReader<Intent>, mut recall: Recall) {
    let asked = Asked::read(&mut intents);
    let Some(player) = recall.local.0 else {
        return;
    };
    for intent in asked.all() {
        if let Some(n) = numbered(intent, SAVE) {
            let sim = &recall.sim.0;
            let own = |&r: &usize| sim.organizations[r].owner == player;
            let organizations = recall.selected.0.iter().copied().filter(own).collect();
            let building = recall
                .chosen
                .0
                .filter(|&b| sim.buildings[b].owner == player);
            recall.groups.save(n, organizations, building);
        } else if let Some(n) = numbered(intent, SELECT) {
            recall.select(n);
        }
    }
}

impl Recall<'_, '_> {
    /// Selects what's left of group `n`, or a second time soon after
    /// while it's still exactly what's selected, looks at it instead.
    fn select(&mut self, n: usize) {
        let sim = &self.sim.0;
        let now = self.time.elapsed_secs_f64();
        let group = &mut self.groups.saved[n];
        group.organizations.retain(|&r| !sim.destroyed(r));
        if group.building.is_some_and(|b| !sim.buildings[b].standing()) {
            group.building = None;
        }
        if group.is_empty() {
            return;
        }
        let again = self
            .groups
            .selected
            .is_some_and(|(m, at)| m == n && now - at <= CENTRE_AGAIN);
        let group = &self.groups.saved[n];
        let still =
            group.is(&self.selected.0) || group.building.is_some_and(|b| self.chosen.0 == Some(b));
        if again && still {
            let n = group.organizations.len() + usize::from(group.building.is_some());
            let at = (group.organizations.iter().map(|&r| sim.centre(r)))
                .chain(group.building.map(|b| sim.buildings[b].position))
                .fold([0.0, 0.0], |[x, y], [a, b]| {
                    [x + a / n as f32, y + b / n as f32]
                });
            let [x, y] = at;
            for mut camera in &mut self.cameras {
                camera.focus = world(x, y, self.loaded.map.ground.height_at([x, y]));
            }
        } else {
            self.selected.0 = group.organizations.clone();
            self.chosen.0 = group.building;
        }
        self.groups.selected = Some((n, now));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_group_has_its_own_switch_and_any_other_selection_the_stand_ins() {
        let mut groups = ControlGroups::default();
        assert_eq!(groups.behavior(&[3]), None);
        assert_eq!(groups.behavior(&[3, 4]), Some(false));
        groups.toggle(&[3, 4]);
        // One switch for every unsaved selection.
        assert!(groups.grouping(&[5, 6, 7]));
        // Saved from it, a group takes its switch, then keeps its own.
        groups.save(0, vec![3, 4], None);
        assert!(groups.grouping(&[4, 3]));
        groups.toggle(&[3, 4]);
        assert!(!groups.grouping(&[3, 4]) && groups.grouping(&[5, 6]));
    }

    #[test]
    fn a_thing_is_in_one_group_at_most() {
        let mut groups = ControlGroups::default();
        groups.save(0, vec![1, 2], None);
        groups.save(1, vec![2, 3], None);
        assert_eq!(groups.saved[0].organizations, [1]);
        groups.save(2, vec![1], None);
        assert!(groups.saved[0].is_empty());
        assert_eq!(
            numbered(&Intent::from("SelectGroup group_10"), SELECT),
            Some(9)
        );
        assert_eq!(
            numbered(&Intent::from("SelectGroup group_11"), SELECT),
            None
        );
    }
}
