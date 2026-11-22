//! The idle buttons and their keys, which step through the player's things
//! of a kind one at a time: builders with nothing to do, and organizations
//! that have no HQ. Which are there, and whether each button shows, is
//! worked out here for both interfaces to draw.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use tgrender::camera::RtsCamera;
use tgrender::space::world;

use crate::intent::{Asked, Intent};
use crate::keys;
use crate::{Loaded, Local, Selected, SelectedBuilding, Sim};

/// What an idle button steps through.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Idle {
    /// Builders with nothing to do.
    Builders,
    /// Organizations with no HQ.
    Detached,
}

impl Idle {
    pub const ALL: [Idle; 2] = [Idle::Builders, Idle::Detached];

    /// What its key and button ask for.
    pub const fn asks(self) -> &'static str {
        match self {
            Idle::Builders => keys::CYCLE_IDLE,
            Idle::Detached => keys::CYCLE_DETACHED,
        }
    }

    /// The panel its button's laid out in.
    pub const fn widget(self) -> &'static str {
        match self {
            Idle::Builders => "IdleButtonBuilder",
            Idle::Detached => "IdleButtonDetachedOrganization",
        }
    }

    /// What its button's [`called`](crate::ui::called), in either
    /// interface.
    pub const fn called(self) -> &'static str {
        match self {
            Idle::Builders => "idle.builders",
            Idle::Detached => "idle.detached",
        }
    }

    /// The organizations of `player`'s it steps through, in order.
    fn candidates(self, sim: &tgsim::World, player: u32) -> Vec<usize> {
        match self {
            Idle::Builders => sim.idle_builders(player),
            Idle::Detached => sim.detached(player),
        }
    }

    /// Whether its button shows for `player`.
    fn shows(self, sim: &tgsim::World, player: u32) -> bool {
        match self {
            Idle::Builders => !sim.idle_builders(player).is_empty(),
            // Only for those an HQ could take, though the cycle takes in
            // those none could.
            Idle::Detached => sim.attachable(player),
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// Which idle buttons show.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shown([bool; 2]);

impl Shown {
    pub fn get(&self, idle: Idle) -> bool {
        self.0[idle.index()]
    }
}

/// Works out which idle buttons show, as the match changes.
pub fn refresh(sim: Res<Sim>, local: Res<Local>, mut shown: ResMut<Shown>) {
    if !sim.is_changed() && !local.is_changed() {
        return;
    }
    let now = Shown(Idle::ALL.map(|idle| local.0.is_some_and(|p| idle.shows(&sim.0, p))));
    shown.set_if_neq(now);
}

/// Lays each idle button out while there's something for it to step
/// through, in whichever interface it's drawn.
pub fn show(shown: Res<Shown>, mut buttons: Query<(&Idle, &mut Node)>) {
    for (&idle, mut node) in &mut buttons {
        crate::ui::show(&mut node, shown.get(idle));
    }
}

/// What stepping through them reads and changes.
#[derive(SystemParam)]
pub struct CycleParts<'w, 's> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    scenery: Res<'w, crate::banners::Scenery>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

impl CycleParts<'_, '_> {
    /// Selects the player's next one of `idle` after the one selected,
    /// round to the first again, and looks at it.
    fn step(&mut self, idle: Idle) {
        let Some(player) = self.local.0 else {
            return;
        };
        let mut candidates = idle.candidates(&self.sim.0, player);
        // Those that can't be selected are passed over.
        candidates.retain(|&r| !self.scenery.has(&self.sim.0, r));
        let Some(r) = next(&candidates, &self.selected.0) else {
            return;
        };
        self.selected.0 = vec![r];
        self.chosen.0 = None;
        let [x, y] = self.sim.0.leader_position(r);
        for mut camera in &mut self.cameras {
            camera.focus = world(x, y, self.loaded.map.ground.height_at([x, y]));
        }
    }
}

/// The one of `candidates` to select next, given what's `selected`: the
/// first after it, round to the first again; `None` where there's none,
/// or it's the one alone selected already.
fn next(candidates: &[usize], selected: &[usize]) -> Option<usize> {
    let after = selected.first().map_or(0, |&now| now + 1);
    let next = (candidates.iter())
        .find(|&&r| r >= after)
        .or(candidates.first());
    next.copied().filter(|&r| selected != [r])
}

/// Steps through what each idle key or button asked for steps through.
pub fn cycle(mut intents: MessageReader<Intent>, mut parts: CycleParts) {
    let asked = Asked::read(&mut intents);
    for idle in Idle::ALL {
        if asked.wants(idle.asks()) {
            parts.step(idle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_go_on_from_the_selection_and_round() {
        let candidates = [2, 5, 9];
        assert_eq!(next(&candidates, &[]), Some(2));
        assert_eq!(next(&candidates, &[2]), Some(5));
        assert_eq!(next(&candidates, &[6, 1]), Some(9));
        assert_eq!(next(&candidates, &[9]), Some(2));
        // Nothing to step to, or nothing else.
        assert_eq!(next(&[], &[3]), None);
        assert_eq!(next(&[4], &[4]), None);
        assert_eq!(next(&[4], &[4, 7]), Some(4));
    }

    #[test]
    fn each_button_is_laid_out_while_it_has_something_to_step_through() {
        let mut app = App::new();
        app.insert_resource(Shown([true, false]))
            .add_systems(Update, show);
        let builders = app
            .world_mut()
            .spawn((Idle::Builders, Node::default()))
            .id();
        let detached = app
            .world_mut()
            .spawn((Idle::Detached, Node::default()))
            .id();
        app.update();
        let display = |app: &App, e| app.world().get::<Node>(e).unwrap().display;
        assert_eq!(display(&app, builders), Display::Flex);
        assert_eq!(display(&app, detached), Display::None);
    }

    #[test]
    fn nothing_shows_with_no_side_to_play() {
        let mut app = App::new();
        let world = crate::special_ops::tests::operating(0.0);
        app.insert_resource(Sim(world))
            .insert_resource(Local(None))
            .insert_resource(Shown([true, true]))
            .add_systems(Update, refresh);
        app.update();
        assert_eq!(*app.world().resource::<Shown>(), Shown::default());
    }
}
