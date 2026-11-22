//! Props: what neither fights nor moves but stands in play, such as the
//! sight-givers and objective markers campaigns place and make.

use serde::{Deserialize, Serialize};
use tgmap::Id;

use crate::World;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prop {
    /// Its object's id, which triggers name it by.
    pub id: u32,
    pub template: Id,
    /// `None` for one that can't be owned.
    pub owner: Option<u32>,
    pub position: [f32; 2],
    pub facing: [f32; 2],
    /// The tick it was destroyed on, which leaves remains of one with
    /// health of its own.
    pub died: Option<u64>,
    /// Taken off the map, which leaves nothing.
    pub gone: bool,
}

impl Prop {
    pub(crate) fn placed(prop: &tgmap::Prop) -> Prop {
        Prop {
            id: prop.id,
            template: prop.template.clone(),
            owner: prop.owner,
            position: prop.position,
            facing: prop.facing,
            died: None,
            gone: false,
        }
    }

    /// Still in the world, if only as remains.
    pub fn there(&self) -> bool {
        !self.gone
    }

    /// In play: neither destroyed nor taken off the map.
    pub fn standing(&self) -> bool {
        !self.gone && self.died.is_none()
    }
}

impl World {
    /// Destroys the prop at `p` as if killed: one with no health of its
    /// own goes at once, one with health lies as remains. Whether it was
    /// standing.
    pub(crate) fn destroy_prop(&mut self, p: usize) -> bool {
        if !self.props[p].standing() {
            return false;
        }
        let template = &self.props[p].template;
        let health = self.rules.props.get(template).and_then(|t| t.health);
        let ticks = self.ticks;
        let prop = &mut self.props[p];
        match health {
            Some(_) => prop.died = Some(ticks),
            None => prop.gone = true,
        }
        true
    }

    /// Takes the prop at `p` off the map. Whether it was there.
    pub(crate) fn remove_prop(&mut self, p: usize) -> bool {
        let prop = &mut self.props[p];
        let there = prop.there();
        prop.gone = true;
        there
    }
}

crate::hash::digest!(Prop {
    id,
    template,
    owner,
    position,
    facing,
    died,
    gone,
});
