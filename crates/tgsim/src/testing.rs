//! What the crate's tests build their matches from.

use std::collections::HashMap;

use tgmap::{
    DeployPoints, Domain, Fighter, Id, Obstacle, Packs, Pathing, Shape, SupplyEffect, Unpacks,
    Weapon,
};

use crate::{Command, Notice, Player, Relation, Sides, TICK, World, distance};

pub(crate) fn member(id: u32, position: [f32; 2], speed: f32) -> tgmap::Member {
    tgmap::Member {
        id,
        template: "soldier".into(),
        position,
        facing: [0.0, 1.0],
        speed,
        shape: None,
        acceleration: 0.0,
        turn_rate: 0.0,
        trailer: None,
        fighter: None,
        health: 1.0,
        lost: None,
    }
}

/// The animations a template plays: its own set alone, dying one of
/// `deaths`.
pub fn plays(deaths: Vec<tgmap::Death>) -> Vec<tgmap::AnimationSet> {
    vec![tgmap::AnimationSet {
        deaths,
        ..tgmap::AnimationSet::default()
    }]
}

/// Two soldiers side by side facing +y, the slower one on the right,
/// who can also line up one behind the other.
pub(crate) fn world(pathing: Pathing) -> World {
    let formation = |id: &str, offsets| tgmap::Formation {
        id: id.into(),
        offsets,
        aggressive: false,
    };
    let organization = tgmap::Organization {
        hq: None,
        id: 1,
        template: "squad".into(),
        owner: 7,
        facing: [0.0, 1.0],
        members: vec![member(2, [9.0, 10.0], 3.0), member(3, [11.0, 10.0], 2.0)],
        formations: vec![
            formation("line", vec![[-1.0, 0.0], [1.0, 0.0]]),
            formation("file", vec![[0.0, 1.0], [0.0, -1.0]]),
        ],
        formation: 0,
        combat: None,
        operational_range: None,
        control_range: None,
        primary: None,
        morale: 1.0,
        experience: 0.0,
        efficiency: None,
        unpacks: None,
    };
    World::from_organizations(Sides::default(), pathing, &[organization])
}

pub(crate) fn run(world: &mut World, seconds: f32) {
    for _ in 0..(seconds / TICK).round() as usize {
        world.tick();
    }
}

pub(crate) fn near(a: [f32; 2], b: [f32; 2]) -> bool {
    distance(a, b) < 1e-3
}

/// A lone vehicle at 10,10 facing +y.
pub(crate) fn vehicle(acceleration: f32, turn_rate: f32, trailer: Option<tgmap::Trailer>) -> World {
    let mut m = member(2, [10.0, 10.0], 2.0);
    (m.acceleration, m.turn_rate, m.trailer) = (acceleration, turn_rate, trailer);
    let organization = tgmap::Organization {
        hq: None,
        id: 1,
        template: "tank".into(),
        owner: 7,
        facing: [0.0, 1.0],
        members: vec![m],
        formations: vec![tgmap::Formation {
            id: "lone".into(),
            offsets: vec![[0.0, 0.0]],
            aggressive: false,
        }],
        formation: 0,
        combat: None,
        operational_range: None,
        control_range: None,
        primary: None,
        morale: 1.0,
        experience: 0.0,
        efficiency: None,
        unpacks: None,
    };
    World::from_organizations(
        Sides::default(),
        Pathing::uniform(20, Domain::Land),
        &[organization],
    )
}

/// The main damage `weapon` does.
pub(crate) fn main_damage(weapon: &Weapon) -> tgmap::Damage {
    weapon
        .main_damage()
        .next()
        .expect("it does main damage")
        .clone()
}

pub(crate) fn rifle(range: f32, amount: f32) -> Weapon {
    Weapon {
        range,
        min_range: 0.0,
        recharge: 2.0,
        area: 0.0,
        max_targets: 0,
        full_within: 1.0,
        edge_damage: 1.0,
        targets: tgmap::Targets {
            required: vec![Id::from("enemy")],
            excluded: vec![Id::from("tank")],
        },
        hits: vec![tgmap::WeaponHit::main(tgmap::Damage {
            kind: Id::from("gunfire"),
            amount,
        })],
        centred_on_user: false,
        animation: None,
        turret: false,
        turret_point: None,
        aimed_by: None,
        spawn_position: [0.0; 3],
        sounds: Vec::new(),
        effects: Default::default(),
        projectile: None,
        flight: None,
        bombard: false,
        vorpal: false,
        spell: false,
        choice: Default::default(),
        shown: Default::default(),
    }
}

pub(crate) fn soldier(weapons: Vec<Weapon>) -> Fighter {
    Fighter {
        health: 100.0,
        defense: 0.0,
        capturable: false,
        destructible: true,
        resists: HashMap::new(),
        flags: vec![Id::from("infantry")],
        builds: Vec::new(),
        weapons,
        on_death: None,
        repairs: Vec::new(),
        bonus: Default::default(),
        check_resistance: false,
        attack_height: 1.45,
        notice: None,
        turrets: Vec::new(),
        soft: Shape::Point,
        target: Shape::Point,
        target_square: false,
    }
}

/// An organization for each of `owners`, of one fighter each, the first at
/// 10,10 and each next 10 further along y.
pub(crate) fn battle(owners: &[u32], fighters: Vec<Fighter>, sides: Sides) -> World {
    let organizations = owners
        .iter()
        .zip(fighters)
        .enumerate()
        .map(|(i, (&owner, fighter))| {
            let mut m = member(i as u32 * 2 + 2, [10.0, 10.0 + 10.0 * i as f32], 2.0);
            m.fighter = Some(fighter);
            tgmap::Organization {
                hq: None,
                id: i as u32 * 2 + 1,
                template: "squad".into(),
                owner,
                facing: [0.0, 1.0],
                members: vec![m],
                formations: vec![tgmap::Formation {
                    id: "line".into(),
                    offsets: vec![[0.0, 0.0]],
                    aggressive: true,
                }],
                formation: 0,
                combat: None,
                operational_range: Some(34.0),
                control_range: Some(18.0),
                primary: None,
                morale: 1.0,
                experience: 0.0,
                efficiency: None,
                unpacks: None,
            }
        })
        .collect::<Vec<_>>();
    let mut w =
        World::from_organizations(sides, Pathing::uniform(40, Domain::Land), &organizations);
    // So shots can be counted from when a fight starts.
    w.rules.engage_reaction = 0.0;
    w
}

/// Players 1 and 2 at war, and 3 allied to 1.
pub(crate) fn at_war() -> Sides {
    let side = |id, relations: &[u32]| Player {
        independent: false,
        ..player(id, None, relations, &[])
    };
    Sides {
        names: Default::default(),
        computers: Default::default(),
        players: vec![
            side(1, &[0, 3, 1]),
            side(2, &[3, 0, 3]),
            side(3, &[1, 3, 0]),
        ],
        teams: Vec::new(),
    }
}

/// Player 1's strong rifleman at 10,10 and player 2's weak one at
/// 10,30, out of each other's range; the first can also stand in a
/// wider line, which it fights in.
pub(crate) fn skirmish() -> World {
    let mut w = battle(
        &[1, 2],
        vec![
            soldier(vec![rifle(12.0, 50.0)]),
            soldier(vec![rifle(12.0, 10.0)]),
        ],
        at_war(),
    );
    w.units[1].position = [10.0, 30.0];
    let r = &mut w.organizations[0];
    // Only the second takes on what comes near it.
    r.formations[0].aggressive = false;
    r.formations.push(tgmap::Formation {
        id: "wide".into(),
        offsets: vec![[0.0, 0.0]],
        aggressive: false,
    });
    r.combat = Some(1);
    w
}

/// An unarmed building of `owner`'s with 100 health, 6 across and
/// square to the axes, at `position`, which is destroyed when its health
/// runs out.
pub(crate) fn building(id: u32, owner: u32, position: [f32; 2]) -> tgmap::Building {
    tgmap::Building {
        id,
        template: "house".into(),
        owner,
        position,
        facing: [0.0, -1.0],
        fighter: Fighter {
            flags: vec![Id::from("building")],
            destructible: true,
            soft: Shape::Rect {
                width: 7.0,
                height: 7.0,
            },
            target: Shape::Rect {
                width: 6.0,
                height: 6.0,
            },
            ..soldier(Vec::new())
        },
        health: 1.0,
        zone: None,
        members: Vec::new(),
        obstacle: Some(Obstacle {
            position,
            facing: [0.0, -1.0],
            shape: Shape::Rect {
                width: 6.0,
                height: 6.0,
            },
            soft: Shape::Rect {
                width: 7.0,
                height: 7.0,
            },
        }),
        deploy: None,
        organization: None,
        awake: false,
    }
}

/// Player 1's rifleman at 10,10 and whatever buildings, on open ground.
pub(crate) fn siege(buildings: &[tgmap::Building]) -> World {
    let w = battle(&[1], vec![soldier(vec![rifle(12.0, 30.0)])], at_war());
    with_buildings(w, buildings)
}

/// `w` with `buildings` added, their ground blocked.
pub(crate) fn with_buildings(mut w: World, buildings: &[tgmap::Building]) -> World {
    for b in buildings {
        if let Some(o) = &b.obstacle {
            w.pathing.block(o);
        }
    }
    w.add_buildings(buildings);
    w
}

pub(crate) fn lost(building: usize, player: u32, by: u32) -> Notice {
    Notice::Lost {
        player,
        building,
        by,
    }
}

/// Player 2's city centred at 40,10 with a zone of 15 and a house of
/// its own, out of the rifleman's reach.
pub(crate) fn city() -> [tgmap::Building; 2] {
    let mut centre = building(30, 2, [40.0, 10.0]);
    centre.fighter.capturable = true;
    centre.fighter.destructible = false;
    centre.zone = Some(15.0);
    centre.members = vec![31];
    let mut house = building(31, 2, [40.0, 20.0]);
    house.fighter.destructible = false;
    [centre, house]
}

pub(crate) fn player(
    id: u32,
    team: Option<usize>,
    relations: &[u32],
    team_relations: &[u32],
) -> Player {
    let stored = |r: &[u32]| r.iter().map(|&r| Relation::from_u32(r).unwrap()).collect();
    Player {
        id,
        name: format!("kingdom_{id}"),
        color: String::new(),
        rgb: [1.0; 3],
        team,
        independent: team.is_none(),
        playable: team.is_some(),
        default_defeat: true,
        outcome: Default::default(),
        won_by_cities: false,
        resigned: false,
        nation: None,
        faction: None,
        relations: stored(relations),
        team_relations: stored(team_relations),
        stockpile: 0.0,
        default_stockpile: false,
        stockpile_bonus: 0.0,
        technologies: Vec::new(),
        handicap: None,
        barred: Vec::new(),
    }
}

/// Player 1's HQ at 20,20, commissioning a [`truck`] in a second, which it
/// puts out 6 to its left, on open ground with nothing else, in a match
/// that doesn't end.
pub(crate) fn headquarters() -> World {
    let mut hq = building(5, 1, [20.0, 20.0]);
    hq.deploy = Some(DeployPoints {
        creation: [0.0, 0.0],
        deployment: [-6.0, 0.0],
    });
    let mut w = World::from_organizations(at_war(), Pathing::uniform(40, Domain::Land), &[]);
    w.pathing.block(hq.obstacle.as_ref().unwrap());
    w.add_buildings(&[hq]);
    recruits(&mut w, "house", truck(), 1.0);
    // Nobody's out for having nothing, the sides with no base included.
    for p in 1..=3 {
        w.set_default_defeat(p, false);
    }
    w
}

/// Has buildings of template `building` commission `organization`, which
/// takes `seconds`.
pub(crate) fn recruits(
    w: &mut World,
    building: &str,
    organization: tgmap::Organization,
    seconds: f32,
) {
    let template = organization.template.clone();
    let list = w.rules.recruit_lists.entry(Id::from(building)).or_default();
    if !list.contains(&template) {
        list.push(template.clone());
    }
    w.rules.recruit_times.insert(template.clone(), seconds);
    w.rules.raisable.insert(template, organization);
}

/// Asks the building at `building` to commission an organization of `template`.
pub(crate) fn commission(building: usize, template: &str) -> Command {
    Command::Commission {
        building,
        template: Id::from(template),
    }
}

/// The way buildings face unless told otherwise.
pub(crate) const NORTH: [f32; 2] = tgmap::UNPACKED_FACING;

/// A truck that unpacks into an 8-wide depot, standing at the origin.
pub(crate) fn truck() -> tgmap::Organization {
    let mut depot = building(0, 0, [0.0, 0.0]);
    depot.template = "depot".into();
    depot.obstacle.as_mut().unwrap().shape = Shape::Rect {
        width: 8.0,
        height: 8.0,
    };
    let mut m = member(0, [0.0, 0.0], 3.0);
    m.template = "truck".into();
    m.fighter = Some(soldier(Vec::new()));
    tgmap::Organization {
        hq: None,
        id: 0,
        template: "truck".into(),
        owner: 0,
        facing: [0.0, 1.0],
        members: vec![m],
        formations: vec![tgmap::Formation {
            id: String::new(),
            offsets: vec![[0.0, 0.0]],
            aggressive: false,
        }],
        formation: 0,
        combat: None,
        operational_range: None,
        control_range: None,
        primary: Some(0),
        morale: 1.0,
        experience: 0.0,
        efficiency: None,
        unpacks: Some(Unpacks {
            building: depot,
            room: Shape::Rect {
                width: 12.0,
                height: 12.0,
            },
            offset: [0.0, 0.0],
            seconds: 7.0,
        }),
    }
}

/// The depot packing into [`truck`], facing east.
pub(crate) fn packs() -> Packs {
    Packs {
        truck: truck(),
        offset: [0.0, 0.0],
        facing: [1.0, 0.0],
        seconds: 7.0,
        health_min: 0.5,
    }
}

pub(crate) fn amounts(of: &[(&str, f32)]) -> tgmap::Amounts {
    let mut a = tgmap::Amounts::default();
    for &(kind, amount) in of {
        a.set(kind, amount);
    }
    a
}

/// [`headquarters`] earning and keeping up like a Corps HQ, its trucks
/// like a Division HQ's and the depot they unpack into earning twice
/// theirs, with the game's limits and shortfall prices, and player 1
/// holding 500.
/// AA's resources: money bought with, ammo and oil kept up, and the
/// organization and building limits.
pub(crate) fn aa_resources() -> Vec<tgmap::ResourceKind> {
    use tgmap::ResourceKind as R;
    let free = || amounts(&[("money", 0.0)]);
    vec![
        R::stockpiled("money"),
        R::upkeep("ammo", amounts(&[("money", 2.0)])),
        R::upkeep("oil", amounts(&[("money", 3.0)])),
        R::upkeep("unit_limit_provided", free()),
        R::limited("unit_limit_consumed", "unit_limit_provided"),
        R::upkeep("building_limit_provided", free()),
        R::limited("building_limit_consumed", "building_limit_provided"),
    ]
}

pub(crate) fn economy() -> World {
    let mut w = headquarters();
    let economy = |cost, production, upkeep| tgmap::Economy {
        cost: amounts(cost),
        production: amounts(production),
        upkeep: amounts(upkeep),
        ..Default::default()
    };
    w.rules.economies.insert(
        Id::from("house"),
        economy(
            &[],
            &[("money", 25.0), ("ammo", 20.0), ("oil", 10.0)],
            &[("building_limit_consumed", 1.0)],
        ),
    );
    w.rules.economies.insert(
        Id::from("truck"),
        economy(
            &[("money", 250.0)],
            &[
                ("money", 12.0),
                ("ammo", 5.0),
                ("oil", 2.0),
                ("unit_limit_provided", 6.0),
            ],
            &[("building_limit_consumed", 1.0)],
        ),
    );
    w.rules.economies.insert(
        Id::from("depot"),
        economy(
            &[],
            &[
                ("money", 25.0),
                ("ammo", 10.0),
                ("oil", 5.0),
                ("unit_limit_provided", 6.0),
            ],
            &[("building_limit_consumed", 1.0)],
        ),
    );
    w.rules.economy = tgmap::EconomyRules {
        resources: aa_resources(),
        limited_max: 20.0,
        stockpile_max: 99999.0,
        ..Default::default()
    };
    w.set_stockpile(1, 500.0);
    w
}

/// An organization of three soldiers of `owner`'s, which with each soldier
/// keeps up ammo.
pub(crate) fn company(w: &mut World, owner: u32) -> usize {
    let organization = raised_company(w, owner);
    w.add_organization(&organization)
}

/// A [`company`] yet to be put on the map, taking a place under the
/// organization limit.
pub(crate) fn raised_company(w: &mut World, owner: u32) -> tgmap::Organization {
    let members = (0..3).map(|i| member(20 + i, [5.0 + i as f32, 5.0], 1.0));
    let mut organization = truck();
    organization.template = "company".into();
    organization.owner = owner;
    organization.members = members.collect();
    organization.unpacks = None;
    organization.primary = None;
    organization.formations[0].offsets = vec![[0.0, 0.0]; 3];
    let economy = |upkeep| tgmap::Economy {
        upkeep: amounts(upkeep),
        ..Default::default()
    };
    w.rules.economies.insert(
        Id::from("company"),
        economy(&[("ammo", 10.0), ("unit_limit_consumed", 1.0)]),
    );
    w.rules
        .economies
        .insert(Id::from("soldier"), economy(&[("ammo", 2.0)]));
    organization
}

pub(crate) fn supply(range: f32, relay: bool, stockpile_max: f32) -> tgmap::Supply {
    tgmap::Supply {
        range,
        relay,
        stockpile_max,
    }
}

pub(crate) fn logistics(attachments: Option<u32>) -> tgmap::Logistics {
    tgmap::Logistics {
        supply_used: attachments.is_none(),
        detached: false,
        shortfall_damage: 0.01,
        attachments,
    }
}

/// [`economy`] with its HQ supplying 30 around it, a Division HQ's
/// truck of player 1's that takes two companies at 30,30 (organization 0)
/// and a company of three at 26,20 (organization 1), detached.
pub(crate) fn division() -> World {
    let mut w = economy();
    w.rules
        .supplies
        .insert(Id::from("house"), supply(30.0, false, 0.0));
    w.rules
        .logistics
        .insert(Id::from("truck"), logistics(Some(2)));
    w.rules
        .logistics
        .insert(Id::from("company"), logistics(None));
    w.rules.resupply_rates.insert(Id::from("soldier"), 0.5);
    w.rules
        .recruit_lists
        .insert(Id::from("depot"), vec![Id::from("company")]);
    let mut t = truck();
    (t.id, t.owner) = (50, 1);
    t.members[0].id = 50;
    t.place([30.0, 30.0], [0.0, 1.0]);
    w.add_organization(&t);
    let mut c = raised_company(&mut w, 1);
    c.renumber(60);
    c.primary = Some(0);
    c.control_range = Some(18.0);
    for m in &mut c.members {
        m.fighter = Some(soldier(Vec::new()));
    }
    c.place([26.0, 20.0], [0.0, 1.0]);
    w.add_organization(&c);
    for u in w.organizations[1].units.clone() {
        w.units[u].template = "soldier".into();
    }
    w
}

/// [`division`] a couple of seconds on, its company attached and in
/// supply.
pub(crate) fn supplied_division() -> World {
    let mut w = division();
    w.apply(1, Command::Attach { organization: 1 }).unwrap();
    run(&mut w, 2.0);
    assert!(w.organizations[1].in_supply);
    w
}

pub(crate) fn effect(range: f32, resupply: f32, supplied: bool, requires: &[&str]) -> SupplyEffect {
    SupplyEffect {
        range,
        resupply,
        supplied,
        requires: requires.iter().map(|&p| Id::from(p)).collect(),
    }
}

pub(crate) fn kind(properties: &[&str]) -> tgmap::Kind {
    tgmap::Kind {
        properties: properties.iter().map(|&p| Id::from(p)).collect(),
        ..tgmap::Kind::default()
    }
}

/// Measures combat values in `w` against an average of 100 health, no
/// defense, speed 2 and 10 damage, and gives `template` the figures of a
/// rifleman doing `damage` every 2 seconds at range 12.
pub(crate) fn rate(w: &mut World, template: &str, damage: f32) {
    w.rules.combat_means = crate::CombatMeans {
        attack: 10.0,
        defense: 0.0,
        speed: 2.0,
        health: 100.0,
    };
    let profile = tgmap::CombatProfile {
        health: 100.0,
        defense: 0.0,
        speed: Some(2.0),
        abilities: vec![tgmap::CombatAbility {
            damage,
            range: 12.0,
            area: 0.0,
            played: 0.0,
            recharge: 2.0,
            ignored: false,
            projectile: false,
            bombard: false,
        }],
    };
    w.rules.combat_profiles.insert(template.into(), profile);
}

/// Has the unit at `k` hit the unit at `u` just now, as kill credit
/// remembers it.
pub(crate) fn hit_by(w: &mut World, u: usize, k: usize) {
    let by = w.organizations[w.units[k].organization].owner;
    let struck = Some(crate::Struck {
        from: Some(crate::Target::Unit(k)),
        by,
        at: w.ticks,
        filter: tgmap::ANY_DAMAGE,
        damage: 1.0,
    });
    w.units[u].struck = struck;
    let r = w.units[u].organization;
    w.organizations[r].struck = struck;
}
