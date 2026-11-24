use std::sync::Arc;

use tgmap::sai::{ActorPriority, Ego, Filter, GoalPriority, Profile, RecruitRequest, TechPriority};
use tgmap::{Id, Region, Regions};

use super::planner::Wanted;
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
) -> std::collections::HashMap<usize, super::planner::Key> {
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
    assert_eq!(serving.get(&0), Some(&Key::Attack(1)));
}

#[test]
fn a_unit_stays_on_its_attack_when_one_nearby_is_ranked_higher() {
    use super::planner::Key;
    let mut w = testing::skirmish();
    halves(&mut w);
    rate(&mut w, "soldier", 50.0);
    let goals = |w: &World| vec![fight(0, true, 1000.0, w), fight(1, true, 500.0, w)];
    let mut p = planner(vec![mood()]);
    p.serving.insert(0, Key::Attack(1));
    // It's locked on to its own.
    assert_eq!(shared_out(&p, &w, goals(&w)).get(&0), Some(&Key::Attack(1)));
    // Not locked, it stays while the higher shares its strength, and goes
    // where it's worth more once that doesn't.
    for (linked, goes) in [(2, Key::Attack(1)), (0, Key::Attack(0))] {
        let mut ego = mood();
        ego.goal_engine.combat.linked_region_distance = linked;
        ego.goal_engine.combat.attack_commit_distance = 0;
        let mut p = planner(vec![ego]);
        p.serving.insert(0, Key::Attack(1));
        assert_eq!(shared_out(&p, &w, goals(&w)).get(&0), Some(&goes));
    }
}
