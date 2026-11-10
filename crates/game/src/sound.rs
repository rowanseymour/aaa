//! Plays the audio the game data gives objects as things happen to them,
//! and the interface's own. What sets off which [`Cue`] lives with what
//! notices it happening; this queues the cues and plays them, read from the
//! archives the first time each file is needed.

use std::collections::HashMap;
use std::sync::Arc;

use aaa_data::{Audio, AudioWho, Cue, GameData, Id, ObjectAudio};
use aaa_render::camera::RtsCamera;
use aaa_render::terrain::map_point;
use aaa_sim::Relation;
use bevy::audio::Volume;
use bevy::prelude::*;

use crate::{Data, Loaded, Local, Sim};

#[derive(Resource)]
pub struct Sounds {
    audio: HashMap<Id, Audio>,
    objects: HashMap<Id, ObjectAudio>,
    /// By archive path; `None` for files that couldn't be read.
    sources: HashMap<String, Option<Handle<AudioSource>>>,
    /// Files read ahead, by archive path, to play even while no game data
    /// is at hand.
    read: HashMap<String, Arc<[u8]>>,
    /// Each sequential audio's next file.
    next: HashMap<Id, usize>,
    /// Cues waiting to play, with when.
    queued: Vec<(f64, Request)>,
    rng: u64,
}

struct Request {
    audio: Id,
    /// Where in the world it plays, for audio heard from a place.
    at: Option<[f32; 2]>,
    /// The player whose object set it off; none for the interface's.
    owner: Option<u32>,
}

/// One play of an [`Audio`].
#[derive(Component)]
struct Playing {
    audio: Id,
    /// When it started, to know which of an audio's plays is oldest.
    started: f64,
}

impl Sounds {
    /// With the files of the `ready` audio read ahead. Objects' audio is
    /// left to [`Sounds::load_objects`], for a match.
    pub fn new(game: &mut GameData, ready: &[&str]) -> Sounds {
        let audio = game.audio();
        debug!("{} audio", audio.len());
        let read = ready
            .iter()
            .filter_map(|id| audio.get(&Id::from(*id)))
            .flat_map(|a| a.leadin.iter().chain(&a.files))
            .filter_map(|path| Some((path.clone(), Arc::from(game.read(path)?))))
            .collect();
        Sounds {
            audio,
            objects: HashMap::new(),
            sources: HashMap::new(),
            read,
            next: HashMap::new(),
            queued: Vec::new(),
            rng: 0x2545_f491_4f6c_dd1d,
        }
    }

    /// Reads the audio object templates play.
    pub fn load_objects(&mut self, game: &mut GameData) {
        self.objects = game.object_audio();
        debug!("audio played by {} object templates", self.objects.len());
    }

    /// Queues the cues `owner`'s object of a template plays for an event,
    /// at `at` for any heard from where they play.
    pub fn cue(
        &mut self,
        time: &Time,
        template: &str,
        event: fn(&ObjectAudio) -> &Vec<Cue>,
        at: [f32; 2],
        owner: u32,
    ) {
        let cues = match self.objects.get(&Id::from(template)) {
            Some(o) => event(o).clone(),
            None => return,
        };
        self.play(time, &cues, |_| at, owner);
    }

    /// Queues the cues of `owner`'s projectile of a template landing at
    /// `at`, on a target at `to`: each where it says, the projectile's own
    /// where it landed.
    pub fn landed(&mut self, time: &Time, template: &Id, at: [f32; 2], to: [f32; 2], owner: u32) {
        let cues = match self.objects.get(template) {
            Some(o) => o.destroyed.clone(),
            None => return,
        };
        self.shot(time, &cues, at, to, owner);
    }

    /// Queues the cues of a shot from `from` at `to`, each where it says.
    pub fn shot(&mut self, time: &Time, cues: &[Cue], from: [f32; 2], to: [f32; 2], owner: u32) {
        let at = |c: &Cue| match c.at.starts_with("TARGET") {
            true => to,
            false => from,
        };
        self.play(time, cues, at, owner);
    }

    /// Queues cues set off by `owner`'s object, each played where `at` says;
    /// those that play only some of the times they're set off may not.
    fn play(&mut self, time: &Time, cues: &[Cue], at: impl Fn(&Cue) -> [f32; 2], owner: u32) {
        for cue in cues {
            if self.random() * cue.every as f32 >= 1.0 {
                continue;
            }
            let request = Request {
                audio: cue.audio.clone(),
                at: Some(at(cue)),
                owner: Some(owner),
            };
            let due = time.elapsed_secs_f64() + f64::from(cue.delay);
            self.queued.push((due, request));
        }
    }

    /// Queues an interface audio, such as a button's click.
    pub fn interface(&mut self, time: &Time, audio: &str) {
        let request = Request {
            audio: Id::from(audio),
            at: None,
            owner: None,
        };
        self.queued.push((time.elapsed_secs_f64(), request));
    }

    /// A number in `0..1`.
    fn random(&mut self) -> f32 {
        // xorshift64
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn between(&mut self, [a, b]: [f32; 2]) -> f32 {
        a + (b - a) * self.random()
    }

    /// The file a play of `audio` plays: the next in turn for sequential
    /// audio, else any.
    fn pick(&mut self, id: &Id, audio: &Audio) -> Option<String> {
        let files = match audio.files.is_empty() {
            true => &audio.leadin,
            false => &audio.files,
        };
        if files.is_empty() {
            return None;
        }
        let i = match audio.control.sequential {
            true => {
                let next = self.next.entry(id.clone()).or_default();
                let i = *next % files.len();
                *next = i + 1;
                i
            }
            false => ((self.random() * files.len() as f32) as usize).min(files.len() - 1),
        };
        Some(files[i].clone())
    }

    fn source(
        &mut self,
        game: Option<&mut GameData>,
        assets: &mut Assets<AudioSource>,
        path: &str,
    ) -> Option<Handle<AudioSource>> {
        if let Some(source) = self.sources.get(path) {
            return source.clone();
        }
        let ahead = self.read.remove(path);
        if ahead.is_none() && game.is_none() {
            return None;
        }
        self.sources
            .entry(path.to_string())
            .or_insert_with(|| {
                let bytes = ahead.or_else(|| game?.read(path).map(Arc::from));
                if bytes.is_none() {
                    warn!("{path}: can't be read");
                }
                Some(assets.add(AudioSource { bytes: bytes? }))
            })
            .clone()
    }
}

/// Whether the local player hears audio for `who` set off by an object of a
/// player it stands toward as `relation`; the interface's, which no object
/// set off, always.
fn hears(who: AudioWho, relation: Option<Relation>) -> bool {
    match relation {
        None => true,
        Some(Relation::Itself) => who.local || who.player,
        Some(Relation::Ally) => who.allies,
        Some(_) => who.enemies,
    }
}

/// How loud audio heard from a place is `distance` away: falling off more
/// steeply the lower `falloff` is, to silence at `range`. The game's own
/// curve isn't known; this keeps to what its data says of it.
fn gain(distance: f32, falloff: f32, range: f32) -> f32 {
    if distance >= range {
        return 0.0;
    }
    let falloff = falloff.max(f32::EPSILON);
    falloff / (falloff + distance) * (1.0 - distance / range)
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, play);
    }
}

/// Plays the queued cues that are due, to whoever hears them where the
/// camera looks. Files are read from the game data wherever it is, the
/// menus' or the match's; one not read before can't be while a match loads.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    mut sounds: ResMut<Sounds>,
    mut loaded: Option<ResMut<Loaded>>,
    mut data: Option<ResMut<Data>>,
    mut assets: ResMut<Assets<AudioSource>>,
    sim: Option<Res<Sim>>,
    local: Option<Res<Local>>,
    time: Res<Time>,
    cameras: Query<&RtsCamera>,
    playing: Query<(Entity, &Playing)>,
) {
    let now = time.elapsed_secs_f64();
    if sounds.queued.iter().all(|(due, _)| *due > now) {
        return;
    }
    let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut sounds.queued)
        .into_iter()
        .partition(|(due, _)| *due <= now);
    sounds.queued = waiting;
    let listener = cameras.single().ok().map(|c| map_point(c.focus));
    let mut counts: HashMap<Id, Vec<(f64, Entity)>> = HashMap::new();
    for (e, p) in &playing {
        counts
            .entry(p.audio.clone())
            .or_default()
            .push((p.started, e));
    }
    for (_, request) in due {
        let Some(audio) = sounds.audio.get(&request.audio).cloned() else {
            continue;
        };
        let relation = request
            .owner
            .map(|o| match (&sim, local.as_ref().and_then(|l| l.0)) {
                (Some(sim), Some(p)) => sim.0.sides.relation(p, o),
                _ => Relation::Neutral,
            });
        if !hears(audio.who, relation) {
            continue;
        }
        let mut volume = audio.volume * (1.0 + sounds.between([audio.volume_adjust, 0.0]));
        if audio.control.world && !audio.world.global {
            let (Some(at), Some(listener)) = (request.at, listener) else {
                continue;
            };
            let distance = Vec2::from(at).distance(Vec2::from(listener));
            volume *= gain(distance, audio.falloff, audio.range);
        }
        if volume <= 0.0 {
            continue;
        }
        let plays = counts.entry(request.audio.clone()).or_default();
        if plays.len() >= audio.limit as usize {
            // The interface's own cut off its oldest, as the game's
            // interface audio template has them do.
            if !audio.control.interrupt && request.owner.is_some() {
                continue;
            }
            plays.sort_by(|a, b| a.0.total_cmp(&b.0));
            let (_, oldest) = plays.remove(0);
            commands.entity(oldest).despawn();
        }
        let Some(file) = sounds.pick(&request.audio, &audio) else {
            continue;
        };
        let game = match (&mut loaded, &mut data) {
            (Some(loaded), _) => Some(&mut loaded.game),
            (None, Some(data)) => Some(&mut data.0),
            (None, None) => None,
        };
        let Some(source) = sounds.source(game, &mut assets, &file) else {
            continue;
        };
        let speed = 1.0 + sounds.between(audio.pitch_adjust);
        debug!("{}: {file} at {volume:.2}", request.audio);
        let e = commands
            .spawn((
                AudioPlayer::new(source),
                PlaybackSettings::DESPAWN
                    .with_volume(Volume::Linear(volume))
                    .with_speed(speed),
                Playing {
                    audio: request.audio.clone(),
                    started: now,
                },
            ))
            .id();
        plays.push((now, e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_fades_to_silence_at_its_range() {
        assert_eq!(gain(0.0, 4.0, 64.0), 1.0);
        assert!(gain(4.0, 4.0, 64.0) < 0.5);
        assert!(gain(4.0, 2.0, 64.0) < gain(4.0, 4.0, 64.0));
        assert_eq!(gain(64.0, 4.0, 64.0), 0.0);
        assert_eq!(gain(100.0, 4.0, 64.0), 0.0);
    }

    #[test]
    fn who_hears() {
        let callout = AudioWho {
            player: true,
            ..AudioWho::default()
        };
        assert!(hears(callout, Some(Relation::Itself)));
        assert!(!hears(callout, Some(Relation::Enemy)));
        let everyone = AudioWho {
            player: true,
            allies: true,
            enemies: true,
            ..AudioWho::default()
        };
        assert!(hears(everyone, Some(Relation::Ally)));
        assert!(hears(everyone, Some(Relation::Neutral)));
        assert!(hears(AudioWho::default(), None));
    }
}
