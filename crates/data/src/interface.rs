//! How the interface looks and behaves, such as its commands and cursors.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{GameData, Id, beside, defs, frames, not_null, scoped, strings::localized, ui_path};

/// An order the player can give, shown as one of the action buttons.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// E.g. `move`, `kill`, `stop`.
    pub ids: Id,
    pub name: String,
    /// What it does, and why it can't be given when it can't.
    pub tooltip: Option<String>,
    pub disabled: Option<String>,
    /// Archive path of its icon.
    pub icon: Option<String>,
    /// Which button it takes, numbered across then down.
    pub slot: Option<usize>,
    /// Whether, that button taken, it takes the last free one rather than
    /// the first.
    pub high_importance: bool,
    /// The [`CursorLook`] shown while it waits for a place or target.
    pub cursor: Option<Id>,
    /// The effect marking where it was given, from [`GameData::vfx`].
    pub marker: Option<Id>,
    /// Whether each of a group goes to its own place, keeping its place
    /// relative to the rest, rather than all to the one place.
    pub group_offset: bool,
}

/// A mouse cursor's frames, which loop.
#[derive(Debug, Clone, PartialEq)]
pub struct CursorLook {
    /// Archive paths, in order.
    pub frames: Vec<String>,
    /// The pixel that points, from the top left.
    pub hotspot: [u16; 2],
    /// Seconds one loop through the frames takes.
    pub period: f32,
}

/// How a formation shows on its button.
#[derive(Debug, Clone, PartialEq)]
pub struct FormationLook {
    pub name: String,
    /// Archive path of its icon.
    pub icon: Option<String>,
}

/// How the supply border is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct SupplyBorder {
    pub colour: [f32; 3],
    /// How wide it is, in world units.
    pub width: f32,
    /// The archive path of the image drawn along it.
    pub texture: Option<String>,
    /// The side, in world units, of the squares the map is cut into,
    /// which decide where each of its loops starts.
    pub block: f32,
}

impl GameData {
    /// The archive path of the image on each organization template's
    /// banner, by template id.
    pub fn banner_icons(&self) -> HashMap<Id, String> {
        self.by_template(|t| ui_path(t.view.as_ref()?.banner_icon.as_deref()?))
    }

    /// The archive path of the image marking each template's objects on
    /// the minimap, by template id.
    pub fn minimap_icons(&self) -> HashMap<Id, String> {
        self.by_template(|t| ui_path(t.view.as_ref()?.minimap_icon.as_deref()?))
    }

    /// The minimap's colours.
    pub fn minimap(&self) -> Option<defs::Minimap> {
        let files = self.parse_tgis(|n| n == "minimap/minimap.tgi");
        let files = scoped(&files);
        files.iter().find_map(|f| f.read("Minimap").next())
    }

    /// How the supply border is drawn; green and thin where the settings
    /// don't say.
    pub fn supply_border(&self) -> SupplyBorder {
        let vars = self.avars();
        SupplyBorder {
            colour: (vars.iter())
                .find_map(|v| v.view_zone_friendly_supply_color)
                .unwrap_or([0.0, 1.0, 0.0]),
            width: (vars.iter())
                .find_map(|v| v.view_zone_friendly_supply_width)
                .unwrap_or(0.15),
            texture: (vars.iter()).find_map(|v| v.view_zone_friendly_supply_texture.clone()),
            block: (vars.iter())
                .find_map(|v| v.terrain_block_size)
                .unwrap_or(8.0),
        }
    }

    /// What colours a figure or option that's no good, such as a shortfall.
    pub fn invalid_colour(&self) -> Option<[f32; 3]> {
        self.interface_setting(|v| v.ui_color_option_invalid)
    }

    /// What colours the key for something where its name is shown.
    pub fn highlight_colour(&self) -> Option<[f32; 3]> {
        self.interface_setting(|v| v.ui_color_highlight)
    }

    /// One of the interface's settings.
    pub(crate) fn interface_setting<T>(&self, of: impl Fn(defs::UVars) -> Option<T>) -> Option<T> {
        self.uvars().iter().cloned().find_map(of)
    }

    /// The interface's settings, the game's own before the engine's.
    pub(crate) fn uvars(&self) -> Arc<Vec<defs::UVars>> {
        self.kept("uvars", || {
            // Not in the load lists.
            let files = self.parse_named(["UVars_AA.tgi", "UVars.tgi"]);
            let files = scoped(&files);
            files.iter().flat_map(|f| f.read("Vars")).collect()
        })
    }

    /// The orders the player can give, in the order the files list them.
    pub fn commands(&self) -> Vec<Command> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| {
            n.starts_with("game/actor_commands") || n.starts_with("game/actorcommands/")
        });
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::ActorCommand>("ActorCommand"))
            .filter_map(|c| {
                let ids = c.ids?;
                Some(Command {
                    name: localized(&strings, c.name.as_deref()).unwrap_or(ids.to_string()),
                    ids,
                    tooltip: localized(&strings, c.tooltip.as_deref()).filter(|t| !t.is_empty()),
                    disabled: localized(&strings, c.description_disabled.as_deref())
                        .filter(|t| !t.is_empty()),
                    icon: c.icon.as_deref().and_then(ui_path),
                    slot: c.preferred_position.and_then(|p| usize::try_from(p).ok()),
                    high_importance: c.high_importance,
                    cursor: not_null(c.cursor_ids),
                    marker: c.command_vfx_ids_location,
                    group_offset: c.apply_group_offset.unwrap_or(true),
                })
            })
            .collect()
    }

    /// The mouse cursors whose frames are all there, by id.
    pub fn cursors(&self) -> HashMap<Id, CursorLook> {
        let files = self.parse_tgis(|n| n.starts_with("cursors/"));
        let files = scoped(&files);
        let mut out = HashMap::new();
        for scope in &files {
            for block in scope.root.blocks_of("Cursor") {
                let Some(c) = scope.deserialize::<defs::Cursor>(block) else {
                    continue;
                };
                let (Some(ids), Some(pattern)) = (c.ids, c.frame_filename) else {
                    continue;
                };
                let frames = frames(&beside(scope.file, &pattern), |f| self.exists(f));
                if frames.is_empty() {
                    continue;
                }
                let [x, y] = c.hotspot_offset.unwrap_or_default();
                out.insert(
                    ids,
                    CursorLook {
                        frames,
                        hotspot: [x as u16, y as u16],
                        period: c.loop_length_ms.unwrap_or(1000.0) / 1000.0,
                    },
                );
            }
        }
        out
    }

    /// How each formation shows on its button, by formation id.
    pub fn formation_looks(&self) -> HashMap<Id, FormationLook> {
        let strings = self.strings();
        let files = self.parse_tgis(|n| n.starts_with("organizations/"));
        let files = scoped(&files);
        let mut out = HashMap::new();
        for scope in &files {
            let dir = scope.file.rsplit_once('/').map_or("", |(d, _)| d);
            for block in scope.root.blocks_of("Formation") {
                let Some(f) = scope.deserialize::<defs::Formation>(block) else {
                    continue;
                };
                let Some(ids) = f.ids else {
                    continue;
                };
                let icon = f.icon.as_deref().map(|i| match i.strip_prefix('/') {
                    Some(abs) => abs.to_string(),
                    None => format!("{dir}/{i}"),
                });
                let icon = icon.filter(|i| self.exists(i));
                let name = localized(&strings, f.name.as_deref()).unwrap_or(ids.to_string());
                out.insert(ids, FormationLook { name, icon });
            }
        }
        out
    }
}
