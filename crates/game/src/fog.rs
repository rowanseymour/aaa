//! The fog of war as the person playing sees it: the ground darkened where
//! their side doesn't see, other sides' things hidden there, and buildings
//! once seen left as they were last seen until seen again.

use std::collections::HashSet;

use aaa_data::Id;
use aaa_render::GroundFog;
use aaa_render::blend::{self, GroundMaterial};
use aaa_sim::{Seen, World};
use bevy::prelude::*;

use crate::{BuildingView, Loaded, Local, Sim, UnitView, keys};

/// How bright ground explored but not seen now looks, of its full
/// brightness.
const EXPLORED_LIGHT: f32 = 0.5;
/// How bright ground never seen looks.
const UNEXPLORED_LIGHT: f32 = 0.0;

/// How bright ground looks under the fog, of its full brightness.
pub fn brightness(seen: Seen) -> f32 {
    match seen {
        Seen::Visible => 1.0,
        Seen::Explored => EXPLORED_LIGHT,
        Seen::Unexplored => UNEXPLORED_LIGHT,
    }
}

/// How a building shows to the person playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// As it is.
    Shown,
    /// As it was last seen.
    Remembered,
    Hidden,
}

/// What the person playing sees of the match, taken afresh each tick.
#[derive(Resource, Default)]
pub struct Fog {
    /// Whether the cheat has lifted the fog from the screen.
    pub lifted: bool,
    /// How each tile stands, row-major from the south-west corner; empty
    /// while nothing's hidden.
    tiles: Vec<Seen>,
    side: usize,
    /// Whether each of the sim's units shows.
    units: Vec<bool>,
    /// How each of the sim's buildings shows.
    buildings: Vec<Look>,
    /// Which buildings the person's side has a picture of.
    known: Vec<bool>,
    /// The templates of what every side has explored from the start.
    revealed: HashSet<Id>,
    /// The tick and lifting it was taken for.
    taken: Option<(u64, bool)>,
    /// Whether it was taken afresh this frame.
    fresh: bool,
}

impl Fog {
    /// How the tile under `at` stands, all visible while nothing's hidden.
    pub fn at(&self, at: [f32; 2]) -> Seen {
        if self.tiles.is_empty() {
            return Seen::Visible;
        }
        let index = aaa_map::Tile::at(at).index(self.side);
        index.map_or(Seen::Unexplored, |i| self.tiles[i])
    }

    pub fn shows_unit(&self, u: usize) -> bool {
        self.units.get(u).copied().unwrap_or(true)
    }

    /// Whether any member of a regiment shows.
    pub fn shows_regiment(&self, sim: &World, r: usize) -> bool {
        sim.regiments[r].units.iter().any(|&u| self.shows_unit(u))
    }

    pub fn building(&self, b: usize) -> Look {
        self.buildings.get(b).copied().unwrap_or(Look::Shown)
    }

    /// How much light the ground under each tile keeps, row-major from
    /// the south-west corner: the share of its brightness it looks, in
    /// linear light.
    fn light(&self) -> impl Iterator<Item = u8> + '_ {
        let linear = |s: Seen| {
            let l = brightness(s);
            (Color::srgb(l, l, l).to_linear().red * 255.0).round() as u8
        };
        let shades = [Seen::Unexplored, Seen::Explored, Seen::Visible].map(linear);
        self.tiles.iter().map(move |&s| shades[s as usize])
    }

    /// How each tile stands, row-major from the south-west corner, and
    /// how many tiles a side; none while nothing's hidden.
    pub fn tiles(&self) -> (&[Seen], usize) {
        (&self.tiles, self.side)
    }

    /// Whether the view was taken afresh this frame.
    pub fn fresh(&self) -> bool {
        self.fresh
    }
}

/// Starts the fog for a match.
pub fn setup(mut commands: Commands, loaded: Res<Loaded>, sim: Res<Sim>) {
    let revealed = loaded.game.revealed();
    let mut fog = Fog {
        revealed,
        ..default()
    };
    fog.known = (sim.0.buildings.iter())
        .map(|b| fog.revealed.contains(&b.template))
        .collect();
    commands.insert_resource(fog);
}

/// Takes the person's view afresh once a tick, and as the cheat lifts or
/// lays the fog.
pub fn look(mut fog: ResMut<Fog>, sim: Res<Sim>, local: Res<Local>) {
    let sim = &sim.0;
    fog.fresh = fog.taken != Some((sim.ticks, fog.lifted));
    if !fog.fresh {
        return;
    }
    fog.taken = Some((sim.ticks, fog.lifted));
    let fog = &mut *fog;
    let player = local.0.filter(|_| !fog.lifted);
    let hidden = player.filter(|_| sim.fog.setting != aaa_map::FogSetting::None);
    let Some(p) = hidden else {
        fog.tiles.clear();
        fog.units.clear();
        fog.buildings.clear();
        return;
    };
    fog.tiles = sim.view(p);
    fog.side = (fog.tiles.len() as f64).sqrt() as usize;
    fog.units = sim.sees_units(p);
    fog.known.resize(sim.buildings.len(), false);
    fog.buildings = (0..sim.buildings.len())
        .map(|b| {
            if sim.sees_building(p, b) {
                fog.known[b] = true;
                return Look::Shown;
            }
            match fog.known[b] && sim.building_seen(p, b) != Seen::Unexplored {
                true => Look::Remembered,
                false => Look::Hidden,
            }
        })
        .collect();
}

/// Draws the ground darkened where the person's side doesn't see.
pub fn shade_ground(
    fog: Res<Fog>,
    ground: Option<Res<GroundFog>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GroundMaterial>>,
) {
    let Some(ground) = ground.filter(|_| fog.fresh()) else {
        return;
    };
    let Some(mut material) = materials.get_mut(&ground.0) else {
        return;
    };
    // A new image rather than the old one changed, which the material
    // wouldn't take up.
    let image = match fog.tiles.is_empty() {
        true => blend::fog_image(1, vec![255]),
        false => blend::fog_image(fog.side, fog.light().collect()),
    };
    material.extension.fog = images.add(image);
}

/// Hides the buildings the person has no picture of.
pub fn hide_buildings(fog: Res<Fog>, mut views: Query<(&BuildingView, &mut Visibility)>) {
    for (view, mut visibility) in &mut views {
        if fog.building(view.building) == Look::Hidden {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

/// Which map objects are the map's own scenery: those the sim has no
/// thing for.
pub type IsScenery = (
    Without<UnitView>,
    Without<BuildingView>,
    Without<crate::TrailerView>,
    // Placeholders stay hidden as they were spawned.
    Without<aaa_render::objects::Marker>,
);

/// Hides the map's own scenery on ground never seen.
pub fn hide_scenery(
    fog: Res<Fog>,
    mut things: Query<(&aaa_render::MapObject, &mut Visibility), IsScenery>,
) {
    if !fog.fresh() {
        return;
    }
    for (object, mut visibility) in &mut things {
        let [x, y, _] = object.0.position;
        let shown = match fog.at([x, y]) {
            Seen::Unexplored => Visibility::Hidden,
            _ => Visibility::Inherited,
        };
        visibility.set_if_neq(shown);
    }
}

/// Lifts or lays the fog over the screen on the developer key, where
/// cheats are allowed.
pub fn cheat(keys: Res<ButtonInput<KeyCode>>, bindings: Res<keys::Bindings>, mut fog: ResMut<Fog>) {
    if bindings.pressed(&keys, "Fog") {
        fog.lifted = !fog.lifted;
    }
}
