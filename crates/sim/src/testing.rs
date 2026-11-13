//! What the crate's tests build their matches from.

use std::collections::HashMap;

use aaa_map::{
    DeployPoints, Domain, Fighter, Id, Obstacle, Packs, Pathing, Shape, SupplyEffect, Unpacks,
    Weapon,
};

use crate::{Command, Notice, Player, Relation, Sides, TICK, World, distance};

pub(crate) fn member(id: u32, position: [f32; 2], speed: f32) -> aaa_map::Member {
    aaa_map::Member {
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

/// Two soldiers side by side facing +y, the slower one on the right,
/// who can also line up one behind the other.
pub(crate) fn world(pathing: Pathing) -> World {
    let formation = |id: &str, offsets| aaa_map::Formation {
        id: id.into(),
        offsets,
        aggressive: false,
    };
    let regiment = aaa_map::Regiment {
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
        unpacks: None,
    };
    World::from_regiments(Sides::default(), pathing, &[regiment])
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
pub(crate) fn vehicle(
    acceleration: f32,
    turn_rate: f32,
    trailer: Option<aaa_map::Trailer>,
) -> World {
    let mut m = member(2, [10.0, 10.0], 2.0);
    (m.acceleration, m.turn_rate, m.trailer) = (acceleration, turn_rate, trailer);
    let regiment = aaa_map::Regiment {
        hq: None,
        id: 1,
        template: "tank".into(),
        owner: 7,
        facing: [0.0, 1.0],
        members: vec![m],
        formations: vec![aaa_map::Formation {
            id: "lone".into(),
            offsets: vec![[0.0, 0.0]],
            aggressive: false,
        }],
        formation: 0,
        combat: None,
        operational_range: None,
        control_range: None,
        primary: None,
        unpacks: None,
    };
    World::from_regiments(
        Sides::default(),
        Pathing::uniform(20, Domain::Land),
        &[regiment],
    )
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
        targets: aaa_map::Targets {
            required: vec![Id::from("enemy")],
            excluded: vec![Id::from("tank")],
        },
        damage: vec![aaa_map::Damage {
            kind: Id::from("gunfire"),
            amount,
        }],
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
        weapons,
        attack_height: 1.45,
        notice: None,
        turrets: Vec::new(),
        soft: Shape::Point,
        target: Shape::Point,
        target_square: false,
    }
}

/// A regiment for each of `owners`, of one fighter each, the first at
/// 10,10 and each next 10 further along y.
pub(crate) fn battle(owners: &[u32], fighters: Vec<Fighter>, sides: Sides) -> World {
    let regiments = owners
        .iter()
        .zip(fighters)
        .enumerate()
        .map(|(i, (&owner, fighter))| {
            let mut m = member(i as u32 * 2 + 2, [10.0, 10.0 + 10.0 * i as f32], 2.0);
            m.fighter = Some(fighter);
            aaa_map::Regiment {
                hq: None,
                id: i as u32 * 2 + 1,
                template: "squad".into(),
                owner,
                facing: [0.0, 1.0],
                members: vec![m],
                formations: vec![aaa_map::Formation {
                    id: "line".into(),
                    offsets: vec![[0.0, 0.0]],
                    aggressive: false,
                }],
                formation: 0,
                combat: None,
                operational_range: Some(34.0),
                control_range: Some(18.0),
                primary: None,
                unpacks: None,
            }
        })
        .collect::<Vec<_>>();
    World::from_regiments(sides, Pathing::uniform(40, Domain::Land), &regiments)
}

/// Players 1 and 2 at war, and 3 allied to 1.
pub(crate) fn at_war() -> Sides {
    let side = |id, relations: &[u32]| Player {
        independent: false,
        ..player(id, None, relations, &[])
    };
    Sides {
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
    let r = &mut w.regiments[0];
    r.formations.push(aaa_map::Formation {
        id: "wide".into(),
        offsets: vec![[0.0, 0.0]],
        aggressive: false,
    });
    r.combat = Some(1);
    w
}

/// An unarmed building of `owner`'s with 100 health, 6 across and
/// square to the axes, at `position`.
pub(crate) fn building(id: u32, owner: u32, position: [f32; 2]) -> aaa_map::Building {
    aaa_map::Building {
        id,
        template: "house".into(),
        owner,
        position,
        facing: [0.0, -1.0],
        fighter: Fighter {
            flags: vec![Id::from("building")],
            destructible: false,
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
pub(crate) fn siege(buildings: &[aaa_map::Building]) -> World {
    let w = battle(&[1], vec![soldier(vec![rifle(12.0, 30.0)])], at_war());
    with_buildings(w, buildings)
}

/// `w` with `buildings` added, their ground blocked.
pub(crate) fn with_buildings(mut w: World, buildings: &[aaa_map::Building]) -> World {
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
pub(crate) fn city() -> [aaa_map::Building; 2] {
    let mut centre = building(30, 2, [40.0, 10.0]);
    centre.fighter.capturable = true;
    centre.zone = Some(15.0);
    centre.members = vec![31];
    [centre, building(31, 2, [40.0, 20.0])]
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
        nation: None,
        general: None,
        relations: stored(relations),
        team_relations: stored(team_relations),
        stockpile: 0.0,
        technologies: Vec::new(),
        handicap: None,
        barred: Vec::new(),
    }
}

/// Player 1's HQ at 20,20, commissioning a [`truck`] in a second, which it
/// puts out 6 to its left, on open ground with nothing else.
pub(crate) fn headquarters() -> World {
    let mut hq = building(5, 1, [20.0, 20.0]);
    hq.deploy = Some(DeployPoints {
        creation: [0.0, 0.0],
        deployment: [-6.0, 0.0],
    });
    let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
    w.pathing.block(hq.obstacle.as_ref().unwrap());
    w.add_buildings(&[hq]);
    recruits(&mut w, "house", truck(), 1.0);
    w
}

/// Has buildings of template `building` commission `regiment`, which
/// takes `seconds`.
pub(crate) fn recruits(w: &mut World, building: &str, regiment: aaa_map::Regiment, seconds: f32) {
    let template = regiment.template.clone();
    let list = w.rules.recruit_lists.entry(Id::from(building)).or_default();
    if !list.contains(&template) {
        list.push(template.clone());
    }
    w.rules.recruit_times.insert(template.clone(), seconds);
    w.rules.raisable.insert(template, regiment);
}

/// Asks the building at `building` to commission a regiment of `template`.
pub(crate) fn commission(building: usize, template: &str) -> Command {
    Command::Commission {
        building,
        template: Id::from(template),
    }
}

/// The way buildings face unless told otherwise.
pub(crate) const NORTH: [f32; 2] = aaa_map::UNPACKED_FACING;

/// A truck that unpacks into an 8-wide depot, standing at the origin.
pub(crate) fn truck() -> aaa_map::Regiment {
    let mut depot = building(0, 0, [0.0, 0.0]);
    depot.template = "depot".into();
    depot.obstacle.as_mut().unwrap().shape = Shape::Rect {
        width: 8.0,
        height: 8.0,
    };
    let mut m = member(0, [0.0, 0.0], 3.0);
    m.template = "truck".into();
    m.fighter = Some(soldier(Vec::new()));
    aaa_map::Regiment {
        hq: None,
        id: 0,
        template: "truck".into(),
        owner: 0,
        facing: [0.0, 1.0],
        members: vec![m],
        formations: vec![aaa_map::Formation {
            id: String::new(),
            offsets: vec![[0.0, 0.0]],
            aggressive: false,
        }],
        formation: 0,
        combat: None,
        operational_range: None,
        control_range: None,
        primary: Some(0),
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

pub(crate) fn amounts(of: &[(&str, f32)]) -> aaa_map::Amounts {
    let mut a = aaa_map::Amounts::default();
    for &(kind, amount) in of {
        a.set(kind, amount);
    }
    a
}

/// [`headquarters`] earning and keeping up like a Corps HQ, its trucks
/// like a Division HQ's and the depot they unpack into earning twice
/// theirs, with the game's limits and shortfall prices, and player 1
/// holding 500.
pub(crate) fn economy() -> World {
    let mut w = headquarters();
    let economy = |cost, production, upkeep| aaa_map::Economy {
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
    w.rules.economy = aaa_map::EconomyRules {
        purchase_cost: amounts(&[("ammo", 2.0), ("oil", 3.0)]),
        limits: vec![
            ("unit_limit_consumed", "unit_limit_provided"),
            ("building_limit_consumed", "building_limit_provided"),
        ],
        limited_max: 20.0,
        stockpile_max: 99999.0,
        ..Default::default()
    };
    w.set_stockpile(1, 500.0);
    w
}

/// A regiment of three soldiers of `owner`'s, which with each soldier
/// keeps up ammo.
pub(crate) fn company(w: &mut World, owner: u32) -> usize {
    let regiment = raised_company(w, owner);
    w.add_regiment(&regiment)
}

/// A [`company`] yet to be put on the map, taking a place under the
/// regiment limit.
pub(crate) fn raised_company(w: &mut World, owner: u32) -> aaa_map::Regiment {
    let members = (0..3).map(|i| member(20 + i, [5.0 + i as f32, 5.0], 1.0));
    let mut regiment = truck();
    regiment.template = "company".into();
    regiment.owner = owner;
    regiment.members = members.collect();
    regiment.unpacks = None;
    regiment.primary = None;
    regiment.formations[0].offsets = vec![[0.0, 0.0]; 3];
    let economy = |upkeep| aaa_map::Economy {
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
    regiment
}

pub(crate) fn supply(range: f32, relay: bool, stockpile_max: f32) -> aaa_map::Supply {
    aaa_map::Supply {
        range,
        relay,
        stockpile_max,
    }
}

pub(crate) fn logistics(attachments: Option<u32>) -> aaa_map::Logistics {
    aaa_map::Logistics {
        supply_used: attachments.is_none(),
        detached: false,
        shortfall_damage: 0.01,
        attachments,
    }
}

/// [`economy`] with its HQ supplying 30 around it, a Division HQ's
/// truck of player 1's that takes two companies at 30,30 (regiment 0)
/// and a company of three at 26,20 (regiment 1), detached.
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
    w.add_regiment(&t);
    let mut c = raised_company(&mut w, 1);
    c.renumber(60);
    c.primary = Some(0);
    c.control_range = Some(18.0);
    for m in &mut c.members {
        m.fighter = Some(soldier(Vec::new()));
    }
    c.place([26.0, 20.0], [0.0, 1.0]);
    w.add_regiment(&c);
    for u in w.regiments[1].units.clone() {
        w.units[u].template = "soldier".into();
    }
    w
}

/// [`division`] a couple of seconds on, its company attached and in
/// supply.
pub(crate) fn supplied_division() -> World {
    let mut w = division();
    w.apply(1, Command::Attach { regiment: 1 }).unwrap();
    run(&mut w, 2.0);
    assert!(w.regiments[1].in_supply);
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

pub(crate) fn kind(properties: &[&str]) -> aaa_map::Kind {
    aaa_map::Kind {
        properties: properties.iter().map(|&p| Id::from(p)).collect(),
        ..aaa_map::Kind::default()
    }
}
