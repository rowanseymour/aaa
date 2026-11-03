//! Plays Axis & Allies (2004) matches, starting one straight away on the
//! map it's given.

use std::path::PathBuf;
use std::process::ExitCode;

use aaa_data::GameData;
use aaa_map::Map;
use aaa_render::camera::{self, RtsCamera};
use aaa_render::{MapAssets, MapRenderPlugin, Markers};
use bevy::prelude::*;

const USAGE: &str = "\
usage:
  aaa-game <game dir> <map>    play a map, e.g. \"(2v2)Berlin\"; aaa-editor lists them";

#[derive(Resource)]
struct Loaded {
    game: GameData,
    map: Map,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, map] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let loaded = GameData::open(&PathBuf::from(dir)).and_then(|mut game| {
        let name = game
            .find_map(map)
            .ok_or_else(|| format!("no map named {map:?}"))?;
        Map::load(&mut game, &name).map(|map| Loaded { game, map })
    });
    let loaded = match loaded {
        Ok(l) => l,
        Err(e) => {
            eprintln!("aaa-game: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: format!("{} - aaa-game", loaded.map.title),
            ..default()
        }),
        ..default()
    }));
    app.add_plugins(MapRenderPlugin)
        .insert_resource(loaded)
        .add_systems(Startup, setup);
    match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

fn setup(mut commands: Commands, mut loaded: ResMut<Loaded>, mut assets: MapAssets) {
    let Loaded { game, map } = &mut *loaded;
    let markers = Markers {
        placeholders: true,
        modelless: false,
    };
    aaa_render::spawn_map(&mut commands, &mut assets, game, map, markers);
    camera::spawn(&mut commands, RtsCamera::centred(&map.ground));
}
