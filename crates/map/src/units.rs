//! The sides in a map and the regiments they start with.

use std::collections::{HashMap, HashSet};

use aaa_data::{
    Body, DeployPoints, Deployment, Fighter, GameData, Id, Layout, Organization, Ranges, Shape,
};
use tgck::map::{Detail, Kingdoms, Relation, RestrictionGroups, Team as StoredTeam, Thing};

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
    /// The general who leads it, e.g. `montgomery`, where the map fixes one
    /// along with its nation.
    pub general: Option<String>,
    /// How it stands toward each player, by index in
    /// [`Map::players`](crate::Map::players).
    pub relations: Vec<Relation>,
    /// How it stands toward each team, by index in
    /// [`Map::teams`](crate::Map::teams).
    pub team_relations: Vec<Relation>,
    /// The money it starts with.
    pub stockpile: f32,
    /// The technologies it starts with.
    pub technologies: Vec<Id>,
    /// The handicap the map gives it, e.g. `handicap_hard`, which the
    /// lobby's choice replaces.
    pub handicap: Option<Id>,
    /// The technologies and templates the map bars it from, through its
    /// team's restriction group or its own.
    pub barred: Vec<Id>,
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

/// How the player `from` stands toward the player `to`, by their ids:
/// neutral where either isn't among `players`, as for things that belong
/// to nobody, and at war where the map doesn't say.
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
    stance.copied().unwrap_or(Relation::Enemy)
}

/// The players and teams, with their relations by index rather than by
/// their positions in the stored sections, which can have gaps.
pub fn sides(
    kingdoms: &Kingdoms,
    teams: &[Option<StoredTeam>],
    groups: &RestrictionGroups,
    colors: &HashMap<Id, [f32; 3]>,
    default_stockpile: f32,
    handicaps: &HashMap<String, Id>,
) -> (Vec<Player>, Vec<Team>) {
    // Only what a group selects bars anything; its other lists are what it
    // leaves alone.
    let barring = |name: &str| -> Vec<Id> {
        (groups.groups.iter())
            .filter(|g| !name.is_empty() && g.name.eq_ignore_ascii_case(name))
            .flat_map(|g| g.required_actors.iter().chain(&g.required_technologies))
            .map(|id| Id::from(id.as_str()))
            .collect()
    };
    let money = |stockpiles: &[(String, f32)]| {
        stockpiles
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case("money"))
            .map_or(0.0, |&(_, amount)| amount)
    };
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
            // Maps whose nation the lobby chooses can still name a general,
            // which the lobby's choice replaces.
            general: (k.faction.as_ref())
                .map(|f| f.name.clone())
                .filter(|n| !n.is_empty() && !k.nation.is_empty()),
            relations: by_player(&k.relations),
            team_relations: by_team(&k.team_relations),
            stockpile: match k.use_default_stockpile {
                true => default_stockpile,
                false => money(&k.resources),
            } + k.bonus_stockpile.as_deref().map_or(0.0, money),
            technologies: (k.start_technologies.iter().flatten())
                .map(|t| Id::from(t.as_str()))
                .collect(),
            handicap: handicaps.get(&k.name).cloned(),
            barred: {
                let team = teams.iter().find(|t| t.head.id == k.team && k.team != 0);
                let team = team.and_then(|t| t.restriction_group.as_deref());
                let mut barred = barring(team.unwrap_or(""));
                barred.extend(barring(&k.restriction_group));
                barred
            },
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
/// something; at war where none is stored.
fn by_index(slots: &[bool], stored: &[u32]) -> Vec<Relation> {
    slots
        .iter()
        .enumerate()
        .filter(|(_, present)| **present)
        .map(|(i, _)| {
            stored
                .get(i)
                .and_then(|&r| Relation::from_u32(r))
                .unwrap_or(Relation::Enemy)
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
    /// Its zone of control's radius.
    pub control_range: Option<f32>,
    /// The member whose position is the regiment's; without one, the
    /// members' average position is.
    pub primary: Option<usize>,
    /// For a packed building, such as a supply truck, what it unpacks into.
    pub unpacks: Option<Unpacks>,
    /// The id of the HQ it's attached to.
    pub hq: Option<u32>,
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
    /// The ground that must be clear for it to unpack: its blocking shape
    /// padded all round, so buildings keep apart.
    pub room: Shape,
    /// Where the packed building stands from where the building will, in
    /// the map's axes.
    pub offset: [f32; 2],
    /// Seconds unpacking takes.
    pub seconds: f32,
}

/// What a building packs into.
#[derive(Debug, Clone)]
pub struct Packs {
    /// The packed building, such as a truck, alone, standing anywhere and
    /// owned by nobody: it's placed and given ids as it's put out.
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

/// The way buildings face as they unpack unless told otherwise, as all of
/// them on the game's maps do.
pub const UNPACKED_FACING: [f32; 2] = [0.0, -1.0];

#[derive(Debug, Clone)]
pub struct Member {
    /// Where it stands and which way it faces.
    pub thing: Thing,
    /// Top speed in world units a second.
    pub speed: f32,
    /// The ground it takes up, which those of things it's on are tested
    /// against; `None` if it takes up none.
    pub shape: Option<Shape>,
    /// World units a second it gains each second, speeding up; 0 for
    /// reaching top speed at once.
    pub acceleration: f32,
    /// Radians a second it turns while moving; 0 for facing its way at
    /// once.
    pub turn_rate: f32,
    pub trailer: Option<Trailer>,
    /// What it can take and deal; `None` for what can't be hurt.
    pub fighter: Option<Fighter>,
    /// Its health as a fraction of its fighter's.
    pub health: f32,
    /// For one its regiment has lost and has yet to replace: the health
    /// its replacement has built up so far.
    pub lost: Option<f32>,
}

/// What a member tows, dragged after the point it's coupled to.
#[derive(Debug, Clone, PartialEq)]
pub struct Trailer {
    pub template: String,
    /// Where it's coupled to the member, from the member: x to its right
    /// and y ahead.
    pub hitch: [f32; 2],
    /// The same point from the trailer.
    pub coupling: [f32; 2],
    /// The id of the object the map has for it, and where that stands and
    /// the way it faces; `None` for one to put straight behind the member.
    pub placed: Option<(u32, [f32; 2], [f32; 2])>,
}

#[derive(Debug, Clone)]
pub struct Formation {
    /// E.g. `regiment_defensive`; empty for the arrangement the map placed
    /// a regiment in, where its layout doesn't fit its members.
    pub id: String,
    /// Per member, where it stands from the regiment's position: x to its
    /// right and y ahead.
    pub offsets: Vec<[f32; 2]>,
    /// Whether a regiment standing or moving in it takes on enemies that
    /// come within its operational range, or runs into.
    pub aggressive: bool,
}

/// The owned organizations whose members can all move, and the owned units
/// that move on their own, such as trucks and ships, each a regiment of its
/// own.
pub fn regiments(
    things: &[Thing],
    bodies: &HashMap<Id, Body>,
    fighters: &HashMap<Id, Fighter>,
    layouts: &HashMap<Id, Layout>,
    ranges: &HashMap<Id, Ranges>,
) -> Vec<Regiment> {
    let by_id: HashMap<u32, &Thing> = things.iter().map(|t| (t.id, t)).collect();
    // Trailers move but go with what tows them.
    let towed: HashSet<u32> = things
        .iter()
        .flat_map(|t| t.organization().map_or(&[][..], |o| &o.members[..]))
        .chain(things)
        .filter_map(Thing::towed)
        .collect();
    let placed = |m: &Thing| {
        let mut member = member(m.clone(), bodies, fighters)?;
        let towed = m.towed().and_then(|id| by_id.get(&id));
        if let (Some(trailer), Some(towed)) = (&mut member.trailer, towed) {
            let [x, y, _] = towed.position;
            trailer.placed = Some((towed.id, [x, y], facing(towed)));
        }
        Some(member)
    };
    let ranges = |template: &str| ranges.get(&Id::from(template)).copied().unwrap_or_default();
    // Ids no thing on the map has, for the members regiments have lost.
    let members = things
        .iter()
        .flat_map(|t| t.organization().map_or(&[][..], |o| &o.members[..]));
    let highest = things
        .iter()
        .chain(members)
        .map(|t| t.id)
        .max()
        .unwrap_or(0);
    let mut spare = highest + 1..;
    things
        .iter()
        .filter_map(|t| {
            let owner = owner(t)?;
            let Some(organization) = t.organization() else {
                if towed.contains(&t.id) {
                    return None;
                }
                return Some(lone(placed(t)?, owner, facing(t), ranges(&t.template)));
            };
            let standing = organization
                .members
                .iter()
                .map(placed)
                .collect::<Option<Vec<_>>>()?;
            let facing = facing(t);
            let layout = layouts.get(&Id::from(organization.layout.as_str()));
            let count = standing.len().max(1) as f32;
            let sum = standing.iter().fold([0.0, 0.0], |[x, y], m| {
                [x + m.thing.position[0], y + m.thing.position[1]]
            });
            let among = match standing.is_empty() {
                true => [t.position[0], t.position[1]],
                false => [sum[0] / count, sum[1] / count],
            };
            // With the members it's lost, where the record says what they
            // were, so that they can be replaced.
            let mut lost = |p: &tgck::map::Position| {
                let template = p.template.as_ref().filter(|t| !t.is_empty())?;
                // Among those standing, so the regiment is where they are.
                let [x, y] = among;
                let thing = Thing {
                    template: template.clone(),
                    class: tgck::map::ACTOR,
                    id: spare.next()?,
                    name: String::new(),
                    position: [x, y, 0.0],
                    facing: [facing[0], facing[1], 0.0],
                    flags: 0,
                    scale: 1.0,
                    variation: -1,
                    seen_by: 0,
                    detail: Detail::None,
                };
                let mut member = member(thing, bodies, fighters)?;
                member.lost = Some(p.replenished.unwrap_or(0.0));
                Some(member)
            };
            let (members, held) = with_lost(organization, standing, &mut lost);
            let places = layout.map(|l| stored_places(l, organization, &held));
            let ranges = ranges(&t.template);
            let mut regiment = regiment(t.id, &t.template, owner, facing, members, places, ranges)?;
            regiment.hq = organization.hq.filter(|&id| id != 0);
            Some(regiment)
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

/// A member standing as `thing` has it, if its template can move.
fn member(
    thing: Thing,
    bodies: &HashMap<Id, Body>,
    fighters: &HashMap<Id, Fighter>,
) -> Option<Member> {
    let template = Id::from(thing.template.as_str());
    let body = bodies.get(&template)?;
    let trailer = body.trailer.as_ref().and_then(|t| {
        Some(Trailer {
            template: t.as_str().to_string(),
            hitch: body.hitch.unwrap_or_default(),
            coupling: bodies.get(t)?.hitch.unwrap_or_default(),
            placed: None,
        })
    });
    let health = match &thing.detail {
        Detail::Actor(actor) => actor.body().map_or(1.0, |b| b.health),
        _ => 1.0,
    };
    Some(Member {
        health,
        lost: None,
        speed: body.speed?,
        acceleration: body.acceleration,
        turn_rate: body.turn_rate,
        shape: body.blocking,
        trailer,
        fighter: fighters.get(&template).cloned(),
        thing,
    })
}

/// Which of a layout's places a regiment's members fill, and the formation
/// it's in.
struct Places<'a> {
    layout: &'a Layout,
    /// Per place in the layout, slot by slot, whether a member stands
    /// there; the members fill them in order.
    filled: Vec<bool>,
    /// Index into the layout's formations.
    formation: usize,
}

/// The places a regiment of `slots`, each slot's element template or
/// empty, fills when it's whole, in the layout's starting formation.
fn whole<'a>(layout: &'a Layout, slots: &[String]) -> Places<'a> {
    let start = &layout.formations[layout.default];
    let filled = start.slots.iter().enumerate().flat_map(|(i, places)| {
        let filled = slots.get(i).is_some_and(|s| !s.is_empty());
        std::iter::repeat_n(filled, places.len())
    });
    Places {
        layout,
        filled: filled.collect(),
        formation: layout.default,
    }
}

/// A map's organization's members in the order of its positions: those
/// `standing`, and for each position it records as empty the member
/// `lost` makes of it, if it can. With them, which positions have a
/// member; none where the record doesn't account for those standing.
fn with_lost(
    o: &tgck::map::Organization,
    standing: Vec<Member>,
    lost: &mut dyn FnMut(&tgck::map::Position) -> Option<Member>,
) -> (Vec<Member>, Vec<bool>) {
    let filled = o.positions.iter().filter(|p| p.index >= 0).count();
    if filled != standing.len() {
        return (standing, Vec::new());
    }
    let mut standing = standing.into_iter();
    let mut held = Vec::new();
    let mut members = Vec::new();
    for p in &o.positions {
        let member = match p.index >= 0 {
            true => standing.next(),
            false => lost(p),
        };
        held.push(member.is_some());
        members.extend(member);
    }
    (members, held)
}

/// The places a map's organization's members fill, `held` saying which of
/// its positions has one, and the formation it records; a regiment that
/// has lost members still knows where the rest stand. Where the record doesn't fit the layout, as when
/// the layout has changed since, it's taken as whole.
fn stored_places<'a>(layout: &'a Layout, o: &tgck::map::Organization, held: &[bool]) -> Places<'a> {
    let mut places = whole(layout, &o.slots);
    if held.len() == places.filled.len() {
        places.filled = held.to_vec();
    }
    if let Some(f) = usize::try_from(o.formation)
        .ok()
        .filter(|&f| f < layout.formations.len())
    {
        places.formation = f;
    }
    places
}

/// A regiment of `members`, arranged by its layout and the places they fill
/// where those account for them all, else as they stand.
fn regiment(
    id: u32,
    template: &str,
    owner: u32,
    facing: [f32; 2],
    members: Vec<Member>,
    places: Option<Places>,
    ranges: Ranges,
) -> Option<Regiment> {
    if members.is_empty() {
        return None;
    }
    let arranged = places.and_then(|p| Some((p.layout, arrange(&p, members.len())?)));
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
        operational_range: ranges.operational,
        control_range: ranges.control,
        primary,
        unpacks: None,
        hq: None,
    })
}

/// What raising regiments from their templates takes: what each template is
/// made of, and what its members are.
pub struct Recruits {
    organizations: HashMap<Id, Organization>,
    pub(crate) bodies: HashMap<Id, Body>,
    pub(crate) fighters: HashMap<Id, Fighter>,
    pub(crate) layouts: HashMap<Id, Layout>,
    pub(crate) ranges: HashMap<Id, Ranges>,
    lists: HashMap<Id, Vec<Id>>,
    deployments: HashMap<Id, Deployment>,
    deploy_points: HashMap<Id, DeployPoints>,
    recruit_times: HashMap<Id, f32>,
    /// The packed building each building packs into.
    packs_into: HashMap<Id, Id>,
    /// How far beyond its footprint a building needs clear ground.
    padding: f32,
}

/// [`Recruits::padding`] where the game's settings don't give it.
const BUILDING_PADDING: f32 = 2.0;

impl Recruits {
    pub fn new(game: &mut GameData) -> Recruits {
        Recruits {
            organizations: game.organizations(),
            bodies: game.bodies(),
            fighters: game.fighters(),
            layouts: game.layouts(),
            ranges: game.organization_ranges(),
            lists: game.recruit_lists(),
            deployments: game.deployments(),
            deploy_points: game.deploy_points(),
            recruit_times: game.recruit_times(),
            packs_into: game.packs_into(),
            padding: game.building_padding().unwrap_or(BUILDING_PADDING),
        }
    }

    fn ranges(&self, template: &str) -> Ranges {
        self.ranges
            .get(&Id::from(template))
            .copied()
            .unwrap_or_default()
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
        let mut out = HashMap::new();
        for (building, truck) in &self.packs_into {
            let Ok(raised) = self.raise(truck.as_str(), 0, 0, [0.0, 0.0], [0.0, -1.0]) else {
                continue;
            };
            let (Some(_), Some(deployment)) = (&raised.unpacks, self.deployments.get(truck)) else {
                continue;
            };
            let packing = self.deployments.get(building);
            let packs = Packs {
                offset: deployment.packed_offset,
                facing: deployment.packed_facing,
                seconds: packing.map_or(7.0, |b| b.packing),
                health_min: packing.map_or(0.5, |b| b.pack_health_min),
                truck: raised,
            };
            out.insert(building.as_str().to_string(), packs);
        }
        out
    }

    /// Gives a regiment of one packed building, such as a truck, what it
    /// unpacks into.
    pub(crate) fn give_unpacks(&self, regiment: &mut Regiment) {
        if let [m] = &regiment.members[..] {
            regiment.unpacks = self.unpacks(&m.thing.template);
        }
    }

    /// What a packed building of `template` unpacks into, if it does and
    /// the building can stand.
    fn unpacks(&self, template: &str) -> Option<Unpacks> {
        let d = self.deployments.get(&Id::from(template))?;
        let into = d.unpacks_into.as_ref()?;
        let body = self.bodies.get(into)?;
        let seconds = self.deployments.get(into).map_or(7.0, |b| b.unpacking);
        Some(Unpacks {
            building: self.building(into)?,
            room: body.shape.padded(self.padding),
            offset: d.packed_offset,
            seconds,
        })
    }

    /// A building of `template` to put on the map during a match, if one
    /// can stand: at the origin facing [`UNPACKED_FACING`], in full health
    /// and owned by nobody.
    pub fn building(&self, template: &Id) -> Option<Building> {
        let fighter = self.fighters.get(template)?.clone();
        let body = self.bodies.get(template)?;
        let obstacle = body.blocking.map(|shape| Obstacle {
            position: [0.0, 0.0],
            facing: UNPACKED_FACING,
            shape,
            soft: body.soft,
        });
        Some(Building {
            id: 0,
            template: template.as_str().to_string(),
            owner: 0,
            position: [0.0, 0.0],
            facing: UNPACKED_FACING,
            fighter,
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle,
            deploy: self.deploy_points.get(template).copied(),
            organization: None,
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
        let ranges = self.ranges(template);
        let mut regiment = regiment(
            id,
            template,
            owner,
            facing,
            members,
            Some(whole(layout, &slots)),
            ranges,
        )
        .ok_or_else(|| format!("{template} has no members"))?;
        regiment.place(position, facing);
        self.give_unpacks(&mut regiment);
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
            unpacks: self.unpacks(template),
            ..lone(member, owner, facing, self.ranges(template))
        })
    }
}

/// A unit that's no organization's member as a regiment of its own, which
/// shares its id and template.
/// The organization `t`, owned by `owner`, without its members, for a
/// building standing in it to put a truck in when it packs: the truck
/// stands where it's put.
pub(crate) fn emptied(t: &Thing, owner: u32, ranges: Ranges) -> Regiment {
    Regiment {
        id: t.id,
        template: t.template.clone(),
        owner,
        facing: facing(t),
        members: Vec::new(),
        ..one_place(ranges)
    }
}

/// A regiment of nothing, owned by nobody, in a formation of one place.
fn one_place(ranges: Ranges) -> Regiment {
    Regiment {
        id: 0,
        template: String::new(),
        owner: 0,
        hq: None,
        facing: [0.0, -1.0],
        members: Vec::new(),
        formations: vec![Formation {
            id: String::new(),
            aggressive: false,
            offsets: vec![[0.0, 0.0]],
        }],
        formation: 0,
        combat: None,
        operational_range: ranges.operational,
        control_range: ranges.control,
        primary: Some(0),
        unpacks: None,
    }
}

fn lone(member: Member, owner: u32, facing: [f32; 2], ranges: Ranges) -> Regiment {
    Regiment {
        id: member.thing.id,
        template: member.thing.template.clone(),
        owner,
        facing,
        members: vec![member],
        ..one_place(ranges)
    }
}

/// The layout's formations for the members filling `places`, the one
/// they're in, and the member in the primary slot. `None` where the places
/// don't account for every member, or a formation's places don't match.
fn arrange(places: &Places, members: usize) -> Option<(Vec<Formation>, usize, Option<usize>)> {
    let Places {
        layout,
        filled,
        formation,
    } = places;
    if filled.iter().filter(|&&f| f).count() != members {
        return None;
    }
    let formations = layout
        .formations
        .iter()
        .map(|f| {
            let all: Vec<[f32; 2]> = f.slots.iter().flatten().copied().collect();
            (all.len() == filled.len()).then(|| Formation {
                id: f.id.to_string(),
                aggressive: f.aggressive,
                offsets: all
                    .into_iter()
                    .zip(filled)
                    .filter_map(|(o, &f)| f.then_some(o))
                    .collect(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let primary = layout.primary.and_then(|p| {
        let slots = &layout.formations[layout.default].slots;
        let first: usize = slots.get(..p)?.iter().map(Vec::len).sum();
        let here = *filled.get(first)?;
        here.then(|| filled[..first].iter().filter(|&&f| f).count())
    });
    Some((formations, *formation, primary))
}

/// Where the members stand from their average position.
fn as_placed(members: &[Member], facing: [f32; 2]) -> Formation {
    let n = members.len() as f32;
    let (cx, cy) = members.iter().fold((0.0, 0.0), |(x, y), m| {
        (x + m.thing.position[0] / n, y + m.thing.position[1] / n)
    });
    Formation {
        id: String::new(),
        aggressive: false,
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
            aggressive: false,
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
        let (formations, formation, primary) = arrange(&whole(&layout(), &slots), 3).unwrap();
        assert_eq!(formation, 1);
        assert_eq!(primary, Some(2));
        assert_eq!(formations[1].id, "defensive");
        assert_eq!(formations[1].offsets, [[-1.0, 1.0], [1.0, 1.0], [0.0, 0.0]]);
        assert_eq!(formations[0].offsets[0], [-1.0, 2.0]);
    }

    #[test]
    fn refuses_layouts_that_miss_members() {
        let slots = ["rifle".to_string(), String::new(), "captain".to_string()];
        assert!(arrange(&whole(&layout(), &slots), 4).is_none());
    }

    /// A squad of `layout()` placed on a map, with a rifle in the front
    /// slot's first place and the captain, in formation `formation`.
    fn squad(formation: i32, positions: &[i32]) -> Vec<Regiment> {
        use crate::fixtures::{organization, placed};
        let members = vec![
            placed(2, "rifle", 0, None, None),
            placed(3, "captain", 0, None, None),
        ];
        let mut o = organization("line", members);
        o.slots = vec!["rifle".into(), String::new(), "captain".into()];
        let place = o.positions[0].clone();
        o.positions = positions
            .iter()
            .map(|&index| tgck::map::Position {
                index,
                ..place.clone()
            })
            .collect();
        o.formation = formation;
        let moves = Body {
            speed: Some(3.0),
            ..Body::default()
        };
        regiments(
            &[placed(1, "squad", 2, None, Some(o))],
            &HashMap::from([
                (Id::from("rifle"), moves.clone()),
                (Id::from("captain"), moves),
            ]),
            &HashMap::new(),
            &HashMap::from([(Id::from("line"), layout())]),
            &HashMap::new(),
        )
    }

    #[test]
    fn regiments_that_lost_members_keep_their_places() {
        // The front's second rifle is gone.
        let [r] = &squad(1, &[0, -1, -1, 3])[..] else {
            panic!();
        };
        assert_eq!(r.formations.len(), 2);
        assert_eq!(r.formations[1].offsets, [[-1.0, 1.0], [0.0, 0.0]]);
        assert_eq!(r.formations[0].offsets, [[-1.0, 2.0], [0.0, 0.0]]);
        assert_eq!((r.primary, r.combat), (Some(1), Some(0)));
    }

    #[test]
    fn regiments_keep_a_place_for_members_they_can_replace() {
        use crate::fixtures::{organization, placed};
        let members = vec![
            placed(2, "rifle", 0, None, None),
            placed(3, "captain", 0, None, None),
        ];
        let mut o = organization("line", members);
        o.slots = vec!["rifle".into(), String::new(), "captain".into()];
        let place = o.positions[0].clone();
        // The front's second rifle is gone, a fifth of the way back; what
        // stood in the empty slot isn't recorded.
        let lost = tgck::map::Position {
            index: -1,
            template: Some("rifle".into()),
            replenished: Some(20.0),
            ..place.clone()
        };
        let empty = tgck::map::Position {
            index: -1,
            ..place.clone()
        };
        o.positions = vec![place.clone(), lost, empty, place];
        let moves = Body {
            speed: Some(3.0),
            ..Body::default()
        };
        let all = regiments(
            &[placed(1, "squad", 2, None, Some(o))],
            &HashMap::from([
                (Id::from("rifle"), moves.clone()),
                (Id::from("captain"), moves),
            ]),
            &HashMap::new(),
            &HashMap::from([(Id::from("line"), layout())]),
            &HashMap::new(),
        );
        let [r] = &all[..] else {
            panic!();
        };
        let lost: Vec<_> = r.members.iter().map(|m| m.lost).collect();
        assert_eq!(lost, [None, Some(20.0), None]);
        // Under an id nothing on the map has.
        assert_eq!(r.members[1].thing.id, 4);
        assert_eq!(r.members[1].thing.template, "rifle");
        assert_eq!(r.formations[1].offsets.len(), 3);
        assert_eq!(r.primary, Some(2));
    }

    #[test]
    fn regiments_start_in_the_formation_the_map_has_them_in() {
        assert_eq!(squad(0, &[0, -1, -1, 3])[0].formation, 0);
        assert_eq!(squad(1, &[0, -1, -1, 3])[0].formation, 1);
        // One the layout doesn't have leaves them in its starting one.
        assert_eq!(squad(5, &[0, -1, -1, 3])[0].formation, 1);
        // Places that don't fit the layout leave them as they stand.
        let r = &squad(0, &[0, 1])[0];
        assert_eq!((r.formations.len(), r.formation), (1, 0));
        assert!(r.formations[0].id.is_empty());
    }

    #[test]
    fn raises_regiments_in_their_starting_formation() {
        let body = Body {
            blocking: None,
            speed: Some(3.0),
            ..Body::default()
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
            ranges: HashMap::new(),
            lists: HashMap::new(),
            deployments: HashMap::new(),
            deploy_points: HashMap::new(),
            recruit_times: HashMap::new(),
            packs_into: HashMap::new(),
            padding: BUILDING_PADDING,
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
                        ..Body::default()
                    },
                ),
                (
                    Id::from("depot"),
                    Body {
                        blocking: Some(aaa_data::Shape::Rect {
                            width: 8.0,
                            height: 8.0,
                        }),
                        shape: aaa_data::Shape::Rect {
                            width: 8.0,
                            height: 8.0,
                        },
                        speed: None,
                        fixed: true,
                        ..Body::default()
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
            ranges: HashMap::new(),
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
            packs_into: HashMap::new(),
            padding: BUILDING_PADDING,
        };
        let mut truck = recruits
            .raise("truck", 1, 2, [0.0, 0.0], [0.0, 1.0])
            .unwrap();
        let unpacks = truck.unpacks.as_ref().unwrap();
        assert_eq!(unpacks.building.template, "depot");
        assert_eq!(unpacks.seconds, 9.0);
        assert!(unpacks.building.obstacle.is_some());
        assert_eq!(
            unpacks.room,
            aaa_data::Shape::Rect {
                width: 12.0,
                height: 12.0
            }
        );
        // So it can commission what it lists once unpacked.
        assert_eq!(
            unpacks.building.deploy.map(|d| d.deployment),
            Some([2.0, 6.0])
        );
        truck.renumber(40);
        assert_eq!((truck.id, truck.members[0].thing.id), (40, 40));
        let mut recruits = recruits;
        recruits
            .packs_into
            .insert(Id::from("depot"), Id::from("truck"));
        let packings = recruits.packings();
        let packs = &packings["depot"];
        assert_eq!(packs.truck.template, "truck");
        assert_eq!((packs.seconds, packs.facing), (8.0, [1.0, 0.0]));

        // The truck the data gives, though another, listed by a building as
        // the battleship lists amphibious ones, unpacks into it too.
        let mut boat = recruits.deployments[&Id::from("truck")].clone();
        boat.packed_facing = [0.0, 1.0];
        recruits.deployments.insert(Id::from("a_boat"), boat);
        recruits.bodies.insert(
            Id::from("a_boat"),
            recruits.bodies[&Id::from("truck")].clone(),
        );
        recruits
            .lists
            .insert(Id::from("battleship"), vec![Id::from("a_boat")]);
        assert_eq!(recruits.packings()["depot"].truck.template, "truck");

        // A truck the map starts with unpacks too.
        let truck = crate::fixtures::placed(5, "truck", 2, None, None);
        let mut placed = regiments(
            &[truck],
            &recruits.bodies,
            &recruits.fighters,
            &recruits.layouts,
            &recruits.ranges,
        );
        recruits.give_unpacks(&mut placed[0]);
        let unpacks = placed[0]
            .unpacks
            .as_ref()
            .map(|u| u.building.template.as_str());
        assert_eq!(unpacks, Some("depot"));
    }

    #[test]
    fn relations_skip_empty_slots_and_are_at_war_where_unset() {
        use Relation::{Ally, Enemy, Neutral};
        assert_eq!(
            by_index(&[true, false, true, true], &[1, 0, 2]),
            [Ally, Neutral, Enemy]
        );
    }
}
