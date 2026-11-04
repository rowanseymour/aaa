//! Map editor for Axis & Allies (2004), which for now views maps, reading
//! straight from the game's installed data files.

use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::Map;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use aaa_render::camera::{self, RtsCamera};
use aaa_render::{Edge, MapAssets, MapRenderPlugin, objects, terrain};

const USAGE: &str = "\
usage:
  aaa-editor <game dir>          list the maps
  aaa-editor <game dir> <map>    view a map, e.g. \"(2v2)Berlin\"

options:
  --camera <x,y,dist,yaw,pitch>  start the free camera looking at x,y (fractions
                                 of the map's width) from dist away, with yaw
                                 and pitch in radians
  --screenshot <file.png>        save a frame to a file and exit, without the
                                 help text or placeholder markers";

const HELP: &str = "WASD/arrows/middle drag: pan   wheel: zoom   Tab: free camera \
                    (Q/E/right drag: rotate   R/F: tilt)   M: placeholder markers   \
                    L: lighting";

/// The help text, naming the lighting in use.
fn help(lighting: &str) -> String {
    format!("{HELP} ({lighting})")
}

/// Marks the help text.
#[derive(Component)]
struct Help;

/// The lighting presets to cycle through: the map's own sun, then the
/// player's choices in the game, each named with its diffuse and ambient
/// colours.
#[derive(Resource)]
struct Lighting {
    presets: Vec<(String, [f32; 3], [f32; 3])>,
    current: usize,
}

#[derive(Resource)]
struct Loaded {
    game: GameData,
    map: Map,
}

/// Where the camera starts, from `--camera`.
#[derive(Resource, Clone, Copy)]
struct StartView([f32; 5]);

/// Saves a screenshot once the scene has settled, then exits.
#[derive(Resource)]
struct ScreenshotTo {
    path: PathBuf,
    frame: u32,
    attempts: u32,
}

/// Frames to let the scene settle before asking for a capture.
const SETTLE_FRAMES: u32 = 60;
/// Frames to wait for a capture before asking again, since one can be dropped
/// (e.g. while the window can't be presented) and never arrive.
const CAPTURE_PATIENCE: u32 = 120;
/// Captures to try before giving up rather than running forever.
const MAX_CAPTURES: u32 = 5;

#[derive(Debug, PartialEq)]
enum ShotStep {
    Wait,
    Capture,
    GiveUp,
}

impl ScreenshotTo {
    /// Advances a frame, saying whether to ask for a capture now.
    fn tick(&mut self) -> ShotStep {
        self.frame += 1;
        if self.frame == SETTLE_FRAMES {
            self.attempts += 1;
            ShotStep::Capture
        } else if self.frame >= SETTLE_FRAMES + CAPTURE_PATIENCE {
            self.retry()
        } else {
            ShotStep::Wait
        }
    }

    /// Starts over after a capture that was unusable or never came.
    fn retry(&mut self) -> ShotStep {
        if self.attempts >= MAX_CAPTURES {
            return ShotStep::GiveUp;
        }
        self.frame = 0;
        ShotStep::Wait
    }
}

/// Removes `flag` and the value after it from `args`.
fn take_option(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, ()> {
    match args.iter().position(|a| a == flag) {
        Some(i) if i + 1 < args.len() => {
            let value = args.remove(i + 1);
            args.remove(i);
            Ok(Some(value))
        }
        Some(_) => Err(()),
        None => Ok(None),
    }
}

/// Parses a comma separated list such as `0.5,0.5,300,0,0.8`.
fn floats<const N: usize>(s: &str) -> Option<[f32; N]> {
    let v: Vec<f32> = s
        .split(',')
        .map(|p| p.trim().parse())
        .collect::<Result<_, _>>()
        .ok()?;
    v.try_into().ok()
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let (Ok(screenshot), Ok(camera)) = (
        take_option(&mut args, "--screenshot"),
        take_option(&mut args, "--camera"),
    ) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let screenshot = screenshot.map(|path| ScreenshotTo {
        path: path.into(),
        frame: 0,
        attempts: 0,
    });
    let camera = match camera.map(|c| floats(&c)) {
        None => None,
        Some(Some(view)) => Some(StartView(view)),
        Some(None) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let (dir, map) = match args.as_slice() {
        [dir] => (PathBuf::from(dir), None),
        [dir, map] => (PathBuf::from(dir), Some(map.as_str())),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let loaded = GameData::open(&dir).and_then(|mut game| {
        let Some(map) = map else {
            for name in game.map_names() {
                println!("{name}");
            }
            return Ok(None);
        };
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}; run without a map name to list them"))?;
        Map::load(&mut game, &name).map(|map| Some(Loaded { game, map }))
    });
    let mut loaded = match loaded {
        Ok(Some(l)) => l,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("aaa-editor: {e}");
            return ExitCode::FAILURE;
        }
    };

    let sun = &loaded.map.sun;
    let mut presets = vec![("map".to_string(), sun.diffuse, sun.ambient)];
    presets.extend(loaded.game.sun_presets().into_iter().map(|s| {
        (
            s.ids.map_or_else(String::new, |i| i.to_string()),
            s.diffuse,
            s.ambient,
        )
    }));
    let lighting = Lighting {
        presets,
        current: 0,
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("{} - aaa-editor", loaded.map.title),
            ..default()
        }),
        ..default()
    }));
    app.add_plugins(MapRenderPlugin { edge: Edge::Hard })
        .insert_resource(loaded)
        .insert_resource(lighting)
        .add_systems(Startup, setup)
        .add_systems(Update, (objects::toggle_markers, cycle_lighting));
    if let Some(c) = camera {
        app.insert_resource(c);
    }
    if let Some(s) = screenshot {
        app.insert_resource(s).add_systems(Update, take_screenshot);
    }
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn give_up(shot: &ScreenshotTo, exit: &mut MessageWriter<AppExit>) {
    error!(
        "no usable frame for {} after {} captures",
        shot.path.display(),
        shot.attempts
    );
    exit.write(AppExit::error());
}

fn take_screenshot(
    mut commands: Commands,
    mut shot: ResMut<ScreenshotTo>,
    mut exit: MessageWriter<AppExit>,
) {
    match shot.tick() {
        ShotStep::Wait => return,
        ShotStep::GiveUp => return give_up(&shot, &mut exit),
        ShotStep::Capture => {}
    }
    commands.spawn(Screenshot::primary_window()).observe(
        |captured: On<ScreenshotCaptured>,
         mut shot: ResMut<ScreenshotTo>,
         mut exit: MessageWriter<AppExit>| {
            // A busy machine can hand back a frame before the window has drawn
            // anything, all zeros, so that one is retried.
            let blank = captured
                .image
                .data
                .as_ref()
                .is_none_or(|d| d.chunks(4).all(|px| px[..3].iter().all(|&c| c == 0)));
            if blank {
                if shot.retry() == ShotStep::GiveUp {
                    give_up(&shot, &mut exit);
                }
                return;
            }
            save_to_disk(shot.path.clone())(captured);
            exit.write(AppExit::Success);
        },
    );
}

fn setup(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut assets: MapAssets,
    lighting: Res<Lighting>,
    start: Option<Res<StartView>>,
    screenshot: Option<Res<ScreenshotTo>>,
) {
    let Loaded { game, map } = &mut *loaded;
    aaa_render::spawn_map(
        &mut commands,
        &mut assets,
        game,
        map,
        screenshot.is_none(),
        |_| false,
    );

    let ground = &map.ground;
    let size = ground.size();
    let mut cam = RtsCamera::centred(ground);
    if let Some(StartView([x, y, distance, yaw, pitch])) = start.as_deref().copied() {
        let (x, y) = (x * size, y * size);
        cam = RtsCamera {
            focus: terrain::world(x, y, ground.height_at(x, y)),
            distance,
            yaw,
            pitch,
            free: true,
        };
    }
    camera::spawn(&mut commands, cam);

    if screenshot.is_some() {
        return;
    }
    commands.spawn((
        Help,
        Text::new(help(&lighting.presets[lighting.current].0)),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            bottom: px(8),
            left: px(8),
            ..default()
        },
    ));
}

/// Moves to the next lighting preset on `L`, keeping the sun's direction.
fn cycle_lighting(
    keys: Res<ButtonInput<KeyCode>>,
    mut lighting: ResMut<Lighting>,
    mut sun: Single<&mut DirectionalLight>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut text: Query<&mut Text, With<Help>>,
) {
    if !keys.just_pressed(KeyCode::KeyL) {
        return;
    }
    lighting.current = (lighting.current + 1) % lighting.presets.len();
    let (name, diffuse, ambient_color) = &lighting.presets[lighting.current];
    aaa_render::color_lights(&mut sun, &mut ambient, *diffuse, *ambient_color);
    for mut t in &mut text {
        t.0 = help(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot() -> ScreenshotTo {
        ScreenshotTo {
            path: "out.png".into(),
            frame: 0,
            attempts: 0,
        }
    }

    #[test]
    fn screenshot_captures_once_settled() {
        let mut shot = shot();
        let steps: Vec<_> = (0..SETTLE_FRAMES).map(|_| shot.tick()).collect();
        assert!(
            steps[..steps.len() - 1]
                .iter()
                .all(|s| *s == ShotStep::Wait)
        );
        assert_eq!(steps.last(), Some(&ShotStep::Capture));
    }

    #[test]
    fn screenshot_gives_up_when_captures_never_arrive() {
        let mut shot = shot();
        let mut captures = 0;
        for _ in 0..MAX_CAPTURES * (SETTLE_FRAMES + CAPTURE_PATIENCE) + 1 {
            match shot.tick() {
                ShotStep::Wait => {}
                ShotStep::Capture => captures += 1,
                ShotStep::GiveUp => {
                    assert_eq!(captures, MAX_CAPTURES);
                    return;
                }
            }
        }
        panic!("never gave up");
    }

    #[test]
    fn screenshot_gives_up_on_blank_frames() {
        let mut shot = shot();
        for _ in 1..MAX_CAPTURES {
            while shot.tick() != ShotStep::Capture {}
            assert_eq!(shot.retry(), ShotStep::Wait);
        }
        while shot.tick() != ShotStep::Capture {}
        assert_eq!(shot.retry(), ShotStep::GiveUp);
    }
}
