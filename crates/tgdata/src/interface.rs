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
    /// The one shown instead over a place or target it can't be given.
    pub invalid_cursor: Option<Id>,
    /// The effect marking an order's target, from [`GameData::vfx`].
    pub marker: Option<Id>,
    /// Whether each of a group goes to its own place, keeping its place
    /// relative to the rest, rather than all to the one place.
    pub group_offset: bool,
    /// Whether it's offered only with one thing selected.
    pub alone: bool,
}

/// An action button that isn't a command, such as the one opening the
/// team commands, as the layouts describe it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ActionButton {
    pub name: Option<String>,
    pub tooltip: Option<String>,
    /// Which button it takes, numbered across then down.
    pub slot: Option<usize>,
    /// Archive path of its icon.
    pub icon: Option<String>,
}

/// The template the layouts' action buttons are written with.
const BUTTON: &str = "ActionButtonInfo";

/// The block among `block`'s descendants describing the action button
/// `name`.
fn button_block<'a>(block: &'a tgi::Block, name: &str) -> Option<&'a tgi::Block> {
    (block.blocks()).find_map(|b| {
        let info = b
            .attr("Template")
            .is_some_and(|t| t.eq_ignore_ascii_case(BUTTON));
        match info && b.kind().is_some_and(|k| k.eq_ignore_ascii_case(name)) {
            true => Some(b),
            false => button_block(b, name),
        }
    })
}

/// A command block as the button it shows; none without an id.
fn command(strings: &crate::Strings, c: defs::ActorCommand) -> Option<Command> {
    let ids = c.ids?;
    Some(Command {
        name: localized(strings, c.name.as_deref()).unwrap_or(ids.to_string()),
        ids,
        tooltip: localized(strings, c.tooltip.as_deref()).filter(|t| !t.is_empty()),
        disabled: localized(strings, c.description_disabled.as_deref()).filter(|t| !t.is_empty()),
        icon: c.icon.as_deref().and_then(ui_path),
        slot: c.preferred_position.and_then(|p| usize::try_from(p).ok()),
        high_importance: c.high_importance,
        cursor: not_null(c.cursor_ids),
        invalid_cursor: not_null(c.cursor_invalid_ids),
        marker: c.command_vfx_ids_actor,
        group_offset: c.apply_group_offset.unwrap_or(true),
        alone: c.non_group_command,
    })
}

/// The images the connection dialog draws, by archive path.
#[derive(Debug, Clone, Default)]
pub struct ConnectionLook {
    /// What marks the host's slot.
    pub host_icon: Option<String>,
    /// What marks a player's slot by their status, by the status's name
    /// as the layout has it, e.g. `out_of_sync`.
    pub status_icons: HashMap<String, String>,
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
    /// How the properties an organization in it holds show beside the
    /// organization, those that have an icon.
    pub properties: Vec<crate::PropertyLook>,
    /// The game's words for each change its properties make, as its
    /// button's tooltip lists them.
    pub modifiers: Vec<String>,
}

/// Seconds between redraws of each of the minimap's layers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MinimapPeriods {
    pub fog: f32,
    pub supply: f32,
    pub objects: f32,
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
        let minimaps = self.read_all(|n| n == "minimap/minimap.tgi", "Minimap");
        minimaps.into_iter().next()
    }

    /// How the supply border is drawn; green and thin where the settings
    /// don't say.
    pub fn supply_border(&self) -> SupplyBorder {
        SupplyBorder {
            colour: self
                .avar(|v| v.view_zone_friendly_supply_color)
                .unwrap_or([0.0, 1.0, 0.0]),
            width: self
                .avar(|v| v.view_zone_friendly_supply_width)
                .unwrap_or(0.15),
            texture: self.avar(|v| v.view_zone_friendly_supply_texture.clone()),
            block: self.avar(|v| v.terrain_block_size).unwrap_or(8.0),
        }
    }

    /// Seconds between the minimap's redraws of its fog, supply border
    /// and marks, a second each where the settings don't say.
    pub fn minimap_periods(&self) -> MinimapPeriods {
        let period = |of: fn(&defs::AVars) -> Option<f32>| self.avar(of).unwrap_or(1.0);
        MinimapPeriods {
            fog: period(|v| v.minimap_period_update_fog),
            supply: period(|v| v.minimap_period_update_supply),
            objects: period(|v| v.minimap_period_update_objects),
        }
    }

    /// Each letterbox bar's share of the screen's height, none where the
    /// settings don't say.
    pub fn letterbox_height_ratio(&self) -> f32 {
        self.avar(|v| v.letterbox_height_ratio).unwrap_or(0.0)
    }

    /// Seconds banner text takes to fade out once its time is up.
    pub fn banner_fade_seconds(&self) -> f32 {
        self.avar(|v| v.banner_fade_duration_ms).unwrap_or(1000.0) / 1000.0
    }

    /// Whether the elapsed game time is shown as the options start.
    pub fn shows_elapsed_game_time(&self) -> bool {
        self.uvar(|v| v.view_show_elapsed_game_time)
            .unwrap_or(false)
    }

    /// Whether the strip of control groups is open as the first match
    /// starts.
    pub fn group_strip_open(&self) -> bool {
        self.uvar(|v| v.ui_group_floater_is_open).unwrap_or(false)
    }

    /// What colours a figure or option that's no good, such as a shortfall.
    pub fn invalid_colour(&self) -> Option<[f32; 3]> {
        self.uvar(|v| v.ui_color_option_invalid)
    }

    /// What colours the key for something where its name is shown.
    pub fn highlight_colour(&self) -> Option<[f32; 3]> {
        self.uvar(|v| v.ui_color_highlight)
    }

    /// What tints the icon of a property its holder's side holds, such as
    /// a technology.
    pub fn technology_property_colour(&self) -> Option<[f32; 3]> {
        self.uvar(|v| v.ui_color_property_technology)
    }

    /// What tints the icon of a property a building hands its side.
    pub fn support_property_colour(&self) -> Option<[f32; 3]> {
        self.uvar(|v| v.ui_color_property_support)
    }

    /// What tints the icon of a property an ability gave, when the side
    /// that used it is the holder's or an ally.
    pub fn friendly_property_colour(&self) -> Option<[f32; 3]> {
        self.uvar(|v| v.ui_color_property_temporary_friendly)
    }

    /// What tints the icon of a property an ability gave, when the side
    /// that used it is an enemy of the holder's.
    pub fn enemy_property_colour(&self) -> Option<[f32; 3]> {
        self.uvar(|v| v.ui_color_property_temporary_enemy)
    }

    /// What colours a tooltip's figure that's above, below and at its
    /// plain value, in that order.
    pub fn modifier_colours(&self) -> [Option<[f32; 3]>; 3] {
        [
            self.uvar(|v| v.ui_color_modifier_positive),
            self.uvar(|v| v.ui_color_modifier_negative),
            self.uvar(|v| v.ui_color_modifier_neutral),
        ]
    }

    /// The images this project's own interface draws in place of the
    /// game's, by the game's image's lower-case path from the archive root:
    /// from the `[IconStandIn]` blocks of the `.tgi` files under `Modern/`,
    /// the first for an image winning.
    pub fn icon_stand_ins(&self) -> Arc<HashMap<String, String>> {
        self.kept("icon_stand_ins", || {
            self.modern_textures("IconStandIn", |b: defs::IconStandIn| {
                let image = b.image?.trim().trim_start_matches('/').to_ascii_lowercase();
                Some((image, b.texture?))
            })
        })
    }

    /// The image this project's own interface draws for the game's at
    /// `path`: its stand-in, if it has one, else the game's.
    pub fn modern_icon(&self, path: &str) -> String {
        let key = path.trim_start_matches('/').to_ascii_lowercase();
        (self.icon_stand_ins().get(&key).cloned()).unwrap_or_else(|| path.to_string())
    }

    /// How this project's own interface looks for the game: each field
    /// from the first `[ModernInterface]` block, in a `.tgi` file under
    /// `Modern/`, that has it, `null` saying there's none; the rest of the
    /// backdrop from the block its map is from.
    pub fn modern_interface(&self) -> defs::ModernInterface {
        let files = self.modern_files();
        let mut look = defs::ModernInterface::default();
        // Words fields given, `None` within where the block said `null`.
        let (mut title, mut logo, mut backdrop) = (None, None, None);
        let given = |v: Option<String>| {
            let v = v?.trim().to_string();
            Some((!v.eq_ignore_ascii_case("null")).then_some(v))
        };
        for file in scoped(&files) {
            for block in file.read::<defs::ModernInterface>("ModernInterface") {
                title = title.or_else(|| given(block.title));
                logo = logo.or_else(|| {
                    let path = given(block.logo)?;
                    Some(path.map(|p| beside(file.file, &p)))
                });
                backdrop = backdrop.or_else(|| {
                    let map = given(block.backdrop_map)?;
                    let rest = (
                        block.backdrop_focus,
                        block.backdrop_troops,
                        block.backdrop_plane,
                        block.backdrop_regiment,
                    );
                    Some(map.map(|m| (m, rest)))
                });
                look.tint = look.tint.or(block.tint);
                look.light = look.light.or(block.light);
                look.accent = look.accent.or(block.accent);
            }
        }
        look.title = title.flatten();
        look.logo = logo.flatten();
        if let Some((map, (focus, troops, plane, regiment))) = backdrop.flatten() {
            look.backdrop_map = Some(map);
            look.backdrop_focus = focus;
            (look.backdrop_troops, look.backdrop_plane) = (troops, plane);
            look.backdrop_regiment = regiment;
        }
        look
    }

    /// One of the interface's fonts, by its `ids` as a label names it:
    /// `[Font]` in a `.tgi` file in `Fonts/`, whose first `[CharacterSet]`
    /// gives the face.
    ///
    /// A face named rather than given as a file is one of Windows' own,
    /// which the data doesn't hold: it's stood in for by the file a
    /// `[SystemFont]` block of the same `name` gives, `bold_font` where the
    /// set is bold and it has one, else `font`.
    pub fn font(&self, ids: &str) -> Option<FontLook> {
        let files = self.font_files();
        let word = |b: &tgi::Block, key: &str| {
            let value = b.field(key)?.value.as_deref()?;
            Some(value.split_whitespace().next()?.to_string())
        };
        let number = |b: &tgi::Block, key: &str| word(b, key)?.parse::<f32>().ok();
        let blocks = |kind| (files.iter()).flat_map(move |(_, f)| f.root.blocks_of(kind));
        let font =
            blocks("Font").find(|f| word(f, "ids").is_some_and(|i| i.eq_ignore_ascii_case(ids)))?;
        let set = font.blocks_of("CharacterSet").next();
        let flag = |key| {
            set.and_then(|s| word(s, key))
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        };
        let bold = flag("bold");
        let face = |key| -> Option<Typeface> {
            let value = set?.field(key)?.value.as_deref()?;
            // Up to any comment after it.
            let named = value.split(";;").next()?.trim();
            let file = named.trim_start_matches('/');
            if self.exists(file) {
                return Some(Typeface {
                    file: Some(file.to_string()),
                    system: false,
                });
            }
            // A path is the data's, if it's missing; a bare name the system's.
            if named.contains('/') {
                return None;
            }
            let system = blocks("SystemFont").find(|f| {
                f.field("name")
                    .and_then(|n| n.value.as_deref())
                    .is_some_and(|n| n.trim().eq_ignore_ascii_case(named))
            });
            let file = system.and_then(|system| {
                let file = (bold.then(|| system.field("bold_font")).flatten())
                    .or_else(|| system.field("font"))?;
                let file = file.value.as_deref()?.trim().trim_start_matches('/');
                self.exists(file).then(|| file.to_string())
            });
            Some(Typeface { file, system: true })
        };
        let backup = set.and_then(|s| {
            Some(Backup {
                face: face("backup_font"),
                size: number(s, "backup_font_size")?,
                below: number(s, "use_backup_font_when_smaller_than")?,
            })
        });
        Some(FontLook {
            face: face("font"),
            size: number(font, "size").unwrap_or(12.0),
            bold,
            outline: flag("outline"),
            shadow: flag("shadow")
                .then(|| set.and_then(|s| number(s, "shadow_opacity")).unwrap_or(1.0)),
            backup,
        })
    }

    /// The `.tgi` files in `Fonts/` itself.
    fn font_files(&self) -> Vec<(String, Arc<tgi::Parsed>)> {
        let names = self.entry_names(|n| {
            n.strip_prefix("fonts/")
                .is_some_and(|f| f.ends_with(".tgi") && !f.contains('/'))
        });
        self.parse_named(names.iter().map(String::as_str))
    }

    /// The font, by its `ids`, that a tooltip's title is in, where the
    /// settings name one.
    pub fn tooltip_title_font(&self) -> Option<String> {
        self.avar(|v| v.text_font_ids_tool_tip_name.clone())
    }

    /// The images the connection dialog draws, as its layout names them.
    pub fn connection_look(&self) -> ConnectionLook {
        // Not in the load lists.
        let files = self.parse_named(["UI/Game/connection_status_dialog.tgi"]);
        let dialog = (files.iter())
            .flat_map(|(_, f)| f.root.blocks_of("ConnectionStatusDialog"))
            .next();
        let Some(dialog) = dialog else {
            return ConnectionLook::default();
        };
        let status_icons = (dialog.fields())
            .filter_map(|f| {
                let status = f.key.to_ascii_lowercase();
                let status = status.strip_prefix("status_icon_")?.to_string();
                Some((status, crate::ui_path(f.value.as_deref()?)?))
            })
            .collect();
        let host_icon = (dialog.blocks_of("ClientSlotWidget"))
            .flat_map(|slot| slot.blocks_of("HostIcon"))
            .flat_map(|icon| icon.blocks_of("Decal"))
            .find_map(|decal| crate::ui_path(decal.field("texture")?.value.as_deref()?));
        ConnectionLook {
            host_icon,
            status_icons,
        }
    }

    /// What the options dialog calls the player's LAN nickname: the label
    /// its LAN panel shows over the box it's typed in.
    pub fn nickname_label(&self) -> Option<String> {
        // Not in the load lists.
        let files = self.parse_named(["UI/Shared/options_dialog.tgi"]);
        let strings = self.strings();
        let panel = (files.iter()).find_map(|(_, f)| descendant(&f.root, "LANOptionsPanel"))?;
        let text = labelled(panel)?;
        localized(&strings, Some(text))
    }

    /// The interface setting that `of` finds, as the last file to set it
    /// sets it.
    pub(crate) fn uvar<T>(&self, of: impl Fn(&defs::UVars) -> Option<T>) -> Option<T> {
        self.uvars().iter().find_map(of)
    }

    /// The interface's settings, the last read first.
    pub(crate) fn uvars(&self) -> Arc<Vec<defs::UVars>> {
        self.kept("uvars", || self.settings("UVars"))
    }

    /// The orders the player can give, in the order the files list them.
    pub fn commands(&self) -> Vec<Command> {
        let filter =
            |n: &str| n.starts_with("game/actor_commands") || n.starts_with("game/actorcommands/");
        self.read_commands(filter, "ActorCommand")
    }

    /// The requests the player can make of allies, such as `team_attack`,
    /// in the order the files list them.
    pub fn team_commands(&self) -> Vec<Command> {
        self.read_commands(|n| n.starts_with("game/team_commands"), "TeamCommand")
    }

    fn read_commands(&self, filter: impl Fn(&str) -> bool, kind: &str) -> Vec<Command> {
        let strings = self.strings();
        (self
            .read_all::<defs::ActorCommand>(filter, kind)
            .into_iter())
        .filter_map(|c| command(&strings, c))
        .collect()
    }

    /// The action button the in-match layouts describe by `name`, such as
    /// `GoToTeamCommands`, for one that isn't a command.
    pub fn action_button(&self, name: &str) -> Option<ActionButton> {
        let strings = self.strings();
        // The interface's layouts aren't in the load lists.
        let names = self.entry_names(|n| n.starts_with("ui/game/") && n.ends_with(".tgi"));
        let files = self.parse_named(names.iter().map(String::as_str));
        let block = files
            .iter()
            .rev()
            .find_map(|(_, f)| button_block(&f.root, name))?;
        let said = |key| localized(&strings, block.value(key)).filter(|t| !t.is_empty());
        Some(ActionButton {
            name: said("name"),
            tooltip: said("tooltip"),
            slot: block.parse_value("index"),
            icon: block.value("texture").and_then(ui_path),
        })
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
                // The only repeats left are those read into a first
                // without a name ([`crate::cut_short`]).
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
    pub fn formation_looks(&self) -> Arc<HashMap<Id, FormationLook>> {
        self.kept("formation_looks", || self.read_formation_looks())
    }

    fn read_formation_looks(&self) -> HashMap<Id, FormationLook> {
        let strings = self.strings();
        let paces = self.paces(&strings);
        let definitions = self.definitions();
        let files = self.parse_tgis(|n| n.starts_with("organizations/"));
        let files = scoped(&files);
        let mut out = HashMap::new();
        for scope in &files {
            for block in scope.root.blocks_of("Formation") {
                let Some(f) = scope.deserialize::<defs::Formation>(block) else {
                    continue;
                };
                let Some(ids) = f.ids else {
                    continue;
                };
                let icon = f.icon.as_deref().map(|i| beside(scope.file, i));
                let icon = icon.filter(|i| self.exists(i));
                let name = localized(&strings, f.name.as_deref()).unwrap_or(ids.to_string());
                let held = scope.deserialize::<defs::FormationProperties>(block);
                let held = held.map_or_else(Vec::new, |h| h.property);
                let look = |p| self.property_look(&strings, &paces, &definitions, scope.file, p);
                let modifiers = (held.iter())
                    .flat_map(|p| &p.modifier)
                    .filter_map(|m| crate::properties::line(&strings, &paces, m));
                let look = FormationLook {
                    name,
                    icon,
                    modifiers: modifiers.collect(),
                    properties: held.iter().filter_map(look).collect(),
                };
                out.insert(ids, look);
            }
        }
        out
    }
}

/// The first block of `kind` under `block`, however deep.
fn descendant<'a>(block: &'a tgi::Block, kind: &str) -> Option<&'a tgi::Block> {
    (block.blocks()).find_map(|b| match b.kind() == Some(kind) {
        true => Some(b),
        false => descendant(b, kind),
    })
}

/// The first text under `block`, however deep.
fn labelled(block: &tgi::Block) -> Option<&str> {
    block
        .value("text")
        .or_else(|| block.blocks().find_map(labelled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nickname_is_labelled_by_the_lan_panel_s_first_text() {
        let parsed = tgi::parse(
            r##"
[OptionsDialog]
{
    [GamePlayPage]
    {
        [Child]
        {
            text = "#other"
        }
        [LANOptionsPanel Inherit=OptionsRightColumnPanel]
        {
            [View]
            {
                [Child Template=SharedMediumLeftLabelVE]
                {
                    text = "#nickname"
                }
            }
            [NetworkLanNicknameEditbox]
            {
                text = "#box"
            }
        }
    }
}
"##,
        );
        let panel = descendant(&parsed.root, "LANOptionsPanel").unwrap();
        assert_eq!(labelled(panel), Some("#nickname"));
        assert!(descendant(&parsed.root, "Elsewhere").is_none());
    }

    #[test]
    fn an_action_button_is_found_by_name_among_the_layout_s_blocks() {
        let parsed = tgi::parse(
            "[ActionButtons Inherit=PanelWidget]\n{\n\t[GoToTeamCommands]\n\t{\n\t}\n\
             \t[GoToTeamCommands Template = ActionButtonInfo]\n\t{\n\t\tindex = 8\n\t}\n}\n",
        );
        let block = button_block(&parsed.root, "gototeamcommands").unwrap();
        assert_eq!(block.parse_value::<usize>("index"), Some(8));
        assert!(button_block(&parsed.root, "ActionButtons").is_none());
    }
}

/// One of the interface's fonts.
#[derive(Debug, Clone, PartialEq)]
pub struct FontLook {
    /// Its face, where it names one.
    pub face: Option<Typeface>,
    /// Its size as Windows gives it: points of a 96 dots-per-inch screen
    /// for a face the data holds, pixels for one of the system's.
    pub size: f32,
    pub bold: bool,
    /// Whether its glyphs are outlined.
    pub outline: bool,
    /// How opaque the shadow behind its glyphs is, where it has one.
    pub shadow: Option<f32>,
    /// The font it's drawn in instead when it would be drawn too small.
    pub backup: Option<Backup>,
}

/// The face a font's glyphs are drawn in.
#[derive(Debug, Clone, PartialEq)]
pub struct Typeface {
    /// The archive path of its file, or of the one standing in for it,
    /// where there's one.
    pub file: Option<String>,
    /// Whether it's a system face the data names rather than holds.
    pub system: bool,
}

/// What a font is drawn in instead when it's drawn smaller than `below`,
/// in the same units as its size.
#[derive(Debug, Clone, PartialEq)]
pub struct Backup {
    pub face: Option<Typeface>,
    pub size: f32,
    pub below: f32,
}
