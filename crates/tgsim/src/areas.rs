//! Who is in each of the map's areas, for the triggers that test them.

use std::sync::Arc;

use tgmap::{Area, Obstacle, Shape, Tile};

use crate::World;
use crate::targets::Thing;
use crate::triggers::Happening;

impl World {
    /// Rechecks who is in each area, posting an entry for each thing newly
    /// in one. A thing that leaves is dropped, so it enters again on coming
    /// back.
    pub(crate) fn check_areas(&mut self) {
        let areas = Arc::clone(&self.triggers.areas);
        if areas.is_empty() {
            return;
        }
        let placed: Vec<(Thing, Obstacle)> = self.placed().collect();
        for (i, area) in areas.iter().enumerate() {
            let now: Vec<Thing> = (placed.iter())
                .filter(|(_, o)| area.holds(o))
                .map(|&(t, _)| t)
                .collect();
            for &thing in &now {
                if !self.triggers.inside[i].contains(&thing) {
                    let area = area.id;
                    self.triggers
                        .happened
                        .push(Happening::Entered { area, thing });
                }
            }
            self.triggers.inside[i] = now;
        }
    }

    /// Every thing that can be in an area, with its soft shape where it
    /// stands.
    fn placed(&self) -> impl Iterator<Item = (Thing, Obstacle)> + '_ {
        let units = (0..self.units.len()).map(Thing::Unit);
        let organizations = (0..self.organizations.len()).map(Thing::Organization);
        let buildings = (0..self.buildings.len()).map(Thing::Building);
        (units.chain(organizations).chain(buildings)).filter_map(|t| Some((t, self.soft_place(t)?)))
    }

    /// `t`'s soft shape where it stands, if it can be in an area: a living
    /// unit that fights, a building with health, or a regiment with members
    /// on the map, which stands where its leader does.
    pub(crate) fn soft_place(&self, t: Thing) -> Option<Obstacle> {
        let (position, facing, soft) = match t {
            // Found by name alone, and taken as where it stands.
            Thing::Prop(p) => {
                let prop = self.props.get(p).filter(|p| p.standing())?;
                (prop.position, prop.facing, Shape::Point)
            }
            Thing::Unit(u) => {
                let unit = &self.units[u];
                let fighter = unit.fighter.as_ref().filter(|_| unit.alive())?;
                (unit.position, unit.facing, fighter.soft)
            }
            Thing::Building(b) => {
                let building = &self.buildings[b];
                if building.gone() || building.health <= 0.0 {
                    return None;
                }
                (building.position, building.facing, building.fighter.soft)
            }
            Thing::Organization(r) => {
                if self.destroyed(r) || self.survivors(r).next().is_none() {
                    return None;
                }
                let organization = &self.organizations[r];
                let leader = (organization.leader)
                    .map(|l| organization.units[l])
                    .filter(|&u| self.units[u].alive());
                let position = match leader {
                    Some(u) => self.units[u].position,
                    None => self.centre(r),
                };
                let soft = self.triggers.organization_soft.get(&organization.template);
                (
                    position,
                    organization.facing,
                    soft.copied().unwrap_or(Shape::Point),
                )
            }
        };
        Some(Obstacle {
            position,
            facing,
            shape: soft,
            soft,
        })
    }

    /// The area triggers name by this id, by index into
    /// [`Triggers::areas`](crate::triggers::Triggers).
    pub(crate) fn area(&self, id: u32) -> Option<usize> {
        self.triggers.areas.iter().position(|a| a.id == id)
    }

    /// The first thing found in `area` by the test its list keeps, going
    /// over its tiles row by row from the lowest y and, in a row, the
    /// lowest x.
    pub(crate) fn first_in(&self, area: &Area) -> Option<Thing> {
        let first = |&(t, ref o): &(Thing, Obstacle)| {
            let Tile { x, y } = area.first_tile(o)?;
            Some(((y, x), t))
        };
        let found: Vec<(Thing, Obstacle)> = self.placed().collect();
        (found.iter().filter_map(first))
            .min_by_key(|&(at, _)| at)
            .map(|(_, t)| t)
    }
}
