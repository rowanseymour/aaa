//! A map as the editor and the game work with it, built from the stored form
//! that [`tgck::Map`] reads and the definitions in [`aaa_data`].

mod buildings;
mod ground;
pub mod pathing;
mod units;

pub use aaa_data::{Damage, Fighter, Id, Shape, Targets, Weapon};
pub use buildings::Building;
pub use ground::{Ground, Tilt};
pub use pathing::{Domain, Obstacle, Pathing};
pub use tgck::map::Relation;
pub use units::{Division, Formation, Member, Player, Recruits, Regiment, Team, divisions, owner};

use aaa_data::GameData;
use tgck::map::{Sun, Terrain, Thing};

pub struct Map {
    /// The file name without its extension, e.g. `(2v2)Berlin`.
    pub title: String,
    pub ground: Ground,
    pub terrain: Terrain,
    pub things: Vec<Thing>,
    /// Which organization members' records depend on.
    pub world_version: u32,
    pub sun: Sun,
    pub players: Vec<Player>,
    pub teams: Vec<Team>,
    pub regiments: Vec<Regiment>,
    pub buildings: Vec<Building>,
    pub pathing: Pathing,
}

impl Map {
    /// Loads the map at an archive path, as [`GameData::find_map`] gives it.
    pub fn load(game: &mut GameData, name: &str) -> Result<Map, String> {
        let mut map = game.open_map(name)?;
        let err = |e: tgck::Error| format!("{name}: {e}");
        let hm = map.height_map().map_err(err)?;
        let mountain = map.mountain_height_map().map_err(err)?;
        let terrain = map.terrain().map_err(err)?;
        let things = map.things().map_err(err)?;
        let world_version = map.world_version().map_err(err)?;
        let bodies = game.bodies();
        // Movers don't block the ground they start on.
        let obstacles: Vec<(u32, Obstacle)> = things
            .iter()
            .filter_map(|t| {
                let body = bodies.get(&Id::from(t.template.as_str()))?;
                let shape = body.blocking.filter(|_| body.speed.is_none())?;
                let [x, y, _] = t.position;
                let obstacle = Obstacle {
                    position: [x, y],
                    facing: units::facing(t),
                    shape,
                };
                Some((t.id, obstacle))
            })
            .collect();
        let pathing = Pathing::new(
            &terrain,
            &game.terrain_variations(),
            obstacles.iter().map(|(_, o)| o.clone()),
        );
        let fighters = game.fighters();
        let (players, teams) = units::sides(
            &map.kingdoms().map_err(err)?,
            &map.teams().map_err(err)?,
            &game.kingdom_colors(),
        );
        Ok(Map {
            title: aaa_data::map_title(name).to_string(),
            ground: Ground::new(&hm, &mountain),
            regiments: units::regiments(
                &things,
                &bodies,
                &fighters,
                &game.layouts(),
                &game.operational_ranges(),
            ),
            buildings: buildings::buildings(
                &things,
                &bodies,
                &fighters,
                &game.city_zones(),
                &obstacles.into_iter().collect(),
            ),
            players,
            teams,
            pathing,
            terrain,
            things,
            world_version,
            sun: map.sun().map_err(err)?,
        })
    }
}

/// The point `right` and `ahead` of `origin` when facing `facing`, a unit
/// vector.
pub fn from_local(origin: [f32; 2], [fx, fy]: [f32; 2], [right, ahead]: [f32; 2]) -> [f32; 2] {
    let [x, y] = origin;
    [x + right * fy + ahead * fx, y - right * fx + ahead * fy]
}

/// How far `point` is to the right of `origin` and ahead of it when facing
/// `facing`, a unit vector; the inverse of [`from_local`].
pub fn to_local(origin: [f32; 2], [fx, fy]: [f32; 2], point: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (point[0] - origin[0], point[1] - origin[1]);
    [dx * fy - dy * fx, dx * fx + dy * fy]
}
