//! The options chosen, kept between runs in a TOML file of this project's
//! own rather than the original game's, whose options are for itself.
//!
//! Only what the player changes is written: an option given on the command
//! line, or never changed, is left as the file has it, so the game's own
//! choice still applies where the file says nothing.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, PrimaryMonitor, PrimaryWindow, RawHandleWrapper, WindowPosition,
    WindowResized,
};
use serde::{Deserialize, Serialize};

use crate::Nickname;
use crate::hud::Hud;
use crate::options::{
    AntiAliasing, Display, Options, Page, WindowedSize, fitting, on_wayland, room, size_of,
};
use crate::sound::{Channel, Sounds, Volumes};
use crate::supply::Border;
use crate::times::Elapsed;
use crate::ui_scale::{UI_SCALES, UiScaleSetting};

/// The options file's name, in the program's own folder for its
/// configuration.
const FILE: &str = "options.toml";

/// Where `program`'s options are kept, if the platform has somewhere.
pub fn default_path(program: &str) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", program)?;
    Some(dirs.config_dir().join(FILE))
}

/// The options the file holds, each unset where it says nothing.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Chosen {
    pub nickname: Option<String>,
    pub hotkeys: Option<String>,
    pub supply_border: Option<bool>,
    pub elapsed_game_time: Option<bool>,
    pub display: Option<Display>,
    /// The window's size while windowed.
    pub resolution: Option<[u32; 2]>,
    /// Where the window is, in physical pixels, which also says the screen
    /// it takes in borderless or fullscreen.
    pub position: Option<[i32; 2]>,
    pub anti_aliasing: Option<AntiAliasing>,
    pub ui_scale: Option<f32>,
    pub hud: Option<Hud>,
    #[serde(skip_serializing_if = "unset")]
    pub volume: Levels,
    #[serde(skip_serializing_if = "unset")]
    pub sounds: Switches,
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Levels {
    pub main: Option<f64>,
    pub fx: Option<f64>,
    pub speech: Option<f64>,
    pub music: Option<f64>,
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Switches {
    pub chat: Option<bool>,
    pub replies: Option<bool>,
}

/// Whether nothing in a table is set, which leaves it out of the file.
fn unset<T: Default + PartialEq>(table: &T) -> bool {
    *table == T::default()
}

impl Levels {
    fn get(&self, channel: Channel) -> Option<f64> {
        match channel {
            Channel::Main => self.main,
            Channel::Fx => self.fx,
            Channel::Speech => self.speech,
            Channel::Music => self.music,
        }
    }
}

/// The options file, and what it holds.
#[derive(Resource, Debug)]
pub struct Saved {
    /// Where it's written; none where it couldn't be read, so as not to
    /// write over what the player may want to mend.
    path: Option<PathBuf>,
    pub chosen: Chosen,
    /// Changed since last written.
    dirty: bool,
    /// When last changed, in real seconds.
    changed_at: f64,
}

impl Saved {
    /// What's at `path`, nothing chosen if there's no file there yet.
    pub fn read(path: PathBuf) -> Saved {
        let read = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Chosen::default()),
            Err(e) => Err(e.to_string()),
        };
        match read {
            Ok(chosen) => Saved {
                path: Some(path),
                chosen,
                dirty: false,
                changed_at: 0.0,
            },
            Err(e) => {
                warn!("{}: {e}; the options chosen won't be kept", path.display());
                Saved::none()
            }
        }
    }

    /// Where the options are kept, if anywhere.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Nothing chosen, and nowhere to keep what is.
    pub fn none() -> Saved {
        Saved {
            path: None,
            chosen: Chosen::default(),
            dirty: false,
            changed_at: 0.0,
        }
    }

    /// The nickname kept, if it still makes a name.
    pub fn nickname(&self) -> Option<String> {
        self.chosen.nickname.as_deref().and_then(tgserver::name)
    }

    /// The interface's scale kept, if it's one it may be.
    pub fn ui_scale(&self) -> Option<f32> {
        self.chosen.ui_scale.filter(|s| UI_SCALES.contains(s))
    }

    /// Sizes and places `window` as kept, windowed until [`restore`] has
    /// it take the screen it's on, as it isn't on any yet.
    pub fn shape(&self, window: &mut Window) {
        if let Some([x, y]) = self.resolution() {
            window.resolution.set(x as f32, y as f32);
        }
        if let Some([x, y]) = self.chosen.position {
            window.position = WindowPosition::At(IVec2::new(x, y));
        }
    }

    fn resolution(&self) -> Option<[u32; 2]> {
        self.chosen.resolution.filter(|r| r.iter().all(|&n| n > 0))
    }

    /// Has the app's options start as kept, where the command line didn't
    /// already choose them.
    pub fn apply(&self, world: &mut World) {
        let chosen = &self.chosen;
        if let Some(id) = &chosen.hotkeys {
            world.resource_mut::<Options>().choose_hotkeys(id);
        }
        if let Some(on) = chosen.supply_border {
            world.insert_resource(Border(on));
        }
        if let Some(on) = chosen.elapsed_game_time {
            world.insert_resource(Elapsed(on));
        }
        if let Some(aa) = chosen.anti_aliasing {
            world.insert_resource(aa);
        }
        if let Some(hud) = chosen.hud {
            world.insert_resource(hud);
        }
        let mut volumes = world.resource_mut::<Volumes>();
        for channel in Channel::ALL {
            if let Some(level) = chosen.volume.get(channel) {
                volumes.set(channel, level as f32);
            }
        }
        let mut sounds = world.resource_mut::<Sounds>();
        if let Some(on) = chosen.sounds.chat {
            sounds.switches.chat = on;
        }
        if let Some(on) = chosen.sounds.replies {
            sounds.switches.replies = on;
        }
    }

    /// Keeps what's in `now` that differs from `before`. What's unset in
    /// `now`, as the window is once it's closed, unsets nothing. Whether
    /// anything kept changed.
    fn note(&mut self, before: &Chosen, now: &Chosen) -> bool {
        let [mut kept, was, now] = [&self.chosen, before, now].map(table);
        if !merge(&mut kept, &was, &now) {
            return false;
        }
        match toml::Value::Table(kept).try_into() {
            Ok(chosen) => {
                self.chosen = chosen;
                self.dirty = true;
                true
            }
            Err(e) => {
                warn!("options: {e}");
                false
            }
        }
    }

    fn write(&mut self) {
        self.dirty = false;
        let Some(path) = &self.path else {
            return;
        };
        if let Err(e) = write(path, &self.chosen) {
            warn!("{}: {e}", path.display());
        }
    }
}

/// The options as the file would hold them, so they're compared one by
/// one without naming each.
fn table(chosen: &Chosen) -> toml::Table {
    toml::Table::try_from(chosen).unwrap_or_default()
}

/// Sets in `kept` each value in `now` that differs from `was`, down through
/// its tables; whether any did.
fn merge(kept: &mut toml::Table, was: &toml::Table, now: &toml::Table) -> bool {
    let mut changed = false;
    for (key, value) in now {
        let before = was.get(key);
        if let toml::Value::Table(now) = value {
            let was = before.and_then(toml::Value::as_table);
            let kept = kept.entry(key).or_insert_with(|| toml::Table::new().into());
            if let toml::Value::Table(kept) = kept {
                changed |= merge(kept, was.unwrap_or(&toml::Table::new()), now);
            }
        } else if before != Some(value) && kept.get(key) != Some(value) {
            kept.insert(key.clone(), value.clone());
            changed = true;
        }
    }
    changed
}

fn write(path: &Path, chosen: &Chosen) -> Result<(), String> {
    let text = toml::to_string(chosen).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Whole or not at all, should the game stop partway.
    let partial = path.with_extension("toml.new");
    std::fs::write(&partial, text).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

/// Keeps the options the player changes in [`Saved`], writing them once
/// they've settled, as a slider dragged or a name typed changes them on
/// every frame, or as the game exits.
pub struct SavedPlugin;

impl Plugin for SavedPlugin {
    fn build(&self, app: &mut App) {
        // After the window's mode follows a resize, which the window being
        // made has, so as not to be taken for the platform leaving the mode.
        app.add_systems(
            PreUpdate,
            restore
                .after(crate::options::follow_window)
                .run_if(resource_exists::<Saved>),
        )
        .add_systems(Last, keep.run_if(resource_exists::<Saved>));
    }
}

/// Once the window is made, keeps it on a screen there still is, no bigger
/// than the options would offer for it, and then, once it's settled there,
/// has it take that screen as kept: a window still being sized as it's
/// made would be taken back out of it.
fn restore(
    mut commands: Commands,
    saved: Res<Saved>,
    mut window: Query<(&mut Window, Ref<RawHandleWrapper>), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    mut resized: MessageReader<WindowResized>,
    mut taking: Local<Option<(Display, MonitorSelection)>>,
) {
    let settling = resized.read().count() > 0;
    let Ok((mut window, handle)) = window.single_mut() else {
        return;
    };
    if let Some((display, on)) = *taking
        && !settling
    {
        *taking = None;
        // To go back to on leaving the whole screen.
        commands.insert_resource(WindowedSize(Some(window.size())));
        window.mode = display.mode_on(on);
    }
    if !handle.is_added() {
        return;
    }
    let at = match window.position {
        WindowPosition::At(at) => Some(at),
        _ => None,
    };
    let on = |m: &Monitor| {
        let size = IVec2::new(m.physical_width as i32, m.physical_height as i32);
        at.is_some_and(|at| {
            at.cmpge(m.physical_position).all() && at.cmplt(m.physical_position + size).all()
        })
    };
    let primary = monitors.iter().find(|(_, _, p)| *p).map(|(_, m, _)| m);
    let monitor = monitors.iter().find(|(_, m, _)| on(m));
    if at.is_some() && monitor.is_none() && !monitors.is_empty() {
        window.position = WindowPosition::Centered(MonitorSelection::Primary);
    }
    let screen = monitor.map_or(MonitorSelection::Primary, |(e, _, _)| {
        MonitorSelection::Entity(e)
    });
    let room = monitor.map(|(_, m, _)| m).or(primary).map(room);
    if room.is_some_and(|room| !size_of(&window).cmplt(room).all())
        && let Some(fits) = fitting(room).last()
    {
        window.resolution.set(fits.x as f32, fits.y as f32);
    }
    let display = match saved.chosen.display {
        Some(Display::Fullscreen) if on_wayland(&handle) => Some(Display::Borderless),
        Some(Display::Windowed) | None => None,
        d => d,
    };
    *taking = display.map(|d| (d, screen));
}

/// A volume as kept, to the nearest percent, which writes as it reads.
fn percent(level: f32) -> f64 {
    (f64::from(level) * 100.0).round() / 100.0
}

/// Seconds after the last change that the options are written.
const SETTLED: f64 = 1.0;

/// The options as they are now.
#[derive(bevy::ecs::system::SystemParam)]
struct Now<'w, 's> {
    nickname: Res<'w, Nickname>,
    options: Res<'w, Options>,
    border: Res<'w, Border>,
    elapsed: Res<'w, Elapsed>,
    anti_aliasing: Res<'w, AntiAliasing>,
    ui_scale: Res<'w, UiScaleSetting>,
    hud: Res<'w, Hud>,
    volumes: Res<'w, Volumes>,
    sounds: Res<'w, Sounds>,
    window: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    /// What [`Now::shown`] last was.
    shown: Local<'s, Option<Shown>>,
}

/// What of the window the options keep, which changes far less often than
/// the window does, e.g. as the cursor moves over it.
type Shown = (Option<Display>, Option<[u32; 2]>, Option<[i32; 2]>);

impl Now<'_, '_> {
    /// Whether any of the options may have changed since last looked at.
    fn changed(&mut self) -> bool {
        let shown = Some(self.shown());
        let moved = *self.shown != shown;
        *self.shown = shown;
        moved
            || self.nickname.is_changed()
            || self.options.is_changed()
            || self.border.is_changed()
            || self.elapsed.is_changed()
            || self.anti_aliasing.is_changed()
            || self.ui_scale.is_changed()
            || self.hud.is_changed()
            || self.volumes.is_changed()
            || self.sounds.is_changed()
    }

    fn shown(&self) -> Shown {
        let window = self.window.single().ok();
        let display = window.map(|w| Display::of(w.mode));
        // Taking the whole screen sizes the window to it.
        let resolution = window
            .filter(|_| display == Some(Display::Windowed))
            .map(|w| size_of(w).to_array());
        let position = window.and_then(|w| match w.position {
            WindowPosition::At(at) => Some(at.to_array()),
            _ => None,
        });
        (display, resolution, position)
    }

    fn chosen(&self) -> Chosen {
        let (display, resolution, position) = self.shown();
        let volumes = *self.volumes;
        let level = |c| Some(percent(volumes.get(c)));
        Chosen {
            nickname: Some(self.nickname.0.clone()),
            hotkeys: self.options.hotkeys().map(|c| c.id.as_str().to_string()),
            supply_border: Some(self.border.0),
            elapsed_game_time: Some(self.elapsed.0),
            display,
            resolution,
            position,
            anti_aliasing: Some(*self.anti_aliasing),
            ui_scale: Some(self.ui_scale.0),
            hud: Some(*self.hud),
            volume: Levels {
                main: level(Channel::Main),
                fx: level(Channel::Fx),
                speech: level(Channel::Speech),
                music: level(Channel::Music),
            },
            sounds: Switches {
                chat: Some(self.sounds.switches.chat),
                replies: Some(self.sounds.switches.replies),
            },
        }
    }
}

fn keep(
    mut now: Now,
    mut saved: ResMut<Saved>,
    mut before: Local<Option<Chosen>>,
    pages: Query<(), With<Page>>,
    time: Res<Time<Real>>,
    mut exits: MessageReader<AppExit>,
) {
    let at = time.elapsed_secs_f64();
    if before.is_none() || now.changed() {
        let chosen = now.chosen();
        // As the game started, which isn't the player's choice.
        if let Some(was) = before.as_ref() {
            let mut was = was.clone();
            // The window's mode and size are chosen in the options; out of
            // them, it's the platform that changes them, e.g. to fit a size
            // too big for the screen.
            if pages.is_empty() {
                was.display = chosen.display;
                was.resolution = chosen.resolution;
            }
            if saved.note(&was, &chosen) {
                saved.changed_at = at;
            }
        }
        *before = Some(chosen);
    }
    let exiting = exits.read().count() > 0;
    if saved.dirty && (exiting || at - saved.changed_at >= SETTLED) {
        saved.write();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tgclient-saved-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&dir);
        dir.join("sub").join(FILE)
    }

    #[test]
    fn what_is_written_is_read_back() {
        let path = temp("round-trip");
        let mut saved = Saved::read(path.clone());
        assert_eq!(saved.chosen, Chosen::default());
        saved.chosen = Chosen {
            nickname: Some("bob".into()),
            hotkeys: Some("visual".into()),
            display: Some(Display::Borderless),
            resolution: Some([1600, 900]),
            anti_aliasing: Some(AntiAliasing::Smaa),
            ui_scale: Some(1.25),
            hud: Some(Hud::Original),
            volume: Levels {
                music: Some(0.25),
                ..default()
            },
            ..default()
        };
        saved.write();
        assert_eq!(Saved::read(path).chosen, saved.chosen);
    }

    #[test]
    fn a_file_that_cant_be_read_is_not_written_over() {
        let path = temp("broken");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "display = \"sideways\"").unwrap();
        let mut saved = Saved::read(path.clone());
        assert_eq!(saved.chosen, Chosen::default());
        saved.chosen.ui_scale = Some(2.0);
        saved.write();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "display = \"sideways\""
        );
    }

    #[test]
    fn only_what_changes_is_kept() {
        let mut saved = Saved::none();
        saved.chosen.ui_scale = Some(1.5);
        let start = Chosen {
            nickname: Some("Player".into()),
            ui_scale: Some(0.75),
            display: Some(Display::Windowed),
            ..default()
        };
        // A scale started with, unchanged, leaves the kept one.
        saved.note(&start, &start);
        assert!(!saved.dirty);
        assert_eq!(saved.chosen.ui_scale, Some(1.5));
        let renamed = Chosen {
            nickname: Some("bob".into()),
            ..start.clone()
        };
        saved.note(&start, &renamed);
        assert!(saved.dirty);
        assert_eq!(saved.chosen.nickname.as_deref(), Some("bob"));
        assert_eq!(saved.chosen.ui_scale, Some(1.5));
        // No window, as the game closes, unsets nothing.
        let closed = Chosen {
            display: None,
            ..renamed.clone()
        };
        saved.note(&renamed, &closed);
        assert_eq!(saved.chosen.display, None);
        saved.chosen.display = Some(Display::Fullscreen);
        saved.note(&renamed, &closed);
        assert_eq!(saved.chosen.display, Some(Display::Fullscreen));
    }

    #[test]
    fn a_volume_changed_is_kept_alone_of_its_table() {
        let mut saved = Saved::none();
        saved.chosen.volume.fx = Some(0.5);
        let level = |music| Chosen {
            volume: Levels {
                main: Some(0.8),
                music: Some(music),
                ..default()
            },
            ..default()
        };
        assert!(saved.note(&level(0.7), &level(0.25)));
        let kept = &saved.chosen.volume;
        assert_eq!(
            (kept.main, kept.fx, kept.music),
            (None, Some(0.5), Some(0.25))
        );
        assert!(!saved.note(&level(0.25), &level(0.25)));
    }

    #[test]
    fn a_volume_is_written_as_it_reads() {
        let mut chosen = Chosen::default();
        chosen.volume.music = Some(percent(0.73));
        chosen.volume.fx = Some(percent(0.7349));
        let text = toml::to_string(&chosen).unwrap();
        assert!(text.contains("music = 0.73\n"), "{text}");
        assert!(text.contains("fx = 0.73\n"), "{text}");
    }

    #[test]
    fn kept_choices_that_no_longer_make_sense_are_passed_over() {
        let mut saved = Saved::none();
        saved.chosen.nickname = Some("   ".into());
        saved.chosen.ui_scale = Some(9.0);
        assert_eq!(saved.nickname(), None);
        assert_eq!(saved.ui_scale(), None);
    }
}
