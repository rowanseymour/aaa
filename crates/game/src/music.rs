//! The music: one entry at a time, heard by the local player only, going
//! on from file to file until other music replaces it. The old fades out
//! as the new fades in, and music falls quieter while audio that asks it
//! to plays.

use std::collections::HashSet;

use aaa_data::{Audio, GameData, Id, MusicSettings};
use bevy::audio::Volume;
use bevy::prelude::*;

use crate::sound::{Fades, Sounds};
use crate::{Data, Loaded, Local, Screen, Sim, lobby};

/// How many files music and speech can stream at once; music on its way
/// out that would need more is cut off.
const STREAMS: usize = 3;

#[derive(Resource)]
pub struct Music {
    settings: MusicSettings,
    /// What was last asked for, to start next.
    asked: Option<Id>,
    current: Option<Current>,
    /// Music replaced, fading out, oldest first.
    leaving: Vec<Leaving>,
    /// How far ducking has it, 1 for not at all.
    duck: f32,
    /// When audio that ducks music was last heard playing.
    ducked_at: Option<f64>,
}

struct Current {
    id: Id,
    audio: Audio,
    started: f64,
    /// The file playing, until it ends.
    playing: Option<Entity>,
    /// Whether it has played its lead-in, or had none.
    led_in: bool,
}

struct Leaving {
    playing: Entity,
    /// Its fade level when it was replaced, and when that was.
    from: f32,
    since: f64,
    /// Seconds it takes to fall from full to silent.
    out: f32,
}

/// Whether the match has yet to ask for its music, which it does on its
/// first tick.
#[derive(Resource)]
struct MatchMusicDue;

impl Music {
    pub fn new(game: &mut GameData) -> Music {
        Music {
            settings: game.music_settings(),
            asked: None,
            current: None,
            leaving: Vec::new(),
            duck: 1.0,
            ducked_at: None,
        }
    }

    /// Asks for `id` to replace what's playing; asking for what's already
    /// playing leaves it be.
    pub fn ask(&mut self, id: &Id) {
        self.asked = match self.current.as_ref().is_some_and(|c| c.id == *id) {
            true => None,
            false => Some(id.clone()),
        };
    }

    /// Asks for the match music: that of the local player's nation, where
    /// it has some, or the observers' where nobody is playing.
    fn ask_for_match(&mut self, nation: Option<Option<&Id>>) {
        let music = match nation {
            Some(nation) => nation.cloned(),
            None => self.settings.observer.clone(),
        };
        if let Some(music) = music {
            self.ask(&music);
        }
    }

    /// How loud music at fade level `fade` plays, at `volume` of its own.
    fn loudness(&self, volume: f32, fade: f32) -> f32 {
        let s = &self.settings;
        s.main_volume * s.music_volume * volume * fade * self.duck
    }
}

/// How far into fading in music is `since` seconds after it started.
fn fade_in(since: f32, fade_in: f32) -> f32 {
    match fade_in > 0.0 {
        true => (since / fade_in).clamp(0.0, 1.0),
        false => 1.0,
    }
}

/// The fade level music starts fading out from, replaced `since` seconds
/// after it started: full, unless still fading in, when it falls as if it
/// had started fading out when it started.
fn fade_out_from(since: f32, fade_in: f32, fade_out: f32) -> f32 {
    match fade_out > 0.0 {
        false => 0.0,
        true if fade_in > 0.0 && since < fade_in => 1.0 - since / fade_out,
        true => 1.0,
    }
}

/// Where ducking takes music next, `by` seconds on: down while audio that
/// ducks it plays, back up once none has for long enough.
fn duck(level: f32, ducking: bool, quiet_for: Option<f32>, s: &MusicSettings, by: f32) -> f32 {
    let step = match s.duck_time > 0.0 {
        true => (1.0 - s.duck_volume) / s.duck_time * by,
        false => 1.0,
    };
    if ducking {
        (level - step).max(s.duck_volume)
    } else if quiet_for.is_none_or(|q| q >= s.recover_after) {
        (level + step).min(1.0)
    } else {
        level
    }
}

pub struct MusicPlugin;

impl Plugin for MusicPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::MainMenu), ask_for_menu)
            .add_systems(OnEnter(Screen::Match), |mut commands: Commands| {
                commands.insert_resource(MatchMusicDue)
            })
            .add_systems(
                Update,
                ask_for_match.run_if(resource_exists::<MatchMusicDue>),
            )
            .add_systems(Update, play.after(ask_for_match));
    }
}

fn ask_for_menu(mut music: ResMut<Music>) {
    if let Some(menu) = music.settings.menu.clone() {
        music.ask(&menu);
    }
}

fn ask_for_match(
    mut commands: Commands,
    mut music: ResMut<Music>,
    sim: Option<Res<Sim>>,
    local: Option<Res<Local>>,
    nations: Res<lobby::Nations>,
) {
    let Some(sim) = sim.filter(|s| s.0.ticks > 0) else {
        return;
    };
    commands.remove_resource::<MatchMusicDue>();
    let nation = local.and_then(|l| l.0).map(|p| {
        let id = sim.0.sides.player(p).and_then(|p| p.nation.as_deref());
        let nation = id.and_then(|id| nations.0.iter().find(|n| n.id.as_str() == id));
        nation.and_then(|n| n.music.as_ref())
    });
    music.ask_for_match(nation);
}

/// Starts what's been asked for, goes on to the next file as each ends,
/// and keeps every file playing at the loudness its fades and ducking give.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    mut music: ResMut<Music>,
    mut sounds: ResMut<Sounds>,
    mut loaded: Option<ResMut<Loaded>>,
    mut data: Option<ResMut<Data>>,
    mut assets: ResMut<Assets<AudioSource>>,
    time: Res<Time<Real>>,
    faders: Query<(), With<Fades>>,
    mut sinks: Query<&mut AudioSink>,
) {
    let now = time.elapsed_secs_f64();
    let mut game = match (&mut loaded, &mut data) {
        (Some(loaded), _) => Some(&mut loaded.game),
        (None, Some(data)) => Some(&mut data.0),
        (None, None) => None,
    };

    let ducking = !faders.is_empty();
    if ducking {
        music.ducked_at = Some(now);
    }
    let quiet_for = music.ducked_at.map(|t| (now - t) as f32);
    music.duck = duck(
        music.duck,
        ducking,
        quiet_for,
        &music.settings,
        time.delta_secs(),
    );

    if let Some(id) = music.asked.take()
        && let Some(audio) = sounds.audio(&id).cloned()
    {
        // Read now, while the game data is at hand, so that it plays on
        // through a match loading.
        let files: HashSet<&String> = audio.leadin.iter().chain(&audio.files).collect();
        for file in files {
            sounds.source(game.as_deref_mut(), &mut assets, file);
        }
        if let Some(old) = music.current.take() {
            leave(&mut commands, &mut music, old, now);
        }
        music.current = Some(Current {
            id,
            led_in: audio.leadin.is_empty(),
            audio,
            started: now,
            playing: None,
        });
    }

    // Fading out.
    let mut leaving = std::mem::take(&mut music.leaving);
    leaving.retain(|l| {
        let level = l.from - (now - l.since) as f32 / l.out;
        let sink = sinks.get_mut(l.playing);
        let ended = sink.as_ref().is_ok_and(|s| s.empty());
        if level <= 0.0 || ended {
            commands.entity(l.playing).despawn();
            return false;
        }
        if let Ok(mut sink) = sink {
            sink.set_volume(Volume::Linear(music.loudness(1.0, level)));
        }
        true
    });
    // The new music takes a stream of its own.
    while leaving.len() >= STREAMS {
        commands.entity(leaving.remove(0).playing).despawn();
    }
    music.leaving = leaving;

    let Some(mut current) = music.current.take() else {
        return;
    };
    let fade = fade_in((now - current.started) as f32, current.audio.fade[0]);
    let loudness = music.loudness(current.audio.volume, fade);
    if let Some(e) = current.playing {
        match sinks.get_mut(e) {
            Ok(sink) if sink.empty() => {
                commands.entity(e).despawn();
                current.playing = None;
            }
            Ok(mut sink) => sink.set_volume(Volume::Linear(loudness)),
            // Not started yet.
            Err(_) => {}
        }
    }
    if current.playing.is_none()
        && let Some(file) = next(&mut sounds, &mut current)
        && let Some(source) = sounds.source(game, &mut assets, &file)
    {
        debug!("music {}: {file}", current.id);
        let e = commands
            .spawn((
                AudioPlayer::new(source),
                PlaybackSettings::ONCE.with_volume(Volume::Linear(loudness)),
            ))
            .id();
        current.playing = Some(e);
    }
    music.current = Some(current);
}

/// The file music plays next: its lead-in, if it has one and hasn't yet,
/// then any of its files, each pick as likely as the next.
fn next(sounds: &mut Sounds, current: &mut Current) -> Option<String> {
    let list = match current.led_in {
        true => &current.audio.files,
        false => {
            current.led_in = true;
            &current.audio.leadin
        }
    };
    if list.is_empty() {
        return None;
    }
    let i = (sounds.random() * list.len() as f32) as usize;
    Some(list[i.min(list.len() - 1)].clone())
}

/// Sets replaced music fading out, or stops it where it has no fade-out.
fn leave(commands: &mut Commands, music: &mut Music, old: Current, now: f64) {
    let Some(playing) = old.playing else {
        return;
    };
    let [fade_in, out] = old.audio.fade;
    let from = fade_out_from((now - old.started) as f32, fade_in, out);
    match from > 0.0 {
        true => music.leaving.push(Leaving {
            playing,
            from,
            since: now,
            out,
        }),
        false => commands.entity(playing).despawn(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> MusicSettings {
        MusicSettings {
            silent: Id::from("music_silent"),
            observer: None,
            menu: None,
            main_volume: 0.8,
            music_volume: 0.7,
            duck_volume: 0.5,
            duck_time: 1.0,
            recover_after: 2.0,
        }
    }

    #[test]
    fn fades_in_once_over_its_fade_in() {
        assert_eq!(fade_in(0.0, 3.0), 0.0);
        assert_eq!(fade_in(1.5, 3.0), 0.5);
        assert_eq!(fade_in(10.0, 3.0), 1.0);
        assert_eq!(fade_in(0.0, 0.0), 1.0);
    }

    #[test]
    fn replaced_while_fading_in_falls_from_where_it_started() {
        assert_eq!(fade_out_from(10.0, 3.0, 1.0), 1.0);
        assert_eq!(fade_out_from(0.5, 4.0, 2.0), 0.75);
        // Already past its fade-out: cut off.
        assert!(fade_out_from(2.5, 4.0, 2.0) <= 0.0);
        assert_eq!(fade_out_from(10.0, 3.0, 0.0), 0.0);
    }

    #[test]
    fn ducks_while_faders_play_and_recovers_after_a_wait() {
        let s = settings();
        let level = duck(1.0, true, Some(0.0), &s, 0.5);
        assert_eq!(level, 0.75);
        assert_eq!(duck(level, true, Some(0.0), &s, 5.0), 0.5);
        // Waits before recovering.
        assert_eq!(duck(0.5, false, Some(1.0), &s, 0.5), 0.5);
        assert_eq!(duck(0.5, false, Some(2.0), &s, 0.5), 0.75);
        assert_eq!(duck(1.0, false, None, &s, 0.5), 1.0);
    }

    fn silent() -> Audio {
        Audio {
            category: aaa_data::AudioCategory::Music,
            leadin: Vec::new(),
            files: Vec::new(),
            fallout: Vec::new(),
            missing: Vec::new(),
            priority: aaa_data::AudioPriority::Medium,
            control: Default::default(),
            world: Default::default(),
            who: Default::default(),
            falloff: 4.0,
            range: 64.0,
            limit: 1,
            volume: 0.0,
            volume_adjust: 0.0,
            pitch_adjust: [0.0; 2],
            delay: [0.0; 2],
            fade: [0.0; 2],
            subtitle: None,
        }
    }

    #[test]
    fn asking_for_the_playing_music_does_nothing() {
        let mut music = Music {
            settings: settings(),
            asked: None,
            current: None,
            leaving: Vec::new(),
            duck: 1.0,
            ducked_at: None,
        };
        music.ask(&Id::from("a"));
        music.ask(&Id::from("b"));
        assert_eq!(music.asked, Some(Id::from("b")));
        music.current = Some(Current {
            id: Id::from("b"),
            audio: silent(),
            started: 0.0,
            playing: None,
            led_in: true,
        });
        music.ask(&Id::from("b"));
        assert_eq!(music.asked, None);
    }
}
