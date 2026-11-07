//! The sides in a map and the regiments they start with.

use std::collections::{HashMap, HashSet};

use aaa_data::{Body, DeployPoints, Deployment, Fighter, GameData, Id, Layout, Organization};
use tgck::map::{Detail, Kingdoms, Relation, Team as StoredTeam, Thing};

use crate::buildings::Building;
use crate::pathing::Obstacle;

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
    /// The nation it plays as, e.g. `nation_usa`, where the map fixes one or
    /// it was loaded with one.
    pub nation: Option<String>,
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

/// How the player `from` stands toward the player `to`, by their ids;
/// neutral toward a player that isn't among `players`.
pub fn relation(players: &[Player], teams: &[Team], from: u32, to: u32) -> Relation {
    if from == to {
        return Relation::Itself;
    }
    let index = |id| players.iter().position(|p| p.id == id);
    let (Some(a), Some(b)) = (index(from), index(to)) else {
        return Relation::Neutral;
    };
    let (pa, pb) = (&players[a], &players[b]);
    // Players put in a team keep their own stored relations at war with
    // everyone, so the team's speak for them.
    let stance = match (pa.team, pb.team) {
        (Some(ta), Some(tb)) => teams[ta].team_relations.get(tb),
        (Some(ta), None) => teams[ta].relations.get(b),
        (None, Some(tb)) => pa.team_relations.get(tb),
        (None, None) => pa.relations.get(b),
    };
    stance.copied().unwrap_or(Relation::Neutral)
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
            nation: (!k.nation.is_empty()).then(|| k.nation.clone()),
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

/// The way a placed object faces.
pub(crate) fn facing(thing: &Thing) -> [f32; 2] {
    match thing.facing {
        // Objects never turned face their model's -y.
        [0.0, 0.0, _] => [0.0, -1.0],
        [fx, fy, _] => [fx, fy],
    }
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
    /// Index into `formations` of the one it fights in once engaged.
    pub combat: Option<usize>,
    /// How far from where it stands it takes on enemies that fire on it.
    pub operational_range: Option<f32>,
    /// The member whose position is the regiment's; without one, the
    /// members' average position is.
    pub primary: Option<usize>,
    /// For a packed building, such as a supply truck, what it unpacks into.
    pub unpacks: Option<Unpacks>,
}

impl Regiment {
    /// Stands the members in its formation around `position`, facing
    /// `facing`.
    pub fn place(&mut self, position: [f32; 2], facing: [f32; 2]) {
        self.facing = facing;
        let offsets = &self.formations[self.formation].offsets;
        for (m, &offset) in self.members.iter_mut().zip(offsets) {
            let [x, y] = crate::from_local(position, facing, offset);
            m.thing.position = [x, y, 0.0];
            m.thing.facing = [facing[0], facing[1], 0.0];
        }
    }

    /// Gives it the id `id`, and its members the ones after it, or the
    /// same one for a lone unit, which is its own regiment.
    pub fn renumber(&mut self, id: u32) {
        self.id = id;
        let lone = self.lone();
        for (m, id) in self.members.iter_mut().zip(id + u32::from(!lone)..) {
            m.thing.id = id;
        }
    }

    /// Whether it's a single unit rather than an organization of them.
    pub fn lone(&self) -> bool {
        matches!(&self.members[..], [m] if m.thing.template == self.template)
    }
}

/// What a packed building unpacks into.
#[derive(Debug, Clone)]
pub struct Unpacks {
    /// The building, standing at the origin facing [`UNPACKED_FACING`]
    /// and owned by nobody.
    pub building: Building,
    /// Where the packed building stands from where the building will, in
    /// the map's axes.
    pub offset: [f32; 2],
    /// Seconds unpacking takes.
    pub seconds: f32,
}

/// What a building packs into.
#[derive(Debug, Clone)]
pub struct Packs {
    /// The packed building, such as a truck, standing anywhere and owned
    /// by nobody: it's placed and given ids as it's put out.
    pub truck: Regiment,
    /// Where it stands from the building, in the map's axes, and the way
    /// it faces there.
    pub offset: [f32; 2],
    pub facing: [f32; 2],
    /// Seconds packing takes.
    pub seconds: f32,
    /// The share of its health at or below which the building can't pack.
    pub health_min: f32,
}

/// The way buildings that unpack face, as all of them on the game's maps
/// do.
pub const UNPACKED_FACING: [f32; 2] = [0.0, -1.0];

#[derive(Debug, Clone)]
pub struct Member {
    /// Where it stands and which way it faces.
    pub thing: Thing,
    /// Top speed in world units a second.
    pub speed: f32,
    /// What it can take and deal; `None` for what can't be hurt.
    pub fighter: Option<Fighter>,
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
    bodies: &HashMap<Id, Body>,
    fighters: &HashMap<Id, Fighter>,
    layouts: &HashMap<Id, Layout>,
    operational_ranges: &HashMap<Id, f32>,
) -> Vec<Regiment> {
    things
        .iter()
        .filter_map(|t| {
            let owner = owner(t)?;
            let organization = t.organization()?;
            let members = organization
                .members
                .iter()
                .map(|m| member(m.clone(), bodies, fighters))
                .collect::<Option<Vec<_>>>()?;
            let facing = facing(t);
            let layout = layouts.get(&Id::from(organization.layout.as_str()));
            let operational_range = operational_ranges
                .get(&Id::from(t.template.as_str()))
                .copied();
            regiment(
                t.id,
                &t.template,
                owner,
                facing,
                members,
                layout.map(|l| (l, &organization.slots[..])),
                operational_range,
            )
        })
        .collect()
}

/// The division an organization is in: the HQ it's attached to, or the HQ
/// it is.
#[derive(Debug, Clone, PartialEq)]
pub struct Division {
    /// The HQ's template, whose nation the badge is from.
    pub template: String,
    /// Which of that nation's badges its regiments wear.
    pub badge: usize,
}

/// The division of each organization in `things` whose HQ has a badge, by
/// the organization's id.
pub fn divisions(things: &[Thing]) -> HashMap<u32, Division> {
    let hqs: HashMap<u32, Division> = things
        .iter()
        .filter_map(|t| {
            let badge = usize::try_from(t.hq_badge()?).ok()?;
            let template = t.template.clone();
            Some((t.id, Division { template, badge }))
        })
        .collect();
    things
        .iter()
        .filter_map(|t| {
            let hq = t.organization()?.hq.filter(|&hq| hq != 0).unwrap_or(t.id);
            Some((t.id, hqs.get(&hq)?.clone()))
        })
        .collect()
}

/// A member standing as `thing` has it, if its template can move.
fn member(
    thing: Thing,
    bodies: &HashMap<Id, Body>,
    fighters: &HashMap<Id, Fighter>,
) -> Option<Member> {
    let template = Id::from(thing.template.as_str());
    Some(Member {
        speed: bodies.get(&template)?.speed?,
        fighter: fighters.get(&template).cloned(),
        thing,
    })
}

/// A regiment of `members`, arranged by its layout and the slots they fill
/// where those account for them all, else as they stand.
fn regiment(
    id: u32,
    template: &str,
    owner: u32,
    facing: [f32; 2],
    members: Vec<Member>,
    layout: Option<(&Layout, &[String])>,
    operational_range: Option<f32>,
) -> Option<Regiment> {
    if members.is_empty() {
        return None;
    }
    let arranged = layout.and_then(|(l, slots)| Some((l, arrange(l, slots, members.len())?)));
    // The layout's formations keep their order once arranged.
    let combat = arranged.as_ref().and_then(|(l, _)| l.combat);
    let (formations, formation, primary) = match arranged {
        Some((_, a)) => a,
        None => (vec![as_placed(&members, facing)], 0, None),
    };
    Some(Regiment {
        id,
        template: template.to_string(),
        owner,
        facing,
        members,
        formations,
        formation,
        combat,
        operational_range,
        primary,
        unpacks: None,
    })
}

/// What raising regiments from their templates takes: what each template is
/// made of, and what its members are.
pub struct Recruits {
    organizations: HashMap<Id, Organization>,
    bodies: HashMap<Id, Body>,
    fighters: HashMap<Id, Fighter>,
    layouts: HashMap<Id, Layout>,
    operational_ranges: HashMap<Id, f32>,
    lists: HashMap<Id, Vec<Id>>,
    deployments: HashMap<Id, Deployment>,
    deploy_points: HashMap<Id, DeployPoints>,
    recruit_times: HashMap<Id, f32>,
    /// Ids the game has since renamed.
    renamed: HashSet<Id>,
}

impl Recruits {
    pub fn new(game: &mut GameData) -> Recruits {
        Recruits {
            organizations: game.organizations(),
            bodies: game.bodies(),
            fighters: game.fighters(),
            layouts: game.layouts(),
            operational_ranges: game.operational_ranges(),
            lists: game.recruit_lists(),
            deployments: game.deployments(),
            deploy_points: game.deploy_points(),
            recruit_times: game.recruit_times(),
            renamed: game.renamed_things(),
        }
    }

    /// Seconds commissioning or recruiting one of `template` takes.
    pub fn recruit_time(&self, template: &str) -> f32 {
        self.recruit_times
            .get(&Id::from(template))
            .copied()
            .unwrap_or(0.0)
    }

    /// What objects of `template` commission or recruit, in the order
    /// their buttons take.
    pub fn list(&self, template: &str) -> &[Id] {
        self.lists
            .get(&Id::from(template))
            .map_or(&[], Vec::as_slice)
    }

    /// Where what objects of `template` recruit appears and goes.
    pub fn deploy_points(&self, template: &str) -> Option<DeployPoints> {
        self.deploy_points.get(&Id::from(template)).copied()
    }

    /// What each building template packs into, by template, for those
    /// some packed building unpacks into and that can be raised.
    pub fn packings(&self) -> HashMap<String, Packs> {
        // What buildings commission comes first, so a Division HQ packs
        // back into its regiment rather than a bare truck; then any packed
        // building under its current id. Where several still pack to one
        // building the first by name wins, so it's always the same one.
        fn sorted<'a>(ids: impl Iterator<Item = &'a Id>) -> Vec<&'a str> {
            let mut ids: Vec<&str> = ids.map(Id::as_str).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        }
        let listed = sorted(self.lists.values().flatten());
        let packed = self
            .deployments
            .keys()
            .filter(|id| !self.renamed.contains(*id));
        let mut out = HashMap::new();
        for id in listed.into_iter().chain(sorted(packed)) {
            let Ok(raised) = self.raise(id, 0, 0, [0.0, 0.0], [0.0, -1.0]) else {
                continue;
            };
            let Some(unpacks) = &raised.unpacks else {
                continue;
            };
            let into = unpacks.building.template.clone();
            let member = Id::from(raised.members[0].thing.template.as_str());
            let (Some(truck), building) = (
                self.deployments.get(&member),
                self.deployments.get(&Id::from(into.as_str())),
            ) else {
                continue;
            };
            let packs = Packs {
                offset: truck.packed_offset,
                facing: truck.packed_facing,
                seconds: building.map_or(7.0, |b| b.packing),
                health_min: building.map_or(0.5, |b| b.pack_health_min),
                truck: raised,
            };
            out.entry(into).or_insert(packs);
        }
        out
    }

    /// What a packed building of `template` unpacks into, if it does and
    /// the building can stand.
    fn unpacks(&self, template: &str) -> Option<Unpacks> {
        let d = self.deployments.get(&Id::from(template))?;
        let into = d.unpacks_into.as_ref()?;
        let fighter = self.fighters.get(into)?.clone();
        let body = self.bodies.get(into)?;
        let seconds = self.deployments.get(into).map_or(7.0, |b| b.unpacking);
        let obstacle = body.blocking.map(|shape| Obstacle {
            position: [0.0, 0.0],
            facing: UNPACKED_FACING,
            shape,
        });
        Some(Unpacks {
            building: Building {
                id: 0,
                template: into.as_str().to_string(),
                owner: 0,
                position: [0.0, 0.0],
                facing: UNPACKED_FACING,
                fighter,
                health: 1.0,
                zone: None,
                members: Vec::new(),
                obstacle,
                deploy: self.deploy_points.get(into).copied(),
            },
            offset: d.packed_offset,
            seconds,
        })
    }

    /// The templates [`Recruits::raise`] can raise, sorted.
    pub fn templates(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self
            .organizations
            .iter()
            .filter(|(_, o)| {
                let mut filled = o.slots.iter().flatten().peekable();
                filled.peek().is_some()
                    && filled.all(|e| self.bodies.get(e).is_some_and(|b| b.speed.is_some()))
            })
            .map(|(id, _)| id.as_str())
            .collect();
        out.sort_unstable_by_key(|t| t.to_ascii_lowercase());
        out
    }

    /// A regiment of `template` owned by `owner`, standing in its starting
    /// formation around `position` and facing `facing`. It takes the id `id`
    /// and its members the ones after it.
    pub fn raise(
        &self,
        template: &str,
        id: u32,
        owner: u32,
        position: [f32; 2],
        facing: [f32; 2],
    ) -> Result<Regiment, String> {
        let Some(organization) = self.organizations.get(&Id::from(template)) else {
            return self.raise_lone(template, id, owner, position, facing);
        };
        let layout = self
            .layouts
            .get(&organization.layout)
            .ok_or_else(|| format!("{template} has no layout {}", organization.layout))?;
        let slots: Vec<String> = organization
            .slots
            .iter()
            .map(|s| {
                s.as_ref()
                    .map_or_else(String::new, |e| e.as_str().to_string())
            })
            .collect();
        let start = &layout.formations[layout.default];
        let elements = slots.iter().enumerate().filter(|(_, e)| !e.is_empty());
        let elements = elements.flat_map(|(i, e)| {
            let n = start.slots.get(i).map_or(1, Vec::len);
            std::iter::repeat_n(e, n)
        });
        let [fx, fy] = facing;
        let members = elements
            .zip(id + 1..)
            .map(|(element, id)| {
                let thing = Thing {
                    template: element.clone(),
                    class: tgck::map::ACTOR,
                    id,
                    name: String::new(),
                    position: [position[0], position[1], 0.0],
                    facing: [fx, fy, 0.0],
                    flags: 0,
                    scale: 1.0,
                    variation: -1,
                    seen_by: 0,
                    detail: Detail::None,
                };
                member(thing, &self.bodies, &self.fighters)
                    .ok_or_else(|| format!("{element}, in {template}, can't move"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let operational_range = self.operational_ranges.get(&Id::from(template)).copied();
        let mut regiment = regiment(
            id,
            template,
            owner,
            facing,
            members,
            Some((layout, &slots)),
            operational_range,
        )
        .ok_or_else(|| format!("{template} has no members"))?;
        regiment.place(position, facing);
        if let [m] = &regiment.members[..] {
            regiment.unpacks = self.unpacks(&m.thing.template);
        }
        Ok(regiment)
    }

    /// A unit that's no organization's member, such as a truck, raised as
    /// a regiment of its own.
    fn raise_lone(
        &self,
        template: &str,
        id: u32,
        owner: u32,
        position: [f32; 2],
        facing: [f32; 2],
    ) -> Result<Regiment, String> {
        let [fx, fy] = facing;
        let thing = Thing {
            template: template.to_string(),
            class: tgck::map::ACTOR,
            id,
            name: String::new(),
            position: [position[0], position[1], 0.0],
            facing: [fx, fy, 0.0],
            flags: 0,
            scale: 1.0,
            variation: -1,
            seen_by: 0,
            detail: Detail::None,
        };
        let member = member(thing, &self.bodies, &self.fighters)
            .ok_or_else(|| format!("{template} isn't a regiment or a unit that can move"))?;
        Ok(Regiment {
            id,
            template: template.to_string(),
            owner,
            facing,
            members: vec![member],
            formations: vec![Formation {
                id: String::new(),
                offsets: vec![[0.0, 0.0]],
            }],
            formation: 0,
            combat: None,
            operational_range: self.operational_ranges.get(&Id::from(template)).copied(),
            primary: Some(0),
            unpacks: self.unpacks(template),
        })
    }
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
fn as_placed(members: &[Member], facing: [f32; 2]) -> Formation {
    let n = members.len() as f32;
    let (cx, cy) = members.iter().fold((0.0, 0.0), |(x, y), m| {
        (x + m.thing.position[0] / n, y + m.thing.position[1] / n)
    });
    Formation {
        id: String::new(),
        offsets: members
            .iter()
            .map(|m| {
                let [x, y, _] = m.thing.position;
                crate::to_local([cx, cy], facing, [x, y])
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
            combat: Some(0),
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
    fn raises_regiments_in_their_starting_formation() {
        let body = Body {
            blocking: None,
            speed: Some(3.0),
        };
        let recruits = Recruits {
            organizations: HashMap::from([(
                Id::from("squad"),
                Organization {
                    layout: Id::from("line"),
                    slots: vec![Some(Id::from("rifle")), None, Some(Id::from("captain"))],
                },
            )]),
            bodies: HashMap::from([
                (Id::from("rifle"), body.clone()),
                (Id::from("captain"), body),
            ]),
            fighters: HashMap::new(),
            layouts: HashMap::from([(Id::from("line"), layout())]),
            operational_ranges: HashMap::new(),
            lists: HashMap::new(),
            deployments: HashMap::new(),
            deploy_points: HashMap::new(),
            recruit_times: HashMap::new(),
            renamed: HashSet::new(),
        };
        assert_eq!(recruits.templates(), ["squad"]);
        // Facing +x, so the front's left-hand man stands on the +y side.
        let r = recruits
            .raise("Squad", 10, 2, [50.0, 50.0], [1.0, 0.0])
            .unwrap();
        assert_eq!((r.id, r.owner, r.formation, r.primary), (10, 2, 1, Some(2)));
        let placed: Vec<_> = r
            .members
            .iter()
            .map(|m| (m.thing.id, m.thing.template.as_str(), m.thing.position))
            .collect();
        assert_eq!(
            placed,
            [
                (11, "rifle", [51.0, 51.0, 0.0]),
                (12, "rifle", [51.0, 49.0, 0.0]),
                (13, "captain", [50.0, 50.0, 0.0]),
            ]
        );
        let lone = recruits
            .raise("rifle", 20, 2, [5.0, 6.0], [1.0, 0.0])
            .unwrap();
        assert!(lone.lone());
        assert_eq!(lone.members[0].thing.id, 20);
        assert_eq!(lone.members[0].thing.position, [5.0, 6.0, 0.0]);
        assert!(lone.unpacks.is_none());
        assert!(
            recruits
                .raise("hut", 10, 2, [0.0, 0.0], [1.0, 0.0])
                .is_err()
        );
    }

    #[test]
    fn trucks_raise_with_what_they_unpack_into() {
        let recruits = Recruits {
            organizations: HashMap::new(),
            bodies: HashMap::from([
                (
                    Id::from("truck"),
                    Body {
                        blocking: None,
                        speed: Some(3.0),
                    },
                ),
                (
                    Id::from("depot"),
                    Body {
                        blocking: Some(aaa_data::Shape::Rect {
                            width: 8.0,
                            height: 8.0,
                        }),
                        speed: None,
                    },
                ),
            ]),
            fighters: HashMap::from([(
                Id::from("depot"),
                Fighter {
                    health: 750.0,
                    ..Fighter::default()
                },
            )]),
            layouts: HashMap::new(),
            operational_ranges: HashMap::new(),
            lists: HashMap::new(),
            deployments: HashMap::from([
                (
                    Id::from("truck"),
                    Deployment {
                        unpacks_into: Some(Id::from("depot")),
                        packed_offset: [0.0, 0.0],
                        packed_facing: [1.0, 0.0],
                        packing: 7.0,
                        unpacking: 7.0,
                        pack_health_min: 0.5,
                    },
                ),
                (
                    Id::from("depot"),
                    Deployment {
                        unpacks_into: None,
                        packed_offset: [0.0, 0.0],
                        packed_facing: [0.0, -1.0],
                        packing: 8.0,
                        unpacking: 9.0,
                        pack_health_min: 0.5,
                    },
                ),
            ]),
            deploy_points: HashMap::from([(
                Id::from("depot"),
                DeployPoints {
                    creation: [2.0, 1.0],
                    deployment: [2.0, 6.0],
                },
            )]),
            recruit_times: HashMap::new(),
            renamed: HashSet::new(),
        };
        let mut truck = recruits
            .raise("truck", 1, 2, [0.0, 0.0], [0.0, 1.0])
            .unwrap();
        let unpacks = truck.unpacks.as_ref().unwrap();
        assert_eq!(unpacks.building.template, "depot");
        assert_eq!(unpacks.seconds, 9.0);
        assert!(unpacks.building.obstacle.is_some());
        // So it can commission what it lists once unpacked.
        assert_eq!(
            unpacks.building.deploy.map(|d| d.deployment),
            Some([2.0, 6.0])
        );
        truck.renumber(40);
        assert_eq!((truck.id, truck.members[0].thing.id), (40, 40));
        let packings = recruits.packings();
        let packs = &packings["depot"];
        assert_eq!(packs.truck.template, "truck");
        assert_eq!((packs.seconds, packs.facing), (8.0, [1.0, 0.0]));

        // A renamed id comes before by name but is passed over, and a
        // regiment of the truck that a building lists comes first.
        let mut recruits = recruits;
        let truck = recruits.deployments[&Id::from("truck")].clone();
        recruits.deployments.insert(Id::from("an_old_truck"), truck);
        recruits.renamed.insert(Id::from("an_old_truck"));
        assert_eq!(recruits.packings()["depot"].truck.template, "truck");
        recruits.organizations.insert(
            Id::from("convoy"),
            Organization {
                layout: Id::from("line"),
                slots: vec![None, None, Some(Id::from("truck"))],
            },
        );
        recruits.layouts.insert(Id::from("line"), layout());
        recruits
            .lists
            .insert(Id::from("corps"), vec![Id::from("convoy")]);
        let packs = &recruits.packings()["depot"];
        assert_eq!(packs.truck.template, "convoy");
        assert_eq!(packs.facing, [1.0, 0.0]);
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
