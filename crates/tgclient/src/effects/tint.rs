//! Tinting units for the properties they hold.

use std::collections::HashMap;

use bevy::prelude::*;
use tgdata::{Id, VfxTint};
use tgrender::Tint;
use tgsim::{Target, World};

use crate::Sim;
use crate::views::UnitView;

use super::Effects;
use super::notice::Seen;
use super::play::fits;

/// A tint a property shows: its effect, its pulse, and whether it runs on
/// the wall clock rather than game time.
pub(super) type Shown<'a> = (&'a Id, VfxTint, bool);

impl Effects {
    /// The tints the properties `held` show on `me`, in order.
    fn tints(&self, sim: &World, me: Target, held: &[Id]) -> Vec<Shown<'_>> {
        (held.iter())
            .flat_map(|p| self.properties.get(p).into_iter().flatten())
            .filter(|c| !c.stop && fits(sim, &c.targets, Some(me), Some(me)))
            .filter_map(|c| {
                let v = self.vfx.get(&c.vfx)?;
                Some((&c.vfx, v.tint?, v.real_time))
            })
            .collect()
    }
}

/// On a unit tinted for the properties it holds: when each of their tints
/// began, on the clock it runs on, so that one a stronger tint hid carries
/// on its pulse once that goes.
#[derive(Component, Default)]
pub(super) struct Tinting {
    pub(super) since: HashMap<Id, f64>,
}

impl Tinting {
    /// The colour of the first of `tints` with the highest priority, at
    /// `clocks`' game and real times.
    pub(super) fn colour(&mut self, tints: &[Shown], [game, real]: [f64; 2]) -> Option<[f32; 3]> {
        let now = |real_time| if real_time { real } else { game };
        (self.since).retain(|id, _| tints.iter().any(|(t, ..)| *t == id));
        for &(id, _, real_time) in tints {
            self.since.entry(id.clone()).or_insert(now(real_time));
        }
        let &(id, pulse, real_time) = (tints.iter().rev()).max_by_key(|(_, t, _)| t.priority)?;
        let steps = crate::supply::TINT_STEPS;
        let age = (now(real_time) - self.since[id]) as f32;
        let level = (pulse.level(age) * steps).round() / steps;
        Some(pulse.colour(level))
    }
}

/// Tints each unit for the properties it holds, and plain again at once
/// when it holds none that tint.
#[allow(clippy::type_complexity)]
pub(super) fn tint_units(
    mut commands: Commands,
    sim: Res<Sim>,
    effects: Res<Effects>,
    real: Res<Time<Real>>,
    mut views: Query<(
        Entity,
        &UnitView,
        &Seen,
        Option<&mut Tinting>,
        Option<&Tint>,
    )>,
) {
    let sim = &sim.0;
    let clocks = [
        sim.ticks as f64 * f64::from(tgsim::TICK),
        real.elapsed_secs_f64(),
    ];
    for (e, view, seen, tinting, tint) in &mut views {
        let tints = effects.tints(sim, Target::Unit(view.unit), &seen.held);
        let tinted = tinting.is_some() || tint.is_some();
        let colour = match tinting {
            Some(mut t) => t.colour(&tints, clocks),
            None => {
                let mut t = Tinting::default();
                let colour = t.colour(&tints, clocks);
                if colour.is_some() {
                    commands.entity(e).insert(t);
                }
                colour
            }
        };
        match colour.map(Tint) {
            Some(colour) if tint != Some(&colour) => drop(commands.entity(e).insert(colour)),
            Some(_) => {}
            None if tinted => drop(commands.entity(e).remove::<(Tinting, Tint)>()),
            None => {}
        }
    }
}
