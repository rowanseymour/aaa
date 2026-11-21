//! Verbs for the match itself: its kingdoms and outcome, what's on the
//! map, and the orders given there.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use tgrender::camera::RtsCamera;
use tgrender::space::map_point;

use crate::actions::Aim;
use crate::lobby::Lineup;
use crate::waypoints::Sent;
use crate::{Loaded, Local, Orders, Raising, Selected, SelectedBuilding, Sim};
use tgmap::Id;
use tgsim::Action;

use super::Console;
use super::camera::place;
use super::verbs::{Reply, no_args, number, usage};

/// Lights the map with the preset `args` names, or lists the presets.
pub(crate) fn sun(world: &mut World, args: &[&str]) -> Reply {
    let game = world.resource::<Loaded>().game.clone();
    let presets = game.sun_presets();
    let ids = presets.iter().filter_map(|p| p.ids.as_ref());
    let [id] = args else {
        return match args {
            [] => Ok(ids.map(Id::to_string).collect::<Vec<_>>().join(" ")),
            _ => Err(usage("sun")),
        };
    };
    let id = Id::from(*id);
    let Some(preset) = presets.iter().find(|p| p.ids.as_ref() == Some(&id)) else {
        return Err(format!("no lighting preset \"{id}\""));
    };
    // The menu's backdrop has a sun of its own, still up as the match starts.
    let mut lights = world.query_filtered::<&mut DirectionalLight, (
        Without<RenderLayers>,
        Without<crate::backdrop::Backdrop>,
    )>();
    if lights.iter(world).next().is_none() {
        return Err("the map isn't lit yet".into());
    }
    world.resource_mut::<Loaded>().map.light(preset);
    let changes = world.resource::<crate::Sim>().0.triggers.suns.len();
    if let Some(mut sunlight) = world.get_resource_mut::<crate::sunlight::Sunlight>() {
        sunlight.set([preset.diffuse, preset.ambient, preset.specular], changes);
    }
    let night = game.night(preset.diffuse);
    world
        .resource_mut::<crate::effects::Effects>()
        .set_night(night);
    let mut ambient = world.resource::<GlobalAmbientLight>().clone();
    for mut light in lights.iter_mut(world) {
        tgrender::light::color_lights(&mut light, &mut ambient, preset.diffuse, preset.ambient);
    }
    world.insert_resource(ambient);
    Ok(String::new())
}

pub(crate) fn record(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    let given = world.resource::<Orders>();
    let record = given.record();
    let orders = (record.given.iter())
        .filter(|g| matches!(g.action, Action::Order { .. }))
        .count();
    let said = format!(
        "{} ticks, {orders} orders, hash {:016x}",
        sim.ticks,
        sim.hash()
    );
    match args {
        [] => Ok(said),
        [file] => {
            let film = tgserver::Film {
                setup: given.setup().clone(),
                record: record.clone(),
            };
            film.write(std::path::Path::new(file))?;
            Ok(said)
        }
        _ => Err(usage("record")),
    }
}

/// The player a word names, by name or id.
pub(crate) fn kingdom(sim: &tgsim::World, word: &str) -> Result<u32, String> {
    let players = &sim.sides.players;
    players
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(word) || p.id.to_string() == word)
        .map(|p| p.id)
        .ok_or_else(|| format!("no kingdom {word:?}; {}", kingdoms(sim, None)))
}

/// The map's kingdoms, marking `current`.
pub(super) fn kingdoms(sim: &tgsim::World, current: Option<u32>) -> String {
    let names: Vec<String> = sim
        .sides
        .players
        .iter()
        .map(|p| {
            let mark = if Some(p.id) == current { "*" } else { "" };
            format!("{mark}\"{}\" ({})", p.name, p.id)
        })
        .collect();
    format!("kingdoms: {}", names.join(", "))
}

/// Whether the match is over, and how it's come out for each side.
pub(crate) fn outcome(sim: &tgsim::World) -> String {
    let sides = sim.sides.players.iter().map(|p| {
        let outcome = match sim.outcome(p.id) {
            tgsim::Outcome::Playing => "playing",
            tgsim::Outcome::Won => "won",
            tgsim::Outcome::Lost => "lost",
        };
        format!("\"{}\" ({}) {outcome}", p.name, p.id)
    });
    let sides: Vec<String> = sides.collect();
    let state = if sim.over { "over" } else { "running" };
    format!("match {state}: {}", sides.join(", "))
}

pub(crate) fn heroes(world: &World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    let player = match args {
        [] => world.resource::<Local>().0.ok_or("nobody to play")?,
        [word] => kingdom(sim, word)?,
        _ => return Err(usage("heroes")),
    };
    let held: Vec<String> = (sim.heroes(player).iter())
        .map(|h| {
            let status = match h.unit {
                Some(u) => {
                    let r = sim.units[u].organization;
                    format!("leading {}", sim.organizations[r].id)
                }
                None if h.fallen => "fallen".into(),
                None => format!("at {:.0}% health", h.health * 100.0),
            };
            format!("{} {status}", h.hero)
        })
        .collect();
    Ok(match held.is_empty() {
        true => "no heroes".into(),
        false => held.join("\n"),
    })
}

pub(super) fn settlements(sim: &tgsim::World) -> String {
    let lines = sim.settlements.iter().enumerate().map(|(s, settlement)| {
        let centre = match sim.centre_of(s) {
            Some(b) => {
                let b = &sim.buildings[b];
                let [x, y] = b.position;
                format!("centre {} {} at {x:.0} {y:.0}", b.template, b.id)
            }
            None => "no centre".into(),
        };
        let (template, id, owner) = (&settlement.template, settlement.id, settlement.owner);
        format!("{template} {id} of kingdom {owner}: {centre}")
    });
    let lines: Vec<String> = lines.collect();
    match lines.is_empty() {
        true => "no settlements".into(),
        false => lines.join("\n"),
    }
}

/// The buildings and organizations still on the map, of the kingdoms and
/// templates `args` name, if any: a word is a kingdom where it names one,
/// else a template.
pub(crate) fn objects(sim: &tgsim::World, args: &[&str]) -> String {
    let (mut kingdoms, mut templates) = (Vec::new(), Vec::new());
    for word in args {
        match kingdom(sim, word) {
            Ok(id) => kingdoms.push(id),
            Err(_) => templates.push(word.to_ascii_lowercase()),
        }
    }
    let wanted = |owner: u32, template: &str| {
        (kingdoms.is_empty() || kingdoms.contains(&owner))
            && (templates.is_empty() || templates.contains(&template.to_ascii_lowercase()))
    };
    let line = |kind: &str, template: &str, id: u32, owner: u32, [x, y]: [f32; 2]| {
        format!("{kind} {template} {id} of kingdom {owner} at {x:.0} {y:.0}")
    };
    let buildings = (sim.buildings.iter())
        .filter(|b| !b.gone() && wanted(b.owner, b.template.as_str()))
        .map(|b| line("building", b.template.as_str(), b.id, b.owner, b.position));
    let organizations = (0..sim.organizations.len())
        .filter(|&r| !sim.destroyed(r))
        .filter(|&r| {
            wanted(
                sim.organizations[r].owner,
                sim.organizations[r].template.as_str(),
            )
        })
        .map(|r| {
            let o = &sim.organizations[r];
            line(
                "organization",
                o.template.as_str(),
                o.id,
                o.owner,
                sim.centre(r),
            )
        });
    let lines: Vec<String> = buildings.chain(organizations).collect();
    match lines.is_empty() {
        true => "nothing on the map".into(),
        false => lines.join("\n"),
    }
}

/// Has a kingdom win or lose the match, as a map's triggers can.
pub(crate) fn decide(world: &mut World, args: &[&str], won: bool) -> Reply {
    let [word] = args else {
        return Err(usage(if won { "missionwon" } else { "missionlost" }));
    };
    let id = kingdom(&world.resource::<Sim>().0, word)?;
    act(
        world,
        match won {
            true => Action::Win(id),
            false => Action::Lose(id),
        },
    )?;
    Ok(outcome(&world.resource::<Sim>().0))
}

pub(super) fn placement_kingdom(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    let current = world
        .resource::<Console>()
        .placement
        .or(world.resource::<Local>().0);
    match args {
        [] => Ok(kingdoms(sim, current)),
        [word] => {
            let id = kingdom(sim, word)?;
            world.resource_mut::<Console>().placement = Some(id);
            Ok(String::new())
        }
        _ => Err(usage("placementkingdom")),
    }
}

pub(super) fn player_kingdom(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    match args {
        [] => Ok(kingdoms(sim, world.resource::<Local>().0)),
        [word] => {
            let id = kingdom(sim, word)?;
            world.resource_mut::<Local>().0 = Some(id);
            if let Some(mut lineup) = world.get_resource_mut::<Lineup>() {
                lineup.seat(Some(id));
            }
            world.resource_mut::<Selected>().0.clear();
            Ok(String::new())
        }
        _ => Err(usage("playerkingdom")),
    }
}

pub(crate) fn create(world: &mut World, args: &[&str]) -> Reply {
    let (template, at) = match args {
        [template] => (template, None),
        [template, x, y] => (template, Some((x, y))),
        _ => return Err(usage("create")),
    };
    let (focus, yaw) = {
        let cam = world
            .query::<&RtsCamera>()
            .single(world)
            .map_err(|_| "no camera")?;
        (map_point(cam.focus), cam.yaw)
    };
    let position = match at {
        Some((x, y)) => place(&world.resource::<Loaded>().map.ground, x, y)?,
        None => focus,
    };
    let owner = world
        .resource::<Console>()
        .placement
        .or(world.resource::<Local>().0)
        .ok_or("no placement kingdom; set one with placementkingdom")?;
    // Facing the way the camera looks, so it's seen from behind.
    let facing = [-yaw.sin(), yaw.cos()];
    // Only for what it says of a template that can't be raised.
    let raising = &world.resource::<Raising>().0;
    raising.raise(template, 0, owner, position, facing)?;
    let template = Id::from(*template);
    let before = world.resource::<Sim>().0.organizations.len();
    let raise = Action::Raise {
        template: template.clone(),
        owner,
        at: position,
        facing,
    };
    act(world, raise)?;
    let sim = &world.resource::<Sim>().0;
    let raised = sim.organizations.get(before..).and_then(<[_]>::last);
    let organization = raised.ok_or_else(|| format!("{template} can't be raised"))?;
    Ok(format!(
        "{} {} of {} units at {:.1} {:.1}",
        organization.template,
        organization.id,
        organization.units.len(),
        position[0],
        position[1]
    ))
}

/// Gives an organization an order at a place, marked as if clicked there.
pub(crate) fn order(world: &mut World, args: &[&str], aim: Aim) -> Reply {
    match (aim, args) {
        (Aim::Attack, [id, enemy]) => return after(world, id, enemy, aim),
        (Aim::Guard | Aim::Move, [id, other]) => return after(world, id, other, aim),
        _ => {}
    }
    let [id, x, y] = args else {
        return Err(usage(match aim {
            Aim::Attack => "attack",
            Aim::Guard => "guard",
            _ => "move",
        }));
    };
    let point = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let sim = &world.resource::<Sim>().0;
    let r = organization_by_id(sim, id)?;
    let owner = sim.organizations[r].owner;
    let (organization, to) = (r, point);
    let command = match aim {
        Aim::Move => tgsim::Command::Move { organization, to },
        Aim::Attack => tgsim::Command::Attack { organization, to },
        Aim::Run => tgsim::Command::Run { organization, to },
        Aim::Guard => tgsim::Command::Guard { organization, to },
        Aim::Unpack => tgsim::Command::Unpack {
            organization,
            at: to,
            facing: crate::actions::UNPACK_FACING,
        },
        Aim::Repair => return Err(usage("repair")),
    };
    if sim.destroyed(r) {
        return Err(format!("organization {id} is destroyed"));
    }
    give(world, owner, command)?;
    world.write_message(Sent {
        to: Some(point),
        aim,
        organizations: vec![(r, point)],
    });
    Ok(String::new())
}

/// Sends an organization after an enemy organization, or to guard or keep
/// near another, both by their ids.
pub(crate) fn after(world: &mut World, id: &str, enemy: &str, aim: Aim) -> Reply {
    let sim = &world.resource::<Sim>().0;
    let find = |id: &str| {
        sim.organizations
            .iter()
            .position(|r| r.id.to_string() == id)
            .ok_or_else(|| format!("no organization {id}"))
    };
    let (r, e) = (find(id)?, find(enemy)?);
    let there = sim.centre(e);
    let owner = sim.organizations[r].owner;
    let command = match aim {
        Aim::Guard => tgsim::Command::GuardPiece {
            organization: r,
            piece: tgsim::Piece::Organization(e),
        },
        Aim::Move => tgsim::Command::MoveOnto {
            organization: r,
            piece: tgsim::Piece::Organization(e),
        },
        _ => tgsim::Command::Pursue {
            organization: r,
            enemy: e,
        },
    };
    give(world, owner, command)?;
    world.write_message(Sent {
        to: None,
        aim,
        organizations: vec![(r, there)],
    });
    Ok(String::new())
}

pub(crate) fn select(world: &mut World, args: &[&str]) -> Reply {
    let sim = &world.resource::<Sim>().0;
    if args.is_empty() {
        let selected = &world.resource::<Selected>().0;
        let mut ids: Vec<String> = selected
            .iter()
            .map(|&r| sim.organizations[r].id.to_string())
            .collect();
        if let Some(b) = world.resource::<SelectedBuilding>().0 {
            ids.push(sim.buildings[b].id.to_string());
        }
        return Ok(match ids.is_empty() {
            true => "nothing selected".into(),
            false => ids.join(" "),
        });
    }
    let local = world.resource::<Local>().0;
    let scenery = world.resource::<crate::banners::Scenery>();
    // What the player can't see would only be let go of again.
    let fog = world.get_resource::<crate::fog::Fog>();
    // Lifting the fog shows everything only from the next frame.
    let lifted = fog.is_none_or(|f| f.lifted);
    let seen_building = |b| lifted || fog.is_some_and(|f| f.building(b) == crate::fog::Look::Shown);
    let seen_organization = |r| lifted || fog.is_some_and(|f| f.shows_organization(sim, r));
    if let [id] = args
        // A building packed up has handed its id to its truck.
        && let Some(b) = sim
            .buildings
            .iter()
            .position(|b| !b.gone() && b.id.to_string() == *id)
    {
        if !sim.buildings[b].standing() || !seen_building(b) {
            return Err(format!("building {id} isn't standing in sight"));
        }
        world.resource_mut::<Selected>().0.clear();
        world.resource_mut::<SelectedBuilding>().0 = Some(b);
        return Ok(String::new());
    }
    let mut ids: Vec<&str> = Vec::new();
    for id in args {
        if !ids.contains(id) {
            ids.push(id);
        }
    }
    if ids.len() > crate::object_display::MAX_SELECTED {
        return Err(format!(
            "takes up to {} organizations",
            crate::object_display::MAX_SELECTED
        ));
    }
    let picked = ids
        .iter()
        .map(|id| {
            let r = organization_by_id(sim, id)?;
            match !sim.destroyed(r) && !scenery.has(sim, r) && seen_organization(r) {
                true => Ok(r),
                false => Err(format!("organization {id} can't be selected")),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Taken as a click and Shift-clicks would take them.
    let (mut organizations, mut building) = (Vec::new(), None);
    let mut selection = crate::selection::Selection {
        organizations: &mut organizations,
        building: &mut building,
    };
    for &r in &picked {
        let rank = |t| crate::selection::rank(sim, local, t);
        selection.add(rank, crate::selection::Thing::Organization(r));
    }
    if organizations != picked {
        return Err(
            "organizations of sides standing differently can't be selected together".into(),
        );
    }
    world.resource_mut::<Selected>().0 = picked;
    world.resource_mut::<SelectedBuilding>().0 = None;
    Ok(String::new())
}

pub(crate) fn commission(world: &mut World, args: &[&str]) -> Reply {
    let (template, to, hero) = match args {
        [template] => (template, None, None),
        [template, hero] => (template, None, Some(hero)),
        [template, x, y] => {
            let to = place(&world.resource::<Loaded>().map.ground, x, y)?;
            (template, Some(to), None)
        }
        _ => return Err(usage("commission")),
    };
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let hero = (hero.map(|h| held_hero(&world.resource::<Sim>().0, player, h))).transpose()?;
    let b = selected_building(world)?;
    let raising = &world.resource::<Raising>().0;
    let hq = &world.resource::<Sim>().0.buildings[b].template;
    let listed = raising
        .list(hq.as_str())
        .iter()
        .find(|t| t.as_str().eq_ignore_ascii_case(template));
    let template = listed.ok_or_else(|| format!("{hq} doesn't commission {template}"))?;
    let (building, template) = (b, template.clone());
    let sim = &world.resource::<Sim>().0;
    // Said when it's refused for it.
    let lacking = (sim.rules().raisable.get(&template)).map(|o| sim.lacks(player, b, o));
    let settlement = sim.settlement_of(b);
    let command = match (to, settlement) {
        (Some(to), _) => tgsim::Command::Deliver {
            building,
            template,
            to,
        },
        (None, Some(settlement)) => tgsim::Command::Recruit {
            settlement,
            template,
            hero,
        },
        (None, None) if hero.is_some() => return Err("only settlements recruit heroes".into()),
        (None, None) => tgsim::Command::Commission { building, template },
    };
    if let (Err(tgsim::Refused::Lacking), Some(l)) = (sim.check(player, &command), lacking) {
        let ids = l.structures.iter().chain(&l.properties).map(|i| i.as_str());
        return Err(format!(
            "refused: Lacking, for {}",
            ids.collect::<Vec<_>>().join(" ")
        ));
    }
    give(world, player, command)?;
    Ok(String::new())
}

pub(super) fn use_faction_ability(world: &mut World, args: &[&str]) -> Reply {
    let [n, x, y] = args else {
        return Err(usage("usefactionability"));
    };
    let i: usize = n.parse().map_err(|_| usage("usefactionability"))?;
    let at = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let sim = &world.resource::<Sim>().0;
    if sim.experience(player).is_none() {
        return Err("no faction leads this side".into());
    }
    let op = sim
        .special_op(player, i)
        .ok_or(format!("the faction has no operation {i}"))?;
    let given = sim.operation_aim(player, i, None, at);
    let at = given.ok_or(format!("{} isn't given on the ground", op.id))?;
    let operation = tgsim::Command::Operate {
        operation: i,
        at,
        on: None,
    };
    give(world, player, operation)?;
    Ok(String::new())
}

pub(crate) fn research(world: &mut World, args: &[&str]) -> Reply {
    let [technology] = args else {
        return Err(usage("research"));
    };
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let b = selected_building(world)?;
    let sim = &world.resource::<Sim>().0;
    let listed = (sim.research_list(b).iter())
        .find(|r| r.technology.as_str().eq_ignore_ascii_case(technology));
    let Some(item) = listed else {
        let template = &sim.buildings[b].template;
        return Err(format!("{template} doesn't research {technology}"));
    };
    let command = tgsim::Command::Research {
        building: b,
        technology: item.technology.clone(),
    };
    give(world, player, command)?;
    Ok(String::new())
}

/// Gives the selected building the order `order` makes for it.
pub(super) fn building_order(
    world: &mut World,
    args: &[&str],
    order: fn(usize) -> tgsim::Command,
) -> Reply {
    no_args(args)?;
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let b = selected_building(world)?;
    give(world, player, order(b))?;
    Ok(String::new())
}

pub(crate) fn disband(world: &mut World, args: &[&str]) -> Reply {
    let [id] = args else {
        return Err(usage("disband"));
    };
    let sim = &world.resource::<Sim>().0;
    let organization = organization_by_id(sim, id)?;
    let owner = sim.organizations[organization].owner;
    give(world, owner, tgsim::Command::Disband { organization })?;
    Ok(String::new())
}

pub(crate) fn attach(world: &mut World, args: &[&str], attach: bool) -> Reply {
    let [id] = args else {
        return Err(usage(if attach { "attach" } else { "detach" }));
    };
    let sim = &world.resource::<Sim>().0;
    let organization = organization_by_id(sim, id)?;
    let owner = sim.organizations[organization].owner;
    let command = match attach {
        true => tgsim::Command::Attach { organization },
        false => tgsim::Command::Detach { organization },
    };
    give(world, owner, command)?;
    Ok(String::new())
}

pub(super) fn attach_hero(world: &mut World, args: &[&str]) -> Reply {
    let [id, hero] = args else {
        return Err(usage("attachhero"));
    };
    let sim = &world.resource::<Sim>().0;
    let organization = organization_by_id(sim, id)?;
    let owner = sim.organizations[organization].owner;
    let hero = held_hero(sim, owner, hero)?;
    give(
        world,
        owner,
        tgsim::Command::AttachHero { organization, hero },
    )?;
    Ok(String::new())
}

pub(super) fn detach_hero(world: &mut World, args: &[&str]) -> Reply {
    let [id] = args else {
        return Err(usage("detachhero"));
    };
    let sim = &world.resource::<Sim>().0;
    let organization = organization_by_id(sim, id)?;
    let owner = sim.organizations[organization].owner;
    give(world, owner, tgsim::Command::DetachHero { organization })?;
    Ok(String::new())
}

/// The hero in `player`'s pool named `word`, ignoring case.
fn held_hero(sim: &tgsim::World, player: u32, word: &str) -> Result<Id, String> {
    (sim.heroes(player).iter())
        .map(|h| &h.hero)
        .find(|h| h.as_str().eq_ignore_ascii_case(word))
        .cloned()
        .ok_or_else(|| format!("kingdom {player} holds no hero {word}"))
}

pub(crate) fn unpack(world: &mut World, args: &[&str]) -> Reply {
    let (x, y, facing) = match args {
        [x, y] => (x, y, crate::actions::UNPACK_FACING),
        [x, y, degrees] => {
            let a = number(degrees)?.to_radians();
            (x, y, [a.cos(), a.sin()])
        }
        _ => return Err(usage("unpack")),
    };
    let point = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let player = world.resource::<Local>().0.ok_or("nobody to play")?;
    let &[r] = &world.resource::<Selected>().0[..] else {
        return Err("select one packed building".into());
    };
    let command = tgsim::Command::Unpack {
        organization: r,
        at: point,
        facing,
    };
    give(world, player, command)?;
    let at = tgsim::unpack_site(point);
    world.write_message(crate::waypoints::Sent {
        to: Some(at),
        aim: crate::actions::Aim::Unpack,
        organizations: vec![(r, at)],
    });
    Ok(format!("unpacking at {:.0} {:.0}", at[0], at[1]))
}

pub(crate) fn build(world: &mut World, args: &[&str]) -> Reply {
    let (id, template, x, y, facing) = match args {
        [id, template, x, y] => (id, template, x, y, crate::actions::UNPACK_FACING),
        [id, template, x, y, degrees] => {
            let a = number(degrees)?.to_radians();
            (id, template, x, y, [a.cos(), a.sin()])
        }
        _ => return Err(usage("build")),
    };
    let at = place(&world.resource::<Loaded>().map.ground, x, y)?;
    let sim = &world.resource::<Sim>().0;
    let organization = organization_by_id(sim, id)?;
    let owner = sim.organizations[organization].owner;
    let template = tgmap::Id::from(*template);
    let command = tgsim::Command::Build {
        organization,
        template,
        at,
        facing,
    };
    give(world, owner, command)?;
    Ok(format!("building at {:.0} {:.0}", at[0], at[1]))
}

pub(crate) fn repair(world: &mut World, args: &[&str]) -> Reply {
    let [id, building] = args else {
        return Err(usage("repair"));
    };
    let sim = &world.resource::<Sim>().0;
    let organization = organization_by_id(sim, id)?;
    let b = (sim.buildings.iter())
        .position(|b| b.id.to_string() == *building && !b.gone())
        .ok_or_else(|| format!("no building {building}"))?;
    let owner = sim.organizations[organization].owner;
    let piece = tgsim::Piece::Building(b);
    give(
        world,
        owner,
        tgsim::Command::Repair {
            organization,
            piece,
        },
    )?;
    Ok(String::new())
}

/// Queues `command` from `player` for the next tick, as a click would.
/// The organization with object id `id`, by index.
fn organization_by_id(sim: &tgsim::World, id: &str) -> Result<usize, String> {
    (sim.organizations.iter())
        .position(|r| r.id.to_string() == id)
        .ok_or_else(|| format!("no organization {id}"))
}

fn selected_building(world: &World) -> Result<usize, String> {
    (world.resource::<SelectedBuilding>().0).ok_or_else(|| "no building selected".into())
}

/// What's said of a cheat tried in a match hosted elsewhere.
const HOSTED_HERE: &str = "only in a match hosted here";

/// Why the match's host wouldn't carry out `action` if asked from here,
/// if it wouldn't.
pub(super) fn barred(orders: &Orders, action: &Action) -> Result<(), String> {
    if orders.may(action) {
        return Ok(());
    }
    let watching = "the match's host takes nothing from those who only watch";
    Err(match (orders.players(), action) {
        _ if orders.gone() => "the match's host has gone".into(),
        _ if orders.waiting() => "the match's host has yet to begin it".into(),
        (Some([]), Action::Order { .. } | Action::Pause(_) | Action::Speed(_)) => watching.into(),
        (Some(players), Action::Order { .. }) => {
            let ids: Vec<String> = players.iter().map(u32::to_string).collect();
            let ids = ids.join(", ");
            format!("the match's host takes only the orders of kingdom {ids} from here")
        }
        _ => HOSTED_HERE.into(),
    })
}

/// Has the match do `action` at once, so that what follows finds it done,
/// which only a match hosted here does for what's outside its rules.
pub(crate) fn act(world: &mut World, action: Action) -> Result<(), String> {
    world.resource_scope(|world, mut orders: Mut<Orders>| {
        barred(&orders, &action)?;
        orders.act(action);
        crate::ticking::log_refused(orders.carry_out(&mut world.resource_mut::<Sim>().0));
        Ok(())
    })
}

pub(crate) fn give(world: &mut World, player: u32, command: tgsim::Command) -> Result<(), String> {
    world.resource_scope(|world, mut orders: Mut<Orders>| {
        let sim = &world.resource::<Sim>().0;
        let refused = |e| format!("refused: {e:?}");
        sim.check(player, &command).map_err(refused)?;
        let action = Action::Order { player, command };
        barred(&orders, &action)?;
        orders.act(action);
        Ok(())
    })
}

pub(super) fn advance(world: &mut World, args: &[&str]) -> Reply {
    let [seconds] = args else {
        return Err(usage("advance"));
    };
    let seconds = number(seconds)?;
    if !(0.0..=600.0).contains(&seconds) {
        return Err("takes 0 to 600 seconds".into());
    }
    let mut orders = world.remove_resource::<Orders>().ok_or("no match")?;
    let mut selected = std::mem::take(&mut world.resource_mut::<Selected>().0);
    let mut chosen = world.resource::<SelectedBuilding>().0;
    let sim = &mut world.resource_mut::<Sim>().0;
    for _ in 0..(seconds / tgsim::TICK).round() as u32 {
        crate::ticking::step_world(sim, &mut orders, &mut selected, &mut chosen);
    }
    world.insert_resource(orders);
    world.resource_mut::<Selected>().0 = selected;
    world.resource_mut::<SelectedBuilding>().0 = chosen;
    Ok(String::new())
}

pub(super) fn destroy_selected(world: &mut World, args: &[&str]) -> Reply {
    no_args(args)?;
    let selected = world.resource::<Selected>().0.clone();
    if selected.is_empty() {
        return Err("nothing selected".into());
    }
    for r in selected {
        act(world, Action::Destroy(r))?;
    }
    world.resource_mut::<Selected>().0.clear();
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_lists_what_a_kingdom_or_template_names() {
        let sim = crate::orders::testing::fresh();
        let all = objects(&sim, &[]);
        assert_eq!(
            all,
            "organization squad 1 of kingdom 1 at 10 10\norganization squad 3 of kingdom 2 at 10 20"
        );
        assert_eq!(
            objects(&sim, &["2"]),
            "organization squad 3 of kingdom 2 at 10 20"
        );
        assert_eq!(objects(&sim, &["SQUAD"]), all);
        assert_eq!(objects(&sim, &["3"]), "nothing on the map");
    }
}
