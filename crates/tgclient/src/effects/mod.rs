//! Plays the visual effects the game data gives objects as things happen to
//! them: dust while buildings pack and unpack, flashes and hit puffs as
//! they fire, explosions where shells land, and smoke, fire and debris as
//! they're hurt. Which of an object's effect commands an event runs, where
//! the effect goes and how long it lasts are the data's; this notices the
//! events in the sim and plays the effects drawn by a model and those that
//! are pictures laid on the ground.

mod follow;
mod notice;
mod place;
mod play;
mod tint;

use follow::{fade, follow, show_by_night};
use notice::{AT_WORK, DEAD, Now, Seen, When, by_state, changes, crossed, notice, on_shot};
pub(crate) use play::Lowers;
use play::play;
use tint::tint_units;

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use tgck::map::CELL_SIZE;
use tgdata::{GameData, Id, ObjectEffects, OperationCues, Vfx, VfxCue, VfxTint, Weapon};
use tgmap::Map;
use tgrender::decals::DecalCache;
use tgsim::{Landing, Operated, Target};

use crate::Screen;
use crate::random::Random;

#[derive(Resource)]
pub struct Effects {
    vfx: Arc<HashMap<Id, Vfx>>,
    objects: HashMap<Id, ObjectEffects>,
    /// What plays as each special operation is used, by id.
    operations: HashMap<Id, OperationCues>,
    /// What each property shows on what gains it, by id.
    properties: HashMap<Id, Vec<VfxCue>>,
    /// The node names of the model points commands name, by id.
    bones: HashMap<Id, String>,
    /// What the pictures laid on the ground so far are drawn with.
    decals: DecalCache,
    /// Each map tile's terrain group, row-major, `side` tiles a row.
    groups: Vec<Option<Id>>,
    side: usize,
    /// Commands waiting to play, with when.
    queued: Vec<(f64, Play)>,
    /// Shots each weapon has fired, by who fired it and the weapon's index.
    shots: HashMap<(Target, usize), u32>,
    /// The tick of the last projectile landing seen.
    landed: Option<u64>,
    /// The tick of the last special operation seen carried out.
    operated: Option<u64>,
    /// How far up an attacker a hit on its target lands, as a share of
    /// the attacker's height.
    aim: f32,
    /// The effects detail setting, from 0 for none to 1 for all, which
    /// effects that aren't critical play by.
    detail: f32,
    /// The map's sun is dark enough that effects for the night show.
    night: bool,
    /// Seconds since a site's health last changed that it's still being
    /// worked on, if the game says.
    at_work: Option<f32>,
    rng: Random,
}

/// An effect command set off, waiting to play.
struct Play {
    cue: VfxCue,
    source: Source,
    /// What the event was aimed at, if anything.
    target: Option<Target>,
    /// The sim tick it happened on.
    tick: u64,
    /// For a projectile, where it landed; for a special operation, its
    /// centre.
    landed: Option<[f32; 2]>,
    /// For an attack, the index of the weapon among its attacker's.
    weapon: Option<usize>,
    /// The property it shows on what holds it, so that losing the property
    /// stops only what it started.
    holding: Option<Id>,
}

/// What set off an effect command.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    Thing(Target),
    /// The projectile of a shot fired by this, which has reached its
    /// target.
    Projectile(Target),
    /// A piece of the map's scenery, by its view.
    Scenery(Entity),
    /// A special operation, on the ground at its centre.
    Operation,
}

/// Something an effect plays on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Subject {
    Thing(Target),
    Scenery(Entity),
}

/// Scenery whose effects for being created are queued.
#[derive(Component)]
struct Scenery;

/// One of the effects playing.
#[derive(Component)]
struct Effect {
    vfx: Id,
    /// Whose command created it, for a stop command to find it by.
    by: Option<Subject>,
    /// The property it shows, for one shown on what holds it.
    holding: Option<Id>,
    /// When it ends, unless it lasts until stopped.
    ends: Option<f64>,
    /// It goes once its spot is off screen.
    discard: bool,
}

/// An effect that stays where it was put for something that stays put,
/// which goes when that does and is hidden while it is.
#[derive(Component)]
struct Tied(Entity);

/// An effect for the night, hidden while the sun's too bright for it.
#[derive(Component)]
struct NightOnly;

/// An effect a command stopped that's still to go as it does.
#[derive(Component)]
struct Stopped;

/// What an effect that stays where it's put goes with: the building that
/// made it, by its view, for one that lasts until stopped; else what it's
/// attached to.
fn tie(view: Option<Entity>, attached: Option<Entity>, permanent: bool) -> Option<Entity> {
    view.filter(|_| permanent).or(attached)
}

/// An attached effect's place on what it's attached to.
#[derive(Component)]
struct Follow {
    anchor: Entity,
    on: OnThing,
    /// It keeps the way it faced when it began.
    gyroscope: bool,
}

/// Where on a thing an effect goes.
#[derive(Clone, Copy)]
struct OnThing {
    /// A point of the thing's model, from its origin in model units.
    point: Transform,
    /// Moved this far, in world units, along the point's axes.
    offset: Vec3,
    /// And turned this way from it.
    turn: Quat,
}

impl OnThing {
    /// Where it is, on a thing placed at `root`; drawn at its own size,
    /// whatever the thing's.
    fn place(&self, root: &Transform) -> Transform {
        let rotation = root.rotation * self.point.rotation;
        Transform {
            translation: root.transform_point(self.point.translation) + rotation * self.offset,
            rotation: rotation * self.turn,
            scale: Vec3::ONE,
        }
    }

    /// Turns it as an effect placed by it facing `was` was turned, to
    /// face `now`, so it goes on facing that way from its thing.
    fn turned(mut self, was: Quat, now: Quat) -> OnThing {
        self.turn = self.turn * was.inverse() * now;
        self
    }
}

impl Effects {
    /// How the tint with this id changes its object's colours.
    pub fn tint(&self, ids: &str) -> Option<VfxTint> {
        self.vfx.get(&Id::from(ids))?.tint
    }

    pub fn new(game: &GameData, map: &Map) -> Effects {
        let vfx = game.vfx();
        let objects = game.object_effects();
        let operations = game.operation_cues();
        let properties = game.property_cues();
        let bones = game.bones();
        let variations = game.terrain_variations();
        let terrain = &map.terrain;
        let groups = terrain
            .tiles
            .iter()
            .map(|t| {
                let name = terrain.name_of(t);
                variations.get(&Id::from(name))?.group.clone()
            })
            .collect();
        let night = game.night(map.sun.diffuse);
        debug!(
            "{} effects shown by {} object templates, at {} named points, by {}",
            vfx.len(),
            objects.len(),
            bones.len(),
            if night { "night" } else { "day" }
        );
        Effects {
            vfx,
            objects,
            operations,
            properties,
            bones,
            decals: DecalCache::default(),
            groups,
            side: terrain.side,
            queued: Vec::new(),
            shots: HashMap::new(),
            landed: None,
            operated: None,
            aim: game.aim_heights().straight,
            detail: 1.0,
            night,
            at_work: game.active_construction_seconds(),
            rng: Random::new(0x9e37_79b9_7f4a_7c15),
        }
    }

    /// Has effects for the night show, or not.
    pub(crate) fn set_night(&mut self, night: bool) {
        self.night = night;
    }

    /// The terrain group of the tile at a map point.
    fn group_at(&self, [x, y]: [f32; 2]) -> Option<&Id> {
        if self.side == 0 {
            return None;
        }
        let tile = |w: f32| ((w / CELL_SIZE).floor().max(0.0) as usize).min(self.side - 1);
        self.groups.get(tile(y) * self.side + tile(x))?.as_ref()
    }

    /// A number in `0..1`.
    fn random(&mut self) -> f32 {
        self.rng.next()
    }

    fn queue(&mut self, when: When, cues: &[VfxCue], source: Source, target: Option<Target>) {
        self.queue_holding(when, cues, source, target, None);
    }

    fn queue_holding(
        &mut self,
        when: When,
        cues: &[VfxCue],
        source: Source,
        target: Option<Target>,
        holding: Option<&Id>,
    ) {
        for cue in cues {
            let play = Play {
                cue: cue.clone(),
                source,
                target,
                tick: when.tick,
                landed: when.landed,
                weapon: when.weapon,
                holding: holding.cloned(),
            };
            self.queued.push((when.at + f64::from(cue.delay), play));
        }
    }

    /// Queues the commands of a projectile's landing: those of its
    /// template's for landing, and its weapon's for each target it lands
    /// on.
    fn land(&mut self, landing: &Landing, weapon: &Weapon, at: f64) {
        let when = When {
            at,
            tick: landing.tick,
            weapon: Some(landing.weapon),
            landed: Some(landing.at),
        };
        let target = landing.target;
        let landed = (weapon.projectile.as_ref())
            .and_then(|p| self.objects.get(p))
            .map(|o| o.destroyed.clone())
            .unwrap_or_default();
        self.queue(when, &landed, Source::Projectile(landing.from), target);
        // A landing on the ground has no target to play them on.
        if target.is_none() {
            return;
        }
        let per_target = weapon.effects.per_target.clone();
        let when = When {
            landed: None,
            ..when
        };
        self.queue(when, &per_target, Source::Thing(landing.from), target);
    }

    /// Queues what plays as a special operation is used, on the ground at
    /// its centre.
    fn operate(&mut self, operated: &Operated, at: f64) {
        let when = When {
            at,
            tick: operated.tick,
            weapon: None,
            landed: operated.at,
        };
        let visuals = (self.operations.get(&operated.operation))
            .map(|c| c.visuals.clone())
            .unwrap_or_default();
        self.queue(when, &visuals, Source::Operation, None);
    }

    /// Queues the commands a thing of `template` has for being created.
    fn create(&mut self, source: Source, template: &str, at: When) {
        if let Some(o) = self.objects.get(&Id::from(template)) {
            let created = o.created.clone();
            self.queue(at, &created, source, None);
        }
    }

    /// Queues the commands of what `me`, of `template`, did since it was
    /// `seen`, and notes what it's doing `now`.
    fn witness(&mut self, me: Target, template: &str, seen: &mut Seen, now: Now, at: When) {
        let objects = self.objects.get(&Id::from(template)).cloned();
        let mut now = now;
        if now.site {
            if now.health != seen.health {
                seen.worked = Some(at.tick);
            }
            let second = (at.tick as f32 * tgsim::TICK) as u64;
            if seen.looked != Some(second) {
                seen.looked = Some(second);
                let ago = |t: u64| (at.tick - t) as f32 * tgsim::TICK;
                let within = |s: f32| seen.worked.is_some_and(|t| ago(t) <= s);
                seen.at_work = self.at_work.is_some_and(within);
            }
            if seen.at_work {
                now.states.push(AT_WORK);
            }
        }
        if let Some(o) = &objects {
            let (entered, left) = changes(&seen.states, &now.states);
            for state in entered {
                let cues = by_state(&o.entered, state);
                self.queue(at, &cues, Source::Thing(me), None);
                if state == DEAD {
                    self.queue(at, &o.destroyed, Source::Thing(me), None);
                }
            }
            for state in left {
                // What lasts on the thing that entering it started ends
                // with it.
                let on_it =
                    |c: &VfxCue| c.attached && matches!(c.at.as_str(), "SELF" | "TARGET_THING");
                let lasting = |c: &VfxCue| {
                    c.duration.is_none()
                        && on_it(c)
                        && self.vfx.get(&c.vfx).is_some_and(|v| v.permanent)
                };
                let ended: Vec<VfxCue> = (by_state(&o.entered, state).into_iter())
                    .filter(|c| !c.stop && lasting(c))
                    .map(|c| VfxCue {
                        stop: true,
                        delay: 0.0,
                        ..c
                    })
                    .collect();
                self.queue(at, &ended, Source::Thing(me), None);
                let cues = by_state(&o.left, state);
                self.queue(at, &cues, Source::Thing(me), None);
            }
            // A site's health going up and down sets nothing off, finishing
            // included.
            let was_site = seen.states.contains(&"construction");
            if now.health != seen.health && !now.site && !was_site {
                let cues = crossed(&o.health, seen.health, now.health);
                self.queue(at, &cues, Source::Thing(me), None);
            }
            if now.finished && !seen.finished {
                self.queue(at, &o.constructed, Source::Thing(me), None);
            }
        }
        if now.shot != seen.shot
            && let (Some((_, w)), Some(weapon)) = (now.shot, now.weapon)
        {
            let fired = self.shots.entry((me, w)).or_default();
            let n = *fired;
            *fired += 1;
            let e = &weapon.effects;
            // A projectile's targets are hit as it lands.
            let per_target = match weapon.flight {
                Some(_) => None,
                None => Some(&e.per_target),
            };
            let attack: Vec<VfxCue> = [Some(&e.start), Some(&e.shot), per_target, Some(&e.end)]
                .into_iter()
                .flatten()
                .flatten()
                .filter(|c| on_shot(c.every, n))
                .cloned()
                .collect();
            let at = When {
                weapon: Some(w),
                ..at
            };
            self.queue(at, &attack, Source::Thing(me), now.target);
        }
        // A property's effects are on what holds it, and go with it.
        let source = Source::Thing(me);
        for p in now.held.iter().filter(|p| !seen.held.contains(p)) {
            let cues = self.properties.get(p).cloned().unwrap_or_default();
            self.queue_holding(at, &cues, source, Some(me), Some(p));
        }
        for p in seen.held.iter().filter(|p| !now.held.contains(p)) {
            // What hasn't started yet never will.
            (self.queued).retain(|(_, q)| q.source != source || q.holding.as_ref() != Some(p));
            let stops: Vec<VfxCue> = (self.properties.get(p).into_iter().flatten())
                .filter(|c| !c.stop)
                .map(|c| VfxCue {
                    stop: true,
                    delay: 0.0,
                    ..c.clone()
                })
                .collect();
            self.queue_holding(at, &stops, source, Some(me), Some(p));
        }
        seen.held = now.held;
        seen.shot = now.shot;
        seen.health = now.health;
        seen.states = now.states;
        seen.finished |= now.finished;
    }
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (notice, play, follow, show_by_night, fade, tint_units)
                .chain()
                .after(crate::views::show_packing)
                .after(crate::views::show_units)
                .after(crate::views::show_buildings)
                .after(crate::fog::hide_buildings)
                .run_if(in_state(Screen::Match))
                .run_if(resource_exists::<Effects>),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tgdata::{
        HealthCue, HealthTrigger, Id, ObjectEffects, Shape, Targets, Vfx, VfxCue, VfxTint, Weapon,
    };

    use tgsim::{Landing, Target};

    use super::follow::*;
    use super::notice::*;
    use super::place::*;
    use super::play::*;
    use super::tint::*;
    use super::*;
    use crate::ui::visible;

    fn cue(vfx: &str) -> VfxCue {
        VfxCue {
            vfx: Id::from(vfx),
            stop: false,
            at: "SELF".into(),
            location: None,
            attached: true,
            translation: [0.0; 3],
            orientation: [0.0, -1.0, 0.0],
            duration: None,
            delay: 0.0,
            every: 1,
            terrain: None,
            targets: Targets::default(),
            stop_on_destructed: true,
            chain: Vec::new(),
            chain_immediate: Vec::new(),
        }
    }

    fn health(trigger: HealthTrigger, health: f32, vfx: &str) -> HealthCue {
        HealthCue {
            health,
            trigger,
            cue: cue(vfx),
        }
    }

    fn ids(cues: &[VfxCue]) -> Vec<&str> {
        cues.iter().map(|c| c.vfx.as_str()).collect()
    }

    #[test]
    fn health_effects_play_on_crossing_their_thresholds() {
        let cues = [
            health(HealthTrigger::Below, 0.4, "smoke"),
            health(HealthTrigger::Below, 0.0, "debris"),
            health(HealthTrigger::Above, 0.5, "mended"),
        ];
        assert!(crossed(&cues, 1.0, 0.6).is_empty());
        assert_eq!(ids(&crossed(&cues, 0.6, 0.4)), ["smoke"]);
        assert_eq!(ids(&crossed(&cues, 1.0, 0.0)), ["smoke", "debris"]);
        // Below one already, it doesn't play again until it's back above.
        assert!(crossed(&cues, 0.4, 0.3).is_empty());
        assert_eq!(ids(&crossed(&cues, 0.3, 0.6)), ["mended"]);
        assert_eq!(ids(&crossed(&cues, 0.6, 0.3)), ["smoke"]);
    }

    #[test]
    fn relative_health_effects_play_on_big_changes_and_with_the_rest() {
        let cues = [
            health(HealthTrigger::Below, 0.5, "smoke"),
            health(HealthTrigger::Decrease, 0.3, "jolt"),
            health(HealthTrigger::Increase, 0.3, "patched"),
        ];
        assert_eq!(ids(&crossed(&cues, 1.0, 0.6)), ["jolt"]);
        assert!(crossed(&cues, 1.0, 0.8).is_empty());
        assert_eq!(ids(&crossed(&cues, 0.6, 0.5)), ["smoke", "jolt", "patched"]);
        assert_eq!(ids(&crossed(&cues, 0.2, 0.6)), ["patched"]);
    }

    #[test]
    fn a_sites_lasting_effects_end_with_it_and_work_on_it_puffs_dust() {
        let site = ObjectEffects {
            entered: vec![
                (Id::from("construction"), cue("scaffold")),
                (Id::from(AT_WORK), cue("dust")),
            ],
            left: vec![(Id::from("construction"), cue("teardown"))],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("bunker"), site)]));
        effects.vfx = Arc::new(HashMap::from([(Id::from("scaffold"), vfx(true, None))]));
        let mut seen = Seen {
            health: 0.1,
            ..Seen::default()
        };
        let me = Target::Building(0);
        let second = (1.0 / tgsim::TICK) as u64;
        // Healed in its first second, left alone for three, then finished.
        let looks = [
            (0, 0.1, true),
            (1, 0.2, true),
            (second, 0.2, true),
            (4 * second, 0.2, true),
            (5 * second, 1.0, false),
        ];
        let mut played = Vec::new();
        for (tick, health, site) in looks {
            let now = Now {
                states: if site {
                    vec!["construction"]
                } else {
                    Vec::new()
                },
                health,
                shot: None,
                weapon: None,
                target: None,
                finished: !site,
                site,
                held: Vec::new(),
            };
            effects.witness(me, "bunker", &mut seen, now, at(tick));
            let queued = std::mem::take(&mut effects.queued);
            let cues = queued
                .iter()
                .map(|(_, p)| (p.cue.vfx.to_string(), p.cue.stop));
            played.push(cues.collect::<Vec<_>>());
        }
        let s = |v: &str, stop| (v.to_string(), stop);
        assert_eq!(played[0], [s("scaffold", false)]);
        assert_eq!(played[2], [s("dust", false)]);
        assert_eq!(played[3], []);
        assert_eq!(played[4], [s("scaffold", true), s("teardown", false)]);
    }

    #[test]
    fn states_entered_and_left() {
        let (entered, left) = changes(&["structure_unpacking"], &["dead"]);
        assert_eq!((entered, left), (vec!["dead"], vec!["structure_unpacking"]));
        let (entered, left) = changes(&["dead"], &["dead"]);
        assert!(entered.is_empty() && left.is_empty());
    }

    #[test]
    fn a_frequency_plays_on_the_first_shot_and_every_nth_after() {
        let plays: Vec<u32> = (0..7).filter(|&n| on_shot(3, n)).collect();
        assert_eq!(plays, [0, 3, 6]);
        assert!((0..4).all(|n| on_shot(1, n)));
    }

    #[test]
    fn detail_thins_out_effects_that_arent_critical() {
        assert!(detail_allows(false, 1.0, 0.999));
        assert!(!detail_allows(false, 0.0, 0.0));
        assert!(detail_allows(false, 0.6, 0.5));
        assert!(!detail_allows(false, 0.6, 0.7));
        assert!(detail_allows(true, 0.0, 0.7));
    }

    #[test]
    fn effects_that_last_go_with_the_building_that_made_them() {
        let mut world = bevy::ecs::world::World::new();
        let (view, model) = (world.spawn_empty().id(), world.spawn_empty().id());
        // One that lasts goes with its building, attached to it or not.
        assert_eq!(tie(Some(view), None, true), Some(view));
        assert_eq!(tie(Some(view), Some(model), true), Some(view));
        // One with its own time is left to it, unless it's attached.
        assert_eq!(tie(Some(view), None, false), None);
        assert_eq!(tie(Some(view), Some(model), false), Some(model));
        // With no building still there, only what it's attached to.
        assert_eq!(tie(None, Some(model), true), Some(model));
        assert_eq!(tie(None, None, true), None);
    }

    #[test]
    fn pictures_on_the_ground_come_in_and_go_over_their_fade_times() {
        // In over 10 seconds, staying for good.
        assert_eq!(opacity([10.0, 1.0], 0.0, None), 0.0);
        assert_eq!(opacity([10.0, 1.0], 5.0, None), 0.5);
        assert_eq!(opacity([10.0, 1.0], 60.0, None), 1.0);
        // There at once for 15 seconds, then gone over 5 more.
        assert_eq!(opacity([0.0, 5.0], 15.0, Some(15.0)), 1.0);
        assert_eq!(opacity([0.0, 5.0], 17.5, Some(15.0)), 0.5);
        assert_eq!(opacity([0.0, 5.0], 21.0, Some(15.0)), 0.0);
        // Ended half way in, it goes from there.
        assert_eq!(opacity([10.0, 4.0], 6.0, Some(5.0)), 0.375);
        // With no time to go in, it's gone when its time's up.
        assert_eq!(opacity([0.0, 0.0], 3.0, Some(2.0)), 0.0);
        assert_eq!(opacity([0.0, 0.0], 3.0, None), 1.0);
        // Only what starts with the match skips coming in.
        assert_eq!(fade_in(10.0, 0, 0.0), 0.0);
        assert_eq!(fade_in(10.0, 0, 0.5), 10.0);
        assert_eq!(fade_in(10.0, 3, 0.0), 10.0);
    }

    #[test]
    fn terrain_filters_match_their_group() {
        let snow = Id::from("snow");
        assert!(terrain_allows(None, None));
        assert!(terrain_allows(Some(&snow), Some(&Id::from("Snow"))));
        assert!(!terrain_allows(Some(&snow), Some(&Id::from("dirt"))));
        assert!(!terrain_allows(Some(&snow), None));
    }

    fn vfx(permanent: bool, duration: Option<f32>) -> Vfx {
        Vfx {
            category: "VFXMODEL".into(),
            model: None,
            texture_dirs: Vec::new(),
            permanent,
            critical: false,
            duration,
            attached_scale: 0.0,
            never_discard: false,
            night_only: false,
            constant_screen_size: false,
            real_time: false,
            random_orientation: false,
            gyroscope: true,
            tint: None,
            decal: None,
        }
    }

    #[test]
    fn effects_last_their_duration_else_their_animation() {
        let flash = cue("flash");
        assert_eq!(lifetime(&vfx(false, None), &flash, Some(0.5)), Some(0.5));
        assert_eq!(lifetime(&vfx(false, None), &flash, None), Some(0.0));
        assert_eq!(
            lifetime(&vfx(false, Some(2.0)), &flash, Some(0.5)),
            Some(2.0)
        );
        assert_eq!(lifetime(&vfx(true, None), &flash, Some(0.5)), None);
        let timed = VfxCue {
            duration: Some(3.0),
            ..flash
        };
        assert_eq!(lifetime(&vfx(true, Some(2.0)), &timed, None), Some(3.0));
    }

    #[test]
    fn effects_play_on_a_things_point_at_their_own_size() {
        let root = Transform::from_xyz(10.0, 0.0, -5.0)
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::splat(2.0));
        let on = OnThing {
            point: Transform::from_xyz(0.0, 1.0, 1.0),
            offset: Vec3::ZERO,
            turn: Quat::IDENTITY,
        };
        let t = on.place(&root);
        assert!(t.translation.distance(Vec3::new(12.0, 2.0, -5.0)) < 1e-5);
        assert_eq!(t.scale, Vec3::ONE);
        assert_eq!(facing([0.0, -1.0, 0.0]), Vec3::Z);
    }

    #[test]
    fn a_turned_effect_keeps_its_turn_from_a_thing_that_turns() {
        let root = Transform::from_rotation(Quat::from_rotation_y(1.0));
        let on = OnThing {
            point: Transform::from_rotation(Quat::from_rotation_x(0.3)),
            offset: Vec3::ZERO,
            turn: Quat::from_rotation_z(0.2),
        };
        let was = on.place(&root).rotation;
        let now = Quat::from_rotation_y(2.0) * was;
        let on = on.turned(was, now);
        let same = |a: Quat, b: Quat| {
            [Vec3::X, Vec3::Y]
                .iter()
                .all(|&v| (a * v).distance(b * v) < 1e-5)
        };
        assert!(same(on.place(&root).rotation, now));
        assert!(!same(was, now));
        // Its thing turning turns it as far.
        let further = Quat::from_rotation_y(0.5);
        let root = Transform::from_rotation(further * root.rotation);
        assert!(same(on.place(&root).rotation, further * now));
    }

    #[test]
    fn models_stand_upright_along_their_heading() {
        // On a point lying on its side, turned a quarter turn about.
        use std::f32::consts::FRAC_PI_2;
        let lying = Quat::from_rotation_y(FRAC_PI_2) * Quat::from_rotation_z(FRAC_PI_2);
        let stood = upright(lying * Vec3::Z);
        assert!((stood * Vec3::Z).distance(Vec3::X) < 1e-5);
        assert!((stood * Vec3::Y).distance(Vec3::Y) < 1e-5);
        // Pitched with a heading up a slope, but not rolled.
        let up = Vec3::new(0.0, 1.0, 1.0).normalize();
        let stood = upright(up);
        assert!((stood * Vec3::Z).distance(up) < 1e-5);
        assert!((stood * Vec3::X).y.abs() < 1e-5);
        assert_eq!(upright(Vec3::Y), Quat::IDENTITY);
    }

    #[test]
    fn effects_for_the_night_show_only_while_it_is_night() {
        let mut app = App::new();
        app.insert_resource(effects(HashMap::new()))
            .add_systems(Update, show_by_night);
        let effect = || Effect {
            vfx: Id::from("lit_window"),
            by: None,
            holding: None,
            ends: None,
            discard: false,
        };
        let world = app.world_mut();
        let nightly = world.spawn((effect(), NightOnly, visible(false))).id();
        let always = world.spawn((effect(), Visibility::Inherited)).id();
        let shown = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        app.world_mut().resource_mut::<Effects>().set_night(true);
        app.update();
        assert_eq!(shown(&app, nightly), Visibility::Inherited);
        app.world_mut().resource_mut::<Effects>().set_night(false);
        app.update();
        assert_eq!(shown(&app, nightly), Visibility::Hidden);
        assert_eq!(shown(&app, always), Visibility::Inherited);
    }

    #[test]
    fn effects_on_things_show_with_them_and_for_the_night_only_at_night() {
        use Visibility::{Hidden, Inherited, Visible};
        assert_eq!(shown_with(Visible, false, false), Inherited);
        assert_eq!(shown_with(Hidden, false, true), Hidden);
        assert_eq!(shown_with(Inherited, true, true), Inherited);
        assert_eq!(shown_with(Visible, true, false), Hidden);
        assert_eq!(shown_with(Hidden, true, true), Hidden);
    }

    fn effects(objects: HashMap<Id, ObjectEffects>) -> Effects {
        Effects {
            vfx: Arc::default(),
            objects,
            operations: HashMap::new(),
            properties: HashMap::new(),
            bones: HashMap::new(),
            decals: DecalCache::default(),
            groups: Vec::new(),
            side: 0,
            queued: Vec::new(),
            shots: HashMap::new(),
            landed: None,
            operated: None,
            aim: 0.65,
            detail: 1.0,
            night: false,
            at_work: Some(2.5),
            rng: Random::new(1),
        }
    }

    fn at(tick: u64) -> When {
        When {
            at: tick as f64,
            tick,
            weapon: None,
            landed: None,
        }
    }

    fn gun(effects: tgdata::WeaponEffects, flight: bool) -> Weapon {
        Weapon {
            range: 10.0,
            min_range: 0.0,
            recharge: 1.0,
            area: 0.0,
            max_targets: 0,
            full_within: 0.0,
            edge_damage: 1.0,
            targets: Targets::default(),
            hits: Vec::new(),
            centred_on_user: false,
            animation: None,
            turret: false,
            turret_point: None,
            aimed_by: None,
            spawn_position: [0.0; 3],
            sounds: Vec::new(),
            effects,
            projectile: Some(Id::from("shell")),
            flight: flight.then_some(tgdata::Flight {
                kind: tgdata::FlightKind::Straight,
                speed: 50.0,
                life: 0.0,
            }),
            bombard: false,
            vorpal: false,
            spell: false,
            choice: Default::default(),
            shown: Default::default(),
        }
    }

    #[test]
    fn shots_queue_attacks_by_frequency_and_projectiles_on_landing() {
        let shell = ObjectEffects {
            destroyed: vec![cue("boom")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("shell"), shell)]));
        let mut weapon_effects = tgdata::WeaponEffects::default();
        weapon_effects.start.push(cue("muzzle"));
        weapon_effects.shot.push(VfxCue {
            every: 2,
            delay: 0.5,
            ..cue("tracer")
        });
        weapon_effects.per_target.push(cue("spark"));
        let me = Target::Unit(0);
        let mut seen = Seen {
            health: 1.0,
            ..Seen::default()
        };
        let shooting = |weapon: &Weapon, effects: &mut Effects, seen: &mut Seen, tick| {
            let now = Now {
                states: Vec::new(),
                health: 1.0,
                shot: Some((tick, 0)),
                weapon: Some(weapon),
                target: Some(Target::Unit(1)),
                finished: false,
                site: false,
                held: Vec::new(),
            };
            effects.witness(me, "tank", seen, now, at(tick));
        };
        // Fired directly, its per-target effects play with the shot.
        let direct = gun(weapon_effects.clone(), false);
        for tick in [1, 2, 3] {
            shooting(&direct, &mut effects, &mut seen, tick);
        }
        let queued = |effects: &Effects| -> Vec<(f64, String, Source)> {
            (effects.queued.iter())
                .map(|(t, p)| (*t, p.cue.vfx.to_string(), p.source))
                .collect()
        };
        let thing = Source::Thing(me);
        let q = |t: f64, v: &str, s| (t, v.to_string(), s);
        assert_eq!(
            queued(&effects),
            [
                q(1.0, "muzzle", thing),
                q(1.5, "tracer", thing),
                q(1.0, "spark", thing),
                q(2.0, "muzzle", thing),
                q(2.0, "spark", thing),
                q(3.0, "muzzle", thing),
                q(3.5, "tracer", thing),
                q(3.0, "spark", thing),
            ]
        );
        // A projectile's land with it, where it landed.
        effects.queued.clear();
        let shelled = gun(weapon_effects, true);
        shooting(&shelled, &mut effects, &mut seen, 4);
        assert_eq!(queued(&effects), [q(4.0, "muzzle", thing)]);
        let landing = Landing {
            tick: 9,
            from: me,
            weapon: 0,
            target: Some(Target::Unit(1)),
            at: [5.0, 6.0],
        };
        effects.land(&landing, &shelled, 9.0);
        let landed: Vec<_> = (effects.queued[1..].iter())
            .map(|(_, p)| (p.cue.vfx.as_str(), p.source, p.tick, p.landed))
            .collect();
        assert_eq!(
            landed,
            [
                ("boom", Source::Projectile(me), 9, Some([5.0, 6.0])),
                ("spark", thing, 9, None),
            ]
        );
    }

    #[test]
    fn entering_a_state_or_being_finished_plays_its_effects_once() {
        let depot = ObjectEffects {
            entered: vec![(Id::from("structure_unpacking"), cue("dust"))],
            constructed: vec![cue("lights")],
            destroyed: vec![cue("collapse")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("depot"), depot)]));
        let mut seen = Seen {
            health: 1.0,
            ..Seen::default()
        };
        let me = Target::Building(0);
        let states = [
            vec!["structure_unpacking"],
            vec!["structure_unpacking"],
            vec![],
            vec![],
            vec![DEAD],
        ];
        for states in states {
            let now = Now {
                finished: states.is_empty(),
                states,
                health: 1.0,
                shot: None,
                weapon: None,
                target: None,
                site: false,
                held: Vec::new(),
            };
            effects.witness(me, "depot", &mut seen, now, at(0));
        }
        let queued: Vec<&str> = (effects.queued.iter())
            .map(|(_, p)| p.cue.vfx.as_str())
            .collect();
        assert_eq!(queued, ["dust", "lights", "collapse"]);
    }

    #[test]
    fn a_property_shows_its_effects_on_what_holds_it_until_it_goes() {
        let mut effects = effects(HashMap::new());
        let radiation = Id::from("radiation");
        (effects.properties).insert(radiation.clone(), vec![cue("sparks")]);
        let me = Target::Unit(0);
        let mut seen = Seen {
            health: 1.0,
            ..Seen::default()
        };
        let mut witness = |effects: &mut Effects, held| {
            let now = Now {
                states: Vec::new(),
                health: 1.0,
                shot: None,
                weapon: None,
                target: None,
                finished: false,
                site: false,
                held,
            };
            effects.witness(me, "tank", &mut seen, now, at(0));
            (effects.queued.iter())
                .map(|(_, p)| {
                    (
                        p.cue.vfx.to_string(),
                        p.cue.stop,
                        p.target,
                        p.holding.clone(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let held = Some(radiation.clone());
        let start = ("sparks".to_string(), false, Some(me), held.clone());
        let stop = ("sparks".to_string(), true, Some(me), held);
        assert_eq!(
            witness(&mut effects, vec![radiation.clone()]),
            std::slice::from_ref(&start)
        );
        assert_eq!(witness(&mut effects, vec![radiation]), [start]);
        // Gone before its effects began, they never do.
        assert_eq!(witness(&mut effects, vec![]), [stop]);
    }

    #[test]
    fn the_strongest_tint_shows_and_one_it_hid_carries_on_after() {
        let tint = |colour, rise, priority| VfxTint {
            colour,
            max: 1.0,
            rise,
            fall: 0.0,
            repeating: false,
            priority,
        };
        let (weak, strong, other) = (Id::from("weak"), Id::from("strong"), Id::from("other"));
        let weak = (&weak, tint([0.0; 3], 2.0, 0), false);
        let strong = (&strong, tint([0.5; 3], 0.0, 1), true);
        let other = (&other, tint([0.25; 3], 0.0, 0), false);
        let mut t = Tinting::default();
        assert_eq!(t.colour(&[weak], [0.0, 50.0]), Some([1.0; 3]));
        // The strong one runs on the wall clock, the weak on game time.
        assert_eq!(t.colour(&[weak, strong], [1.0, 60.0]), Some([0.5; 3]));
        assert_eq!(t.colour(&[weak], [1.5, 70.0]), Some([0.25; 3]));
        assert_eq!(t.colour(&[], [2.0, 80.0]), None);
        assert!(t.since.is_empty());
        // Of equal ones, the first.
        assert_eq!(t.colour(&[other, weak], [3.0, 90.0]), Some([0.25; 3]));
    }

    #[test]
    fn being_created_plays_a_templates_effects_for_it() {
        let wreck = ObjectEffects {
            created: vec![cue("smoke")],
            ..ObjectEffects::default()
        };
        let mut effects = effects(HashMap::from([(Id::from("wreck"), wreck)]));
        let scenery = Source::Scenery(Entity::PLACEHOLDER);
        effects.create(scenery, "wreck", at(0));
        effects.create(scenery, "rock", at(0));
        let queued: Vec<(&str, Source)> = (effects.queued.iter())
            .map(|(_, p)| (p.cue.vfx.as_str(), p.source))
            .collect();
        assert_eq!(queued, [("smoke", scenery)]);
    }

    #[test]
    fn hits_land_on_the_edge_facing_the_attacker() {
        let circle = Shape::Circle { radius: 2.0 };
        let rect = Shape::Rect {
            width: 2.0,
            height: 6.0,
        };
        let north = Vec2::Y;
        assert_eq!(edge(circle, north, Vec2::X), 2.0);
        assert_eq!(edge(Shape::Point, north, Vec2::X), 0.0);
        // Across a target facing north, half its width; along, half its
        // length; corner-on, as far as the nearer side.
        assert!((edge(rect, north, Vec2::X) - 1.0).abs() < 1e-5);
        assert!((edge(rect, north, -Vec2::Y) - 3.0).abs() < 1e-5);
        let diagonal = Vec2::ONE.normalize();
        assert!((edge(rect, north, diagonal) - 2f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn hits_land_at_the_attackers_aim_height_within_the_target() {
        let tank = [0.0, 3.0];
        // A tank's hit on a tall building, at two thirds of the tank.
        assert!((hit_height(0.65, Some(tank), [0.0, 12.0]) - 1.95).abs() < 1e-5);
        // A tall ship's on a low jeep, on the jeep's top.
        assert_eq!(hit_height(0.65, Some([0.0, 20.0]), [0.0, 1.5]), 1.5);
        assert!((hit_height(0.5, None, [0.0, 4.0]) - 2.0).abs() < 1e-5);
    }

    #[test]
    fn a_duration_stretches_the_models_animation_to_it() {
        let stretched = VfxCue {
            duration: Some(4.0),
            ..cue("smoke")
        };
        assert_eq!(rate(&vfx(false, None), &stretched, Some(2.0)), Some(0.5));
        assert_eq!(
            rate(&vfx(false, Some(1.0)), &cue("smoke"), Some(2.0)),
            Some(2.0)
        );
        assert_eq!(rate(&vfx(false, None), &cue("smoke"), Some(2.0)), None);
        assert_eq!(rate(&vfx(false, None), &stretched, None), None);
    }
}
