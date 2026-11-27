//! Keeping effects with what they're on, by night only where they're
//! for the night, and fading them in and out.

use bevy::prelude::*;
use tgdata::Decal;
use tgrender::camera::RtsCamera;
use tgrender::decals::{DecalMaterial, decal_colour};

use crate::Sim;
use crate::ui::visible;
use crate::views::BuildingView;

use super::play::on_screen;
use super::{Effect, Effects, Follow, NightOnly, NotInFilm, Stopped, Subject, Tied};

/// A picture laid on the ground that takes time to show or to go.
#[derive(Component)]
pub(super) struct Fading {
    pub(super) decal: Decal,
    pub(super) made: f64,
    /// Seconds it lasts before it starts to go, for one that does.
    pub(super) life: Option<f32>,
    /// It's shown in full and stays so, with nothing left to change.
    pub(super) settled: bool,
}

/// Seconds a picture laid on the ground takes to show, given its own time
/// to: none for one whose effect starts the moment the match does.
pub(super) fn fade_in(own: f32, tick: u64, delay: f32) -> f32 {
    match tick == 0 && delay <= 0.0 {
        true => 0.0,
        false => own,
    }
}

/// How much of a picture laid on the ground shows `age` seconds after it
/// was made: it comes in over `fade`'s first time, and once `life` is up
/// goes over its second, from however far it had come.
pub(super) fn opacity([rise, fall]: [f32; 2], age: f32, life: Option<f32>) -> f32 {
    let risen = |age: f32| match rise > 0.0 {
        true => (age / rise).clamp(0.0, 1.0),
        false => 1.0,
    };
    match life.filter(|l| age > *l) {
        Some(life) if fall > 0.0 => risen(life) * (1.0 - (age - life) / fall).max(0.0),
        Some(_) => 0.0,
        None => risen(age),
    }
}

/// What [`follow`] reads and moves of an effect.
pub(super) type Playing<'a> = (
    Entity,
    &'a mut Effect,
    Option<&'a Follow>,
    Option<&'a Tied>,
    Option<&'a mut Fading>,
    Has<Stopped>,
    Has<NightOnly>,
    Has<NotInFilm>,
    &'a mut Transform,
    &'a mut Visibility,
);

/// Moves attached effects with what they're on, and ends those whose time
/// is up, whose thing has gone, or that went off screen.
pub(super) fn follow(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    mut effects: Query<Playing>,
    lit: Res<Effects>,
    anchors: Query<(&Transform, &Visibility, Option<&BuildingView>), Without<Effect>>,
    cameras: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    let now = time.elapsed_secs_f64();
    let camera = cameras.iter().find(|(c, _)| c.is_active);
    // A building's hidden once it's gone, though its view stays.
    let there = |anchor: Entity| {
        (anchors.get(anchor).ok())
            .filter(|(_, v, building)| !(building.is_some() && **v == Visibility::Hidden))
    };
    // Fog hides a building that's still there, so that's asked of the sim.
    let stays = |anchor: Entity| {
        (anchors.get(anchor).ok())
            .filter(|(_, _, b)| b.is_none_or(|v| !sim.0.buildings[v.building].gone()))
    };
    for (
        e,
        mut effect,
        follow,
        tied,
        fading,
        stopped,
        nightly,
        unfilmed,
        mut transform,
        mut visibility,
    ) in &mut effects
    {
        let with = |shown: &Visibility| shown_with(*shown, lit.shows(nightly, unfilmed));
        if effect.ends.is_some_and(|t| t <= now) {
            commands.entity(e).despawn();
            continue;
        }
        let orphaned = tied.is_some_and(|t| stays(t.0).is_none());
        if stopped || orphaned {
            // A picture with a time to go in goes over it, from however
            // much of it shows.
            match fading.filter(|f| f.decal.fade[1] > 0.0) {
                Some(mut f) => {
                    (f.life, f.settled) = (Some((now - f.made) as f32), false);
                    effect.ends = Some(now + f64::from(f.decal.fade[1]));
                    commands.entity(e).remove::<(Tied, Stopped)>();
                }
                None => commands.entity(e).despawn(),
            }
            continue;
        }
        if let Some((_, shown, _)) = tied.and_then(|t| stays(t.0)) {
            visibility.set_if_neq(with(shown));
        }
        // Scenery's effects go when it's cleared, and are hidden with it
        // on ground never seen.
        if let Some(Subject::Scenery(s)) = effect.by {
            let Ok((_, shown, _)) = anchors.get(s) else {
                commands.entity(e).despawn();
                continue;
            };
            visibility.set_if_neq(with(shown));
        }
        if let Some(f) = follow {
            let Some((root, _, building)) = there(f.anchor) else {
                commands.entity(e).despawn();
                continue;
            };
            // What's on a site stays on the ground as its model's sunk.
            let standing = building.filter(|v| sim.0.buildings[v.building].constructing);
            let root = match standing {
                Some(v) => &Transform {
                    translation: v.placed,
                    ..*root
                },
                None => root,
            };
            let placed = f.on.place(root);
            transform.translation = placed.translation;
            if !f.gyroscope {
                transform.rotation = placed.rotation;
            }
        }
        if effect.discard && !on_screen(camera, transform.translation) {
            trace!("{}: off screen", effect.vfx);
            commands.entity(e).despawn();
        }
    }
}

/// The effects shown only at times.
type AtTimes = Or<(With<NightOnly>, With<NotInFilm>)>;

/// What [`show_by_time`] looks at of an effect shown only at times.
type Gated = (
    &'static Effect,
    &'static mut Visibility,
    Has<Tied>,
    Has<NightOnly>,
    Has<NotInFilm>,
);

/// Shows effects for the night only while it's night, and those kept out
/// of scripted scenes only while none plays. Those shown with what
/// they're on are left to `follow`.
pub(super) fn show_by_time(effects: Res<Effects>, mut gated: Query<Gated, AtTimes>) {
    for (effect, mut visibility, tied, nightly, unfilmed) in &mut gated {
        if tied || matches!(effect.by, Some(Subject::Scenery(_))) {
            continue;
        }
        visibility.set_if_neq(visible(effects.shows(nightly, unfilmed)));
    }
}

/// How an effect shows with what it's on, `shown` as that is: hidden with
/// it, and while it isn't to show at this time of day or in this scene.
pub(super) fn shown_with(shown: Visibility, now: bool) -> Visibility {
    match shown {
        Visibility::Hidden => Visibility::Hidden,
        _ if !now => Visibility::Hidden,
        _ => Visibility::Inherited,
    }
}

/// Shows pictures laid on the ground as far as they've come in or gone.
pub(super) fn fade(
    time: Res<Time>,
    mut laid: Query<(&mut Fading, &MeshMaterial3d<DecalMaterial>)>,
    mut materials: ResMut<Assets<DecalMaterial>>,
) {
    let now = time.elapsed_secs_f64();
    for (mut fading, material) in &mut laid {
        if fading.settled {
            continue;
        }
        let age = (now - fading.made) as f32;
        let shown = opacity(fading.decal.fade, age, fading.life);
        if fading.life.is_none() && age >= fading.decal.fade[0] {
            fading.settled = true;
        }
        let colour = decal_colour(&fading.decal, shown);
        if materials
            .get(&material.0)
            .is_some_and(|m| m.base.base_color != colour)
            && let Some(mut m) = materials.get_mut(&material.0)
        {
            m.base.base_color = colour;
        }
    }
}
