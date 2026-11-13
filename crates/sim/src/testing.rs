//! What the crate's tests build their matches from.

use std::collections::HashMap;

use aaa_map::{Obstacle, Shape, SupplyEffect, TerrainEffect};

use crate::*;

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

/// A guard of player 1's, an ally's regiment for it to guard and an
/// enemy out of range, 10 apart in a row, the guard noticing nothing
/// for itself.
pub(crate) fn escort() -> World {
    let mut w = battle(
        &[1, 3, 2],
        vec![
            soldier(vec![rifle(6.0, 50.0)]),
            soldier(Vec::new()),
            soldier(vec![rifle(6.0, 1.0)]),
        ],
        at_war(),
    );
    w.regiments[0].control_range = 4.0;
    let guard = Command::GuardThing {
        regiment: 0,
        thing: Enemy::Regiment(1),
    };
    w.apply(1, guard).unwrap();
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

/// A player 2 pillbox at (10, 24), noticing within 20, whose rifle
/// can't hit aircraft, and unarmed soldiers of two of player 1's
/// regiments: the first in its zone, the second, of `flags`, far off.
pub(crate) fn sentry(flags: &[&str]) -> World {
    let mut far = soldier(Vec::new());
    far.flags = flags.iter().map(|&f| Id::from(f)).collect();
    let mut w = battle(&[1, 1], vec![soldier(Vec::new()), far], at_war());
    w.units[1].position = [40.0, 40.0];
    let mut pillbox = building(20, 2, [10.0, 24.0]);
    let mut rifle = rifle(12.0, 10.0);
    rifle.targets.excluded.push(Id::from("air"));
    pillbox.fighter.weapons = vec![rifle];
    pillbox.fighter.notice = Some(20.0);
    with_buildings(w, &[pillbox])
}

/// A pillbox south of the siege's soldier, facing it, whose rifle is on
/// a turret that starts `start` degrees round, turns 90° a second and
/// aims within `sweep` degrees.
pub(crate) fn turreted(start: f32, sweep: f32) -> World {
    let mut pillbox = building(20, 2, [10.0, 24.0]);
    pillbox.fighter.weapons = vec![Weapon {
        aimed_by: Some(0),
        ..rifle(12.0, 10.0)
    }];
    pillbox.fighter.notice = Some(20.0);
    pillbox.fighter.turrets = vec![Turret {
        turn_rate: 90f32.to_radians(),
        sweep: sweep.to_radians(),
        start: start.to_radians(),
        reset: None,
        offset: [0.0; 2],
    }];
    let mut w = siege(&[pillbox]);
    w.units[0].fighter = Some(soldier(Vec::new()));
    // Already woken, as if saved so, since the soldier starts in reach.
    w.buildings[0].awake = true;
    w
}

/// What `w`'s sides have been told of buildings changing hands.
pub(crate) fn captures(w: &World) -> Vec<Notice> {
    let notices = w.notices.iter().cloned();
    notices
        .filter(|n| matches!(n, Notice::Lost { .. } | Notice::Captured { .. }))
        .collect()
}

pub(crate) fn lost(building: usize, player: u32, by: u32) -> Notice {
    Notice::Lost {
        player,
        building,
        by,
    }
}

pub(crate) fn captured(building: usize, player: u32) -> Notice {
    Notice::Captured { player, building }
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

/// Everything about a match that orders and ticks can change.
pub(crate) fn snapshot(w: &World) -> String {
    let units: Vec<_> = w
        .units
        .iter()
        .map(|u| (u.position, u.facing, u.health, u.target, u.shot, u.died))
        .collect();
    let regiments: Vec<_> = w
        .regiments
        .iter()
        .map(|r| (r.position, r.facing, r.formation, r.order, &r.engaged))
        .collect();
    let buildings: Vec<_> = w
        .buildings
        .iter()
        .map(|b| (b.owner, b.health, b.target, b.state, b.captured))
        .collect();
    format!("{:?}", (w.ticks, units, regiments, buildings))
}

/// Four regiments of riflemen moving, running and fighting around a
/// house, played out to the end.
pub(crate) fn played_battle() -> World {
    let rifles = |n| (0..n).map(|_| soldier(vec![rifle(12.0, 25.0)])).collect();
    let mut w = battle(&[1, 2, 3, 2], rifles(4), at_war());
    let house = building(20, 2, [30.0, 10.0]);
    w.pathing.block(house.obstacle.as_ref().unwrap());
    w.add_buildings(&[house]);
    let to = [30.0, 30.0];
    w.apply(1, Command::Move { regiment: 0, to }).unwrap();
    let to = [10.0, 50.0];
    w.apply(3, Command::Run { regiment: 2, to }).unwrap();
    run(&mut w, 15.0);
    let to = [20.0, 20.0];
    w.apply(2, Command::Move { regiment: 3, to }).unwrap();
    run(&mut w, 30.0);
    w
}

/// A match's numbers hashed with FNV-1a, which unlike std's hashers is
/// fixed for good, and which unlike hashing its Debug output doesn't
/// change when something's renamed.
pub(crate) fn fingerprint(worlds: &[&World]) -> u64 {
    let f = |x: &f32| u64::from(x.to_bits());
    let mut numbers = Vec::new();
    for w in worlds {
        numbers.push(w.ticks);
        for u in &w.units {
            numbers.extend(u.position.iter().chain(&u.facing).map(f));
            numbers.extend([f(&u.health), u.died.unwrap_or(u64::MAX)]);
            if let Some(t) = &u.trailer {
                numbers.extend(t.position.iter().chain(&t.facing).map(f));
            }
        }
        for r in &w.regiments {
            numbers.extend(r.position.iter().chain(&r.facing).map(f));
            numbers.push(r.formation as u64);
        }
        for b in &w.buildings {
            numbers.extend([u64::from(b.owner), f(&b.health)]);
            numbers.push(b.captured.unwrap_or(u64::MAX));
        }
    }
    numbers
        .iter()
        .flat_map(|n| n.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
}

/// Player 1's HQ at 20,20, putting out what it commissions 6 to its
/// left, on open ground with nothing else.
pub(crate) fn headquarters() -> World {
    let mut hq = building(5, 1, [20.0, 20.0]);
    hq.deploy = Some(DeployPoints {
        creation: [0.0, 0.0],
        deployment: [-6.0, 0.0],
    });
    let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
    w.pathing.block(hq.obstacle.as_ref().unwrap());
    w.add_buildings(&[hq]);
    w
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

/// A regiment of one truck.
pub(crate) fn convoy() -> aaa_map::Regiment {
    aaa_map::Regiment {
        hq: None,
        template: "convoy".into(),
        ..truck()
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
    w.economies.insert(
        Id::from("house"),
        economy(
            &[],
            &[("money", 25.0), ("ammo", 20.0), ("oil", 10.0)],
            &[("building_limit_consumed", 1.0)],
        ),
    );
    w.economies.insert(
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
    w.economies.insert(
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
    w.economy = aaa_map::EconomyRules {
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
    w.economies.insert(
        Id::from("company"),
        economy(&[("ammo", 10.0), ("unit_limit_consumed", 1.0)]),
    );
    w.economies
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

/// Player 1's Corps HQ at 20,20 and two depots east of it, the first
/// in the HQ's zone and the second only in the first's, each reaching
/// 20 and the depots stocking up to 3 seconds.
pub(crate) fn chain() -> World {
    let mut w = World::from_regiments(at_war(), Pathing::uniform(40, Domain::Land), &[]);
    let mut corps = building(5, 1, [20.0, 20.0]);
    corps.template = "corps".into();
    let depot = |id, x| {
        let mut depot = building(id, 1, [x, 20.0]);
        depot.template = "depot".into();
        depot
    };
    w.add_buildings(&[depot(7, 50.0), depot(6, 34.0), corps]);
    w.supplies
        .insert(Id::from("corps"), supply(20.0, false, 0.0));
    w.supplies
        .insert(Id::from("depot"), supply(20.0, true, 3.0));
    w
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
    w.supplies
        .insert(Id::from("house"), supply(30.0, false, 0.0));
    w.logistics.insert(Id::from("truck"), logistics(Some(2)));
    w.logistics.insert(Id::from("company"), logistics(None));
    w.resupply_rates.insert(Id::from("soldier"), 0.5);
    w.recruit_lists
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

/// [`division`] with a second company at 26,28 (regiment 2), both
/// infantry, and an operation that resupplies the nearest infantry
/// regiment within 15 for 10 seconds.
pub(crate) fn resupplied() -> (World, Id) {
    let mut w = division();
    let mut c = raised_company(&mut w, 1);
    c.renumber(80);
    c.place([26.0, 28.0], [0.0, 1.0]);
    w.add_regiment(&c);
    let kind = aaa_map::Kind {
        properties: Vec::new(),
        flags: vec![Id::from("org_infantry")],
        soft: Shape::Circle { radius: 1.0 },
    };
    w.kinds.insert(Id::from("company"), kind);
    w.supply_effects
        .insert(Id::from("resupplied"), effect(0.0, 0.25, true, &[]));
    let operation = aaa_map::Operation::Enchant {
        targets: aaa_map::Targets {
            required: ["FRIEND", "ORGANIZATION", "ORG_INFANTRY"]
                .map(Id::from)
                .to_vec(),
            excluded: Vec::new(),
        },
        area: 15.0,
        max_targets: 1,
        property: Id::from("resupplied"),
        seconds: 10.0,
    };
    w.operations.insert(Id::from("resupply"), operation);
    (w, Id::from("resupply"))
}

/// Players 1 and 2 at war, as `battle` has them, seeing `range` round
/// themselves and showing those fighting them `remote` round them.
pub(crate) fn sighted(
    fighters: Vec<Fighter>,
    range: f32,
    remote: f32,
    fog: aaa_map::FogSetting,
) -> World {
    let mut w = battle(&[1, 2], fighters, at_war());
    w.sights
        .insert(Id::from("squad"), aaa_map::Sight { range, remote });
    w.set_fog(fog);
    w
}

pub(crate) fn put(w: &mut World, u: usize, at: [f32; 2]) {
    w.units[u].position = at;
    w.units[u].previous = at;
}

/// Forest, whose first property is for infantry and whose second for
/// vehicles, and the rules those properties work by.
pub(crate) fn forest() -> (aaa_map::TileTerrain, aaa_map::TerrainRules) {
    let forest = aaa_map::TileTerrain {
        properties: vec![Id::from("forest_infantry"), Id::from("forest_vehicles")],
        mountain: false,
    };
    let infantry = TerrainEffect {
        requires: vec![Id::from("infantry")],
        defense: 4.0,
        resists: vec![(Id::from("gunfire"), 0.5)],
        speed_limit: Some(1.5),
    };
    let vehicles = TerrainEffect {
        requires: vec![Id::from("mobile"), Id::from("vehicle")],
        defense: 0.0,
        resists: vec![(Id::from("gunfire"), -0.2)],
        speed_limit: Some(1.0),
    };
    let rules = aaa_map::TerrainRules {
        effects: HashMap::from([
            (Id::from("forest_infantry"), infantry),
            (Id::from("forest_vehicles"), vehicles),
        ]),
        unlimited: vec![Id::from("trailblazing")],
        ..aaa_map::TerrainRules::default()
    };
    (forest, rules)
}

pub(crate) fn kind(properties: &[&str]) -> aaa_map::Kind {
    aaa_map::Kind {
        properties: properties.iter().map(|&p| Id::from(p)).collect(),
        ..aaa_map::Kind::default()
    }
}

/// The economy's HQ listing two technologies for its side, 60 and 100
/// money, the second needing the first and a depot.
pub(crate) fn laboratory() -> World {
    let mut w = economy();
    let item = |technology: &str, money| aaa_map::Research {
        technology: Id::from(technology),
        grant: aaa_map::Grant::Side,
        seconds: 1.0,
        cost: amounts(&[("money", money)]),
        structures: Vec::new(),
        structures_any: false,
        properties: Vec::new(),
        properties_any: false,
    };
    let mut tiger = item("tech_tiger", 100.0);
    tiger.properties = vec![Id::from("tech_heavy")];
    tiger.structures = vec![vec![Id::from("depot")]];
    (w.research_lists).insert(Id::from("house"), vec![item("tech_heavy", 60.0), tiger]);
    w
}

pub(crate) fn research(technology: &str) -> Command {
    Command::Research {
        building: 0,
        technology: Id::from(technology),
    }
}

/// [`economy`] with its house, building 0, selling for half its cost
/// of 301 money and 40 ammo.
pub(crate) fn market() -> World {
    let mut w = economy();
    w.economies.get_mut(&Id::from("house")).unwrap().cost =
        amounts(&[("money", 301.0), ("ammo", 40.0)]);
    w.sellable.insert(Id::from("house"));
    w.economy.sell_factor = 0.5;
    w
}
