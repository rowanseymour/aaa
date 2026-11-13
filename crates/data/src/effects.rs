//! Visual effects: what each is drawn with, and what sets templates'
//! effects off.

use std::collections::HashMap;
use std::sync::Arc;

use crate::{GameData, Id, Targets, beside, defs, not_null, scoped, views::texture_dirs};

/// A visual effect the game shows by id.
#[derive(Debug, Clone, PartialEq)]
pub struct Vfx {
    /// What kind it is, e.g. `VFXMODEL`, `VFXPROJECTEDTEXTURE` or
    /// `VFXTRACER`.
    pub category: String,
    /// Archive path of its model, if it has one that's there.
    pub model: Option<String>,
    /// Archive folders searched for the model's textures.
    pub texture_dirs: Vec<String>,
    /// It lasts until stopped, rather than for a time.
    pub permanent: bool,
    /// It shows whatever the effects detail setting.
    pub critical: bool,
    /// Seconds a temporary one lasts; `None` for its model's own length.
    pub duration: Option<f32>,
    /// Attached, its size as a multiple of what it's attached to; 0 for
    /// its own size.
    pub attached_scale: f32,
    /// A temporary one is created off screen too.
    pub never_discard: bool,
    pub night_only: bool,
    /// The same size on screen however far the camera is.
    pub constant_screen_size: bool,
    /// It runs in real time rather than game time.
    pub real_time: bool,
    /// Each one is turned a random way about the vertical.
    pub random_orientation: bool,
    /// Attached, it keeps its facing as what it's attached to turns.
    pub gyroscope: bool,
    /// For one that tints what it's on, how.
    pub tint: Option<VfxTint>,
    /// For a picture laid on the ground, how.
    pub decal: Option<Decal>,
}

/// How a [`Vfx`] that's a picture laid on the ground shows.
#[derive(Debug, Clone, PartialEq)]
pub struct Decal {
    /// Archive path of the picture.
    pub texture: String,
    /// World units across it is, square.
    pub size: f32,
    /// What its colours are multiplied by.
    pub colour: [f32; 3],
    /// It's lit as the ground is, rather than looking the same day and
    /// night.
    pub lit: bool,
    /// It's added to the ground rather than laid over it.
    pub additive: bool,
    /// Seconds it takes to show once made, and to go once its time's up.
    pub fade: [f32; 2],
    /// Those of a higher layer are drawn over those of a lower.
    pub layer: u8,
}

/// The kinds of picture laid on the ground, the lowest first.
const DECAL_LAYERS: [&str; 6] = ["terrain", "scorch", "track", "generic", "ui", "shadow"];

/// How a [`Vfx`] tints what it's on, which pulses from untinted up to a
/// strength and back down, each way at a steady pace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VfxTint {
    /// What it multiplies colours by at full strength.
    pub colour: [f32; 3],
    /// The strength it rises to, where 1 is full.
    pub max: f32,
    /// Seconds it takes to rise from untinted to full; 0 to start at its
    /// top.
    pub rise: f32,
    /// Seconds it takes to fall from full to untinted; 0 to stay at its
    /// top.
    pub fall: f32,
    /// It rises again once it's fallen to untinted, rather than staying so.
    pub repeating: bool,
}

impl VfxTint {
    /// Its strength this many seconds after it started.
    pub fn level(&self, secs: f32) -> f32 {
        let (up, down) = (self.max * self.rise, self.max * self.fall);
        let t = match self.repeating && down > 0.0 {
            true => secs % (up + down),
            false => secs,
        };
        if t < up {
            t / self.rise
        } else if down <= 0.0 {
            self.max
        } else if t < up + down {
            self.max - (t - up) / self.fall
        } else {
            0.0
        }
    }

    /// What it multiplies colours by at a strength.
    pub fn colour(&self, level: f32) -> [f32; 3] {
        self.colour.map(|c| 1.0 - (1.0 - c) * level)
    }
}

/// A [`Vfx`] an object creates, or stops, when something happens.
#[derive(Debug, Clone, PartialEq)]
pub struct VfxCue {
    pub vfx: Id,
    /// It stops the effect of that id that the object's cues created,
    /// rather than creating one.
    pub stop: bool,
    /// Where it appears, e.g. `SELF`, `TARGET_GROUND` or
    /// `TARGET_HIT_POSITION`.
    pub at: String,
    /// The point of the model it appears at, e.g. `cannon_ae_01`; `None`
    /// for the model's origin.
    pub location: Option<String>,
    /// It follows what it's at as that moves.
    pub attached: bool,
    /// Offset from its spot, and the way it faces there.
    pub translation: [f32; 3],
    pub orientation: [f32; 3],
    /// Seconds it lasts in place of the effect's own.
    pub duration: Option<f32>,
    /// Seconds after what sets it off.
    pub delay: f32,
    /// It plays on one in this many of the times it's set off.
    pub every: u32,
    /// The terrain group it plays on, if only one.
    pub terrain: Option<Id>,
    /// What the event's target must be for it to play.
    pub targets: Targets,
    /// For a building being finished, it stops when the building's
    /// destroyed.
    pub stop_on_destructed: bool,
    /// Run when its effect ends, and alongside it.
    pub chain: Vec<VfxCue>,
    pub chain_immediate: Vec<VfxCue>,
}

/// A [`VfxCue`] run as an object's health crosses a share of its maximum.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthCue {
    pub health: f32,
    pub trigger: HealthTrigger,
    pub cue: VfxCue,
}

/// Which crossing of a [`HealthCue`]'s health runs it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HealthTrigger {
    #[default]
    Below,
    Above,
    /// Relative to the health it had, falling or rising by that share.
    Decrease,
    Increase,
}

/// The effects an object template shows, by what sets them off; its
/// weapons' are on its [`Weapon`](crate::Weapon)s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObjectEffects {
    pub created: Vec<VfxCue>,
    /// As a projectile, on reaching its target.
    pub destroyed: Vec<VfxCue>,
    /// When a building's finished.
    pub constructed: Vec<VfxCue>,
    pub start_moving: Vec<VfxCue>,
    pub stop_moving: Vec<VfxCue>,
    /// On entering and leaving states, by state, e.g. `construction`.
    pub entered: Vec<(Id, VfxCue)>,
    pub left: Vec<(Id, VfxCue)>,
    pub health: Vec<HealthCue>,
    /// When something disturbs it, as a vehicle crushing a doodad does.
    pub disturbed: Vec<VfxCue>,
}

impl ObjectEffects {
    fn is_empty(&self) -> bool {
        *self == ObjectEffects::default()
    }
}

/// The effects a [`Weapon`](crate::Weapon) shows each time it fires.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeaponEffects {
    /// As a shot begins, e.g. a tank gun's muzzle flash.
    pub start: Vec<VfxCue>,
    pub shot: Vec<VfxCue>,
    /// For each target a shot lands on.
    pub per_target: Vec<VfxCue>,
    pub end: Vec<VfxCue>,
}

impl GameData {
    /// Every visual effect, by id.
    pub fn vfx(&self) -> Arc<HashMap<Id, Vfx>> {
        self.kept("vfx", || self.read_vfx())
    }

    fn read_vfx(&self) -> HashMap<Id, Vfx> {
        // Effects live beside what uses them as well as in their own folder.
        let files = self.parse_tgis(|_| true);
        let files = scoped(&files);
        let mut out = HashMap::new();
        for scope in &files {
            // Ground decals are declared as things of a VFX category. Every
            // thing is tried as an effect to find them, so one that doesn't
            // read as an effect is no fault in the data.
            let effects = (scope.root.blocks_of("VFX")).filter_map(|b| scope.deserialize(b));
            let things = (scope.root.blocks_of("Thing"))
                .filter_map(|b| scope.templates.deserialize::<defs::Vfx>(b).ok());
            for v in effects.chain(things) {
                let is_vfx = v
                    .category
                    .as_deref()
                    .is_some_and(|c| c.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("VFX")));
                let (true, Some(ids)) = (is_vfx, v.ids.clone()) else {
                    continue;
                };
                let model = v
                    .model
                    .as_deref()
                    .map(|m| beside(scope.file, m))
                    .filter(|m| self.exists(m));
                let texture = (v.texture.as_deref())
                    .map(|t| beside(scope.file, t.trim()))
                    .filter(|t| self.exists(t));
                out.insert(ids, vfx(v, model, texture));
            }
        }
        out
    }

    /// The effects of each object template that shows any, by template id.
    pub fn object_effects(&self) -> HashMap<Id, ObjectEffects> {
        let mut out = self.by_template(|t| Some(object_effects(t)));
        out.retain(|_, e| !e.is_empty());
        out
    }

    /// The node names of the model points effects name by id, such as
    /// `gun_ae_01`.
    pub fn bones(&self) -> HashMap<Id, String> {
        let files = self.parse_tgis(|n| n.starts_with("game/bones"));
        let files = scoped(&files);
        files
            .iter()
            .flat_map(|f| f.read::<defs::Bone>("Bone"))
            .filter_map(|b| Some((b.ids?, b.bone_name?.trim().to_string())))
            .collect()
    }
}

fn vfx(v: defs::Vfx, model: Option<String>, texture: Option<String>) -> Vfx {
    let category = v.category.unwrap_or_default().to_ascii_uppercase();
    let layer = |kind: &str| {
        DECAL_LAYERS
            .iter()
            .position(|l| l.eq_ignore_ascii_case(kind))
    };
    // One sized or shaped by what it's attached to isn't a picture of its
    // own size.
    let shaped = v.oval_shape == Some(true) || v.attached_shape == Some(true);
    let own_size = v.attached_scale.unwrap_or(0.0) == 0.0 && !shaped;
    let decal = texture
        .filter(|_| category.starts_with("VFXPROJECTEDTEXTURE") && own_size)
        .map(|texture| Decal {
            texture,
            size: v.texture_size.unwrap_or(0.0),
            colour: v.color.unwrap_or([1.0; 3]),
            lit: v.respect_lighting.unwrap_or(false),
            additive: v.additive.unwrap_or(false),
            fade: v.decal_fade_time.unwrap_or([0.0; 2]),
            layer: (v.decal_type.as_deref().and_then(layer)).unwrap_or(3) as u8,
        })
        .filter(|d| d.size > 0.0);
    let flagged = v.flags.iter().rev().find_map(|f| f.random_orientation);
    Vfx {
        category,
        decal,
        model,
        texture_dirs: texture_dirs(&v.texture_search_path),
        permanent: v.permanent.unwrap_or(false),
        critical: v.critical.unwrap_or(true),
        duration: v.duration.filter(|d| *d > 0.0),
        attached_scale: v.attached_scale.unwrap_or(0.0),
        never_discard: v.never_discard_temporary.unwrap_or(false),
        night_only: v.nightonly.unwrap_or(false),
        constant_screen_size: v.constant_screen_size.unwrap_or(false),
        real_time: v.force_real_time.unwrap_or(false),
        random_orientation: flagged.or(v.random_orientation) == Some(true),
        gyroscope: v.gyroscope.unwrap_or(true),
        tint: v.color.map(|colour| VfxTint {
            colour,
            max: v.tint_max.unwrap_or(1.0),
            rise: v.enhance_duration.unwrap_or(0.0),
            fall: v.fade_duration.unwrap_or(0.0),
            repeating: v.repeating.unwrap_or(false),
        }),
    }
}

fn object_effects(thing: &defs::Thing) -> ObjectEffects {
    let view = thing.view.as_ref();
    let mover = thing.mover_component.as_ref();
    let of_view =
        |f: fn(&defs::View) -> &Vec<defs::Fxc>| view.map(|v| vfx_cues(f(v))).unwrap_or_default();
    let of_mover = |f: fn(&defs::MoverComponent) -> &Vec<defs::Fxc>| {
        mover.map(|m| vfx_cues(f(m))).unwrap_or_default()
    };
    let by_state = |fxcs: &[defs::Fxc]| -> Vec<(Id, VfxCue)> {
        fxcs.iter()
            .filter_map(|f| {
                Some((
                    not_null(f.actor_state.as_deref().map(Id::from))?,
                    vfx_cue(f)?,
                ))
            })
            .collect()
    };
    let health = thing
        .body_component
        .iter()
        .flat_map(|b| &b.when_health)
        .filter_map(|f| {
            let trigger = match f
                .health_trigger_type
                .as_deref()?
                .to_ascii_uppercase()
                .as_str()
            {
                "ABSOLUTE_BELOW" => HealthTrigger::Below,
                "ABSOLUTE_ABOVE" => HealthTrigger::Above,
                "RELATIVE_DECREASE" => HealthTrigger::Decrease,
                "RELATIVE_INCREASE" => HealthTrigger::Increase,
                _ => return None,
            };
            Some(HealthCue {
                health: f.health.filter(|h| *h >= 0.0)?,
                trigger,
                cue: vfx_cue(f)?,
            })
        })
        .collect();
    ObjectEffects {
        created: of_view(|v| &v.when_created),
        destroyed: of_view(|v| &v.when_destroyed),
        constructed: of_view(|v| &v.when_constructed),
        start_moving: of_mover(|m| &m.when_start_moving),
        stop_moving: of_mover(|m| &m.when_stop_moving),
        entered: by_state(&thing.when_enter),
        left: by_state(&thing.when_leave),
        health,
        disturbed: thing
            .component
            .iter()
            .flat_map(|c| vfx_cues(&c.action))
            .collect(),
    }
}

/// The effects the commands create or stop.
pub(crate) fn vfx_cues(fxcs: &[defs::Fxc]) -> Vec<VfxCue> {
    fxcs.iter().filter_map(vfx_cue).collect()
}

/// The effect a command creates or stops, if it's one that does.
fn vfx_cue(f: &defs::Fxc) -> Option<VfxCue> {
    let stop = match f.kind.as_deref()?.to_ascii_uppercase().as_str() {
        "CREATE_VFX" => false,
        "STOP_VFX" => true,
        _ => return None,
    };
    let every = f.frequency.unwrap_or(1);
    if every == 0 {
        return None;
    }
    Some(VfxCue {
        vfx: not_null(f.vfx_ids.clone())?,
        stop,
        at: f.at.as_deref().unwrap_or("SELF").to_ascii_uppercase(),
        location: f
            .location
            .clone()
            .filter(|l| !l.is_empty() && !l.eq_ignore_ascii_case("null")),
        attached: f.attached.unwrap_or(true),
        translation: f.translation.unwrap_or_default(),
        orientation: f.orientation.unwrap_or([0.0, -1.0, 0.0]),
        duration: f.duration.filter(|d| *d > 0.0),
        delay: f.delay.unwrap_or(0.0),
        every,
        terrain: not_null(f.terrain_group_filter.as_deref().map(Id::from)),
        targets: f
            .target_flags
            .as_deref()
            .filter(|t| !t.eq_ignore_ascii_case("null"))
            .map(Targets::parse)
            .unwrap_or_default(),
        stop_on_destructed: f.stop_on_destructed.unwrap_or(true),
        chain: vfx_cues(&f.chain),
        chain_immediate: vfx_cues(&f.chain_immediate),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::fighter;

    #[test]
    fn objects_show_effects_from_their_vfx_commands() {
        let parsed = tgi::parse(
            "[Template FXC]\n{\n\tstring type\n\tint frequency = 1\n\tfloat delay = 0\n\
             \tstring target_flags = null\n\tstring terrain_group_filter = null\n\
             \tfloat health = -1\n\tflag stop_on_destructed = true\n\
             \tstring health_trigger_type = ABSOLUTE_BELOW\n\tstring actor_state = NULL\n\
             \t[list Chain]\n\t[list ChainImmediate]\n}\n\
             [Template FXC_CreateVFX inherit=FXC]\n{\n\ttype = CREATE_VFX\n\tstring VFX_IDS\n\
             \tfloat duration = 0\n\tstring at = SELF\n\tflag attached = true\n\
             \tstring location = null\n\tpoint3 translation = 0,0,0\n\
             \tpoint3 orientation = 0,-1,0\n}\n\
             [Template FXC_StopVFX inherit=FXC]\n{\n\ttype = STOP_VFX\n\tstring VFX_IDS\n}\n\
             [Template FXC_PlaySFX inherit=FXC]\n{\n\ttype = PLAY_SFX\n\tstring audio_ids\n}\n\
             [Thing]\n{\n\tIDS = bunker\n\
             \t[View]\n\t{\n\
             \t\t[WhenConstructed Template=FXC_CreateVFX]\n\t\tvfx_ids = lit_window\n\
             \t\tlocation = window_01\n\t\tstop_on_destructed = false\n\
             \t\t[WhenDestroyed Template=FXC_PlaySFX]\n\t\taudio_ids = boom\n\
             \t\t[WhenDestroyed Template=FXC_CreateVFX]\n\t\tvfx_ids = rare\n\t\tfrequency = 0\n\
             \t}\n\
             \t[WhenEnter Template=FXC_CreateVFX]\n\tvfx_ids = dust\n\
             \tactor_state = construction\n\
             \t[WhenLeave Template=FXC_StopVFX]\n\tvfx_ids = dust\n\
             \tactor_state = construction\n\
             \t[BodyComponent]\n\t{\n\t\thealth = 100\n\
             \t\t[WhenHealth Template=FXC_CreateVFX]\n\t\t{\n\t\t\tvfx_ids = flame\n\
             \t\t\thealth = 0.4\n\t\t\tattached = false\n\
             \t\t\t[Chain Template=FXC_CreateVFX]\n\t\t\tvfx_ids = smoke\n\t\t}\n\t}\n\
             \t[PerformerComponent]\n\t{\n\t\t[Ability]\n\t\t{\n\
             \t\t\t[AbilityDefinition]\n\t\t\t{\n\t\t\t\tability_type = attack\n\
             \t\t\t\trange = 10\n\t\t\t\tprojectile_IDS = shell\n\
             \t\t\t\t[Effect]\n\t\t\t\ttype = MAIN_DAMAGE\n\
             \t\t\t\tdamage_type_IDS = gunfire\n\t\t\t\tamount = 5\n\
             \t\t\t\t[WhenStart Template=FXC_CreateVFX]\n\t\t\t\tvfx_ids = flash\n\
             \t\t\t\tlocation = cannon_ae_01\n\
             \t\t\t\t[WhenDo Template=FXC_CreateVFX]\n\t\t\t\tvfx_ids = puff\n\
             \t\t\t\tat = target_hit_position\n\t\t\t\ttarget_flags = +infantry\n\
             \t\t\t\tterrain_group_filter = snow\n\t\t\t\tdelay = 0.5\n\t\t\t\tfrequency = 2\n\
             \t\t\t}\n\t\t}\n\t}\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("Thing").next().unwrap();
        let thing: defs::Thing = templates.deserialize(block).unwrap();
        let cue = |vfx: &str| VfxCue {
            vfx: Id::from(vfx),
            stop: false,
            at: "SELF".to_string(),
            location: None,
            attached: true,
            translation: [0.0; 3],
            orientation: [0.0, -1.0, 0.0],
            duration: None,
            delay: 0.0,
            every: 1,
            terrain: None,
            targets: Targets::default(),
            stop_on_destructed: true,
            chain: vec![],
            chain_immediate: vec![],
        };

        let effects = object_effects(&thing);
        let window = VfxCue {
            location: Some("window_01".to_string()),
            stop_on_destructed: false,
            ..cue("lit_window")
        };
        assert_eq!(effects.constructed, [window]);
        assert!(effects.destroyed.is_empty());
        let construction = Id::from("construction");
        assert_eq!(effects.entered, [(construction.clone(), cue("dust"))]);
        let stop = VfxCue {
            stop: true,
            ..cue("dust")
        };
        assert_eq!(effects.left, [(construction, stop)]);
        let flame = VfxCue {
            attached: false,
            chain: vec![cue("smoke")],
            ..cue("flame")
        };
        let health = HealthCue {
            health: 0.4,
            trigger: HealthTrigger::Below,
            cue: flame,
        };
        assert_eq!(effects.health, [health]);

        let weapon = &fighter(&thing, &HashMap::new()).unwrap().weapons[0];
        assert_eq!(weapon.projectile, Some(Id::from("shell")));
        let flash = VfxCue {
            location: Some("cannon_ae_01".to_string()),
            ..cue("flash")
        };
        assert_eq!(weapon.effects.start, [flash]);
        let puff = VfxCue {
            at: "TARGET_HIT_POSITION".to_string(),
            targets: Targets::parse("+infantry"),
            terrain: Some(Id::from("snow")),
            delay: 0.5,
            every: 2,
            ..cue("puff")
        };
        assert_eq!(weapon.effects.shot, [puff]);
    }

    #[test]
    fn vfx_take_their_template_defaults() {
        let parsed = tgi::parse(
            "[Template VFX]\n{\n\tcategory = VFX\n\tflag permanent\n\tflag critical = true\n\
             \tfloat duration = 0\n\tfloat attached_scale = 0\n}\n\
             [Template VFXModel inherit=VFX]\n{\n\tcategory = VFXMODEL\n\tstring model\n\
             \tflag gyroscope = true\n}\n\
             [Template VFXModelTemporary inherit=VFXModel]\n{\n\tpermanent = false\n\
             \tcritical = false\n\t[TextureSearchPath]\n\tpath = /Shared/SharedTextures\n}\n\
             [VFX Template=VFXModelTemporary]\n{\n\tIDS = boom\n\tmodel = Boom.NIF\n\
             \tduration = 2\n\t[Flags]\n\trandom_orientation = true\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let block = parsed.root.blocks_of("VFX").next().unwrap();
        let v = vfx(
            templates.deserialize(block).unwrap(),
            Some("Effects/Boom.NIF".to_string()),
            None,
        );
        assert_eq!(
            v,
            Vfx {
                category: "VFXMODEL".to_string(),
                model: Some("Effects/Boom.NIF".to_string()),
                texture_dirs: vec!["Shared/SharedTextures".to_string()],
                permanent: false,
                critical: false,
                duration: Some(2.0),
                attached_scale: 0.0,
                never_discard: false,
                night_only: false,
                constant_screen_size: false,
                real_time: false,
                random_orientation: true,
                gyroscope: true,
                tint: None,
                decal: None,
            }
        );
    }

    #[test]
    fn projected_textures_of_their_own_size_are_decals() {
        let parsed = tgi::parse(
            "[Template VFX]\n{\n\tcategory = VFX\n\tflag permanent\n\tfloat attached_scale = 0\n}\n\
             [Template VFXProjectedTexture inherit=VFX]\n{\n\tcategory = VFXPROJECTEDTEXTURE\n\
             \tpermanent = true\n\tstring texture\n\tfloat texture_size\n\
             \tpoint3 color = 1,1,1\n\tflag respect_lighting = false\n\
             \tstring decal_type = generic\n\tpoint2 decal_fade_time = 0, 0\n\
             \tflag random_orientation = false\n}\n\
             [Template VFXScorchMark inherit=VFXProjectedTexture]\n{\n\tpermanent = false\n\
             \tdecal_type = scorch\n\trandom_orientation = true\n}\n\
             [Thing Inherit=VFXScorchMark]\n{\n\tIDS = scorch\n\ttexture = Scorch.dds\n\
             \ttexture_size = 7\n\tduration = 15\n\tdecal_fade_time = 0, 5\n}\n\
             [Thing Inherit=VFXProjectedTexture]\n{\n\tIDS = ring\n\ttexture = Ring.dds\n\
             \tattached_scale = 1.5\n}\n\
             [Thing Inherit=VFXProjectedTexture]\n{\n\tIDS = oval\n\ttexture = Oval.dds\n\
             \ttexture_size = 2\n\tflag oval_shape = true\n}\n",
        );
        let templates = tgi::Templates::new([&parsed.root]);
        let mut blocks = parsed.root.blocks_of("Thing");
        let mut read = |texture: &str| {
            let v = templates.deserialize(blocks.next().unwrap()).unwrap();
            vfx(v, None, Some(texture.to_string()))
        };
        let scorch = read("Effects/Scorch.dds");
        assert!(scorch.random_orientation && !scorch.permanent);
        assert_eq!(scorch.duration, Some(15.0));
        assert_eq!(
            scorch.decal,
            Some(Decal {
                texture: "Effects/Scorch.dds".to_string(),
                size: 7.0,
                colour: [1.0; 3],
                lit: false,
                additive: false,
                fade: [0.0, 5.0],
                layer: 1,
            })
        );
        // One sized or shaped by what it's attached to isn't.
        assert_eq!(read("Effects/Ring.dds").decal, None);
        assert_eq!(read("Effects/Oval.dds").decal, None);
    }

    #[test]
    fn tints_rise_then_fall_and_repeat_if_they_do() {
        let pulse = VfxTint {
            colour: [0.5, 0.5, 0.5],
            max: 1.0,
            rise: 1.0,
            fall: 0.8,
            repeating: true,
        };
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        assert!(near(pulse.level(0.0), 0.0));
        assert!(near(pulse.level(0.5), 0.5));
        assert!(near(pulse.level(1.0), 1.0));
        assert!(near(pulse.level(1.4), 0.5));
        assert!(near(pulse.level(1.8 + 0.25), 0.25));
        assert_eq!(pulse.colour(0.0), [1.0; 3]);
        assert_eq!(pulse.colour(1.0), [0.5; 3]);
        assert_eq!(pulse.colour(0.5), [0.75; 3]);

        let once = VfxTint {
            repeating: false,
            ..pulse
        };
        assert!(near(once.level(1.4), 0.5));
        assert_eq!(once.level(5.0), 0.0);

        let held = VfxTint {
            max: 0.3,
            rise: 0.2,
            fall: 0.0,
            ..pulse
        };
        assert!(near(held.level(0.03), 0.15));
        assert_eq!(held.level(10.0), 0.3);

        let at_once = VfxTint {
            rise: 0.0,
            fall: 0.0,
            ..pulse
        };
        assert_eq!(at_once.level(0.0), 1.0);
    }
}
