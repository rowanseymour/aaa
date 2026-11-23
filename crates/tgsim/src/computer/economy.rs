//! The goals that build a side up: recruiting, research, unpacking its
//! trucks, attaching companies to headquarters and putting up buildings.

use std::collections::HashMap;

use tgmap::Id;
use tgmap::sai::{Ego, RecruitRequest};

use super::liking;
use super::picture::Picture;
use super::planner::{Goal, Key, Planner, Work};
use crate::economy::Change;
use crate::{Command, Piece, Refused, Researchable};

/// The combat value one request for fighting strength asks for.
const MILITARY_REQUEST: f32 = 10.0;

/// The widest region patch on a map of [`BASE_MAP_SIDE`] tiles a side,
/// which sets how far round a region's centre a spot is looked for, and
/// how it grows with the map.
const PATCH_WIDTH: f32 = 12.0;
const BASE_MAP_SIDE: f32 = 64.0;
const PATCH_WIDTH_EXPONENT: f32 = 1.0 / 3.0;

pub(super) fn goals(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let mut out = recruit(planner, picture);
    out.extend(research(planner, picture));
    out.extend(unpack(planner, picture));
    out.extend(attach(planner, picture));
    out.extend(construct(planner, picture));
    out
}

/// Something one of its buildings could recruit now.
struct Offer<'w> {
    building: usize,
    template: &'w Id,
    organization: &'w tgmap::Organization,
    cost: f32,
    combat_value: f32,
    /// The order that recruits it.
    command: Command,
}

impl Offer<'_> {
    /// Its templates, whose properties are its own: the organization's and
    /// its members', but not what it unpacks into.
    fn templates(&self) -> Vec<&Id> {
        let o = self.organization;
        let mut out = vec![&o.template];
        for m in &o.members {
            if !o.lone() {
                out.push(&m.template);
            }
        }
        out
    }

    /// What it's worth to the side.
    fn worth(&self, ego: &Ego, picture: &Picture) -> f32 {
        let unpacks = self
            .organization
            .unpacks
            .as_ref()
            .map(|u| &u.building.template);
        liking::worth(ego, picture, &self.templates(), unpacks, self.combat_value)
    }

    /// Whether it's a builder or a truck, rather than for fighting.
    fn works(&self) -> bool {
        self.organization.unpacks.is_some()
            || (self.organization.members.iter())
                .any(|m| m.fighter.as_ref().is_some_and(|f| !f.builds.is_empty()))
    }
}

/// What its recruiting buildings offer, as a person would see it: each
/// standing, not busy and with room in its queue for another of its items.
fn offers<'w>(ego: &Ego, picture: &Picture<'w>) -> Vec<Offer<'w>> {
    let world = picture.world;
    let most = ego.goal_engine.build_queue_max_items.max(1) as usize;
    let mut out = Vec::new();
    for &b in &picture.buildings {
        let building = &world.buildings[b];
        if building.busy() || building.queue.len() >= most {
            continue;
        }
        let Some(list) = world.rules.recruit_lists.get(&building.template) else {
            continue;
        };
        for template in list {
            // A settlement recruits at once; other buildings commission.
            let command = match world.settlement_of(b) {
                Some(settlement) => Command::Recruit {
                    settlement,
                    template: template.clone(),
                    hero: None,
                },
                None => Command::Commission {
                    building: b,
                    template: template.clone(),
                },
            };
            match world.check(picture.player, &command) {
                Ok(()) | Err(Refused::Unaffordable) => {}
                Err(_) => continue,
            }
            let Some(organization) = world.rules.raisable.get(template) else {
                continue;
            };
            let (_, after) = world.preview(picture.player, Change::Commission(b, organization));
            if after.limits.iter().any(|l| l.used > l.provided) {
                continue;
            }
            let combat_value = (organization.members.iter())
                .map(|m| world.template_value(&m.template))
                .sum();
            out.push(Offer {
                building: b,
                template,
                organization,
                cost: world.cost(b, organization),
                combat_value,
                command,
            });
        }
    }
    out
}

/// What it's asking a recruit for.
enum Asked<'a> {
    Specific(&'a RecruitRequest),
    Military,
}

/// Each think's recruit goals, made afresh from the mood's requests.
fn recruit(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let ego = planner.ego().clone();
    let world = picture.world;
    let recruiting = &ego.goal_engine.recruiting;
    let offers = offers(&ego, picture);
    let listed: Vec<String> = (offers.iter())
        .map(|o| format!("{} {:.0}", o.template, o.combat_value))
        .collect();
    log::debug!("{}: recruits from {listed:?}", picture.player);
    if offers.is_empty() {
        return Vec::new();
    }
    // Requests by priority, the fighting ones at 0.
    let mut requests: Vec<(f32, Asked)> = Vec::new();
    for request in &recruiting.specific_recruit_request {
        let fits: Vec<&Offer> = (offers.iter())
            .filter(|o| answers(picture, request, o))
            .collect();
        let Some(first) = fits.first() else {
            continue;
        };
        let property = request.property_ids.as_ref().filter(|p| !is_null(p));
        let n = liking::owned(picture, &first.templates(), property);
        let mut priority = request.base_priority
            + first.worth(&ego, picture) * request.economic_value_multiplier
            - request.repeat_penalty * n as f32;
        if n == 0 {
            priority += request.one_time_bonus;
        }
        if is_hq(picture, first) && !hq_with_room(picture, first) {
            priority += request.hq_type_not_available_bonus;
        }
        for _ in 0..request.number_to_request.max(1) {
            requests.push((priority, Asked::Specific(request)));
        }
    }
    for _ in 0..recruiting.num_military_goals {
        requests.push((0.0, Asked::Military));
    }
    requests.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut picked: HashMap<&Id, u32> = HashMap::new();
    let mut military_done = false;
    let mut out: Vec<Goal> = Vec::new();
    for (priority, asked) in requests {
        let military = matches!(asked, Asked::Military);
        if military && military_done {
            continue;
        }
        let mut strength = 0.0;
        loop {
            let mut best: Option<(f32, &Offer)> = None;
            for offer in &offers {
                let fits = match asked {
                    Asked::Specific(r) => answers(picture, r, offer),
                    Asked::Military => offer.combat_value > 0.0,
                };
                let templates = offer.templates();
                let most = liking::most_per_think(&ego, world, &templates);
                if !fits || picked.get(offer.template).copied().unwrap_or(0) >= most {
                    continue;
                }
                let score = candidate(planner, &ego, picture, offer, military);
                if score > 0.0 && best.is_none_or(|(b, _)| score > b) {
                    best = Some((score, offer));
                }
            }
            let Some((score, offer)) = best else {
                break;
            };
            *picked.entry(offer.template).or_default() += 1;
            // Among those that recruit it now, any will do.
            let at: Vec<&Offer> = (offers.iter())
                .filter(|o| o.template == offer.template)
                .collect();
            let chosen = at[(planner.chance() * at.len() as f32) as usize % at.len()];
            out.push(Goal {
                key: Key::Recruit(offer.template.clone()),
                kind: "RECRUIT",
                priority: priority + score,
                cost: chosen.cost,
                work: Work::Order(chosen.command.clone()),
            });
            strength += offer.combat_value;
            if !military || strength >= MILITARY_REQUEST {
                break;
            }
        }
        if military && strength > 0.0 {
            military_done = true;
        }
    }
    out.sort_by(|a, b| b.priority.total_cmp(&a.priority));
    out.truncate(recruiting.max_goals_per_think as usize);
    // One goal per kind, which the planner tells apart by key.
    let mut seen = Vec::new();
    out.retain(|g| {
        let fresh = !seen.contains(&g.key);
        seen.push(g.key.clone());
        fresh
    });
    out
}

/// Whether `offer` is what `request` asks for.
fn answers(picture: &Picture, request: &RecruitRequest, offer: &Offer) -> bool {
    let world = picture.world;
    let named = (request.recruit_ids.as_ref())
        .is_some_and(|id| id.as_str().eq_ignore_ascii_case(offer.template.as_str()));
    let property = request.property_ids.as_ref().filter(|p| !is_null(p));
    named || property.is_some_and(|p| offer.templates().iter().any(|t| liking::has(world, t, p)))
}

fn is_null(id: &Id) -> bool {
    id.as_str().eq_ignore_ascii_case("null")
}

/// Whether recruiting `offer` gives the side a headquarters.
fn is_hq(picture: &Picture, offer: &Offer) -> bool {
    let rules = &picture.world.rules;
    (rules.logistics.get(&offer.organization.template)).is_some_and(|l| l.attachments.is_some())
}

/// Whether the side has a headquarters of `offer`'s kind with room.
fn hq_with_room(picture: &Picture, offer: &Offer) -> bool {
    let world = picture.world;
    let building = offer
        .organization
        .unpacks
        .as_ref()
        .map(|u| &u.building.template);
    (world.hqs().iter())
        .filter(|h| h.owner == picture.player && Some(&h.building) == building)
        .any(|h| world.free_places(h) > 0)
}

/// What recruiting `offer` is worth, for fighting strength or not.
fn candidate(
    planner: &mut Planner,
    ego: &Ego,
    picture: &Picture,
    offer: &Offer,
    military: bool,
) -> f32 {
    let r = &ego.goal_engine.recruiting;
    let world = picture.world;
    let (cv_mult, ratio_mult) = match military {
        true => (r.core_cv_multiplier, r.core_cv_ratio_multiplier),
        false => (r.support_cv_multiplier, r.support_cv_ratio_multiplier),
    };
    let cv = offer.combat_value;
    let mut score = cv * cv_mult + cv / offer.cost.max(1.0) * ratio_mult;
    score += offer.worth(ego, picture) * r.economic_value_multiplier;
    score += liking::priority(ego, picture, &offer.templates(), false);
    let money = crate::economy::money(&world.rules.economy);
    let now = world.balance(picture.player).net.get(money);
    let (_, after) = world.preview(
        picture.player,
        Change::Commission(offer.building, offer.organization),
    );
    let after = after.net.get(money);
    if after < r.insufficient_income_level {
        score -= r.insufficient_income_penalty;
    } else if after < now {
        score -= r.decreased_income_penalty;
    }
    if military && offer.works() {
        score -= r.mismatching_builder_penalty;
    }
    score + planner.fuzz(r.candidate_fuzziness)
}

/// A goal for each technology its research buildings could start now.
fn research(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let ego = planner.ego();
    let world = picture.world;
    let most = ego.goal_engine.build_queue_max_items.max(1) as usize;
    let mut out: Vec<Goal> = Vec::new();
    for &b in &picture.buildings {
        let building = &world.buildings[b];
        if building.busy() || building.queue.len() >= most {
            continue;
        }
        for item in world.research_list(b) {
            if world.researchable(picture.player, b, item) != Researchable::Ready {
                continue;
            }
            let key = Key::Research(item.technology.clone());
            if out.iter().any(|g| g.key == key) {
                continue;
            }
            out.push(Goal {
                key,
                kind: "RESEARCH",
                priority: ego.goal_engine.research.priority(&item.technology),
                cost: world.research_cost(item),
                work: Work::Order(Command::Research {
                    building: b,
                    technology: item.technology.clone(),
                }),
            });
        }
    }
    out
}

/// A goal for each packed building, to drive it somewhere useful and
/// unpack it there.
fn unpack(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let world = picture.world;
    let u = planner.ego().goal_engine.unpacking.clone();
    let mut out = Vec::new();
    for &r in &picture.organizations {
        let o = &world.organizations[r];
        let Some(unpacks) = &o.unpacks else {
            continue;
        };
        if o.unpack_at.is_some() || !o.engaged.is_empty() {
            continue;
        }
        let from = world.centre(r);
        let Some(here) = picture.region(from) else {
            continue;
        };
        let island = world.regions.regions[here].island;
        let steps = world.regions.steps(here);
        let mut candidates: Vec<usize> = (steps.iter().enumerate())
            .filter(|(_, s)| s.is_some_and(|s| s <= u.adjacent_region_depth))
            .map(|(i, _)| i)
            .collect();
        let on_island: Vec<usize> = (0..world.regions.regions.len())
            .filter(|&i| island.is_some() && world.regions.regions[i].island == island)
            .collect();
        for _ in 0..u.num_random_regions {
            if on_island.is_empty() {
                break;
            }
            let i = (planner.chance() * on_island.len() as f32) as usize % on_island.len();
            if !candidates.contains(&on_island[i]) {
                candidates.push(on_island[i]);
            }
        }
        let before = planner.unpacking.get(&r).copied();
        candidates.extend(before.filter(|b| !candidates.contains(b)));
        let template = &unpacks.building.template;
        let needs_supply = (world.rules.economies.get(template)).is_some_and(|e| e.supply_required);
        let step = placement_step(&unpacks.room);
        let mut best: Option<(f32, usize, [f32; 2])> = None;
        for region in candidates {
            let Some(at) = find_spot(planner, world, region, step, |at| {
                (!needs_supply || world.supplied(picture.player, at))
                    && world.room_to_unpack(unpacks, at, tgmap::UNPACKED_FACING)
            }) else {
                continue;
            };
            let mut score = u.base_priority
                - picture.enemy[region] * u.enemy_dynamic_influence_mult
                - picture.enemy_static[region] * u.enemy_static_influence_mult;
            // Its buildings nearby, the further the more.
            let mut nearby = 0.0;
            for &b in &picture.buildings {
                let d = crate::distance(world.buildings[b].position, at);
                if d < u.max_distance && u.max_distance > 0.0 {
                    nearby += u.max_friendly_building_bonus_per_building
                        * libm::powf(d / u.max_distance, u.distance_exponent);
                }
            }
            score += nearby.min(u.max_friendly_building_bonus);
            let travel = crate::distance(from, at) * u.travel_distance_mult;
            score -= libm::powf(travel, u.travel_distance_exponent);
            score += planner.fuzz(u.fuzzy_factor);
            if before == Some(region) {
                score += u.inertia;
            }
            if best.is_none_or(|(b, ..)| score > b) {
                best = Some((score, region, at));
            }
        }
        let Some((priority, region, at)) = best else {
            continue;
        };
        planner.unpacking.insert(r, region);
        let order = Command::Unpack {
            organization: r,
            at,
            facing: tgmap::UNPACKED_FACING,
        };
        out.push(Goal {
            key: Key::Unpack(r),
            kind: "UNPACK",
            priority: priority.max(0.001),
            cost: 0.0,
            work: Work::Unit {
                organization: r,
                order: Some(order),
            },
        });
    }
    out
}

/// How far apart the points tried for a building of `shape` are: its
/// radius, or its smaller half-side, and at least a tile.
fn placement_step(shape: &tgmap::Shape) -> f32 {
    let size = match *shape {
        tgmap::Shape::Circle { radius } => radius,
        tgmap::Shape::Rect { width, height } => width.min(height) / 2.0,
        tgmap::Shape::Point => 0.0,
    };
    size.max(tgmap::pathing::CELL_SIZE)
}

/// A spot in `region` where `fits`: the first that does of points in
/// rings round its centre, `step` apart, from a ring and angle drawn at
/// random, trying twice.
fn find_spot(
    planner: &mut Planner,
    world: &crate::World,
    region: usize,
    step: f32,
    fits: impl Fn([f32; 2]) -> bool,
) -> Option<[f32; 2]> {
    let centre = world.regions.regions.get(region)?.centre;
    let side = world.pathing.side as f32;
    let width = (PATCH_WIDTH * libm::powf(side / BASE_MAP_SIDE, PATCH_WIDTH_EXPONENT)).round();
    let rings = (0.75 * width / step).ceil() as usize;
    for _ in 0..2 {
        let first = (planner.chance() * (rings + 1) as f32) as usize % (rings + 1);
        let angle = planner.chance() * std::f32::consts::TAU;
        for k in (first..=rings).chain(0..first) {
            let points = (6 * k).max(1);
            for i in 0..points {
                let a = angle + i as f32 / points as f32 * std::f32::consts::TAU;
                let radius = k as f32 * step;
                let at = [
                    centre[0] + radius * libm::cosf(a),
                    centre[1] + radius * libm::sinf(a),
                ];
                if world.regions.at(at) == Some(region) && fits(at) {
                    return Some(at);
                }
            }
        }
    }
    None
}

/// A goal for each detached company that isn't fighting, to attach it to
/// a headquarters with room.
fn attach(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let ego = planner.ego();
    let world = picture.world;
    let a = &ego.goal_engine.attach;
    let mut out = Vec::new();
    for r in world.detached(picture.player) {
        if !a.attach_when_engaged && !world.organizations[r].engaged.is_empty() {
            continue;
        }
        if world.attaches_to(r).is_none() {
            continue;
        }
        let templates = picture.templates(Piece::Organization(r));
        let priority = world.full_value(r) * a.cv_multiplier
            + liking::priority(ego, picture, &templates, true);
        out.push(Goal {
            key: Key::Attach(r),
            kind: "ATTACH",
            priority,
            cost: 0.0,
            work: Work::Unit {
                organization: r,
                order: Some(Command::Attach { organization: r }),
            },
        });
    }
    out
}

/// A goal for each region where it builds: the building worth most there
/// that a builder of its own, not busy, can put up.
fn construct(planner: &mut Planner, picture: &Picture) -> Vec<Goal> {
    let ego = planner.ego().clone();
    let c = &ego.goal_engine.construction;
    let world = picture.world;
    let builders: Vec<usize> = (picture.organizations.iter().copied())
        .filter(|&r| {
            let o = &world.organizations[r];
            o.unpacks.is_none()
                && o.build.is_none()
                && o.mending.is_none()
                && !world.buildable(r).is_empty()
        })
        .collect();
    if builders.is_empty() {
        return Vec::new();
    }
    // The buildings each region holds that it builds to defend.
    let mut by_region: std::collections::BTreeSet<usize> = Default::default();
    for &b in &picture.buildings {
        let building = &world.buildings[b];
        let template = &building.template;
        let recruits = world.rules.recruit_lists.contains_key(template);
        let researches = world.rules.research_lists.contains_key(template);
        let city = world.settlement_of(b).is_some();
        let defended = c.construct_in_all_regions
            || (c.construct_to_defend_recruiters && recruits)
            || (c.construct_to_defend_research && researches)
            || (c.construct_to_defend_cities && city);
        if !defended {
            continue;
        }
        if c.construct_in_supply_only && !world.supplied(picture.player, building.position) {
            continue;
        }
        by_region.extend(picture.region(building.position));
    }
    let mut out = Vec::new();
    for region in by_region {
        let mut best: Option<(f32, usize, Id)> = None;
        for &r in &builders {
            for template in world.buildable(r) {
                let value = world.template_value(&template);
                let templates = [&template];
                let score = liking::worth(&ego, picture, &templates, None, value)
                    + liking::priority(&ego, picture, &templates, false);
                if score > 0.0 && best.as_ref().is_none_or(|(b, ..)| score > *b) {
                    best = Some((score, r, template));
                }
            }
        }
        let Some((priority, r, template)) = best else {
            continue;
        };
        let facing = tgmap::UNPACKED_FACING;
        let needs_supply = c.construct_in_supply_only
            || (world.rules.economies.get(&template)).is_some_and(|e| e.supply_required);
        let fits = |at: [f32; 2]| {
            (!needs_supply || world.supplied(picture.player, at))
                && world.room_to_build(picture.player, &template, at, facing)
        };
        // A spot settled on before is kept while there's still room there.
        let kept = (planner.sites.get(&region))
            .filter(|(t, at)| *t == template && fits(*at))
            .map(|(_, at)| *at);
        let shape = (world.rules.buildable.get(&template))
            .and_then(|b| b.obstacle.as_ref())
            .map_or(tgmap::Shape::Point, |o| o.shape);
        let step = placement_step(&shape) + world.rules.building_padding;
        let Some(at) = kept.or_else(|| find_spot(planner, world, region, step, fits)) else {
            continue;
        };
        planner.sites.insert(region, (template.clone(), at));
        out.push(Goal {
            key: Key::Construct(region),
            kind: "CONSTRUCT",
            priority,
            cost: world.site_cost(&template),
            work: Work::Unit {
                organization: r,
                order: Some(Command::Build {
                    organization: r,
                    template,
                    at,
                    facing,
                }),
            },
        });
    }
    out
}
