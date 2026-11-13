//! Map editor for Axis & Allies (2004), which views maps, reading straight
//! from the game's installed data files.

use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::Map;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui, input::egui_wants_any_pointer_input,
};

use aaa_render::camera::{self, RtsCamera};
use aaa_render::shot::{self, ShotPlugin};
use aaa_render::{Edge, MapAssets, MapObject, MapRenderPlugin, objects, terrain};

const USAGE: &str = "\
usage:
  aaa-editor <game dir>          list the maps
  aaa-editor <game dir> <map>    view a map, e.g. \"(2v2)Berlin\"

options:
  --camera <x,y,dist,yaw,pitch>  start the free camera looking at x,y (fractions
                                 of the map's width) from dist away, with yaw
                                 and pitch in radians
  --inspect <id,...>             open the windows on the objects with these ids
  --screenshot <file.png>        save a frame to a file and exit, without a
                                 window, help text or placeholder markers
  --log <filter>                 what to log, as for RUST_LOG, e.g. \"debug\" or
                                 \"aaa_render=debug\"; RUST_LOG still applies
                                 on top";

const HELP: &str = "click: inspect   WASD/arrows/middle drag: pan   wheel: zoom   Tab: free camera \
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

/// Marks an object clicked on, whose map record shows in a window until
/// that's closed.
#[derive(Component)]
struct Inspected {
    /// Whether to bring its window to the front, for a click on an object
    /// that already has one.
    raise: bool,
}

/// Whether the run is only here to save a frame, from `--screenshot`.
#[derive(Resource)]
struct Shooting(bool);

/// The game camera's zoom limits: the game's own nearest, but out far
/// enough to see a whole map.
const EDITOR_ZOOM: (f32, f32) = (camera::GAME_ZOOM.0, 12.0);

/// Where the camera starts, from `--camera`.
#[derive(Resource, Clone, Copy)]
struct StartView([f32; 5]);

/// The objects to open windows on from the start, from `--inspect`.
#[derive(Resource)]
struct StartInspecting(Vec<u32>);

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

/// Parses a comma separated list of finite numbers such as
/// `0.5,0.5,300,0,0.8`.
fn floats<const N: usize>(s: &str) -> Option<[f32; N]> {
    let v: Vec<f32> = s
        .split(',')
        .map(|p| p.trim().parse().ok().filter(|f: &f32| f.is_finite()))
        .collect::<Option<_>>()?;
    v.try_into().ok()
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let (Ok(screenshot), Ok(camera), Ok(inspect_ids), Ok(log)) = (
        take_option(&mut args, "--screenshot"),
        take_option(&mut args, "--camera"),
        take_option(&mut args, "--inspect"),
        take_option(&mut args, "--log"),
    ) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let log = match aaa_render::log::plugin(log.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("aaa-editor: {e}");
            return ExitCode::from(2);
        }
    };
    let screenshot = screenshot.map(|path| ShotPlugin {
        path: Some(path.into()),
    });
    let camera = match camera.map(|c| floats(&c)) {
        None => None,
        Some(Some(view)) => Some(StartView(view)),
        Some(None) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let inspecting = match inspect_ids.map(|ids| ids.split(',').map(|i| i.trim().parse()).collect())
    {
        None => Vec::new(),
        Some(Ok(ids)) => ids,
        Some(Err(_)) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    // What's left that looks like an option is one this doesn't have, not a
    // game folder or map.
    if args.iter().any(|a| a.starts_with("--")) {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let (dir, map) = match args.as_slice() {
        [dir] => (PathBuf::from(dir), None),
        [dir, map] => (PathBuf::from(dir), Some(map.as_str())),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let loaded = GameData::open(&dir).and_then(|game| {
        let Some(map) = map else {
            for name in game.map_names() {
                println!("{name}");
            }
            return Ok(None);
        };
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}; run without a map name to list them"))?;
        let recruits = aaa_map::Recruits::new(&game);
        Map::load(&game, &recruits, &name).map(|map| Some(Loaded { game, map }))
    });
    let loaded = match loaded {
        Ok(Some(l)) => {
            for w in &l.map.warnings {
                eprintln!("aaa-editor: {w}");
            }
            l
        }
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
    let title = format!("{} - aaa-editor", loaded.map.title);
    app.add_plugins(
        DefaultPlugins
            .set(shot::window(title, screenshot.is_some()))
            .set(log),
    );
    app.add_plugins(MapRenderPlugin {
        edge: Edge::Hard,
        pickable: screenshot.is_none(),
    })
    .insert_resource(loaded)
    .insert_resource(lighting)
    .add_systems(Startup, setup)
    .add_systems(Update, (objects::toggle_markers, cycle_lighting));
    if let Some(c) = camera {
        app.insert_resource(c);
    }
    app.insert_resource(Shooting(screenshot.is_some()))
        .insert_resource(StartInspecting(inspecting))
        .add_plugins(EguiPlugin::default())
        .add_systems(Update, start_inspecting)
        .add_systems(EguiPrimaryContextPass, show_inspected);
    match screenshot {
        Some(s) => {
            app.add_plugins(s);
        }
        None => {
            app.add_plugins(MeshPickingPlugin)
                .configure_sets(
                    Update,
                    camera::Controls.run_if(not(egui_wants_any_pointer_input)),
                )
                .add_observer(inspect);
        }
    }
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn setup(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
    lighting: Res<Lighting>,
    start: Option<Res<StartView>>,
    shooting: Res<Shooting>,
) {
    let Loaded { game, map } = &*loaded;
    aaa_render::spawn_map(&mut commands, &mut assets, game, map, !shooting.0, |_| {
        false
    });

    let ground = &map.ground;
    let size = ground.size();
    let mut cam = RtsCamera {
        zoom: EDITOR_ZOOM,
        freeable: true,
        ..RtsCamera::centred(ground)
    };
    if let Some(StartView([x, y, distance, yaw, pitch])) = start.as_deref().copied() {
        let (x, y) = (x * size, y * size);
        cam = RtsCamera {
            focus: terrain::world(x, y, ground.height_at([x, y])),
            distance,
            yaw,
            pitch,
            free: true,
            ..cam
        };
    }
    camera::spawn(&mut commands, cam);

    if shooting.0 {
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

/// Opens the windows `--inspect` asks for, on the first frame, when the
/// objects are there.
fn start_inspecting(
    mut start: ResMut<StartInspecting>,
    objects: Query<(Entity, &MapObject)>,
    mut commands: Commands,
) {
    if start.0.is_empty() {
        return;
    }
    let mut ids = std::mem::take(&mut start.0);
    ids.sort_unstable();
    ids.dedup();
    for (entity, MapObject(thing)) in &objects {
        if let Ok(i) = ids.binary_search(&thing.id) {
            ids.remove(i);
            commands.entity(entity).insert(Inspected { raise: true });
        }
    }
    for id in ids {
        eprintln!("aaa-editor: no object with id {id} to inspect");
    }
}

/// Opens a window on the object clicked on.
fn inspect(
    mut click: On<PointerClick>,
    objects: Query<(), With<MapObject>>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    // Clicks bubble up through the parts of a model, so act on the first.
    click.propagate(false);
    if click.button != PointerButton::Primary {
        return;
    }
    // Clicks on egui's windows land on its context's entity instead, which
    // isn't an object.
    let hit = click.original_event_target();
    let object = std::iter::once(hit)
        .chain(parents.iter_ancestors(hit))
        .find(|&e| objects.contains(e));
    if let Some(object) = object {
        commands.entity(object).insert(Inspected { raise: true });
    }
}

/// Shows a window with the map record of each object being inspected, with
/// a line out to it from the object.
fn show_inspected(
    mut contexts: EguiContexts,
    camera: Single<(&Camera, &GlobalTransform), With<RtsCamera>>,
    mut inspected: Query<(Entity, &MapObject, &GlobalTransform, &mut Inspected)>,
    mut commands: Commands,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let (camera, view) = *camera;
    let color = egui::Color32::from_rgb(255, 230, 50);
    for (entity, MapObject(thing), transform, mut inspected) in &mut inspected {
        let id = egui::Id::new(entity);
        if inspected.raise {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
            inspected.raise = false;
        }
        let at = camera
            .world_to_viewport(view, transform.translation())
            .ok()
            .map(|p| egui::pos2(p.x, p.y));
        let mut open = true;
        let mut window = egui::Window::new(&thing.template)
            .id(id)
            .open(&mut open)
            .default_width(320.0);
        if let Some(at) = at {
            window = window.default_pos(at + egui::vec2(60.0, -120.0));
        }
        let shown = window.show(ctx, |ui| {
            egui::Grid::new("fields")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    let mut row = |name: &str, value: String| {
                        ui.label(name);
                        ui.monospace(value);
                        ui.end_row();
                    };
                    row("name", thing.name.clone());
                    row("id", thing.id.to_string());
                    row("class", thing.class.to_string());
                    row("position", format!("{:.2?}", thing.position));
                    row("facing", format!("{:.3?}", thing.facing));
                    row("scale", thing.scale.to_string());
                    row("variation", thing.variation.to_string());
                    row("flags", format!("{:#010x}", thing.flags));
                    row("seen by", format!("{:#06x}", thing.seen_by));
                });
            egui::CollapsingHeader::new("detail").show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        ui.monospace(format!("{:#?}", thing.detail));
                    });
            });
        });
        if !open {
            commands.entity(entity).remove::<Inspected>();
            continue;
        }
        if let (Some(at), Some(shown)) = (at, shown) {
            let painter = ctx.layer_painter(egui::LayerId::background());
            painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                leader(shown.response.rect, at),
                false,
                egui::Color32::TRANSPARENT,
                egui::Stroke::new(1.5, color),
            ));
            painter.circle_filled(at, 4.0, color);
        }
    }
    Ok(())
}

/// The curve from a window to the point on screen it's about, like a pipe:
/// out of the side of its title bar facing the point, and round to arrive
/// level with it too.
fn leader(window: egui::Rect, to: egui::Pos2) -> [egui::Pos2; 4] {
    let (x, out) = if to.x > window.center().x {
        (window.right(), 1.0)
    } else {
        (window.left(), -1.0)
    };
    let from = egui::pos2(x, window.top() + 14.0);
    let bend = egui::vec2(out * ((to.x - from.x).abs() / 2.0).max(40.0), 0.0);
    [from, from + bend, to - bend, to]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_wants_every_number_finite() {
        assert_eq!(floats("0.5, 1,-2"), Some([0.5, 1.0, -2.0]));
        assert_eq!(floats::<3>("0.5,1"), None);
        assert_eq!(floats::<2>("0.5,x"), None);
        assert_eq!(floats::<2>("0.5,nan"), None);
        assert_eq!(floats::<2>("inf,1"), None);
    }

    #[test]
    fn take_option_removes_a_flag_and_its_value() {
        let mut args: Vec<String> = ["dir", "--camera", "1,2", "map"].map(String::from).into();
        assert_eq!(take_option(&mut args, "--camera"), Ok(Some("1,2".into())));
        assert_eq!(args, ["dir", "map"]);
        assert_eq!(take_option(&mut args, "--camera"), Ok(None));
        args.push("--inspect".into());
        assert_eq!(take_option(&mut args, "--inspect"), Err(()));
    }
}
