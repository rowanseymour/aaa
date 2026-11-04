//! The sides in a map and the regiments they start with.

use std::collections::HashMap;

use aaa_data::{Body, Id};
use tgck::map::{Detail, Kingdoms, Thing};

/// A side: a player, human or AI, or an independent power such as
/// neutral civilians.
#[derive(Debug, Clone)]
pub struct Player {
    /// What objects name as their owner.
    pub id: u32,
    pub name: String,
    pub color: String,
    /// The team's id; 0 for none.
    pub team: u32,
    pub independent: bool,
}

pub fn players(kingdoms: &Kingdoms) -> Vec<Player> {
    kingdoms
        .kingdoms
        .iter()
        .flatten()
        .map(|k| Player {
            id: k.thing.id,
            name: k.name.clone(),
            color: k.color.clone(),
            team: k.team,
            independent: k.independent != 0,
        })
        .collect()
}

/// A group of units that moves and fights together.
#[derive(Debug, Clone)]
pub struct Regiment {
    /// Its object's id, as in [`Map::things`](crate::Map::things).
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`].
    pub owner: u32,
    pub members: Vec<Member>,
}

#[derive(Debug, Clone)]
pub struct Member {
    /// Where it stands and which way it faces.
    pub thing: Thing,
    /// Top speed in world units a second.
    pub speed: f32,
}

/// The owned organizations whose members can all move.
pub fn regiments(
    things: &[Thing],
    world_version: u32,
    bodies: &HashMap<Id, Body>,
) -> Vec<Regiment> {
    let speed = |t: &Thing| bodies.get(&Id::from(t.template.as_str()))?.speed;
    things
        .iter()
        .filter_map(|t| {
            let Detail::Actor(actor) = &t.detail else {
                return None;
            };
            if actor.kingdom == 0 {
                return None;
            }
            let organization = t.organization(world_version)?;
            let members = organization
                .members
                .into_iter()
                .map(|m| {
                    Some(Member {
                        speed: speed(&m)?,
                        thing: m,
                    })
                })
                .collect::<Option<Vec<_>>>()?;
            (!members.is_empty()).then(|| Regiment {
                id: t.id,
                template: t.template.clone(),
                owner: actor.kingdom,
                members,
            })
        })
        .collect()
}
