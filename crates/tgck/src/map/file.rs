//! A whole map, every section decoded, that writes itself back.

use std::fs;
use std::io::{Read, Seek};
use std::path::Path;

use super::{
    HeightMap, IdManager, Kingdoms, Map, MapHeader, MiscData, Objectives, RestrictionGroups, Sun,
    Team, Terrain, Thing, Triggers, Updatables, World, WorldRules, misc, players, write_things,
};
use crate::container::{self, Section};
use crate::{Error, Result};

/// Every section of a map, decoded. [`MapFile::to_bytes`] writes them back
/// in the order and with the compression they were read with, so a map
/// that's loaded and saved unchanged holds the same section payloads.
#[derive(Debug, Clone)]
pub struct MapFile {
    pub header: MapHeader,
    /// The map resource's data version (MAP).
    pub data_version: u32,
    /// Its version decides the layout of several other sections.
    pub world: World,
    pub rules: WorldRules,
    pub terrain: Terrain,
    pub height_map: HeightMap,
    pub mountain_height_map: HeightMap,
    /// Whether the world was saved mid-cutscene; None exactly before world
    /// version 10.
    pub cutscene_mode: Option<bool>,
    pub ids: IdManager,
    pub restriction_groups: RestrictionGroups,
    pub teams: Vec<Option<Team>>,
    pub objectives: Objectives,
    pub kingdoms: Kingdoms,
    pub things: Vec<Thing>,
    /// None if the map is older than the sections.
    pub used_required_objects: Option<Vec<Thing>>,
    pub triggers: Triggers,
    pub sun: Sun,
    pub updatables: Updatables,
    pub misc_data: MiscData,
    /// The string table, in .tgi text syntax.
    pub strings: String,
    /// The sections in file order, for their names and compression.
    sections: Vec<Section>,
}

impl MapFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Map::open(path)?.load()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        Ok(fs::write(path, self.to_bytes()?)?)
    }

    /// The map as a `.rmp` file.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let payloads = self
            .sections
            .iter()
            .map(|s| self.section_bytes(&s.name))
            .collect::<Result<Vec<_>>>()?;
        let pairs: Vec<_> = self
            .sections
            .iter()
            .zip(payloads.iter().map(Vec::as_slice))
            .collect();
        let mut out = Vec::new();
        container::write(&mut out, &pairs)?;
        Ok(out)
    }

    /// A section's payload, encoded from what's decoded.
    pub fn section_bytes(&self, name: &str) -> Result<Vec<u8>> {
        let world_version = self.world.version;
        let used_required = || {
            let objects = self.used_required_objects.as_deref().ok_or_else(|| {
                Error::Unwritable(format!("{name} without used required objects"))
            })?;
            write_things(objects)
        };
        Ok(match name {
            "MAP" => self.data_version.to_le_bytes().to_vec(),
            "MAP_HEADER" => self.header.to_bytes()?,
            "GWORLD" => self.world.to_bytes()?,
            "RULES" => self.rules.to_bytes()?,
            "TERRAIN" => self.terrain.to_bytes()?,
            "HEIGHT_MAP" => self.height_map.to_bytes(),
            "MOUNTAIN_HEIGHT_MAP" => {
                if self.cutscene_mode.is_some() != (world_version >= 10) {
                    return Err(Error::Unwritable(
                        "cutscene mode must be set exactly from world version 10".into(),
                    ));
                }
                super::write_mountain_height_map(&self.mountain_height_map, self.cutscene_mode)
            }
            "IDMGR" => self.ids.to_bytes(),
            "RESTRICTION_GROUPS" => self.restriction_groups.to_bytes()?,
            "TEAMS" => players::write_teams(&self.teams)?,
            "OBJECTIVES" => self.objectives.to_bytes()?,
            "KINGDOMS" => self.kingdoms.to_bytes(world_version)?,
            "DYNAMIC_THINGS" => write_things(&self.things)?.1,
            "DYNAMIC_THINGS_INDEX" => write_things(&self.things)?.0,
            "USED_REQUIRED_OBJECTS" => used_required()?.1,
            "USED_REQUIRED_OBJECTS_INDEX" => used_required()?.0,
            "TRIGGERS" => self.triggers.to_bytes()?,
            "DENIZEN_GROUPS" => misc::write_denizen_groups(),
            "SUN" => self.sun.to_bytes()?,
            "UPDATABLES" => self.updatables.to_bytes(world_version)?,
            "MISC_DATA" => self.misc_data.to_bytes(world_version)?,
            "MAPSTRINGS" => super::write_strings(&self.strings)?,
            _ => return Err(Error::Unwritable(format!("section {name:?}"))),
        })
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }
}

impl<R: Read + Seek> Map<R> {
    /// Decodes every section; a section that isn't known is an error, so
    /// nothing is lost when the map is written back.
    pub fn load(&mut self) -> Result<MapFile> {
        let sections = self.sections().to_vec();
        if let Some(s) = sections.iter().find(|s| !KNOWN.contains(&s.name.as_str())) {
            return Err(Error::Corrupt(format!(
                "section {:?} isn't decoded",
                s.name
            )));
        }
        self.denizen_groups()?;
        Ok(MapFile {
            header: self.header()?,
            data_version: self.data_version()?,
            world: self.world()?,
            rules: self.rules()?,
            terrain: self.terrain()?,
            height_map: self.height_map()?,
            mountain_height_map: self.mountain_height_map()?,
            cutscene_mode: self.cutscene_mode()?,
            ids: self.ids()?,
            restriction_groups: self.restriction_groups()?,
            teams: self.teams()?,
            objectives: self.objectives()?,
            kingdoms: self.kingdoms()?,
            things: self.things()?,
            used_required_objects: self.used_required_objects()?,
            triggers: self.triggers()?,
            sun: self.sun()?,
            updatables: self.updatables()?,
            misc_data: self.misc_data()?,
            strings: self.strings()?,
            sections,
        })
    }
}

/// The sections [`MapFile`] decodes.
const KNOWN: [&str; 22] = [
    "MAP",
    "MAP_HEADER",
    "GWORLD",
    "RULES",
    "TERRAIN",
    "HEIGHT_MAP",
    "MOUNTAIN_HEIGHT_MAP",
    "IDMGR",
    "RESTRICTION_GROUPS",
    "TEAMS",
    "OBJECTIVES",
    "KINGDOMS",
    "DYNAMIC_THINGS",
    "DYNAMIC_THINGS_INDEX",
    "USED_REQUIRED_OBJECTS",
    "USED_REQUIRED_OBJECTS_INDEX",
    "TRIGGERS",
    "DENIZEN_GROUPS",
    "SUN",
    "UPDATABLES",
    "MISC_DATA",
    "MAPSTRINGS",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::tests::{
        height_section, sun_section, terrain_section, thing_record, things_sections,
    };
    use crate::map::{Objective, SCENERY, header, misc, players, triggers};
    use crate::tests::build_container;
    use std::io;

    const WORLD: u32 = 28;

    /// Every section a map has, encoded from small made-up values, as
    /// (name, payload, compressed).
    fn sections() -> Vec<(&'static str, Vec<u8>, bool)> {
        let mut mountain = height_section(3, 0);
        mountain.push(0);
        let (index, things) =
            things_sections(&[thing_record(SCENERY, "rock", 5, [1.0, 2.0, 0.0], &[])]);
        let (used_index, used) = things_sections(&[]);
        let objectives = Objectives {
            objectives: vec![Objective {
                name: "objective_0".into(),
                text: "objective_0".into(),
                primary: true,
            }],
            next_number: 1,
        };
        let terrain = terrain_section(7, &[&[0; 8], &[1, 0, 0, 0, 1, 0, 0, 0], &[0; 8], &[0; 8]]);
        vec![
            ("MAP", vec![0; 4], false),
            (
                "MAP_HEADER",
                header::tests::header(7, 13).to_bytes().unwrap(),
                true,
            ),
            (
                "GWORLD",
                players::tests::world(WORLD).to_bytes().unwrap(),
                true,
            ),
            (
                "RULES",
                header::tests::header(7, 13).rules.to_bytes().unwrap(),
                true,
            ),
            ("TERRAIN", terrain, true),
            ("HEIGHT_MAP", height_section(3, 4), true),
            ("MOUNTAIN_HEIGHT_MAP", mountain, true),
            (
                "IDMGR",
                IdManager {
                    next_slot: 1,
                    generations: vec![0],
                }
                .to_bytes(),
                true,
            ),
            ("RESTRICTION_GROUPS", vec![0; 12], true),
            ("TEAMS", players::write_teams(&[]).unwrap(), true),
            ("OBJECTIVES", objectives.to_bytes().unwrap(), true),
            ("KINGDOMS", vec![0; 8], true),
            ("USED_REQUIRED_OBJECTS", used, true),
            ("USED_REQUIRED_OBJECTS_INDEX", used_index, true),
            ("DYNAMIC_THINGS", things, true),
            ("DYNAMIC_THINGS_INDEX", index, true),
            (
                "TRIGGERS",
                triggers::tests::triggers(5).to_bytes().unwrap(),
                true,
            ),
            ("DENIZEN_GROUPS", misc::write_denizen_groups(), true),
            ("SUN", sun_section(2), true),
            ("UPDATABLES", updatables(), true),
            (
                "MISC_DATA",
                misc::tests::misc(WORLD).to_bytes(WORLD).unwrap(),
                true,
            ),
            ("MAPSTRINGS", b"[strings]\n".to_vec(), true),
        ]
    }

    fn updatables() -> Vec<u8> {
        let queue = |slots| crate::map::UpdateQueue {
            current_slot: 0,
            slots: vec![Vec::new(); slots],
        };
        Updatables {
            count: 0,
            delayed: [queue(1), queue(1)],
            periodic: Some([queue(1), queue(1), queue(1)]),
        }
        .to_bytes(WORLD)
        .unwrap()
    }

    fn file(sections: &[(&str, Vec<u8>, bool)]) -> Vec<u8> {
        let refs: Vec<_> = sections
            .iter()
            .map(|(n, p, c)| (*n, p.as_slice(), *c))
            .collect();
        build_container(&refs)
    }

    #[test]
    fn writes_back_what_it_reads() {
        let sections = sections();
        let bytes = file(&sections);
        let map = Map::new(io::Cursor::new(&bytes)).unwrap().load().unwrap();
        assert_eq!(map.things[0].template, "rock");
        assert_eq!(map.cutscene_mode, Some(false));
        assert_eq!(map.to_bytes().unwrap(), bytes, "laid out as read");
    }

    #[test]
    fn writes_edits() {
        let bytes = file(&sections());
        let mut map = Map::new(io::Cursor::new(&bytes)).unwrap().load().unwrap();
        map.things[0].position = [7.0, 8.0, 0.0];
        map.strings = "[strings]\nname_5 = \"Rock\"\n".into();
        let saved = map.to_bytes().unwrap();
        let again = Map::new(io::Cursor::new(&saved)).unwrap().load().unwrap();
        assert_eq!(again.things[0].position, [7.0, 8.0, 0.0]);
        assert_eq!(again.strings, map.strings);
        assert_eq!(again.to_bytes().unwrap(), saved);
    }

    #[test]
    fn refuses_what_it_cant_write_back() {
        let mut sections = sections();
        sections.push(("STATIC_THINGS", vec![0; 8], true));
        let mut map = Map::new(io::Cursor::new(file(&sections))).unwrap();
        assert!(map.load().is_err(), "a section it doesn't decode");

        sections.pop();
        let bytes = file(&sections);
        let mut map = Map::new(io::Cursor::new(&bytes)).unwrap().load().unwrap();
        map.cutscene_mode = None;
        assert!(map.to_bytes().is_err(), "the world version has the flag");
    }
}
