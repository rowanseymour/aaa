//! Things the map's triggers make in play, and the names they give them.

use tgmap::pathing::CELL_SIZE;
use tgmap::triggers::{Parameter, Value};
use tgmap::{Id, Shape};

use crate::summons::{FACING, rings};
use crate::{Prop, World, normalised};

/// How many rings round the place a clear spot is looked for in, a half
/// tile apart.
const RINGS: u32 = 8;

impl World {
    /// Makes `template` for the side `owner` at the clear spot nearest
    /// `at`, giving it `name` unless that's empty. Whether anything was
    /// made: nothing is for a name something in the world bears already.
    pub(crate) fn make_named(
        &mut self,
        template: &Id,
        owner: u32,
        at: [f32; 2],
        name: &str,
    ) -> bool {
        if !name.is_empty() && self.name_taken(name) {
            return false;
        }
        let id = match self.make(template, owner, at) {
            Some(id) => id,
            None => {
                log::debug!("triggers can't make {template} at {at:?}");
                return false;
            }
        };
        if !name.is_empty() {
            self.triggers.names.push((name.to_string(), id));
        }
        true
    }

    /// Makes `template` at the clear spot nearest `at`, returning its id.
    fn make(&mut self, template: &Id, owner: u32, at: [f32; 2]) -> Option<u32> {
        self.sides.player(owner)?;
        if let Some(kind) = self.rules.props.get(template).copied() {
            let (to, facing) = self.made_spot(at, Some(kind.shape))?;
            let id = self.take_ids(1);
            self.props.push(Prop {
                id,
                template: template.clone(),
                owner: kind.ownable.then_some(owner),
                position: to,
                facing,
                died: None,
                gone: false,
            });
            return Some(id);
        }
        let mut organization = self.rules.raisable.get(template)?.clone();
        organization.owner = owner;
        let r = match organization.lone() {
            true => {
                let (to, facing) = self.made_spot(at, organization.members[0].shape)?;
                organization.place(to, facing);
                self.add_new_organization(organization)
            }
            false => {
                let shape = self.rules.organization_shapes.get(template).copied();
                let (to, facing) = self.made_spot(at, shape)?;
                self.summon_regiment(organization, to, facing)?
            }
        };
        Some(self.organizations[r].id)
    }

    /// The first clear spot for something of `shape` at or round `at`,
    /// and the way it faces there: the default way at `at`, else away
    /// from it.
    fn made_spot(&self, at: [f32; 2], shape: Option<Shape>) -> Option<([f32; 2], [f32; 2])> {
        let spots = rings(at, FACING, CELL_SIZE / 2.0, RINGS);
        let to = self.first_clear(spots, FACING, shape, &self.blocking())?;
        let away = normalised([to[0] - at[0], to[1] - at[1]]);
        Some((to, away.unwrap_or(FACING)))
    }

    /// Whether something still in the world bears `name`.
    pub(crate) fn name_taken(&self, name: &str) -> bool {
        (self.triggers.names.iter())
            .any(|(n, id)| n.eq_ignore_ascii_case(name) && self.by_id(*id).is_some())
    }
}

/// The template a creating effect's parameter names, whichever kind of
/// template the effect offers.
pub(crate) fn made_template(p: Option<&Parameter>) -> Option<Id> {
    match p.map(|p| (p.source.as_str(), &p.value))? {
        ("mi_actor_data_named" | "mi_unit_data" | "mi_organization_data", Value::String(n))
            if !n.is_empty() =>
        {
            Some(Id::from(n.as_str()))
        }
        _ => None,
    }
}

/// The map position a parameter gives.
pub(crate) fn location(p: Option<&Parameter>) -> Option<[f32; 2]> {
    match p.map(|p| (p.source.as_str(), &p.value))? {
        ("mi_location", Value::Location([x, y, _])) => Some([*x, *y]),
        _ => None,
    }
}
