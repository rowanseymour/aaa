use crate::testing::*;

use tgmap::{Id, Turret, Weapon};

use crate::{Command, State, World};

/// Player 2's bunker, id 20, at (10, 24) facing the siege's unarmed
/// soldier, id 2, to its south: it defends itself with a `Gun` that
/// reaches only 1, and its performer's
/// `Shell` (range 30, 10 damage, 16 seconds to recharge) is on a turret
/// that starts `start` degrees round, turns 90° a second and aims within
/// `sweep` degrees.
fn bunker(start: f32, sweep: f32, bombard: bool) -> World {
    let mut bunker = building(20, 2, [10.0, 24.0]);
    bunker.fighter.weapons = vec![
        Weapon {
            definition: Some(Id::from("Shell")),
            performed: true,
            aimed_by: Some(0),
            recharge: 16.0,
            bombard,
            ..rifle(30.0, 10.0)
        },
        Weapon {
            definition: Some(Id::from("Gun")),
            ..rifle(1.0, 1.0)
        },
    ];
    bunker.fighter.notice = Some(20.0);
    bunker.fighter.turrets = vec![Turret {
        turn_rate: 90f32.to_radians(),
        sweep: sweep.to_radians(),
        start: start.to_radians(),
        reset: None,
        offset: [0.0; 2],
    }];
    let mut w = siege(&[bunker]);
    w.units[0].fighter = Some(soldier(Vec::new()));
    w.buildings[0].awake = true;
    w
}

/// Tells `thing` to use `ability` on `target`, as player 1, whose
/// `thing` needn't be.
fn tell(w: &mut World, thing: u32, ability: &str, target: u32) {
    let ability = Id::from(ability);
    let command = Command::Ability {
        thing,
        ability,
        target,
    };
    w.check(1, &command).unwrap();
    w.apply(1, command).unwrap();
}

fn health(w: &World) -> f32 {
    w.units[0].health
}

#[test]
fn a_building_uses_its_performers_ability_only_once_when_told() {
    let mut w = bunker(0.0, 360.0, false);
    run(&mut w, 3.0);
    assert!(!w.buildings[0].noticed.is_empty());
    assert_eq!(health(&w), 100.0);
    // Named whatever the case.
    tell(&mut w, 20, "shell", 2);
    w.tick();
    assert_eq!(health(&w), 90.0);
    assert_eq!(w.buildings[0].shot.map(|(_, weapon)| weapon), Some(0));
    run(&mut w, 40.0);
    assert_eq!(health(&w), 90.0);
}

#[test]
fn the_use_is_refused_where_it_cant_be_made_now() {
    let refused = |w: &mut World, ability: &str, target: u32| {
        tell(w, 20, ability, target);
        assert!(w.told.is_empty());
        run(w, 3.0);
        assert_eq!(health(w), 100.0);
    };
    // Out of range, and nothing brings it closer.
    let mut w = bunker(0.0, 360.0, false);
    w.units[0].position = [10.0, -10.0];
    refused(&mut w, "Shell", 2);
    // Within its least range.
    let mut w = bunker(0.0, 360.0, false);
    w.buildings[0].fighter.weapons[0].min_range = 15.0;
    refused(&mut w, "Shell", 2);
    // Outside its turret's arc, to its left.
    let mut w = bunker(90.0, 70.0, false);
    refused(&mut w, "Shell", 2);
    // What it doesn't fire on.
    let mut w = bunker(0.0, 360.0, false);
    w.buildings[0].fighter.weapons[0]
        .targets
        .excluded
        .push(Id::from("infantry"));
    refused(&mut w, "Shell", 2);
    // An ability it doesn't have, or one that isn't its performer's.
    let mut w = bunker(0.0, 360.0, false);
    refused(&mut w, "Bomb", 2);
    refused(&mut w, "Gun", 2);
    // Nothing of those ids.
    let mut w = bunker(0.0, 360.0, false);
    refused(&mut w, "Shell", 99);
    refused(&mut w, "Shell", 1);
    // Still recharging.
    let mut w = bunker(0.0, 360.0, false);
    tell(&mut w, 20, "Shell", 2);
    run(&mut w, 5.0);
    tell(&mut w, 20, "Shell", 2);
    assert!(w.told.is_empty());
    run(&mut w, 3.0);
    assert_eq!(health(&w), 90.0);
}

#[test]
fn its_turret_turns_to_the_target_before_the_shot() {
    // Pointing 90° off the soldier, it takes a second to turn.
    let mut w = bunker(90.0, 360.0, false);
    tell(&mut w, 20, "Shell", 2);
    run(&mut w, 0.9);
    assert_eq!(health(&w), 100.0);
    run(&mut w, 0.2);
    assert_eq!(health(&w), 90.0);
    assert_eq!(w.buildings[0].turrets[0].yaw, 0.0);
    assert!(w.told.is_empty());
}

#[test]
fn a_shot_whose_target_goes_while_the_turret_turns_goes_off_at_once() {
    let mut w = bunker(90.0, 360.0, false);
    let shed = building(30, 1, [10.0, 4.0]);
    w.add_buildings(&[shed]);
    tell(&mut w, 20, "Shell", 30);
    run(&mut w, 0.5);
    let yaw = w.buildings[0].turrets[0].yaw;
    w.buildings[1].state = State::Destroyed(w.ticks);
    w.tick();
    assert!(w.told.is_empty());
    assert_eq!(w.buildings[0].shot.map(|(tick, _)| tick), Some(w.ticks - 1));
    assert_eq!(w.buildings[0].turrets[0].yaw, yaw);
}

#[test]
fn the_shot_is_bombardment_as_its_ability_is_flagged() {
    for bombard in [true, false] {
        let mut w = bunker(0.0, 360.0, bombard);
        tell(&mut w, 20, "Shell", 2);
        w.tick();
        assert_eq!(health(&w), 90.0);
        assert_eq!(!w.organizations[0].bombarded.is_empty(), bombard);
    }
}

#[test]
fn a_unit_can_be_told_and_is_stopped_first() {
    let mut w = skirmish();
    // Out of its control range, so it wouldn't fire by itself.
    w.units[1].position = [10.0, 32.0];
    w.units[1].fighter = Some(soldier(Vec::new()));
    let rifle = &mut w.units[0].fighter.as_mut().unwrap().weapons[0];
    (rifle.definition, rifle.performed, rifle.range) = (Some(Id::from("Rifle")), true, 25.0);
    let to = [30.0, 10.0];
    w.apply(
        1,
        Command::Move {
            organization: 0,
            to,
        },
    )
    .unwrap();
    run(&mut w, 1.0);
    // Its enemy can tell it, too.
    let command = Command::Ability {
        thing: 2,
        ability: Id::from("Rifle"),
        target: 4,
    };
    w.apply(2, command).unwrap();
    run(&mut w, 3.0);
    assert_eq!(w.units[1].health, 50.0);
    assert!(!w.organizations[0].travelling);
    assert_ne!(w.destination(0), to);
}

#[test]
fn a_lone_unit_is_named_by_its_organizations_id() {
    let mut w = bunker(0.0, 360.0, false);
    // As a lone unit is raised: its own template, sharing its id.
    let r = w.units[0].organization;
    w.organizations[r].template = w.units[0].template.clone();
    w.units[0].id = w.organizations[r].id;
    let id = w.units[0].id;
    tell(&mut w, 20, "Shell", id);
    w.tick();
    assert_eq!(health(&w), 90.0);
}

#[test]
fn a_building_not_yet_woken_recharges_what_its_told_to_use() {
    let mut w = bunker(0.0, 360.0, false);
    w.buildings[0].awake = false;
    // Far enough off not to wake it, still in the shell's reach.
    w.buildings[0].fighter.notice = Some(5.0);
    tell(&mut w, 20, "Shell", 2);
    w.tick();
    assert_eq!(health(&w), 90.0);
    run(&mut w, 17.0);
    assert!(!w.buildings[0].awake);
    tell(&mut w, 20, "Shell", 2);
    w.tick();
    assert_eq!(health(&w), 80.0);
}

#[test]
fn a_unit_that_cant_fire_now_isnt_told_to() {
    let told = |routing: bool| {
        let mut w = skirmish();
        w.units[1].position = [10.0, 32.0];
        w.units[1].fighter = Some(soldier(Vec::new()));
        let rifle = &mut w.units[0].fighter.as_mut().unwrap().weapons[0];
        (rifle.definition, rifle.performed, rifle.range) = (Some(Id::from("Rifle")), true, 25.0);
        if routing {
            w.organizations[0].morale = 0.0;
            w.rout(0);
        }
        let command = Command::Ability {
            thing: 2,
            ability: Id::from("Rifle"),
            target: 4,
        };
        w.apply(1, command).unwrap();
        run(&mut w, 1.0);
        w.units[1].health
    };
    assert_eq!(told(false), 50.0);
    assert_eq!(told(true), 100.0);
}

#[test]
fn a_turret_told_to_turn_leaves_its_other_weapons_without_a_target() {
    // Its gun reaches the soldier, but the shell has the turret.
    let mut w = bunker(90.0, 360.0, false);
    let gun = &mut w.buildings[0].fighter.weapons[1];
    (gun.range, gun.aimed_by) = (30.0, Some(0));
    tell(&mut w, 20, "Shell", 2);
    w.tick();
    assert_eq!(w.buildings[0].target, None);
}
