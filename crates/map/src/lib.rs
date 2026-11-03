//! A map as the editor and the game work with it, built from the stored form
//! that [`tgck::Map`] reads and the definitions in [`aaa_data`].

mod ground;

pub use ground::Ground;

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
}

impl Map {
    /// Loads the map at an archive path, as [`GameData::find_map`] gives it.
    pub fn load(game: &mut GameData, name: &str) -> Result<Map, String> {
        let mut map = game.open_map(name)?;
        let err = |e: tgck::Error| format!("{name}: {e}");
        let hm = map.height_map().map_err(err)?;
        let mountain = map.mountain_height_map().map_err(err)?;
        Ok(Map {
            title: aaa_data::map_title(name).to_string(),
            ground: Ground::new(&hm, &mountain),
            terrain: map.terrain().map_err(err)?,
            things: map.things().map_err(err)?,
            world_version: map.world_version().map_err(err)?,
            sun: map.sun().map_err(err)?,
        })
    }
}
