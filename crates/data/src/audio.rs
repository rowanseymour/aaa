//! What the game plays: sounds, speech and music, how loud each is, and
//! the cues that set templates' sounds off.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{AnimationKind, GameData, Id, defs, logged, scoped};

/// An [`Audio`] an object plays when something happens.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub audio: Id,
    /// Where it plays, e.g. `SELF`, or `TARGET_GROUND` for a shot's landing.
    pub at: String,
    /// Seconds after what sets it off.
    pub delay: f32,
    pub looping: bool,
    /// It plays on one in this many of the times it's set off.
    pub every: u32,
}

/// The audio an object template plays, by what sets it off; its weapons'
/// are on its [`Weapon`](crate::Weapon)s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectAudio {
    pub selected: Vec<Cue>,
    /// Given an order, or an order to attack.
    pub commanded: Vec<Cue>,
    pub commanded_aggressive: Vec<Cue>,
    pub created: Vec<Cue>,
    pub destroyed: Vec<Cue>,
    pub start_moving: Vec<Cue>,
    pub stop_moving: Vec<Cue>,
    /// On entering and leaving states, by state, e.g. `construction`.
    pub entered: Vec<(Id, Cue)>,
    pub left: Vec<(Id, Cue)>,
    /// When something disturbs it, as a vehicle crushing a doodad does.
    pub disturbed: Vec<Cue>,
    /// Played as its model's animations start.
    pub animations: Vec<AnimationAudio>,
}

impl ObjectAudio {
    fn is_empty(&self) -> bool {
        *self == ObjectAudio::default()
    }
}

/// An [`Audio`] played as one of a model's animations starts.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationAudio {
    pub kind: AnimationKind,
    pub ids: Option<String>,
    /// For [`AnimationKind::Random`] animations, the range of seconds it
    /// waits, from being created and from the end of each play, before it
    /// starts again.
    pub interval: Option<[f32; 2]>,
    pub audio: Id,
    pub looping: bool,
}

/// What music plays when, and how loud, beyond what each music entry says.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicSettings {
    /// The music that plays nothing, for stopping music.
    pub silent: Id,
    /// The match music of someone watching rather than playing.
    pub observer: Option<Id>,
    /// What the main menu asks for.
    pub menu: Option<Id>,
    /// What music falls to while audio that fades it plays, as a fraction
    /// of its level.
    pub duck_volume: f32,
    /// Seconds music takes to fall or recover.
    pub duck_time: f32,
    /// Seconds after the last audio that fades it that music recovers.
    pub recover_after: f32,
}

/// Where the options' FX volume starts, which the data doesn't set: where
/// the game's own options show it.
const FX_VOLUME: f32 = 0.8;

/// How loud the options have audio play, each from 0 for silent to 1 for
/// full.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volumes {
    pub main: f32,
    pub fx: f32,
    pub speech: f32,
    pub music: f32,
}

/// Which of the sounds the options can switch off are on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SoundSwitches {
    /// Units' spoken replies to being selected or given an order.
    pub replies: bool,
    /// The sound that greets a chat message in a match.
    pub chat: bool,
}

/// A sound, speech or music, as the game asks for it by id.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub category: AudioCategory,
    /// Archive paths: one of the lead-ins plays first, then the main files
    /// (one, or all where [`AudioControl::all`]), then one of the fall-outs.
    pub leadin: Vec<String>,
    pub files: Vec<String>,
    pub fallout: Vec<String>,
    /// Files it names that aren't in the archives, as written.
    pub missing: Vec<String>,
    pub priority: AudioPriority,
    pub control: AudioControl,
    pub world: AudioWorld,
    pub who: AudioWho,
    /// How steeply it falls off with distance; lower is steeper.
    pub falloff: f32,
    /// The distance beyond which it can't be heard.
    pub range: f32,
    /// How many can play at once.
    pub limit: u32,
    /// Its loudness, 1 for full.
    pub volume: f32,
    /// Up to how much quieter each play is picked, as a fraction of
    /// `volume`, 0 or less.
    pub volume_adjust: f32,
    /// The range each play's pitch is picked from, as deltas from 1.
    pub pitch_adjust: [f32; 2],
    /// The range each play's delay is picked from, in seconds.
    pub delay: [f32; 2],
    /// Seconds to fade in and out.
    pub fade: [f32; 2],
    /// The localisation key of the text shown while it plays.
    pub subtitle: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCategory {
    Sound,
    Speech,
    Music,
}

/// Which plays win when there are more than channels to play them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioPriority {
    Lowest,
    Low,
    Medium,
    High,
    Critical,
}

/// How an [`Audio`] is played.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioControl {
    /// A new play takes over an old one when out of channels or at its limit.
    pub interrupt: bool,
    /// Its files are played in order instead of picked at random.
    pub sequential: bool,
    /// Each play plays all its files.
    pub all: bool,
    /// It is spoken by an object, which says one such at a time.
    pub voice: bool,
    /// It is heard from a place in the world.
    pub world: bool,
    /// It fades other kinds of audio down while it plays.
    pub cause_fade: bool,
    /// It is the sample played by the options' volume sliders.
    pub test: bool,
}

/// How an [`Audio`] played in the world is heard.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioWorld {
    /// From anywhere on the map, however far.
    pub global: bool,
    /// Not where the listener's fog covers it.
    pub fogged: bool,
}

/// Who hears an [`Audio`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioWho {
    /// The local player, whom it is meant for.
    pub local: bool,
    /// Its owner, when they caused it.
    pub player: bool,
    pub allies: bool,
    pub enemies: bool,
}

impl GameData {
    /// The game's settings for how it looks and sounds, the game's own
    /// before the engine's.
    pub(crate) fn avars(&self) -> Arc<Vec<defs::AVars>> {
        self.kept("avars", || {
            // Not in the load lists.
            let files = self.parse_named(["AVars_AA.tgi", "AVars.tgi"]);
            let files = scoped(&files);
            files.iter().flat_map(|f| f.read("Vars")).collect()
        })
    }

    /// What music plays when, and how loud.
    pub fn music_settings(&self) -> MusicSettings {
        let vars = self.avars();
        let var =
            |of: fn(&defs::AVars) -> Option<f32>, or: f32| vars.iter().find_map(of).unwrap_or(or);
        let id = |of: fn(&defs::AVars) -> Option<Id>| vars.iter().find_map(of);
        let menu = (self.parsed("UI/Menus/main.tgi")).and_then(|parsed| {
            let block = parsed.root.blocks_of("MainMenuInterface").next()?;
            logged(
                "UI/Menus/main.tgi",
                block,
                tgi::from_block::<defs::Interface>(block),
            )?
            .interface_music
        });
        MusicSettings {
            silent: id(|v| v.audio_music_silent.clone())
                .unwrap_or_else(|| Id::from("music_silent")),
            observer: id(|v| v.observer_game_music.clone()),
            menu,
            duck_volume: var(|v| v.audio_fade_volume, 0.5),
            duck_time: var(|v| v.audio_fade_time, 1.0),
            recover_after: var(|v| v.audio_recover_from_fade_time, 2.0),
        }
    }

    /// The options' volumes, as they start.
    pub fn volumes(&self) -> Volumes {
        let uvars = self.uvars();
        let var =
            |of: fn(&defs::UVars) -> Option<f32>, or: f32| uvars.iter().find_map(of).unwrap_or(or);
        Volumes {
            main: var(|v| v.audio_main_volume, 0.8),
            fx: FX_VOLUME,
            speech: var(|v| v.audio_speech_volume, 0.95),
            music: var(|v| v.audio_music_volume, 0.7),
        }
    }

    /// The options' sound switches, as they start.
    pub fn sound_switches(&self) -> SoundSwitches {
        let uvars = self.uvars();
        let on = |of: fn(&defs::UVars) -> Option<bool>| uvars.iter().find_map(of).unwrap_or(true);
        SoundSwitches {
            replies: on(|v| v.unit_acknowledgement),
            chat: on(|v| v.chat_sound),
        }
    }

    /// The audio of each object template that plays any, by template id.
    pub fn object_audio(&self) -> HashMap<Id, ObjectAudio> {
        let mut out = self.by_template(|t| Some(object_audio(t)));
        out.retain(|_, a| !a.is_empty());
        out
    }

    /// Every sound, speech and music, by id. Streamed audio, music and
    /// speech, plays a stand-in, if the settings name one, for each file the
    /// archives don't have.
    pub fn audio(&self) -> Arc<HashMap<Id, Audio>> {
        self.kept("audio", || self.read_audio())
    }

    fn read_audio(&self) -> HashMap<Id, Audio> {
        let stand_in = (self.avars().iter())
            .find_map(|v| v.default_audio_streaming_resource.clone())
            .and_then(|p| self.path(&p.replace('\\', "/")));
        let files = self.parse_tgis(|n| n.starts_with("audio/") || n.starts_with("templates/"));
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::Audio>("Audio"))
            .filter_map(|a| {
                let streamed = (a.category.as_deref()).is_some_and(|c| {
                    c.eq_ignore_ascii_case("MUSIC") || c.eq_ignore_ascii_case("SPEECH")
                });
                let stand_in = stand_in.as_deref().filter(|_| streamed);
                Some((a.ids.clone()?, audio(&a, |p| self.path(p), stand_in)?))
            })
            .collect()
    }

    /// An archive path as the archives spell it.
    pub(crate) fn path(&self, path: &str) -> Option<String> {
        let &entry = self.paths.get(&path.to_ascii_lowercase())?;
        Some(self.entry_name(entry).to_string())
    }
}

fn object_audio(thing: &defs::Thing) -> ObjectAudio {
    let view = thing.view.as_ref();
    let mover = thing.mover_component.as_ref();
    let of_view =
        |f: fn(&defs::View) -> &Vec<defs::Fxc>| view.map(|v| cues(f(v))).unwrap_or_default();
    let of_mover = |f: fn(&defs::MoverComponent) -> &Vec<defs::Fxc>| {
        mover.map(|m| cues(f(m))).unwrap_or_default()
    };
    let by_state = |fxcs: &[defs::Fxc]| -> Vec<(Id, Cue)> {
        fxcs.iter()
            .filter_map(|f| Some((Id::from(f.actor_state.as_deref()?), cue(f)?)))
            .filter(|(state, _)| *state != Id::from("null"))
            .collect()
    };
    let aspects = view
        .into_iter()
        .flat_map(|v| v.aspect.iter().chain(&v.aspect_normal));
    let animations = aspects
        .flat_map(|a| &a.animation)
        .flat_map(|anim| {
            let kind = AnimationKind::parse(anim.kind.as_deref().unwrap_or_default());
            let interval = match (&kind, anim.start_min, anim.start_max) {
                (AnimationKind::Random, Some(min), Some(max)) => Some([min, max]),
                _ => None,
            };
            anim.play_sfx.iter().filter_map(move |p| {
                Some(AnimationAudio {
                    kind: kind.clone(),
                    ids: anim.ids.clone(),
                    interval,
                    audio: p.sfx_ids.clone()?,
                    looping: p.looping,
                })
            })
        })
        .collect();
    ObjectAudio {
        selected: of_view(|v| &v.when_selected),
        commanded: of_view(|v| &v.when_commanded),
        commanded_aggressive: of_view(|v| &v.when_commanded_aggressive),
        created: of_view(|v| &v.when_created),
        destroyed: of_view(|v| &v.when_destroyed),
        start_moving: of_mover(|m| &m.when_start_moving),
        stop_moving: of_mover(|m| &m.when_stop_moving),
        entered: by_state(&thing.when_enter),
        left: by_state(&thing.when_leave),
        disturbed: thing
            .component
            .iter()
            .flat_map(|c| cues(&c.action))
            .collect(),
        animations,
    }
}

/// The audio the commands play.
pub(crate) fn cues(fxcs: &[defs::Fxc]) -> Vec<Cue> {
    fxcs.iter().filter_map(cue).collect()
}

/// The audio a command plays, if it's one that plays audio.
fn cue(f: &defs::Fxc) -> Option<Cue> {
    let plays = f
        .kind
        .as_deref()
        .is_some_and(|k| k.eq_ignore_ascii_case("PLAY_SFX"));
    let every = f.frequency.unwrap_or(1);
    if !plays || every == 0 {
        return None;
    }
    Some(Cue {
        audio: f.audio_ids.clone()?,
        at: f.at.as_deref().unwrap_or("SELF").to_ascii_uppercase(),
        delay: f.delay.unwrap_or(0.0),
        looping: f.looping.unwrap_or(false),
        every,
    })
}

/// An audio definition with its files found by `find`, which gives an
/// archive path as the archives spell it.
/// Each file the archives don't have plays `stand_in`, where there's one.
fn audio(
    a: &defs::Audio,
    find: impl Fn(&str) -> Option<String>,
    stand_in: Option<&str>,
) -> Option<Audio> {
    let category = match a.category.as_deref()?.to_ascii_uppercase().as_str() {
        "SOUND" => AudioCategory::Sound,
        "SPEECH" => AudioCategory::Speech,
        "MUSIC" => AudioCategory::Music,
        _ => return None,
    };
    let mut missing = Vec::new();
    let mut paths = |list: &Option<String>| -> Vec<String> {
        let mut found = Vec::new();
        for file in audio_files(list.as_deref()) {
            match find(&audio_path(file)) {
                Some(p) => found.push(p),
                None => {
                    missing.push(file.to_string());
                    found.extend(stand_in.map(str::to_string));
                }
            }
        }
        found
    };
    let (leadin, files, fallout) = (paths(&a.leadin), paths(&a.files), paths(&a.fallout));
    let priority = match a
        .priority
        .as_deref()
        .map(str::to_ascii_uppercase)
        .as_deref()
    {
        Some("LOWEST") => AudioPriority::Lowest,
        Some("LOW") => AudioPriority::Low,
        Some("HIGH") => AudioPriority::High,
        Some("CRITICAL") => AudioPriority::Critical,
        _ => AudioPriority::Medium,
    };
    let mut control = AudioControl::default();
    for flag in flags(&a.control_flags) {
        match flag.as_str() {
            "INTERRUPT" => control.interrupt = true,
            "SEQUENTIAL" => control.sequential = true,
            "ALL" => control.all = true,
            "VOICE" => control.voice = true,
            "WORLD" => control.world = true,
            "CAUSEFADE" => control.cause_fade = true,
            "TESTSOUND" => control.test = true,
            _ => {}
        }
    }
    let mut world = AudioWorld::default();
    for flag in flags(&a.world_flags) {
        match flag.as_str() {
            "GLOBAL" => world.global = true,
            "FOGGED" => world.fogged = true,
            _ => {}
        }
    }
    let mut who = AudioWho::default();
    for flag in flags(&a.who_flags) {
        match flag.as_str() {
            "LOCAL" => who.local = true,
            "PLAYER" => who.player = true,
            "ALLIES" => who.allies = true,
            "ENEMIES" => who.enemies = true,
            "EVERYONE" => (who.player, who.allies, who.enemies) = (true, true, true),
            _ => {}
        }
    }
    let [falloff, range] = a.range.unwrap_or([4.0, 64.0]);
    Some(Audio {
        category,
        leadin,
        files,
        fallout,
        missing,
        priority,
        control,
        world,
        who,
        falloff,
        range,
        limit: a.simultaneous_limit.unwrap_or(2),
        volume: a.volume.unwrap_or(0.8),
        volume_adjust: a.volume_adjust.unwrap_or(0.0),
        pitch_adjust: a.pitch_adjust.unwrap_or([0.0; 2]),
        delay: a.delay_ms.unwrap_or_default().map(|ms| ms / 1000.0),
        fade: a.fade_time_s.unwrap_or_default(),
        subtitle: a.subtitle.as_ref().and_then(|s| s.text.clone()),
    })
}

/// The archive path of a file an [`Audio`] names: from the root where it
/// starts with a `/`, else from `Audio/`.
fn audio_path(file: &str) -> String {
    let file = file.replace('\\', "/");
    match file.strip_prefix('/') {
        Some(rooted) => rooted.to_string(),
        None => format!("Audio/{file}"),
    }
}

/// The files of an audio file list, which `NULL` leaves empty.
fn audio_files(list: Option<&str>) -> impl Iterator<Item = &str> {
    list.into_iter()
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .filter(|f| !f.is_empty() && !f.eq_ignore_ascii_case("null"))
}

/// The upper-case flags of a comma separated list.
fn flags(list: &Option<String>) -> Vec<String> {
    audio_files(list.as_deref())
        .map(str::to_ascii_uppercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::fighter;

    #[test]
    fn audio_takes_schema_defaults_and_finds_its_files() {
        // Shaped like the game's schema: typed declarations with defaults.
        let parsed = tgi::parse(
            "[Template Audio]\n{\n\tstring IDS\n\tstring category\n\tstring files = NULL\n\
             \tstring priority = MEDIUM\n\tstring control_flags = NULL\n\
             \tstring who_flags = NULL\n\tpoint2 range = 4, 64\n\
             \tint simultaneous_limit = 2\n\tfloat volume = 80%\n\
             \tpoint2 pitch_adjust = 0%, 0%\n\tpoint2 delay_ms = 0, 0\n\
             \t[optional Subtitle]\n\t{\n\t\tfixed duration = 0\n\t\tlocalized text\n\t}\n}\n\
             [Template AudioSound inherit=Audio]\n{\n\tcategory = SOUND\n}\n\
             [Audio Template=AudioSound]\n{\n\tIDS = tank_select\n\
             \tfiles = Sounds/a.wav, sounds/B.wav,/Audio/Default/c.wav, Sounds/gone.wav\n\
             \tpriority = high\n\tcontrol_flags = VOICE, SEQUENTIAL\n\
             \twho_flags = LOCAL, EVERYONE\n\tvolume = 125%\n\
             \tpitch_adjust = -5%, 5%\n\tdelay_ms = 0, 250\n}\n\
             [Audio Template=AudioSound]\n{\n\tIDS = quiet\n\
             \t[Subtitle]\n\ttext = \"#quiet_text\"\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let archive = [
            "Audio/Sounds/a.wav",
            "Audio/Sounds/b.wav",
            "Audio/Default/c.wav",
        ];
        let find = |p: &str| {
            archive
                .iter()
                .find(|a| a.eq_ignore_ascii_case(p))
                .map(|a| a.to_string())
        };
        let first = parsed.root.blocks_of("Audio").next().unwrap();
        let stood_in = audio(&templates.deserialize(first).unwrap(), find, Some("x.wav")).unwrap();
        assert_eq!(stood_in.files.len(), archive.len() + 1);
        assert!(stood_in.files.iter().any(|f| f == "x.wav"));
        assert_eq!(stood_in.missing, ["Sounds/gone.wav"]);

        let mut blocks = parsed.root.blocks_of("Audio");
        let mut next = || {
            audio(
                &templates.deserialize(blocks.next().unwrap()).unwrap(),
                find,
                None,
            )
        };

        let select = next().unwrap();
        assert_eq!(select.category, AudioCategory::Sound);
        assert_eq!(select.files, archive);
        assert_eq!(select.missing, ["Sounds/gone.wav"]);
        assert_eq!(select.priority, AudioPriority::High);
        assert!(select.control.voice && select.control.sequential && !select.control.world);
        let everyone = AudioWho {
            local: true,
            player: true,
            allies: true,
            enemies: true,
        };
        assert_eq!(select.who, everyone);
        assert_eq!(select.volume, 1.25);
        assert_eq!(select.pitch_adjust, [-0.05, 0.05]);
        assert_eq!(select.delay, [0.0, 0.25]);

        let quiet = next().unwrap();
        assert!(quiet.files.is_empty() && quiet.missing.is_empty());
        assert_eq!(quiet.priority, AudioPriority::Medium);
        assert_eq!(quiet.control, AudioControl::default());
        assert_eq!((quiet.falloff, quiet.range, quiet.limit), (4.0, 64.0, 2));
        assert_eq!(quiet.volume, 0.8);
        assert_eq!(quiet.subtitle.as_deref(), Some("#quiet_text"));
    }

    #[test]
    fn objects_play_audio_from_their_sound_commands() {
        let parsed = tgi::parse(
            "[Template FXC]\n{\n\tstring type\n\tint frequency = 1\n\tfloat delay = 0\n\
             \tstring actor_state = NULL\n}\n\
             [Template FXC_PlaySFX inherit=FXC]\n{\n\ttype = PLAY_SFX\n\tstring audio_ids\n\
             \tstring at = SELF\n\tflag looping = false\n}\n\
             [Template FXC_CreateVFX inherit=FXC]\n{\n\ttype = CREATE_VFX\n\tstring vfx_ids\n}\n\
             [Thing]\n{\n\tIDS = bunker\n\
             \t[View]\n\t{\n\
             \t\t[WhenSelected Template=FXC_CreateVFX]\n\t\tvfx_ids = glow\n\
             \t\t[WhenSelected Template=FXC_PlaySFX]\n\t\taudio_ids = bunker_select\n\
             \t\t[WhenDestroyed Template=FXC_PlaySFX]\n\t\taudio_ids = rare\n\t\tfrequency = 0\n\
             \t\t[AspectNormal]\n\t\t{\n\t\t\t[Animation]\n\t\t\t{\n\
             \t\t\t\ttype = RANDOM\n\t\t\t\tstart_min = 20\n\t\t\t\tstart_max = 60\n\
             \t\t\t\t[PlaySFX]\n\t\t\t\tsfx_ids = crickets\n\t\t\t}\n\t\t}\n\t}\n\
             \t[WhenLeave Template=FXC_PlaySFX]\n\taudio_ids = built\n\
             \tactor_state = construction\n\
             \t[PerformerComponent]\n\t{\n\t\t[Ability]\n\t\t{\n\
             \t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\tability_type = attack\n\
             \t\t\t\trange = 10\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\
             \t\t\t\tdamage_type_IDS = gunfire\n\t\t\t\tamount = 5\n\
             \t\t\t\t[WhenDo Template=FXC_PlaySFX]\n\t\t\t\taudio_ids = shot\n\
             \t\t\t\tat = target_ground\n\t\t\t\tdelay = 0.5\n\
             \t\t\t}\n\t\t}\n\t}\n\
             \t[BodyComponent]\n\thealth = 100\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let cue = |audio: &str, at: &str, delay| Cue {
            audio: Id::from(audio),
            at: at.to_string(),
            delay,
            looping: false,
            every: 1,
        };

        let audio = object_audio(&thing);
        assert_eq!(audio.selected, [cue("bunker_select", "SELF", 0.0)]);
        assert!(audio.destroyed.is_empty());
        assert_eq!(
            audio.left,
            [(Id::from("construction"), cue("built", "SELF", 0.0))]
        );
        let crickets = AnimationAudio {
            kind: AnimationKind::Random,
            ids: None,
            interval: Some([20.0, 60.0]),
            audio: Id::from("crickets"),
            looping: false,
        };
        assert_eq!(audio.animations, [crickets]);

        let fighter = fighter(&thing, &HashMap::new()).unwrap();
        assert_eq!(
            fighter.weapons[0].sounds,
            [cue("shot", "TARGET_GROUND", 0.5)]
        );
    }
}
