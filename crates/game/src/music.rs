//! The music: one entry at a time, heard by the local player only, going
//! on from file to file until other music replaces it. The old fades out
//! as the new fades in, and music falls quieter while audio that asks it
//! to plays.

use std::collections::HashMap;

use aaa_data::{Audio, AudioCategory, GameData, Id, MusicSettings};
use bevy::audio::Volume;
use bevy::prelude::*;

use crate::sound::{Fades, Sounds, Volumes};
use crate::{Data, Loaded, Local, Screen, Sim, lobby};

/// How many files music and speech can stream at once; music on its way
/// out that would need more is cut off.
const STREAMS: usize = 3;

/// Seconds a file may take to start playing before it's given up on, as
/// one that can't be played.
const START_TIMEOUT: f64 = 2.0;

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
    /// The current music's files, read before a match loads takes the game
    /// data away, so that it plays on through the loading.
    ahead: HashMap<String, Handle<AudioSource>>,
}

struct Current {
    id: Id,
    audio: Audio,
    started: f64,
    /// The file playing, until it ends, and when it was set going.
    playing: Option<(Entity, f64)>,
    /// Whether it has played its lead-in, or had none.
    led_in: bool,
}

struct Leaving {
    playing: Entity,
    /// Its own volume.
    volume: f32,
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
        Music::with(game.music_settings())
    }

    fn with(settings: MusicSettings) -> Music {
        Music {
            settings,
            asked: None,
            current: None,
            leaving: Vec::new(),
            duck: 1.0,
            ducked_at: None,
            ahead: HashMap::new(),
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

    /// How loud music at fade level `fade` plays, at `volume` of its own,
    /// with the options' music at `options`.
    fn loudness(&self, options: f32, volume: f32, fade: f32) -> f32 {
        options * volume * fade * self.duck
    }

    /// How loud music on its way out plays `now`; none once it's silent.
    fn leaving_loudness(&self, options: f32, l: &Leaving, now: f64) -> Option<f32> {
        let level = l.from - (now - l.since) as f32 / l.out;
        (level > 0.0).then(|| self.loudness(options, l.volume, level))
    }

    /// A file of the current music's, read ahead or else from `game`.
    fn source(
        &self,
        game: Option<&mut GameData>,
        assets: &mut Assets<AudioSource>,
        path: &str,
    ) -> Option<Handle<AudioSource>> {
        if let Some(source) = self.ahead.get(path) {
            return Some(source.clone());
        }
        let bytes = game?.read(path);
        if bytes.is_none() {
            warn!("{path}: can't be read");
        }
        Some(assets.add(AudioSource {
            bytes: bytes?.into(),
        }))
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
            .add_systems(OnEnter(Screen::Loading), read_ahead)
            .add_systems(OnEnter(Screen::Match), |mut commands: Commands| {
                commands.insert_resource(MatchMusicDue)
            })
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.remove_resource::<MatchMusicDue>()
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
    // The local player's nation's, where it has some; the observers' where
    // nobody is playing.
    let asked = match local.and_then(|l| l.0) {
        Some(p) => {
            let id = sim.0.sides.player(p).and_then(|p| p.nation.as_deref());
            let id = id.map(Id::from);
            let nation = id.and_then(|id| nations.0.iter().find(|n| n.id == id));
            nation.and_then(|n| n.music.clone())
        }
        None => music.settings.observer.clone(),
    };
    if let Some(asked) = asked {
        music.ask(&asked);
    }
}

/// Reads the current music's files while the game data is still at hand.
fn read_ahead(
    mut music: ResMut<Music>,
    mut loaded: Option<ResMut<Loaded>>,
    mut data: Option<ResMut<Data>>,
    mut assets: ResMut<Assets<AudioSource>>,
) {
    let Some(current) = &music.current else {
        return;
    };
    let game = match (&mut loaded, &mut data) {
        (Some(loaded), _) => &mut loaded.game,
        (None, Some(data)) => &mut data.0,
        (None, None) => return,
    };
    let files: Vec<String> = (current.audio.leadin.iter().chain(&current.audio.files))
        .filter(|f| !music.ahead.contains_key(*f))
        .cloned()
        .collect();
    for file in files {
        if let Some(source) = music.source(Some(&mut *game), &mut assets, &file) {
            music.ahead.insert(file, source);
        }
    }
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
    volumes: Res<Volumes>,
    time: Res<Time<Real>>,
    faders: Query<(), With<Fades>>,
    mut sinks: Query<&mut AudioSink>,
) {
    let now = time.elapsed_secs_f64();
    let game = match (&mut loaded, &mut data) {
        (Some(loaded), _) => Some(&mut loaded.game),
        (None, Some(data)) => Some(&mut data.0),
        (None, None) => None,
    };
    let options = volumes.of(AudioCategory::Music);
    // Turned all the way down, music isn't decoded: what's leaving stops,
    // and the current file waits, paused, to go on from where it was.
    let silent = options <= 0.0;

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
        music.ahead.clear();
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
        let loudness = music.leaving_loudness(options, l, now);
        let sink = sinks.get_mut(l.playing);
        let ended = sink.as_ref().is_ok_and(|s| s.empty());
        let Some(loudness) = loudness.filter(|_| !ended && !silent) else {
            commands.entity(l.playing).try_despawn();
            return false;
        };
        if let Ok(mut sink) = sink {
            sink.set_volume(Volume::Linear(loudness));
        }
        true
    });
    // The new music takes a stream of its own.
    while leaving.len() >= STREAMS {
        commands.entity(leaving.remove(0).playing).try_despawn();
    }
    music.leaving = leaving;

    let Some(mut current) = music.current.take() else {
        return;
    };
    let fade = fade_in((now - current.started) as f32, current.audio.fade[0]);
    let loudness = music.loudness(options, current.audio.volume, fade);
    if let Some((e, set_going)) = current.playing {
        match sinks.get_mut(e) {
            Ok(sink) if sink.empty() => {
                commands.entity(e).despawn();
                current.playing = None;
            }
            Ok(mut sink) => {
                sink.set_volume(Volume::Linear(loudness));
                match (silent, sink.is_paused()) {
                    (true, false) => sink.pause(),
                    (false, true) => sink.play(),
                    _ => {}
                }
            }
            Err(_) if now - set_going > START_TIMEOUT => {
                warn!("music {}: a file didn't start playing", current.id);
                commands.entity(e).try_despawn();
                current.playing = None;
            }
            // Not started yet.
            Err(_) => {}
        }
    }
    // Music waiting for the volume to come up fades in once it does,
    // rather than from when it was asked for.
    if silent && current.playing.is_none() {
        current.started = now;
    }
    if current.playing.is_none()
        && !silent
        && let Some(file) = next(&mut current, || sounds.random())
        && let Some(source) = music.source(game, &mut assets, &file)
    {
        debug!("music {}: {file}", current.id);
        let e = commands
            .spawn((
                AudioPlayer::new(source),
                PlaybackSettings::ONCE.with_volume(Volume::Linear(loudness)),
            ))
            .id();
        current.playing = Some((e, now));
    }
    music.current = Some(current);
}

/// The file music plays next: its lead-in, if it has one and hasn't yet,
/// then any of its files, each pick as likely as the next.
fn next(current: &mut Current, random: impl FnOnce() -> f32) -> Option<String> {
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
    let i = (random() * list.len() as f32) as usize;
    Some(list[i.min(list.len() - 1)].clone())
}

/// Sets replaced music fading out, or stops it where it has no fade-out.
fn leave(commands: &mut Commands, music: &mut Music, old: Current, now: f64) {
    let Some((playing, _)) = old.playing else {
        return;
    };
    let [fade_in, out] = old.audio.fade;
    let from = fade_out_from((now - old.started) as f32, fade_in, out);
    match from > 0.0 {
        true => music.leaving.push(Leaving {
            playing,
            volume: old.audio.volume,
            from,
            since: now,
            out,
        }),
        false => commands.entity(playing).try_despawn(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use super::*;

    fn settings() -> MusicSettings {
        MusicSettings {
            silent: Id::from("music_silent"),
            observer: None,
            menu: None,
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

    pub(crate) fn silent() -> Audio {
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
        let mut music = Music::with(settings());
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

    #[test]
    fn leads_in_once_then_picks_from_its_files() {
        let mut audio = silent();
        audio.leadin = vec!["theme".into()];
        audio.files = vec!["a".into(), "b".into()];
        let mut current = Current {
            id: Id::from("title"),
            led_in: false,
            audio,
            started: 0.0,
            playing: None,
        };
        assert_eq!(next(&mut current, || 0.9).as_deref(), Some("theme"));
        assert_eq!(next(&mut current, || 0.9).as_deref(), Some("b"));
        assert_eq!(next(&mut current, || 0.0).as_deref(), Some("a"));
        // Never past the end, however the pick falls.
        assert_eq!(next(&mut current, || 1.0).as_deref(), Some("b"));
    }

    /// Music asked for long ago with a five-second fade-in, of one file
    /// read ahead, playing with the music volume at `music`.
    fn app(music: f32) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Assets<AudioSource>>()
            .insert_resource(Sounds::of(HashMap::new(), &[]))
            .insert_resource(Volumes(aaa_data::Volumes {
                main: 1.0,
                fx: 1.0,
                speech: 1.0,
                music,
            }))
            .add_systems(Update, play);
        let mut audio = silent();
        audio.files = vec!["theme.mp3".into()];
        audio.volume = 1.0;
        audio.fade = [5.0, 0.0];
        let source = app
            .world_mut()
            .resource_mut::<Assets<AudioSource>>()
            .add(AudioSource {
                bytes: Arc::from(&[][..]),
            });
        let mut music = Music::with(settings());
        music.ahead.insert("theme.mp3".into(), source);
        music.current = Some(Current {
            id: Id::from("theme"),
            audio,
            started: -100.0,
            playing: None,
            led_in: true,
        });
        app.insert_resource(music);
        app
    }

    fn players(app: &mut App) -> Vec<f32> {
        let world = app.world_mut();
        let mut players = world.query_filtered::<&PlaybackSettings, With<AudioPlayer>>();
        players.iter(world).map(|p| p.volume.to_linear()).collect()
    }

    #[test]
    fn music_turned_down_waits_and_fades_in_once_turned_up() {
        let mut app = app(0.0);
        app.update();
        assert!(players(&mut app).is_empty());
        app.world_mut().resource_mut::<Volumes>().0.music = 1.0;
        app.update();
        // Started now, not when it was asked for: at the start of its fade.
        let started = players(&mut app);
        assert_eq!(started.len(), 1);
        assert!(started[0] < 0.1, "{started:?}");
    }

    #[test]
    fn music_turned_up_plays_at_once() {
        let mut app = app(1.0);
        app.update();
        let started = players(&mut app);
        assert_eq!(started.len(), 1);
        assert!(started[0] > 0.9, "{started:?}");
    }

    #[test]
    fn fades_out_at_its_own_volume() {
        let music = Music::with(settings());
        let leaving = Leaving {
            playing: Entity::PLACEHOLDER,
            volume: 1.33,
            from: 1.0,
            since: 10.0,
            out: 2.0,
        };
        let full = 0.56 * 1.33;
        let at = |now| music.leaving_loudness(0.56, &leaving, now);
        assert!((at(10.0).unwrap() - full).abs() < 1e-6);
        assert!((at(11.0).unwrap() - full / 2.0).abs() < 1e-6);
        assert_eq!(at(12.0), None);
    }
}
