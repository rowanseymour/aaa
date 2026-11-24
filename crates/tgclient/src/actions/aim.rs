//! Giving an aimed order where the player clicks.

use bevy::prelude::*;

use crate::control_groups::ControlGroups;
use crate::messages::{About, By, Failed, Refused};
use crate::{Loaded, Local, Orders, Selected, SelectedBuilding, Sim};

use super::card_buttons::AimedFor;
use super::looks::Looks;
use super::tips::fill;
use super::{Aim, Page, UNPACK_FACING};

/// What an attack aimed at `focus` would strike.
#[derive(Debug, PartialEq)]
pub(super) enum Struck {
    /// The enemy organization, to go after.
    Enemy(usize),
    /// The player's own organization or building, or an ally's, which Attack
    /// refuses.
    Friend(About),
    /// An organization or building that's nobody's enemy, so can't be attacked.
    Neutral(About),
    /// The ground, or an enemy building on it, to attack-move to.
    Ground,
}

pub(super) fn attack_at(sim: &tgsim::World, player: u32, focus: crate::Focus) -> Struck {
    let relation = focus
        .piece()
        .map(|p| sim.sides.relation(player, sim.piece_owner(p)));
    struck(focus, relation)
}

/// What an attack strikes at `focus`, toward whose owner `player` stands
/// as `toward`.
fn struck(focus: crate::Focus, toward: Option<tgsim::Relation>) -> Struck {
    match (focus, toward) {
        (crate::Focus::Nothing, _) => Struck::Ground,
        (crate::Focus::Organization(e), Some(tgsim::Relation::Enemy)) => Struck::Enemy(e),
        (crate::Focus::Building(_), Some(tgsim::Relation::Enemy)) => Struck::Ground,
        (crate::Focus::Organization(r), Some(tgsim::Relation::Neutral)) => {
            Struck::Neutral(About::Organization(r))
        }
        (crate::Focus::Building(b), Some(tgsim::Relation::Neutral)) => {
            Struck::Neutral(About::Building(b))
        }
        (crate::Focus::Organization(r), _) => Struck::Friend(About::Organization(r)),
        (crate::Focus::Building(b), _) => Struck::Friend(About::Building(b)),
    }
}

/// The enemy's organization or building at `focus`, which Guard refuses.
fn enemy_at(sim: &tgsim::World, player: u32, focus: crate::Focus) -> Option<About> {
    let piece = focus.piece()?;
    (sim.sides.relation(player, sim.piece_owner(piece)) == tgsim::Relation::Enemy)
        .then(|| About::from(piece))
}

/// What a guard aimed at `focus` is posted beside: an organization or building
/// that's no enemy's, and not one of the `selected` giving the guard, which
/// guard where they are instead.
fn guard_at(
    sim: &tgsim::World,
    player: u32,
    selected: &[usize],
    focus: crate::Focus,
) -> Option<tgsim::Piece> {
    let piece = focus.piece()?;
    let own = matches!(piece, tgsim::Piece::Organization(r) if selected.contains(&r));
    (!own && sim.sides.relation(player, sim.piece_owner(piece)) != tgsim::Relation::Enemy)
        .then_some(piece)
}

/// Sends the airfield at `b`'s air attack on what's at `focus`, or at
/// `point` on the ground; the player's own things and allies' it refuses,
/// telling them so.
fn air_attack(
    sim: &tgsim::World,
    orders: &mut Orders,
    refused: &mut MessageWriter<Refused>,
    player: u32,
    b: usize,
    focus: crate::Focus,
    point: [f32; 2],
) {
    let on = focus.piece();
    if let Some(piece) = on {
        let toward = sim.sides.relation(player, sim.piece_owner(piece));
        if !matches!(toward, tgsim::Relation::Enemy | tgsim::Relation::Neutral) {
            refused.write(Refused {
                about: About::from(piece),
                by: Some(By::AirAttack),
            });
            return;
        }
    }
    let attack = tgsim::Command::AirAttack {
        building: b,
        at: point,
        on,
    };
    if let Err(e) = orders.give(sim, player, attack) {
        debug!("can't air attack from {}: {e:?}", sim.buildings[b].template);
    }
}

/// While an order is aimed, a left click on the map or minimap gives it
/// there and a right click calls it off. Attacking an enemy organization, or
/// its mark on the minimap, sends the selected after it instead, and
/// guarding a friend posts them beside it. A click on something the order
/// refuses tells the player so and calls the order off. Unpacking somewhere
/// the building wouldn't fit waits for another click. A rally point is set
/// for the selected building.
#[allow(clippy::too_many_arguments)]
pub fn aim(
    (buttons, keys): (Res<ButtonInput<MouseButton>>, Res<ButtonInput<KeyCode>>),
    pointer: crate::pointing::Pointer,
    scenery: Res<crate::banners::Scenery>,
    loaded: Res<Loaded>,
    local: Res<Local>,
    (selected, chosen, groups, aimed_for): (
        Res<Selected>,
        Res<SelectedBuilding>,
        Res<ControlGroups>,
        Res<AimedFor>,
    ),
    focus: Res<crate::Focus>,
    (looks, menus): (Res<crate::waypoints::Looks>, Res<Looks>),
    sim: Res<Sim>,
    mut orders: ResMut<Orders>,
    mut page: ResMut<Page>,
    time: Res<Time>,
    mut sounds: ResMut<crate::sound::Sounds>,
    (mut refused, mut failed): (MessageWriter<Refused>, MessageWriter<Failed>),
) {
    if !matches!(
        *page,
        Page::Aiming(_) | Page::Rally | Page::AirAttack | Page::Delivering(_)
    ) {
        return;
    }
    if buttons.just_pressed(MouseButton::Right) {
        *page = Page::Orders;
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let (Some(player), Some(at)) = (local.0, pointer.at(&loaded.map.ground)) else {
        return;
    };
    let point = at.point();
    if let Page::Delivering(i) = *page {
        // Only the map aims one, and a place it can't land goes on
        // being aimed.
        let organization = menus.delivered(&sim.0, chosen.0, *page);
        let (Some(b), Some(organization), crate::pointing::Pointing::Ground(_)) =
            (chosen.0, organization, at)
        else {
            return;
        };
        let deliver = tgsim::Command::Deliver {
            building: b,
            template: organization.template.clone(),
            to: point,
        };
        match orders.give(&sim.0, player, deliver) {
            Err(tgsim::Refused::NoRoom) => {}
            // The money may have gone since it was aimed.
            Err(tgsim::Refused::Unaffordable) => {
                let listed = menus.menu(&sim.0, chosen.0).get(i);
                let name = listed.map_or("", |r| r.look.name.as_str());
                let cost = sim.0.cost(b, organization).ceil().to_string();
                let text = fill(&menus.affording, &[name, &cost]);
                let about = About::Building(b);
                failed.write(Failed {
                    text,
                    about: Some(about),
                    at: None,
                });
                *page = Page::Orders;
            }
            Err(e) => {
                debug!("can't deliver {}: {e:?}", organization.template);
                *page = Page::Orders;
            }
            Ok(()) => *page = Page::Orders,
        }
        return;
    }
    let focus = match at {
        crate::pointing::Pointing::Ground(_) => *focus,
        // The minimap's marks are too close together to pick out friends,
        // so a click there is only ever on an enemy or the ground.
        crate::pointing::Pointing::Minimap(p) => {
            crate::pointing::nearest_organization(&sim.0, p, pointer.minimap.reach(), |r| {
                !scenery.has(&sim.0, r)
                    && sim.0.sides.relation(player, sim.0.organizations[r].owner)
                        == tgsim::Relation::Enemy
            })
            .map_or(crate::Focus::Nothing, crate::Focus::Organization)
        }
    };
    if *page == Page::AirAttack {
        *page = Page::Orders;
        if let Some(b) = chosen.0 {
            air_attack(&sim.0, &mut orders, &mut refused, player, b, focus, point);
        }
        return;
    }
    let Page::Aiming(aim) = *page else {
        if let Some(b) = chosen.0
            && let Err(e) = orders.give(
                &sim.0,
                player,
                tgsim::Command::Rally {
                    building: b,
                    at: Some(point),
                },
            )
        {
            debug!("can't rally {}: {e:?}", sim.0.buildings[b].template);
        }
        *page = Page::Orders;
        return;
    };
    // Aimed from a card's button, it's that card's regiment's alone.
    let given = match aimed_for.0 {
        Some(r) => vec![r],
        None => selected.0.clone(),
    };
    orders.queue(crate::commanding::queuing(&keys));
    // With Group Behavior on, what each is told goes as one group.
    if groups.grouping(&given) {
        orders.gather(player);
    }
    if aim == Aim::Repair {
        *page = Page::Orders;
        let Some(piece) = focus.piece() else {
            let text = fill(&menus.no_target, &[menus.order(aim)]);
            failed.write(Failed {
                text,
                about: None,
                at: None,
            });
            return;
        };
        let (organizations, _) =
            crate::commanding::give_each(&sim.0, &mut orders, player, &given, point, |r| {
                tgsim::Command::Repair {
                    organization: r,
                    piece,
                }
            });
        if organizations.is_empty() {
            refused.write(Refused {
                about: About::from(piece),
                by: Some(aim.into()),
            });
            return;
        }
        crate::commanding::acknowledge(&sim.0, &mut sounds, &time, player, &given);
        return;
    }
    if aim == Aim::Bombard {
        *page = Page::Orders;
        // It refuses only what's the player's own or an ally's; the rest
        // is down to what each regiment can shell.
        if let Struck::Friend(about) = attack_at(&sim.0, player, focus) {
            refused.write(Refused {
                about,
                by: Some(aim.into()),
            });
            return;
        }
        let on = focus.piece();
        let target = on.map_or(tgsim::Shelled::Place(point), tgsim::Shelled::Thing);
        let (organizations, _) =
            crate::commanding::give_each(&sim.0, &mut orders, player, &given, point, |r| {
                tgsim::Command::Bombard {
                    organization: r,
                    target,
                }
            });
        if organizations.is_empty() {
            if on.is_none() {
                failed.write(Failed {
                    text: menus.no_bombard.clone(),
                    about: None,
                    at: Some(point),
                });
            }
            return;
        }
        crate::commanding::acknowledge(&sim.0, &mut sounds, &time, player, &given);
        return;
    }
    if aim == Aim::Attack {
        match attack_at(&sim.0, player, focus) {
            Struck::Friend(about) => {
                refused.write(Refused {
                    about,
                    by: Some(aim.into()),
                });
                *page = Page::Orders;
                return;
            }
            Struck::Neutral(about) => {
                refused.write(Refused { about, by: None });
                *page = Page::Orders;
                return;
            }
            Struck::Ground => {}
            Struck::Enemy(e) => {
                let (pursuing, rest) =
                    crate::commanding::pursue(&sim.0, &mut orders, player, &given, e);
                // An enemy none of them can attack is no target either.
                if pursuing.is_empty() {
                    let about = About::Organization(e);
                    refused.write(Refused { about, by: None });
                    *page = Page::Orders;
                    return;
                }
                // Those with nothing to fire go there as they would anywhere.
                crate::commanding::send(
                    &sim.0,
                    &mut orders,
                    player,
                    &rest,
                    point,
                    (aim, looks.together(aim)),
                    loaded.map.ground.size(),
                );
                crate::commanding::acknowledge(&sim.0, &mut sounds, &time, player, &given);
                *page = Page::Orders;
                return;
            }
        }
    }
    // Enemy marks are all a minimap click picks out, to attack; guarding
    // there guards the place.
    if aim == Aim::Guard
        && matches!(at, crate::pointing::Pointing::Ground(_))
        && let Some(about) = enemy_at(&sim.0, player, focus)
    {
        refused.write(Refused {
            about,
            by: Some(aim.into()),
        });
        *page = Page::Orders;
        return;
    }
    if aim == Aim::Guard
        && let Some(piece) = guard_at(&sim.0, player, &given, focus)
    {
        let (organizations, _) =
            crate::commanding::give_each(&sim.0, &mut orders, player, &given, point, |r| {
                tgsim::Command::GuardPiece {
                    organization: r,
                    piece,
                }
            });
        if organizations.is_empty() {
            return;
        }
        crate::commanding::acknowledge(&sim.0, &mut sounds, &time, player, &given);
        *page = Page::Orders;
        return;
    }
    match aim {
        Aim::Unpack => {
            let &[r] = &given[..] else {
                return;
            };
            let unpack = tgsim::Command::Unpack {
                organization: r,
                at: point,
                facing: UNPACK_FACING,
            };
            if orders.give(&sim.0, player, unpack).is_err() {
                return;
            }
            crate::commanding::acknowledge(&sim.0, &mut sounds, &time, player, &given);
        }
        // Given above.
        Aim::Repair | Aim::Bombard => return,
        Aim::Move | Aim::Attack | Aim::Run | Aim::Guard | Aim::Explore => {
            crate::commanding::acknowledge(&sim.0, &mut sounds, &time, player, &given);
            crate::commanding::send(
                &sim.0,
                &mut orders,
                player,
                &given,
                point,
                (aim, looks.together(aim)),
                loaded.map.ground.size(),
            );
        }
    }
    *page = Page::Orders;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attacks_strike_only_enemies() {
        use crate::Focus::{Building, Nothing, Organization};
        use tgsim::Relation::{Ally, Enemy, Itself, Neutral};
        assert_eq!(struck(Organization(3), Some(Enemy)), Struck::Enemy(3));
        for friend in [Itself, Ally] {
            let refused = |about| Struck::Friend(about);
            assert_eq!(
                struck(Organization(3), Some(friend)),
                refused(About::Organization(3))
            );
            assert_eq!(
                struck(Building(3), Some(friend)),
                refused(About::Building(3))
            );
        }
        let neutral = Struck::Neutral(About::Organization(3));
        assert_eq!(struck(Organization(3), Some(Neutral)), neutral);
        assert_eq!(struck(Building(3), Some(Enemy)), Struck::Ground);
        assert_eq!(struck(Nothing, None), Struck::Ground);
    }
}
