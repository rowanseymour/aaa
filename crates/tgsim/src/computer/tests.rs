use std::sync::Arc;

use tgmap::sai::{ActorPriority, Ego, Filter, GoalPriority, Profile, RecruitRequest, TechPriority};
use tgmap::{Id, Region, Regions};

use super::planner::{Member, Wanted};
use crate::testing::{self, kind, rate};
use crate::{Command, ComputerPlayer, Planner, SECOND, World};

/// Two regions, the left half of the map and the right, next to each
/// other.
fn halves(w: &mut World) {
    let side = w.pathing.side as f32 * tgmap::pathing::CELL_SIZE;
    let at = |x: f32, next: usize| Region {
        centre: [x, side / 2.0],
        neighbours: vec![(next, side / 2.0)],
        island: None,
    };
    let regions = vec![at(side / 4.0, 1), at(side * 3.0 / 4.0, 0)];
    w.regions = Arc::new(Regions::laid_out(&w.pathing, regions));
}

/// A mood that weighs goals as they come, with no random amounts.
fn mood() -> Ego {
    let mut ego = Ego::default();
    let settings = &mut ego.goal_engine.general_settings;
    settings.fuzziness = 0.0;
    settings.goal_inertia = 0.0;
    ego.goal_engine.recruiting.candidate_fuzziness = 0.0;
    ego.goal_engine.unpacking.fuzzy_factor = 0.0;
    // However little it earns.
    ego.goal_engine.recruiting.insufficient_income_level = f32::MIN;
    ego
}

fn planner(moods: Vec<Ego>) -> Planner {
    let profile = Profile {
        id: Id::from("test_sai"),
        ego_fuzziness: 0.0,
        max_ge_commands: 100,
        max_rai_commands: 40,
        moods,
        sends_air_attacks: true,
        explored_share: 0.1,
    };
    Planner::new(Arc::new(profile), 1, 0.0)
}

/// Asks for one of whatever has `property`, liking it by `value`.
fn asks_for(ego: &mut Ego, property: &str, value: f32) {
    let property = Some(Id::from(property));
    ego.goal_engine
        .recruiting
        .specific_recruit_request
        .push(RecruitRequest {
            property_ids: property.clone(),
            ..Default::default()
        });
    ego.goal_engine.actor_priority.push(ActorPriority {
        property_ids: property,
        value,
        ..Default::default()
    });
}

/// [`testing::economy`]'s HQ, whose trucks are oil trucks, cut into
/// halves.
fn base() -> World {
    let mut w = testing::economy();
    w.rules.kinds.insert(Id::from("truck"), kind(&["oil"]));
    // With room for the buildings it raises.
    let house = w.rules.economies.get_mut(&Id::from("house")).unwrap();
    house.production.set("building_limit_provided", 10.0);
    halves(&mut w);
    w
}

#[test]
fn it_recruits_what_its_mood_asks_for() {
    let mut w = base();
    let mut ego = mood();
    asks_for(&mut ego, "oil", 250.0);
    let mut p = planner(vec![ego]);
    let orders = p.think(&w, 1);
    assert_eq!(orders, [testing::commission(0, "truck")]);
    // One at a time: the HQ's queue is full until it's out.
    w.apply(1, orders[0].clone()).unwrap();
    w.ticks += 30 * SECOND;
    assert_eq!(p.think(&w, 1), []);
}

#[test]
fn it_recruits_nothing_its_mood_doesnt_like() {
    let w = base();
    let mut ego = mood();
    asks_for(&mut ego, "oil", -100.0);
    assert_eq!(planner(vec![ego]).think(&w, 1), []);
}

#[test]
fn it_saves_for_what_it_cant_yet_pay_for() {
    let mut w = base();
    w.set_stockpile(1, 100.0);
    let mut ego = mood();
    asks_for(&mut ego, "oil", 250.0);
    // It earns enough within the time it saves for.
    ego.goal_engine.resource_management.saving_time_limit = 3600.0;
    assert_eq!(planner(vec![ego]).think(&w, 1), []);
}

#[test]
fn it_plans_again_only_once_its_period_is_out() {
    let mut w = base();
    let mut ego = mood();
    asks_for(&mut ego, "oil", 250.0);
    ego.goal_engine.build_queue_max_items = 5;
    let mut p = planner(vec![ego]);
    assert_eq!(p.think(&w, 1).len(), 1);
    w.ticks += 29 * SECOND;
    assert_eq!(p.think(&w, 1), []);
    w.ticks += SECOND;
    assert_eq!(p.think(&w, 1).len(), 1);
}

#[test]
fn its_mood_changes_only_once_its_filters_fail() {
    let w = base();
    // The HQ nets 25 a minute.
    let filtered = |name: &str, min: f32, max: f32| {
        let mut ego = mood();
        ego.ids = Some(Id::from(name));
        ego.filters.item.push(Filter {
            stat: Some(Id::from("money_rate")),
            min,
            max,
        });
        ego
    };
    let mut p = planner(vec![
        filtered("poor", -100.0, 40.0),
        filtered("rich", 20.0, 100.0),
    ]);
    p.think(&w, 1);
    assert_eq!(p.mood(), Some(&Id::from("poor")));
    let mut p = planner(vec![
        filtered("poor", -100.0, 10.0),
        filtered("rich", 20.0, 100.0),
    ]);
    p.think(&w, 1);
    assert_eq!(p.mood(), Some(&Id::from("rich")));
}

#[test]
fn its_trucks_drive_off_to_unpack_away_from_the_enemy() {
    let mut w = base();
    let truck = {
        let mut t = testing::truck();
        t.owner = 1;
        t.id = 40;
        t.place([20.0, 40.0], [0.0, 1.0]);
        t
    };
    let t = w.add_organization(&truck);
    // Player 2's strength fills the right half.
    let mut enemy = testing::raised_company(&mut w, 2);
    enemy.id = 50;
    enemy.place([60.0, 40.0], [0.0, 1.0]);
    let e = w.add_organization(&enemy);
    rate(&mut w, "soldier", 50.0);
    assert!(w.full_value(e) > 0.0);
    let mut ego = mood();
    ego.goal_engine.unpacking.num_random_regions = 0;
    let orders = planner(vec![ego]).think(&w, 1);
    let [
        Command::Unpack {
            organization, at, ..
        },
    ] = orders[..]
    else {
        panic!("not unpacked: {orders:?}");
    };
    assert_eq!(organization, t);
    assert_eq!(w.regions.at(at), Some(0));
}

#[test]
fn it_researches_what_its_mood_rates() {
    let mut w = base();
    let research = |id: &str| tgmap::Research {
        technology: Id::from(id),
        grant: tgmap::Grant::Side,
        seconds: 1.0,
        cost: testing::amounts(&[("money", 50.0)]),
        structures: Vec::new(),
        structures_any: false,
        properties: Vec::new(),
        properties_any: false,
    };
    w.rules.research_lists.insert(
        Id::from("house"),
        vec![research("tech_slow"), research("tech_fast")],
    );
    let mut ego = mood();
    ego.goal_engine.research.base_priority = 10.0;
    ego.goal_engine.research.tech_priority.push(TechPriority {
        tech: Some(Id::from("tech_fast")),
        priority: 50.0,
    });
    let orders = planner(vec![ego]).think(&w, 1);
    assert_eq!(
        orders,
        [Command::Research {
            building: 0,
            technology: Id::from("tech_fast"),
        }]
    );
}

#[test]
fn it_attacks_an_enemy_it_knows_of() {
    let mut w = testing::skirmish();
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    // It's worth attacking only for what the mood makes of it.
    let mut ego = mood();
    ego.goal_engine.actor_priority.push(ActorPriority {
        actor_ids: Some(Id::from("squad")),
        value: 500.0,
        ..Default::default()
    });
    ego.goal_engine.offensive_cv.min_cv_to_assign = 1.0;
    let orders = planner(vec![ego]).think(&w, 1);
    assert!(
        orders.contains(&Command::Attack {
            organization: 0,
            to: w.centre(1),
        }),
        "{orders:?}"
    );
}

#[test]
fn a_mood_weighs_its_goals() {
    let mut w = testing::skirmish();
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    let mut ego = mood();
    ego.goal_engine.actor_priority.push(ActorPriority {
        actor_ids: Some(Id::from("squad")),
        value: 500.0,
        ..Default::default()
    });
    ego.goal_engine.offensive_cv.min_cv_to_assign = 1.0;
    // Attacking is ruled out, and defending pays nothing.
    for goal in ["ATTACK_REGION", "DEFEND_REGION"] {
        ego.goal_engine.goal_priority.push(GoalPriority {
            goal_ids: Some(Id::from(goal)),
            multiplier: 0.0,
            ..Default::default()
        });
    }
    assert_eq!(planner(vec![ego]).think(&w, 1), []);
}

#[test]
fn a_fight_is_worth_more_with_the_strength_it_wants() {
    let wanted = Wanted {
        target: 100.0,
        enough: 250.0,
        exponent: 2.0,
        over: 4.0,
        given: 0.0,
    };
    assert_eq!(wanted.factor(50.0), 0.25);
    assert_eq!(wanted.factor(100.0), 1.0);
    assert_eq!(wanted.factor(175.0), 2.5);
    assert_eq!(wanted.factor(1000.0), 4.0);
    let helped = Wanted {
        given: 50.0,
        ..wanted
    };
    assert_eq!(helped.factor(50.0), 1.0);
}

#[test]
fn it_gives_no_orders_while_triggers_have_switched_it_off() {
    let mut w = base();
    let mut ego = mood();
    asks_for(&mut ego, "oil", 250.0);
    let mut p = planner(vec![ego]);
    w.triggers.computer.off_for_sides.insert(1);
    assert_eq!(p.think(&w, 1), []);
    // Back on, it plans at once.
    w.triggers.computer.off_for_sides.remove(&1);
    assert_eq!(p.think(&w, 1), [testing::commission(0, "truck")]);
}

#[test]
fn a_truck_answers_requests_by_its_own_properties_alone() {
    let mut w = base();
    // Its depot makes oil, but the truck itself is only a truck.
    w.rules.kinds.insert(Id::from("truck"), kind(&["truck"]));
    w.rules.kinds.insert(Id::from("depot"), kind(&["oil"]));
    let mut ego = mood();
    asks_for(&mut ego, "oil", 250.0);
    assert_eq!(planner(vec![ego]).think(&w, 1), []);
}

#[test]
fn a_thing_takes_the_best_of_the_entries_it_matches_not_their_sum() {
    let mut w = base();
    w.rules
        .kinds
        .insert(Id::from("truck"), kind(&["oil", "ammo"]));
    let mut ego = mood();
    asks_for(&mut ego, "oil", 250.0);
    // Liked as ammo less than it's disliked as nothing else.
    ego.goal_engine.actor_priority.push(ActorPriority {
        property_ids: Some(Id::from("ammo")),
        value: 100.0,
        ..Default::default()
    });
    ego.goal_engine.actor_priority.push(ActorPriority {
        actor_ids: Some(Id::from("truck")),
        value: -1000.0,
        ..Default::default()
    });
    // 250, not 250 + 100 - 1000.
    assert_eq!(
        planner(vec![ego]).think(&w, 1),
        [testing::commission(0, "truck")]
    );
}

#[test]
fn a_hurt_unit_out_of_supply_runs_back_into_it() {
    let mut w = testing::division();
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    let c = 1;
    for u in w.organizations[c].units.clone() {
        w.units[u].position = [70.0, 70.0];
        w.units[u].health = 0.1 * w.max_health(u);
    }
    w.organizations[c].position = [70.0, 70.0];
    // Its supply laid.
    testing::run(&mut w, 1.0);
    assert!(!w.supplied(1, w.centre(c)) && w.supplied(1, [20.0, 25.0]));
    let orders = planner(vec![mood()]).think(&w, 1);
    let ran = orders.iter().find_map(|o| match *o {
        Command::Run { organization, to } if organization == c => Some(to),
        _ => None,
    });
    let to = ran.unwrap_or_else(|| panic!("didn't run: {orders:?}"));
    assert!(w.supplied(1, to));
    // Within reach of the house that gives it.
    assert!(crate::distance(to, w.buildings[0].position) <= 0.9 * 30.0);
}

/// A fight in `region` of player 1's with `priority`, wanting next to no
/// strength.
fn fight(region: usize, attack: bool, priority: f32, w: &World) -> super::planner::Goal {
    use super::planner::{Goal, Key, Work};
    Goal {
        key: match attack {
            true => Key::Attack(region),
            false => Key::Defend(region),
        },
        kind: match attack {
            true => "ATTACK_REGION",
            false => "DEFEND_REGION",
        },
        priority,
        cost: 0.0,
        work: Work::Force {
            region,
            to: w.regions.regions[region].centre,
            attack,
            strength: Wanted {
                target: 1.0,
                enough: 1.0,
                exponent: 1.0,
                over: 1.0,
                given: 0.0,
            },
        },
    }
}

/// The goal each of player 1's units serves once `p` has shared them out
/// among `goals`.
fn shared_out(
    p: &Planner,
    w: &World,
    goals: Vec<super::planner::Goal>,
) -> std::collections::HashMap<Member, super::planner::Key> {
    let mut memory = Vec::new();
    let picture = super::picture::Picture::new(w, 1, &mut memory);
    super::choosing::choose(p, &picture, goals).serving
}

#[test]
fn a_goal_takes_a_unit_from_one_ranked_above_that_gains_less_from_it() {
    use super::planner::Key;
    let mut w = testing::skirmish();
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    let mut ego = mood();
    // Its rifleman is next to no use defending.
    ego.goal_engine.goal_priority.push(GoalPriority {
        goal_ids: Some(Id::from("DEFEND_REGION")),
        actor_preference: vec![tgmap::sai::ActorPreference {
            actor_ids: Some(Id::from("squad")),
            multiplier: 0.01,
            ..Default::default()
        }],
        ..Default::default()
    });
    let p = planner(vec![ego]);
    let serving = shared_out(
        &p,
        &w,
        vec![fight(0, false, 1000.0, &w), fight(1, true, 900.0, &w)],
    );
    assert_eq!(serving.get(&Member::Organization(0)), Some(&Key::Attack(1)));
}

#[test]
fn a_unit_stays_on_its_attack_when_one_nearby_is_ranked_higher() {
    use super::planner::Key;
    let mut w = testing::skirmish();
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    let goals = |w: &World| vec![fight(0, true, 1000.0, w), fight(1, true, 500.0, w)];
    let mut p = planner(vec![mood()]);
    p.serving.insert(Member::Organization(0), Key::Attack(1));
    // It's locked on to its own.
    assert_eq!(
        shared_out(&p, &w, goals(&w)).get(&Member::Organization(0)),
        Some(&Key::Attack(1))
    );
    // Not locked, it stays while the higher shares its strength, and goes
    // where it's worth more once that doesn't.
    for (linked, goes) in [(2, Key::Attack(1)), (0, Key::Attack(0))] {
        let mut ego = mood();
        ego.goal_engine.combat.linked_region_distance = linked;
        ego.goal_engine.combat.attack_commit_distance = 0;
        let mut p = planner(vec![ego]);
        p.serving.insert(Member::Organization(0), Key::Attack(1));
        assert_eq!(
            shared_out(&p, &w, goals(&w)).get(&Member::Organization(0)),
            Some(&goes)
        );
    }
}

/// [`crate::airfields::tests::field`], cut into halves, with player 2's
/// house at 80,50 worth attacking and at `health` of its 100.
fn airfield(health: f32) -> World {
    let mut w = crate::airfields::tests::field();
    halves(&mut w);
    w.buildings[2].health = health;
    w
}

/// A mood that likes attacking houses.
fn likes_houses() -> Ego {
    let mut ego = mood();
    ego.goal_engine.actor_priority.push(ActorPriority {
        actor_ids: Some(Id::from("house")),
        value: 500.0,
        ..Default::default()
    });
    ego
}

fn air_attacks(orders: &[Command]) -> Vec<&Command> {
    (orders.iter())
        .filter(|c| matches!(c, Command::AirAttack { .. }))
        .collect()
}

#[test]
fn it_air_attacks_a_damaged_target_with_its_escort_on() {
    use crate::Piece;
    use crate::airfields::tests::FIELD;
    // Whole, alone and in no region it attacks, the house isn't worth it.
    let w = airfield(100.0);
    assert_eq!(
        air_attacks(&planner(vec![likes_houses()]).think(&w, 1)),
        [] as [&Command; 0]
    );
    let w = airfield(60.0);
    let mut p = planner(vec![likes_houses()]);
    let orders = p.think(&w, 1);
    let escort = Command::Escort {
        building: FIELD,
        on: true,
    };
    let strike = Command::AirAttack {
        building: FIELD,
        at: [80.0, 50.0],
        on: Some(Piece::Building(2)),
    };
    let at = |c: &Command| orders.iter().position(|o| o == c);
    assert!(at(&escort).unwrap() < at(&strike).unwrap(), "{orders:?}");
}

#[test]
fn it_waits_before_air_attacking_the_same_target_again() {
    let mut w = airfield(60.0);
    let mut p = planner(vec![likes_houses()]);
    assert_eq!(air_attacks(&p.think(&w, 1)).len(), 1);
    // Nothing went, but it plans again before 120 seconds are out.
    w.ticks += 90 * SECOND;
    assert_eq!(air_attacks(&p.think(&w, 1)), [] as [&Command; 0]);
    w.ticks += 30 * SECOND;
    assert_eq!(air_attacks(&p.think(&w, 1)).len(), 1);
}

#[test]
fn a_side_whose_game_has_no_air_attacks_sends_none() {
    let w = airfield(60.0);
    let profile = Profile {
        id: Id::from("test_sai"),
        ego_fuzziness: 0.0,
        max_ge_commands: 100,
        max_rai_commands: 40,
        moods: vec![likes_houses()],
        sends_air_attacks: false,
        explored_share: 0.1,
    };
    let mut p = Planner::new(Arc::new(profile), 1, 0.0);
    assert_eq!(air_attacks(&p.think(&w, 1)), [] as [&Command; 0]);
}

#[test]
fn an_airfield_sends_nothing_else_while_its_attack_is_out() {
    let mut w = airfield(60.0);
    let mut other = w.buildings[2].clone();
    other.position = [80.0, 90.0];
    w.buildings.push(other);
    let mut p = planner(vec![likes_houses()]);
    // One airfield, one attack.
    let orders = p.think(&w, 1);
    assert_eq!(air_attacks(&orders).len(), 1);
    for order in orders {
        w.apply(1, order).unwrap();
    }
    // The bomber and its escort.
    assert_eq!(w.sorties.len(), 3);
    w.ticks += 30 * SECOND;
    assert_eq!(air_attacks(&p.think(&w, 1)), [] as [&Command; 0]);
}

#[test]
fn an_air_attack_cut_from_its_orders_doesnt_count_as_sent() {
    let mut w = airfield(60.0);
    let profile = |most: usize| Profile {
        id: Id::from("test_sai"),
        ego_fuzziness: 0.0,
        max_ge_commands: most,
        max_rai_commands: 40,
        moods: vec![likes_houses()],
        sends_air_attacks: true,
        explored_share: 0.1,
    };
    // Room for the escort, not the attack after it.
    let mut p = Planner::new(Arc::new(profile(1)), 1, 0.0);
    assert_eq!(air_attacks(&p.think(&w, 1)), [] as [&Command; 0]);
    w.ticks += 30 * SECOND;
    // With room again, it sends it at the next think.
    let mut q = Planner::new(Arc::new(profile(100)), 1, 0.0);
    std::mem::swap(&mut p.struck, &mut q.struck);
    assert_eq!(air_attacks(&q.think(&w, 1)).len(), 1);
}

/// Player 1's mortarman at 10,10 worth 50, its mortar reaching `range`, and
/// player 2's house at 10,30, in two halves of the map.
fn mortar_and_house(range: f32) -> World {
    let mortar = tgmap::Weapon {
        bombard: true,
        area: 2.0,
        ..testing::rifle(range, 10.0)
    };
    let mut w = testing::battle(
        &[1],
        vec![testing::soldier(vec![mortar])],
        testing::at_war(),
    );
    w.add_buildings(&[testing::building(9, 2, [10.0, 30.0])]);
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    w
}

fn bombards(orders: &[Command]) -> Vec<&Command> {
    (orders.iter())
        .filter(|c| matches!(c, Command::Bombard { .. }))
        .collect()
}

#[test]
fn it_bombards_a_building_its_bombarder_already_reaches() {
    use crate::{Piece, Shelled};
    let mut w = mortar_and_house(50.0);
    // Across in the other half, where the attack on it doesn't hold the
    // mortarman.
    w.buildings[0].position = [50.0, 10.0];
    let mut ego = likes_houses();
    ego.goal_engine.combat.attack_commit_distance = 0;
    let orders = planner(vec![ego]).think(&w, 1);
    let shell = Command::Bombard {
        organization: 0,
        target: Shelled::Thing(Piece::Building(0)),
    };
    assert_eq!(bombards(&orders), [&shell], "{orders:?}");
}

#[test]
fn it_never_moves_anything_to_bombard() {
    let w = mortar_and_house(10.0);
    let orders = planner(vec![likes_houses()]).think(&w, 1);
    assert_eq!(bombards(&orders), [] as [&Command; 0], "{orders:?}");
}

#[test]
fn it_bombards_a_unit_only_while_its_fighting() {
    let mortar = tgmap::Weapon {
        bombard: true,
        area: 2.0,
        ..testing::rifle(30.0, 10.0)
    };
    let mut w = testing::battle(
        &[1, 2],
        vec![testing::soldier(vec![mortar]), testing::soldier(Vec::new())],
        testing::at_war(),
    );
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    let mut ego = mood();
    ego.goal_engine.actor_priority.push(ActorPriority {
        actor_ids: Some(Id::from("squad")),
        value: 500.0,
        ..Default::default()
    });
    assert!(!w.fighting(1));
    let orders = planner(vec![ego]).think(&w, 1);
    assert_eq!(bombards(&orders), [] as [&Command; 0], "{orders:?}");
}

/// A bombard goal on player 2's house with `priority`.
fn shell_house(priority: f32) -> super::planner::Goal {
    use super::planner::{Goal, Key, Work};
    use crate::Piece;
    Goal {
        key: Key::Bombard(Piece::Building(0)),
        kind: "BOMBARD",
        priority,
        cost: 0.0,
        work: Work::Bombard {
            target: Piece::Building(0),
            at: [50.0, 10.0],
            seen: true,
        },
    }
}

#[test]
fn a_bombard_goal_takes_its_bombarder_from_a_fight_ranked_above() {
    use super::planner::Key;
    use crate::Piece;
    let mut w = mortar_and_house(50.0);
    w.buildings[0].position = [50.0, 10.0];
    let p = planner(vec![mood()]);
    let goals = vec![fight(0, false, 1000.0, &w), shell_house(50.0)];
    let serving = shared_out(&p, &w, goals);
    let shelling = Some(&Key::Bombard(Piece::Building(0)));
    assert_eq!(serving.get(&Member::Organization(0)), shelling);
    // But not from an attack it's held to.
    let goals = vec![fight(1, true, 1000.0, &w), shell_house(50.0)];
    let serving = shared_out(&p, &w, goals);
    assert_eq!(serving.get(&Member::Organization(0)), Some(&Key::Attack(1)));
}

/// A mood using operation `id` on enemy buildings, worth `base`.
fn spies_on_buildings(ego: &mut Ego, id: &str, base: f32) {
    use tgmap::sai::{ActorRequirements, GeneralPower, Named, PowerLocation, PowerPriority};
    ego.goal_engine.general_powers.power.push(GeneralPower {
        ability_ids: Some(Id::from(id)),
        location: PowerLocation {
            kind: Some(Id::from("ON_ACTOR")),
            actor_requirements: ActorRequirements {
                must_be_enemy: true,
                required_property: vec![Named {
                    ids: Some(Id::from("building")),
                }],
                ..Default::default()
            },
            ..Default::default()
        },
        priority: PowerPriority {
            base,
            fuzzy_factor: 0.0,
            ..Default::default()
        },
    });
}

/// Player 1's man at 10,10 and player 2's house at 50,10, player 1's
/// general holding `experience` and operations of `costs`, each lifting
/// the fog.
fn general(experience: f32, costs: &[f32]) -> World {
    let mut w = testing::battle(&[1], vec![testing::soldier(Vec::new())], testing::at_war());
    w.add_buildings(&[testing::building(9, 2, [50.0, 10.0])]);
    w.rules.kinds.insert(Id::from("house"), kind(&["building"]));
    halves(&mut w);
    let ops = (costs.iter().enumerate())
        .map(|(i, &cost)| {
            let id = Id::from(format!("spy{i}").as_str());
            w.rules
                .operations
                .insert(id.clone(), tgmap::Operation::Reveal { seconds: 10.0 });
            crate::SpecialOp::new(id, cost, 60.0)
        })
        .collect();
    w.lead(1, experience, None, ops);
    w
}

fn operations(orders: &[Command]) -> Vec<&Command> {
    (orders.iter())
        .filter(|c| matches!(c, Command::Operate { .. }))
        .collect()
}

#[test]
fn it_uses_its_generals_operation_on_what_its_mood_aims_it_at() {
    use crate::Piece;
    let w = general(100.0, &[50.0]);
    let mut ego = mood();
    spies_on_buildings(&mut ego, "spy0", 200.0);
    let orders = planner(vec![ego]).think(&w, 1);
    let spy = Command::Operate {
        operation: 0,
        at: [50.0, 10.0],
        on: Some(Piece::Building(0)),
    };
    assert_eq!(operations(&orders), [&spy], "{orders:?}");
}

#[test]
fn it_stops_at_the_first_operation_it_cant_afford() {
    let w = general(40.0, &[50.0, 10.0]);
    let mut ego = mood();
    // The dear one always comes first: the cheap one has no weight, but
    // scores for the money it brings.
    spies_on_buildings(&mut ego, "spy0", 200.0);
    spies_on_buildings(&mut ego, "spy1", 0.0);
    let brings = &mut ego.goal_engine.general_powers.power[1]
        .priority
        .economy_generated;
    brings.multiplier = 1.0;
    brings.resource.push(tgmap::sai::Generated {
        ids: Some(Id::from("money")),
        amount: 100.0,
    });
    let orders = planner(vec![ego.clone()]).think(&w, 1);
    assert_eq!(operations(&orders), [] as [&Command; 0], "{orders:?}");
    // With enough for the first, both are used.
    let w = general(60.0, &[50.0, 10.0]);
    assert_eq!(operations(&planner(vec![ego]).think(&w, 1)).len(), 2);
}

#[test]
fn it_never_uses_an_operation_its_mood_doesnt_list() {
    let w = general(100.0, &[50.0]);
    let mut ego = mood();
    spies_on_buildings(&mut ego, "other", 200.0);
    let orders = planner(vec![ego]).think(&w, 1);
    assert_eq!(operations(&orders), [] as [&Command; 0], "{orders:?}");
}

#[test]
fn a_place_is_ruled_out_by_a_factor_below_its_minimum() {
    use tgmap::sai::{ActorRequirements, GeneralPower, PowerLocation, PowerPriority, Tally};
    let mut w = general(100.0, &[50.0]);
    rate(&mut w, "soldier", 50.0);
    // Round its own man, weighing its own strength in reach.
    let near_its_own = |minimum: f32| {
        let mut ego = mood();
        ego.goal_engine.general_powers.power.push(GeneralPower {
            ability_ids: Some(Id::from("spy0")),
            location: PowerLocation {
                kind: Some(Id::from("NEAR_ACTOR")),
                actor_requirements: ActorRequirements {
                    must_be_owned: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            priority: PowerPriority {
                base: 100.0,
                fuzzy_factor: 0.0,
                cv: Tally {
                    minimum,
                    multiplier: 1.0,
                    consider_owned: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        });
        ego
    };
    // An operation that reaches nothing round where it's aimed counts
    // nothing in reach.
    let orders = planner(vec![near_its_own(0.0)]).think(&w, 1);
    assert_eq!(operations(&orders).len(), 1, "{orders:?}");
    let orders = planner(vec![near_its_own(1.0)]).think(&w, 1);
    assert_eq!(operations(&orders), [] as [&Command; 0], "{orders:?}");
}
