//! Where on a thing an effect goes, and which way it faces.

use std::collections::HashMap;

use bevy::prelude::*;
use tgdata::Id;
use tgmap::Ground;
use tgrender::models::Models;
use tgrender::terrain::{map_point, world};

use super::play::{Placed, edge, hit_height};
use super::{OnThing, Play, Source, Subject};

/// What placing an effect needs to know.
pub(super) struct Place<'a, 'b> {
    pub(super) placed: &'b dyn Fn(Subject) -> Option<Placed<'a>>,
    pub(super) ground: &'b Ground,
    pub(super) bones: &'b HashMap<Id, String>,
    /// How far up an attacker a hit lands.
    pub(super) aim: f32,
}

impl Place<'_, '_> {
    /// Where a command's effect goes, and for an attached one, what it
    /// follows and where on it; `None` if what it's on is gone, or it's
    /// attached to a point the model doesn't have.
    pub(super) fn spot(
        &self,
        play: &Play,
        models: &mut Models,
    ) -> Option<(Transform, Option<(Entity, OnThing)>)> {
        let Play {
            cue,
            source,
            target,
            landed,
            ..
        } = play;
        let at = cue.at.as_str();
        let offset = world(cue.translation[0], cue.translation[1], cue.translation[2]);
        let turn = Quat::from_rotation_arc(Vec3::Z, facing(cue.orientation));
        let on_ground = |[x, y]: [f32; 2]| {
            let mut t = Transform::from_translation(world(x, y, self.ground.height_at([x, y])));
            t.translation += offset;
            t.rotation = turn;
            t
        };
        let (attacker, own) = match *source {
            Source::Thing(t) => (Some(Subject::Thing(t)), Some(Subject::Thing(t))),
            Source::Projectile(t) => (Some(Subject::Thing(t)), None),
            Source::Scenery(e) => (None, Some(Subject::Scenery(e))),
            Source::Operation => (None, None),
        };
        match at {
            "MAP_CENTER" => {
                let c = self.ground.size() / 2.0;
                return Some((on_ground([c, c]), None));
            }
            // A projectile is where it landed, on its own.
            "SELF" | "SOURCE_GROUND" if own.is_none() => {
                return Some((on_ground((*landed)?), None));
            }
            // An operation's others are on the ground at its centre.
            _ if *source == Source::Operation => return Some((on_ground((*landed)?), None)),
            _ => {}
        }
        let subject = match at {
            "TARGET_THING" | "TARGET_GROUND" | "TARGET_HIT_POSITION" => Subject::Thing((*target)?),
            "SOURCE_ABILITY_POSITION" => attacker?,
            _ => own?,
        };
        let thing = (self.placed)(subject)?;
        let here = map_point(thing.root.translation);
        let extent = |models: &mut Models, p: &Placed| {
            let path = models.model_path(p.thing, p.packing)?.to_string();
            let [bottom, top] = models.extent(&path)?;
            let scale = p.root.scale.y;
            Some([bottom * scale, top * scale])
        };
        match at {
            "SOURCE_GROUND" | "TARGET_GROUND" => Some((on_ground(here), None)),
            "TARGET_HIT_POSITION" => {
                if !thing.living {
                    return None;
                }
                let target_extent = extent(models, &thing)?;
                let by = attacker.and_then(self.placed);
                let across = match &by {
                    Some(a) => {
                        let dir = -a.facing;
                        Vec2::from(here) + dir * edge(thing.footprint, thing.facing, dir)
                    }
                    None => Vec2::from(here),
                };
                let height = match by {
                    Some(a) => hit_height(self.aim, extent(models, &a), target_extent),
                    None => hit_height(self.aim, None, target_extent),
                };
                let base = thing.root.translation.y;
                let mut t = Transform::from_translation(world(across.x, across.y, base + height));
                t.translation += offset;
                t.rotation = turn;
                Some((t, None))
            }
            "SOURCE_ABILITY_POSITION" => {
                let base = thing.root.translation.y;
                let weapons = thing.fighter.map_or(&[][..], |f| &f.weapons[..]);
                let weapon = weapons.get(play.weapon.unwrap_or(0));
                let turret = weapon.and_then(|w| w.turret_point.as_ref());
                if let Some(point) = turret.and_then(|id| self.point(models, &thing, id.as_str())) {
                    let on = OnThing {
                        point,
                        offset,
                        turn,
                    };
                    return Some((on.place(&thing.root), None));
                }
                let on_thing = |at: Vec3| {
                    let at = thing.root.translation + thing.root.rotation * (at + offset);
                    Transform::from_translation(at).with_rotation(thing.root.rotation * turn)
                };
                match weapon {
                    Some(w) if w.projectile.is_some() => {
                        let [x, y, z] = w.spawn_position;
                        Some((on_thing(world(x, y, z)), None))
                    }
                    Some(_) => {
                        let ahead = thing.facing;
                        let reach = edge(thing.footprint, thing.facing, ahead);
                        let front = Vec2::from(here) + ahead * reach;
                        let height = thing.fighter.map_or(0.0, |f| f.attack_height);
                        let at = world(front.x, front.y, base + height);
                        let mut t = Transform::from_translation(at + offset);
                        t.rotation = turn;
                        Some((t, None))
                    }
                    None => Some((on_thing(Vec3::ZERO), None)),
                }
            }
            _ => {
                let point = match &cue.location {
                    Some(id) => match (self.point(models, &thing, id), cue.attached) {
                        (Some(p), _) => p,
                        // Detached, it plays as though it had no location.
                        (None, false) => Transform::IDENTITY,
                        (None, true) => return None,
                    },
                    None => Transform::IDENTITY,
                };
                let on = OnThing {
                    point,
                    offset,
                    turn,
                };
                let transform = on.place(&thing.root);
                let attach = cue.attached.then_some((thing.anchor, on));
                Some((transform, attach))
            }
        }
    }

    /// Where the point a command names by `id` is in the model `thing` is
    /// drawn with now, if it has it.
    fn point(&self, models: &mut Models, thing: &Placed, id: &str) -> Option<Transform> {
        let name = self.bones.get(&Id::from(id)).map_or(id, |n| n);
        let path = models.model_path(thing.thing, thing.packing)?.to_string();
        models.point(&path, name)
    }
}

/// The way a command's orientation, in the game's axes, points an effect:
/// the default, the -y its models face along, leaves it as it is.
pub(super) fn facing([x, y, z]: [f32; 3]) -> Vec3 {
    // The game's -y is Bevy's +z.
    world(x, y, z).try_normalize().unwrap_or(Vec3::Z)
}

/// The turn that points a model along `heading` the right way up; none
/// for a heading straight up or down, which says nothing of which way.
pub(super) fn upright(heading: Vec3) -> Quat {
    match heading.cross(Vec3::Y).length_squared() > 1e-6 {
        // A model's forward is +z, where looking is along -z.
        true => Transform::IDENTITY.looking_to(-heading, Vec3::Y).rotation,
        false => Quat::IDENTITY,
    }
}
