//! The sides in a map and the regiments they start with.

use std::collections::HashMap;

use aaa_data::{Body, Id, Layout};
use tgck::map::{Detail, Kingdoms, Relation, Team as StoredTeam, Thing};

/// A side: a player, human or AI, or an independent power such as
/// neutral civilians.
#[derive(Debug, Clone)]
pub struct Player {
    /// What objects name as their owner.
    pub id: u32,
    pub name: String,
    /// Its colour's id, e.g. `blue`.
    pub color: String,
    /// The colour that id names, white where the game data has none.
    pub rgb: [f32; 3],
    /// Index into [`Map::teams`](crate::Map::teams).
    pub team: Option<usize>,
    pub independent: bool,
    /// A person may play it.
    pub playable: bool,
    /// How it stands toward each player, by index in
    /// [`Map::players`](crate::Map::players).
    pub relations: Vec<Relation>,
    /// How it stands toward each team, by index in
    /// [`Map::teams`](crate::Map::teams).
    pub team_relations: Vec<Relation>,
}

/// Players that share victory.
#[derive(Debug, Clone)]
pub struct Team {
    pub id: u32,
    pub name: String,
    /// How it stands toward each player, by index in
    /// [`Map::players`](crate::Map::players).
    pub relations: Vec<Relation>,
    /// How it stands toward each team, by index in
    /// [`Map::teams`](crate::Map::teams).
    pub team_relations: Vec<Relation>,
}

/// The players and teams, with their relations by index rather than by
/// their positions in the stored sections, which can have gaps.
pub fn sides(
    kingdoms: &Kingdoms,
    teams: &[Option<StoredTeam>],
    colors: &HashMap<Id, [f32; 3]>,
) -> (Vec<Player>, Vec<Team>) {
    let kingdom_slots: Vec<bool> = kingdoms.kingdoms.iter().map(Option::is_some).collect();
    let team_slots: Vec<bool> = teams.iter().map(Option::is_some).collect();
    let by_player = |stored: &[u32]| by_index(&kingdom_slots, stored);
    let by_team = |stored: &[u32]| by_index(&team_slots, stored);
    let teams: Vec<&StoredTeam> = teams.iter().flatten().collect();
    let players = kingdoms
        .kingdoms
        .iter()
        .flatten()
        .map(|k| Player {
            id: k.thing.id,
            name: k.name.clone(),
            color: k.color.clone(),
            rgb: colors
                .get(&Id::from(k.color.as_str()))
                .copied()
                .unwrap_or([1.0; 3]),
            team: teams
                .iter()
                .position(|t| t.head.id == k.team && k.team != 0),
            independent: k.independent != 0,
            playable: k.playable,
            relations: by_player(&k.relations),
            team_relations: by_team(&k.team_relations),
        })
        .collect();
    let teams = teams
        .iter()
        .map(|t| Team {
            id: t.head.id,
            name: t.head.name.clone(),
            relations: by_player(&t.kingdom_relations),
            team_relations: by_team(&t.team_relations),
        })
        .collect();
    (players, teams)
}

/// Relations stored by section position, kept for the positions holding
/// something; neutral where none is stored.
fn by_index(slots: &[bool], stored: &[u32]) -> Vec<Relation> {
    slots
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .map(|(i, _)| {
            stored
                .get(i)
                .and_then(|&r| Relation::from_u32(r))
                .unwrap_or(Relation::Neutral)
        })
        .collect()
}

/// The player that owns a placed object, where the map records one.
pub fn owner(thing: &Thing) -> Option<u32> {
    match &thing.detail {
        Detail::Actor(actor) if actor.kingdom != 0 => Some(actor.kingdom),
        _ => None,
    }
}

/// A group of units that moves and fights together.
#[derive(Debug, Clone)]
pub struct Regiment {
    /// Its object's id, as in [`Map::things`](crate::Map::things).
    pub id: u32,
    pub template: String,
    /// The owning [`Player::id`].
    pub owner: u32,
    /// The way it faces, a unit vector.
    pub facing: [f32; 2],
    pub members: Vec<Member>,
    /// The arrangements it can take; at least one.
    pub formations: Vec<Formation>,
    /// Index into `formations` of the one it's in.
    pub formation: usize,
    /// The member whose position is the regiment's; without one, the
    /// members' average position is.
    pub primary: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Member {
    /// Where it stands and which way it faces.
    pub thing: Thing,
    /// Top speed in world units a second.
    pub speed: f32,
}

#[derive(Debug, Clone)]
pub struct Formation {
    /// E.g. `regiment_defensive`; empty for the arrangement the map placed
    /// a regiment in, where its layout doesn't fit its members.
    pub id: String,
    /// Per member, where it stands from the regiment's position: x to its
    /// right and y ahead.
    pub offsets: Vec<[f32; 2]>,
}

/// The owned organizations whose members can all move.
pub fn regiments(
    things: &[Thing],
    world_version: u32,
    bodies: &HashMap<Id, Body>,
    layouts: &HashMap<Id, Layout>,
) -> Vec<Regiment> {
    let speed = |t: &Thing| bodies.get(&Id::from(t.template.as_str()))?.speed;
    things
        .iter()
        .filter_map(|t| {
            let owner = owner(t)?;
            let organization = t.organization(world_version)?;
            let layout = layouts.get(&Id::from(organization.layout.as_str()));
            let arranged =
                layout.and_then(|l| arrange(l, &organization.slots, organization.members.len()));
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
            if members.is_empty() {
                return None;
            }
            let facing = match t.facing {
                [0.0, 0.0, _] => [0.0, -1.0],
                [fx, fy, _] => [fx, fy],
            };
            let (formations, formation, primary) =
                arranged.unwrap_or_else(|| (vec![as_placed(&members, facing)], 0, None));
            Some(Regiment {
                id: t.id,
                template: t.template.clone(),
                owner,
                facing,
                members,
                formations,
                formation,
                primary,
            })
        })
        .collect()
}

/// A layout's formations for the members filling `slots`, each slot's
/// element template or empty, and the member in the primary slot. `None`
/// where the layout doesn't account for every member.
fn arrange(
    layout: &Layout,
    slots: &[String],
    members: usize,
) -> Option<(Vec<Formation>, usize, Option<usize>)> {
    let filled = |i: usize| slots.get(i).is_some_and(|s| !s.is_empty());
    let formations: Vec<Formation> = layout
        .formations
        .iter()
        .map(|f| Formation {
            id: f.id.to_string(),
            offsets: f
                .slots
                .iter()
                .enumerate()
                .filter(|&(i, _)| filled(i))
                .flat_map(|(_, p)| p.iter().copied())
                .collect(),
        })
        .collect();
    if formations.iter().any(|f| f.offsets.len() != members) {
        return None;
    }
    let primary = layout.primary.filter(|&p| filled(p)).map(|p| {
        let slots = &layout.formations[layout.default].slots;
        (0..p).filter(|&i| filled(i)).map(|i| slots[i].len()).sum()
    });
    Some((formations, layout.default, primary))
}

/// Where the members stand from their average position.
fn as_placed(members: &[Member], [fx, fy]: [f32; 2]) -> Formation {
    let n = members.len() as f32;
    let (cx, cy) = members.iter().fold((0.0, 0.0), |(x, y), m| {
        (x + m.thing.position[0] / n, y + m.thing.position[1] / n)
    });
    Formation {
        id: String::new(),
        offsets: members
            .iter()
            .map(|m| {
                let (dx, dy) = (m.thing.position[0] - cx, m.thing.position[1] - cy);
                [dx * fy - dy * fx, dx * fx + dy * fy]
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_data::Formation as LayoutFormation;

    fn layout() -> Layout {
        // A front slot of two, an optional flank and the captain.
        let formation = |id: &str, dy: f32| LayoutFormation {
            id: Id::from(id),
            slots: vec![
                vec![[-1.0, dy], [1.0, dy]],
                vec![[3.0, 0.0]],
                vec![[0.0, 0.0]],
            ],
        };
        Layout {
            formations: vec![formation("aggressive", 2.0), formation("defensive", 1.0)],
            default: 1,
            primary: Some(2),
        }
    }

    #[test]
    fn arranges_the_filled_slots() {
        let slots = ["rifle".to_string(), String::new(), "captain".to_string()];
        let (formations, formation, primary) = arrange(&layout(), &slots, 3).unwrap();
        assert_eq!(formation, 1);
        assert_eq!(primary, Some(2));
        assert_eq!(formations[1].id, "defensive");
        assert_eq!(formations[1].offsets, [[-1.0, 1.0], [1.0, 1.0], [0.0, 0.0]]);
        assert_eq!(formations[0].offsets[0], [-1.0, 2.0]);
    }

    #[test]
    fn refuses_layouts_that_miss_members() {
        let slots = ["rifle".to_string(), String::new(), "captain".to_string()];
        assert!(arrange(&layout(), &slots, 4).is_none());
    }

    #[test]
    fn relations_skip_empty_slots() {
        use Relation::{Ally, Enemy, Neutral};
        assert_eq!(
            by_index(&[true, false, true, true], &[1, 0, 3]),
            [Ally, Enemy, Neutral]
        );
    }
}
