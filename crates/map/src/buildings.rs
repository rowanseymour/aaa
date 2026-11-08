//! The buildings a map starts with, such as houses, bunkers and cities'
//! centres.

use std::collections::HashMap;

use aaa_data::{Body, DeployPoints, Fighter, Id};
use tgck::map::{Detail, Thing};

use crate::pathing::Obstacle;
use crate::units::{facing, owner};

/// An owned object that can be hurt but not move and isn't a regiment.
#[derive(Debug, Clone)]
pub struct Building {
    /// Its object's id, as in [`Map::things`](crate::Map::things).
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`](crate::Player::id).
    pub owner: u32,
    pub position: [f32; 2],
    /// The way its model's -y axis points, a unit vector.
    pub facing: [f32; 2],
    pub fighter: Fighter,
    /// Its health as a fraction of its fighter's.
    pub health: f32,
    /// For a city's centre, how far around it the city reaches.
    pub zone: Option<f32>,
    /// For a city's centre, the ids of the buildings that make up the city
    /// and change hands with it.
    pub members: Vec<u32>,
    /// The ground it keeps movers off, if any.
    pub obstacle: Option<Obstacle>,
    /// Where what it recruits appears and goes, for one that recruits.
    pub deploy: Option<DeployPoints>,
}

/// The owned objects that can be hurt but not move and aren't regiments,
/// placed on their own or standing in an organization, as a Division HQ's
/// building does while it's unpacked.
pub fn buildings(
    things: &[Thing],
    bodies: &HashMap<Id, Body>,
    fighters: &HashMap<Id, Fighter>,
    zones: &HashMap<Id, f32>,
    obstacles: &HashMap<u32, Obstacle>,
    deploy_points: &HashMap<Id, DeployPoints>,
) -> Vec<Building> {
    standing(things)
        .filter_map(|(t, owner)| {
            let owner = owner?;
            let template = Id::from(t.template.as_str());
            let fighter = fighters.get(&template)?;
            let moves = bodies.get(&template).is_some_and(|b| b.speed.is_some());
            if moves || t.organization().is_some() {
                return None;
            }
            let Detail::Actor(actor) = &t.detail else {
                return None;
            };
            let [x, y, _] = t.position;
            Some(Building {
                id: t.id,
                template: t.template.clone(),
                owner,
                position: [x, y],
                facing: facing(t),
                fighter: fighter.clone(),
                health: actor.body().map_or(1.0, |b| b.health),
                zone: zones.get(&template).copied(),
                members: actor
                    .city()
                    .and_then(|c| c.buildings.clone())
                    .unwrap_or_default(),
                obstacle: obstacles.get(&t.id).cloned(),
                deploy: deploy_points.get(&template).copied(),
            })
        })
        .collect()
}

/// The placed objects and the members of their organizations, which stand
/// on the map as objects do, each with its owner: its own, else its
/// organization's.
pub(crate) fn standing(things: &[Thing]) -> impl Iterator<Item = (&Thing, Option<u32>)> {
    things.iter().flat_map(|t| {
        let within = owner(t);
        let members = t.organization().map_or(&[][..], |o| &o.members[..]);
        let members = members.iter().map(move |m| (m, owner(m).or(within)));
        std::iter::once((t, within)).chain(members)
    })
}
