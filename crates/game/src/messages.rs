//! The message log at the top left of the screen: a line, behind its kind's
//! icon, for each thing the player is told, fading a while after it's
//! posted. Clicking a line's icon, or the key for it, looks at where it
//! happened. `spec/messages.md` is what it's built to.

use std::collections::{HashMap, VecDeque};

use aaa_data::{GameEvent, Id, InterfaceVars};
use aaa_render::camera::RtsCamera;
use aaa_render::terrain::world;
use aaa_render::textures;
use aaa_sim::{Fallen, Notice, Pause, Relation, Seen, World};
use bevy::ecs::system::SystemParam;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::{self, Aim, fill};
use crate::displays::{self, Contents, Displays, Panel, Rim, Window};
use crate::keys::Bindings;
use crate::sound::Sounds;
use crate::ui::{Font, Hint, font};
use crate::{Leaders, Loaded, Local, Screen, Sim};

/// The key that steps the camera back through the lines.
pub const CYCLE: &str = "CycleMessageLog";
/// The log's width, before it wraps a line.
const WIDTH: f32 = 564.0;
const ICON: Vec2 = Vec2::new(16.0, 17.0);
/// Between the icon and its text.
const ICON_GAP: f32 = 8.0;
const LINE_GAP: f32 = 3.0;
/// What keeps the text legible over pale ground, as there's no box behind
/// it.
const SHADOW: TextShadow = TextShadow {
    offset: Vec2::splat(1.0),
    color: Color::srgba(0.0, 0.0, 0.0, 0.9),
};
/// How long a line takes to fade once it's been up its while.
const FADE_SECONDS: f32 = 1.0;

/// Stock settings, for data that lacks them.
const SHOWN: usize = 5;
const KEPT: usize = 50;
const UP_MS: f32 = 10000.0;
const CYCLE_TIMEOUT: f32 = 1.0;

/// A kind of message, as its lines look.
struct Kind {
    icon: Option<Handle<Image>>,
    colour: Color,
    filter: Option<String>,
    sound: Option<String>,
}

/// What lines are made from, read as the match starts.
struct Look {
    kinds: HashMap<Id, Kind>,
    events: HashMap<Id, GameEvent>,
    vars: InterfaceVars,
    strings: aaa_data::Strings,
    names: HashMap<Id, String>,
    /// What an icon that looks somewhere says.
    to_location: String,
}

impl Look {
    fn name(&self, template: &str) -> String {
        (self.names.get(&Id::from(template)).cloned()).unwrap_or_else(|| template.to_string())
    }

    fn regiment(&self, sim: &World, r: usize) -> String {
        crate::object_display::regiment_name(sim, &self.names, &self.strings, r)
    }

    fn building(&self, sim: &World, b: usize) -> String {
        crate::object_display::building_name(sim, &self.names, &self.strings, b)
    }

    /// An HQ's name: its building's, numbered as its organization is,
    /// packed or not.
    fn hq(&self, sim: &World, hq: &aaa_sim::Hq) -> String {
        let standing = (sim.buildings.iter())
            .position(|b| !b.gone() && b.organization.as_ref().is_some_and(|o| o.id == hq.id));
        if let Some(b) = standing {
            return self.building(sim, b);
        }
        let number = sim
            .regiments
            .iter()
            .find(|r| r.id == hq.id)
            .and_then(|r| r.number);
        crate::object_display::numbered(&self.strings, number, self.name(&hq.building))
    }

    /// The text under `key`, or `or` where the strings lack it.
    fn said(&self, key: &str, or: &str) -> String {
        self.strings.get(key).map_or(or, String::as_str).to_string()
    }

    /// Whether lines of `kind` are posted, as its filter has it.
    fn shows(&self, kind: &Id) -> bool {
        let filter = self.kinds.get(kind).and_then(|k| k.filter.as_deref());
        filter.is_none_or(|f| self.vars.flag(f, true))
    }
}

/// What a line is about, for looking at it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum About {
    Regiment(usize),
    Building(usize),
}

/// Where a line happened: at what it's about, while that's there, else
/// where it was when the line was posted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub about: Option<About>,
    pub at: [f32; 2],
}

impl Place {
    fn of(sim: &World, about: About) -> Place {
        let at = match about {
            About::Regiment(r) => sim.leader_position(r),
            About::Building(b) => sim.buildings[b].position,
        };
        Place {
            about: Some(about),
            at,
        }
    }

    /// Where it is now, as `player` sees it.
    fn now(&self, sim: &World, player: Option<u32>) -> [f32; 2] {
        let now = match self.about {
            Some(About::Regiment(r)) if r < sim.regiments.len() && !sim.destroyed(r) => {
                let sees = |p| sim.survivors(r).any(|u| sim.sees_unit(p, u));
                player.is_none_or(sees).then(|| sim.leader_position(r))
            }
            Some(About::Building(b)) if b < sim.buildings.len() && !sim.buildings[b].gone() => {
                let sees = |p| sim.sees_building(p, b);
                player.is_none_or(sees).then_some(sim.buildings[b].position)
            }
            _ => None,
        };
        now.unwrap_or(self.at)
    }
}

/// A line the player was told.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Counts up from the match's first line.
    pub serial: u64,
    pub kind: Id,
    pub text: String,
    pub place: Option<Place>,
    /// When it was posted, in seconds of the app's time.
    pub posted: f32,
}

/// Every line the player has been told, oldest first, as many as the
/// settings keep.
#[derive(Resource)]
pub struct MessageLog {
    look: Look,
    lines: VecDeque<Line>,
    next: u64,
    /// The line the key last looked at, and when.
    cycled: Option<(u64, f32)>,
    /// How many lines show at once, how long each stays up, how many are
    /// kept, and how soon the key has to be pressed again to step on.
    shown: usize,
    up: f32,
    kept: usize,
    cycle_timeout: f32,
}

impl MessageLog {
    fn new(look: Look) -> MessageLog {
        let vars = &look.vars;
        MessageLog {
            shown: vars.get("UIMessageLogMaximumLines").unwrap_or(SHOWN),
            up: vars.get::<f32>("UIMessageLogFadeTime").unwrap_or(UP_MS) / 1000.0,
            kept: vars.get("MessageLogMaximumLines").unwrap_or(KEPT),
            cycle_timeout: vars
                .get("UIMessageLogCycleTimeout")
                .unwrap_or(CYCLE_TIMEOUT),
            look,
            lines: VecDeque::new(),
            next: 0,
            cycled: None,
        }
    }

    /// Posts a line of `kind`, unless its filter hides that kind; whether
    /// it did.
    pub fn post(&mut self, kind: &str, text: String, place: Option<Place>, now: f32) -> bool {
        let kind = Id::from(kind);
        if !self.look.shows(&kind) {
            return false;
        }
        info!("{text}");
        self.lines.push_back(Line {
            serial: self.next,
            kind,
            text,
            place,
            posted: now,
        });
        self.next += 1;
        while self.lines.len() > self.kept.max(1) {
            self.lines.pop_front();
        }
        true
    }

    /// Posts a line of `kind`, heard as its kind sounds.
    fn say(
        &mut self,
        kind: &str,
        text: String,
        place: Option<Place>,
        time: &Time,
        to: &mut Sounds,
    ) {
        if self.post(kind, text, place, time.elapsed_secs()) {
            self.sound(time, to);
        }
    }

    /// Plays the sound of the kind of line just posted.
    fn sound(&self, time: &Time, to: &mut Sounds) {
        let kind = self.lines.back().and_then(|l| self.look.kinds.get(&l.kind));
        if let Some(sound) = kind.and_then(|k| k.sound.as_deref()) {
            to.interface(time, sound);
        }
    }

    /// Posts the line of the game event `event`, its placeholders filled
    /// with `with` in turn, unless the event's filters or its kind's drop
    /// it.
    pub fn event(&mut self, event: &str, with: &[&str], place: Option<Place>, now: f32) -> bool {
        let Some(e) = self.look.events.get(&Id::from(event)) else {
            return false;
        };
        // Event filters the files don't set are off.
        let off = |f: &Option<String>| f.as_deref().is_some_and(|f| !self.look.vars.flag(f, false));
        if off(&e.filter) || off(&e.message_filter) {
            return false;
        }
        let (Some(text), Some(kind)) = (e.message.clone(), e.kind.clone()) else {
            return false;
        };
        self.post(kind.as_str(), fill_all(&text, with), place, now)
    }

    /// The lines on screen at `now`, oldest first, with how opaque each is:
    /// the newest still up, as many as show at once.
    fn on_screen(&self, now: f32) -> Vec<(&Line, f32)> {
        let mut out: Vec<(&Line, f32)> = (self.lines.iter().rev())
            .map(|l| (l, self.opacity(l, now)))
            .take_while(|&(_, a)| a > 0.0)
            .take(self.shown.max(1))
            .collect();
        out.reverse();
        out
    }

    fn opacity(&self, line: &Line, now: f32) -> f32 {
        let age = now - line.posted;
        (1.0 - (age - self.up) / FADE_SECONDS).clamp(0.0, 1.0)
    }

    /// The line the key looks at next: the newest with a place, or, pressed
    /// again soon enough, the next older one, round to the newest again.
    fn cycle(&mut self, now: f32) -> Option<&Line> {
        let placed: Vec<&Line> = self
            .lines
            .iter()
            .rev()
            .filter(|l| l.place.is_some())
            .collect();
        let after = (self.cycled)
            .filter(|&(_, at)| now - at <= self.cycle_timeout)
            .and_then(|(serial, _)| placed.iter().position(|l| l.serial == serial));
        let next = match after {
            Some(i) => placed.get(i + 1).or(placed.first()),
            None => placed.first(),
        };
        let serial = next?.serial;
        self.cycled = Some((serial, now));
        self.lines.iter().find(|l| l.serial == serial)
    }
}

/// Fills the `%s`, `%d` and `%i` placeholders of a line in turn, and reads
/// `%%` as `%`.
fn fill_all(text: &str, with: &[&str]) -> String {
    let mut out = String::new();
    let mut with = with.iter();
    let mut rest = text;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.chars().next() {
            Some('%') => {
                out.push('%');
                rest = &after[1..];
            }
            Some('s' | 'd' | 'i') => {
                out.push_str(with.next().copied().unwrap_or_default());
                rest = &after[1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The log's box.
#[derive(Component)]
pub struct LogBox;

/// A line on screen, by its serial.
#[derive(Component)]
pub struct Row(u64);

/// A row's icon and text, faded with it.
#[derive(Component)]
pub struct Faded(Color);

/// A row's parts, to fade.
type Parts<'a> = (
    &'a Faded,
    Option<&'a mut TextColor>,
    Option<&'a mut TextShadow>,
    Option<&'a mut ImageNode>,
);

/// The icon of the line with this serial, which looks where it happened.
#[derive(Component)]
struct LookAt(u64);

#[allow(clippy::too_many_arguments)]
pub fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    bindings: Res<Bindings>,
    mut icons: ResMut<displays::Icons>,
) {
    let corner = crate::special_ops::corner(&local, &leaders);
    let game = &mut loaded.game;
    let kinds = game
        .message_kinds()
        .into_iter()
        .map(|k| {
            let icon = k.icon.as_deref().and_then(|path| {
                let image = textures::decode(&game.read(path)?, path, formats.0, true)?;
                Some(images.add(image))
            });
            let [r, g, b] = k.colour;
            let kind = Kind {
                icon,
                colour: Color::srgb(r, g, b),
                filter: k.filter,
                sound: k.sound,
            };
            (k.id, kind)
        })
        .collect();
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    displays::spawn_button(
        &mut commands,
        &bindings,
        Panel::MessageLog,
        Rim::BottomRight,
        icons.get(displays::icon!("message-log"), &mut images, formats.0),
        said(
            "control_panel_ButtonMessageLog_ToolTip",
            "Display Message Log",
        ),
        said(
            "control_panel_ButtonMessageLog_tooltip_2",
            "Displays a log of all recent messages.",
        ),
    );
    displays::spawn_window(
        &mut commands,
        Window {
            panel: Panel::MessageLog,
            title: said("game_interface_Child_text", "Message Log"),
            at: Vec2::new(279.0, 53.0),
            size: Vec2::new(542.0, 514.0),
            pane: true,
        },
    );
    let look = Look {
        kinds,
        events: game.game_events(),
        vars: game.interface_vars(),
        to_location: (strings.get("ui_tooltip_message_location").cloned())
            .unwrap_or_else(|| "Move camera to message location".into()),
        strings,
        names: (game.listings().into_iter())
            .map(|(id, l)| (id, l.name))
            .chain(game.technologies().into_iter().map(|(id, t)| (id, t.name)))
            .collect(),
    };
    commands.insert_resource(MessageLog::new(look));
    commands.spawn((
        LogBox,
        Node {
            position_type: PositionType::Absolute,
            top: px(corner.y),
            left: px(corner.x),
            width: px(WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(LINE_GAP),
            ..default()
        },
        Pickable::IGNORE,
        GlobalZIndex(1),
        DespawnOnExit(Screen::Match),
    ));
}

/// A click that aimed an order at something it can't be given on: `by`
/// the order that refuses such a thing, or none where none of the selected
/// can attack it.
#[derive(Message)]
pub struct Refused {
    pub about: About,
    pub by: Option<Aim>,
}

/// An order the sim refused, as the player's told.
#[derive(Message)]
pub struct Failed {
    pub text: String,
    /// What it was given to, where the message points.
    pub about: About,
}

/// Tells the player of each order just [`Refused`] or [`Failed`].
pub fn refuse(
    mut refused: MessageReader<Refused>,
    mut failed: MessageReader<Failed>,
    sim: Res<Sim>,
    looks: Res<actions::Looks>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    for Refused { about, by } in refused.read() {
        let name = match *about {
            About::Regiment(r) => log.look.regiment(&sim.0, r),
            About::Building(b) => log.look.building(&sim.0, b),
        };
        let text = match by {
            Some(aim) => {
                let said = log.look.said(
                    "ui_error_command_target_actor|ss",
                    "%s is an invalid target for %s",
                );
                fill(&said, &[&name, looks.order(*aim)])
            }
            None => {
                let said = log
                    .look
                    .said("kill_error_invalid_target|s", "Invalid target: %s");
                fill(&said, &[&name])
            }
        };
        let place = Some(Place::of(&sim.0, *about));
        log.say("GENERIC_ERROR", text, place, &time, &mut sounds);
    }
    for Failed { text, about } in failed.read() {
        let place = Some(Place::of(&sim.0, *about));
        log.say("GENERIC_ERROR", text.clone(), place, &time, &mut sounds);
    }
}

/// Posts a line for each thing the sim has told the player since last
/// time.
pub fn tell(
    mut sim: ResMut<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    // Taking them marks the sim changed, which what redraws on a change
    // would then do every frame.
    if sim.0.notices.is_empty() {
        return;
    }
    let notices = std::mem::take(&mut sim.0.notices);
    let now = time.elapsed_secs();
    for notice in notices {
        if let Some(sound) = sound(&notice, local.0) {
            sounds.interface(&time, sound);
        }
        let posted = match told(&sim.0, local.0, &leaders, &log.look, notice) {
            Some(Told::Event(event, with, place)) => {
                let with: Vec<&str> = with.iter().map(String::as_str).collect();
                log.event(event, &with, place, now)
            }
            Some(Told::Line(kind, text, place)) => log.post(kind, text, place, now),
            None => false,
        };
        if posted {
            log.sound(&time, &mut sounds);
        }
    }
}

/// The sound `local`, the player here, hears for a notice: none for one
/// told to anyone else.
fn sound(notice: &Notice, local: Option<u32>) -> Option<&'static str> {
    match *notice {
        Notice::Lost { player, .. } if Some(player) == local => Some("actor_captured_by_enemy_sfx"),
        Notice::Captured { player, .. } if Some(player) == local => Some("actor_captured_sfx"),
        _ => None,
    }
}

/// What a notice tells the player.
enum Told {
    /// A game event, with what fills its line.
    Event(&'static str, Vec<String>, Option<Place>),
    /// A line of a kind of message.
    Line(&'static str, String, Option<Place>),
}

/// What `notice` tells `local`, if it's for them.
fn told(
    sim: &World,
    local: Option<u32>,
    leaders: &Leaders,
    look: &Look,
    notice: Notice,
) -> Option<Told> {
    let ours = |player: u32| Some(player) == local;
    let at = |about| Some(Place::of(sim, about));
    // What happens to another side's thing is told only where it's seen.
    let sees = |owner: u32, place: &Place| {
        ours(owner) || local.is_some_and(|p| sim.seen(p, place.at) == Seen::Visible)
    };
    let side = |id: u32| {
        let player = sim.sides.player(id);
        player.map_or_else(String::new, |p| leaders.owner(p, local))
    };
    Some(match notice {
        Notice::Short { player, percent } if ours(player) => {
            let percent = (percent as i64).to_string();
            Told::Event("kingdom_upkeep_shortage", vec![percent], None)
        }
        Notice::Repaired { player } if ours(player) => {
            Told::Event("kingdom_upkeep_repaired", vec![], None)
        }
        Notice::Paused {
            player,
            building,
            template,
            why,
        } if ours(player) => {
            let text = match why {
                Pause::Limit(_) => {
                    let said = look.said(
                        "recruit_error_unit_limit|s",
                        "Insufficient unit limit to recruit %s",
                    );
                    fill(&said, &[&look.name(&template)])
                }
                Pause::Disabled | Pause::Supply => {
                    let said = match why {
                        Pause::Supply => look.said(
                            "structure_error_queue_no_supply|s",
                            "%s paused production queue - no supply",
                        ),
                        _ => look.said(
                            "structure_error_queue_disabled|s",
                            "%s paused production queue - disabled",
                        ),
                    };
                    fill(&said, &[&look.building(sim, building)])
                }
            };
            Told::Line("GENERIC_ERROR", text, at(About::Building(building)))
        }
        Notice::CannotUnpack { player, regiment } if ours(player) => {
            let said = look.said("tai_error_unpack|s", "Cannot unpack %s");
            let text = fill(&said, &[&look.name(&sim.regiments[regiment].template)]);
            Told::Line("GENERIC_ERROR", text, at(About::Regiment(regiment)))
        }
        Notice::Attached {
            player,
            hq,
            regiment,
        }
        | Notice::Detached {
            player,
            hq,
            regiment,
        } if ours(player) => {
            let event = match notice {
                Notice::Attached { .. } => "hq_attach",
                _ => "hq_detach",
            };
            let hqs = sim.hqs();
            let hq = hqs.iter().find(|h| h.id == hq);
            let hq = hq.map(|h| look.hq(sim, h)).unwrap_or_default();
            let joining = look.regiment(sim, regiment);
            Told::Event(event, vec![hq, joining], at(About::Regiment(regiment)))
        }
        Notice::PutOut {
            player, regiment, ..
        } if ours(player) => {
            // A regiment is commissioned, a unit on its own such as a truck
            // recruited.
            let event = match crate::object_display::lone(sim, regiment) {
                true => "company_recruited",
                false => "company_commissioned",
            };
            let name = look.regiment(sim, regiment);
            Told::Event(event, vec![name], at(About::Regiment(regiment)))
        }
        Notice::Researched {
            player,
            building,
            technology,
        } if ours(player) => {
            let with = vec![look.name(technology.as_str())];
            Told::Event("research_complete", with, at(About::Building(building)))
        }
        Notice::Gained { player, technology } if ours(player) => Told::Event(
            "technology_gained",
            vec![look.name(technology.as_str())],
            None,
        ),
        // A side always sees its own, so is always told: the loser while
        // it's still theirs, the taker once it is.
        Notice::Lost {
            player,
            building,
            by,
        } if ours(player) => {
            let with = vec![look.building(sim, building), side(by)];
            Told::Event(
                "actor_captured_by_enemy",
                with,
                at(About::Building(building)),
            )
        }
        Notice::Captured { player, building } if ours(player) => {
            let with = vec![look.building(sim, building)];
            Told::Event("actor_captured", with, at(About::Building(building)))
        }
        Notice::Sold {
            player,
            building,
            paid,
        } if ours(player) => {
            let with = vec![look.building(sim, building), paid.to_string()];
            Told::Event("building_sold", with, at(About::Building(building)))
        }
        // Its owner is told it's lost, and its enemies who see it go that
        // it's destroyed, whoever did it. What's nobody's tells no one.
        Notice::Destroyed { owner, what } if sim.sides.player(owner).is_some() => {
            let (name, about, event) = match what {
                Fallen::Regiment(r) => (
                    look.regiment(sim, r),
                    About::Regiment(r),
                    "mobile_actor_lost",
                ),
                Fallen::Building(b) => (
                    look.building(sim, b),
                    About::Building(b),
                    "static_actor_lost",
                ),
            };
            let place = Place::of(sim, about);
            let enemy = local.is_some_and(|p| sim.sides.relation(p, owner) == Relation::Enemy);
            match ours(owner) {
                true => Told::Event(event, vec![name], Some(place)),
                false if enemy && sees(owner, &place) => {
                    Told::Event("actor_destroyed", vec![side(owner), name], Some(place))
                }
                false => return None,
            }
        }
        _ => return None,
    })
}

/// Lays out the lines on screen, and fades each as it goes.
pub fn show(
    mut commands: Commands,
    time: Res<Time>,
    log: Res<MessageLog>,
    root: Single<(Entity, Option<&Children>), With<LogBox>>,
    rows: Query<(&Row, &Children)>,
    mut faded: Query<Parts>,
) {
    let now = time.elapsed_secs();
    let shown = log.on_screen(now);
    let (root, children) = *root;
    let children = children.map_or(&[][..], |c| &c[..]);
    let laid: Vec<u64> = (children.iter())
        .filter_map(|&c| Some(rows.get(c).ok()?.0.0))
        .collect();
    let wanted: Vec<u64> = shown.iter().map(|(l, _)| l.serial).collect();
    if laid != wanted {
        for &c in children {
            commands.entity(c).despawn();
        }
        for &(line, _) in &shown {
            spawn_row(&mut commands, root, &log.look, line);
        }
        return;
    }
    for (&row, &(_, opacity)) in children.iter().zip(&shown) {
        let Ok((_, parts)) = rows.get(row) else {
            continue;
        };
        for &part in parts {
            let Ok((Faded(colour), text, shadow, image)) = faded.get_mut(part) else {
                continue;
            };
            let colour = colour.with_alpha(colour.alpha() * opacity);
            if let Some(mut t) = text {
                t.set_if_neq(TextColor(colour));
            }
            if let Some(mut shadow) = shadow {
                let faded = SHADOW.color.with_alpha(SHADOW.color.alpha() * opacity);
                if shadow.color != faded {
                    shadow.color = faded;
                }
            }
            if let Some(mut i) = image
                && i.color != colour
            {
                i.color = colour;
            }
        }
    }
}

/// The lines the Message Log window shows, by the serials of its oldest
/// and newest, once it's shown any: `None` until then, `Some(None)` for none.
#[derive(Default)]
pub struct Listed(Option<Option<(u64, u64)>>);

/// Lists the whole log in its window while that's up, oldest first, kept
/// scrolled to the newest.
pub fn list(
    mut commands: Commands,
    mut listed: bevy::prelude::Local<Listed>,
    displays: Res<Displays>,
    log: Res<MessageLog>,
    mut contents: Query<(Entity, &Contents, &mut ScrollPosition)>,
) {
    if !displays.is_open(Panel::MessageLog) {
        listed.0 = None;
        return;
    }
    let span = (log.lines.front())
        .zip(log.lines.back())
        .map(|(a, b)| (a.serial, b.serial));
    if listed.0 == Some(span) {
        return;
    }
    let Some((list, _, mut scroll)) = contents
        .iter_mut()
        .find(|(_, c, _)| c.0 == Panel::MessageLog)
    else {
        return;
    };
    commands.entity(list).despawn_children();
    for line in &log.lines {
        spawn_row(&mut commands, list, &log.look, line);
    }
    // Layout holds it to the bottom of the list.
    scroll.y = f32::MAX;
    listed.0 = Some(span);
}

fn spawn_row(commands: &mut Commands, root: Entity, look: &Look, line: &Line) {
    let kind = look.kinds.get(&line.kind);
    let colour = kind.map_or(Color::WHITE, |k| k.colour);
    let icon = kind.and_then(|k| k.icon.clone()).unwrap_or_default();
    commands
        .spawn((
            Row(line.serial),
            Node {
                column_gap: px(ICON_GAP),
                align_items: AlignItems::FlexStart,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .with_children(|row| {
            let mut icon = row.spawn((
                Faded(Color::WHITE),
                ImageNode::new(icon),
                Node {
                    width: px(ICON.x),
                    height: px(ICON.y),
                    flex_shrink: 0.0,
                    margin: UiRect::top(px(1)),
                    ..default()
                },
            ));
            match line.place {
                Some(_) => {
                    icon.insert((LookAt(line.serial), Button, Hint(look.to_location.clone())))
                        .observe(look_at);
                }
                None => {
                    icon.insert(Pickable::IGNORE);
                }
            }
            row.spawn((
                Faded(colour),
                Text::new(line.text.clone()),
                font(Font::Hud),
                TextColor(colour),
                SHADOW,
                // The rest of the row, wrapping only at the log's edge.
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    ..default()
                },
                Pickable::IGNORE,
            ));
        });
}

/// What looks at where a line happened.
#[derive(SystemParam)]
pub struct Viewer<'w, 's> {
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

impl Viewer<'_, '_> {
    fn look_at(&mut self, place: Place) {
        let [x, y] = place.now(&self.sim.0, self.local.0);
        let height = self.loaded.map.ground.height_at([x, y]);
        for mut camera in &mut self.cameras {
            camera.focus = world(x, y, height);
        }
    }
}

/// Looks at where the clicked icon's line happened.
fn look_at(
    activate: On<Activate>,
    icons: Query<&LookAt>,
    log: Res<MessageLog>,
    mut viewer: Viewer,
) {
    let Ok(LookAt(serial)) = icons.get(activate.entity) else {
        return;
    };
    let line = log.lines.iter().find(|l| l.serial == *serial);
    if let Some(place) = line.and_then(|l| l.place) {
        viewer.look_at(place);
    }
}

/// Looks at the next line back on the key for it.
pub fn cycle(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut viewer: Viewer,
) {
    if !bindings.pressed(&keys, CYCLE) {
        return;
    }
    if let Some(place) = log.cycle(time.elapsed_secs()).and_then(|l| l.place) {
        viewer.look_at(place);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(vars: &[(&str, &str)]) -> Look {
        let kind = |filter: Option<&str>| Kind {
            icon: None,
            colour: Color::WHITE,
            filter: filter.map(Into::into),
            sound: None,
        };
        let event = |id: &str, kind: &str, message: &str, filter: Option<&str>| {
            let e = GameEvent {
                id: Id::from(id),
                message: Some(message.into()),
                kind: Some(Id::from(kind)),
                filter: filter.map(Into::into),
                message_filter: None,
            };
            (Id::from(id), e)
        };
        Look {
            kinds: HashMap::from([
                (
                    Id::from("GENERIC_EVENT"),
                    kind(Some("UIMessageFilterGeneric")),
                ),
                (Id::from("GENERIC_ERROR"), kind(None)),
            ]),
            events: HashMap::from([
                event("hq_attach", "GENERIC_EVENT", "%s: attaching %s", None),
                event(
                    "engaged",
                    "GENERIC_EVENT",
                    "%s engaged",
                    Some("UIEventFilterEngagement"),
                ),
            ]),
            vars: vars_of(vars),
            strings: aaa_data::Strings::default(),
            names: HashMap::new(),
            to_location: String::new(),
        }
    }

    fn vars_of(vars: &[(&str, &str)]) -> InterfaceVars {
        let text: String = vars.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
        InterfaceVars::parse(&format!("[Vars]\n{{\n{text}}}\n"))
    }

    fn place(at: [f32; 2]) -> Option<Place> {
        Some(Place { about: None, at })
    }

    #[test]
    fn only_the_player_here_hears_buildings_change_hands() {
        let lost = |player, by| Notice::Lost {
            player,
            building: 0,
            by,
        };
        let captured = |player| Notice::Captured {
            player,
            building: 0,
        };
        assert_eq!(
            sound(&lost(1, 2), Some(1)),
            Some("actor_captured_by_enemy_sfx")
        );
        assert_eq!(sound(&captured(1), Some(1)), Some("actor_captured_sfx"));
        // One AI taking from another.
        assert_eq!(sound(&lost(2, 3), Some(1)), None);
        assert_eq!(sound(&captured(3), Some(1)), None);
        assert_eq!(sound(&captured(1), None), None);
        assert_eq!(sound(&Notice::Repaired { player: 1 }, Some(1)), None);
    }

    #[test]
    fn placeholders_fill_in_turn() {
        assert_eq!(
            fill_all("%s: attaching %s", &["1st HQ", "2nd Regiment"]),
            "1st HQ: attaching 2nd Regiment"
        );
        assert_eq!(
            fill_all("Your production is %d%% short", &["40"]),
            "Your production is 40% short"
        );
        assert_eq!(
            fill_all("%i%s given to %s", &["50", "$", "Bob"]),
            "50$ given to Bob"
        );
        assert_eq!(fill_all("100% %x", &[]), "100% %x");
    }

    #[test]
    fn events_post_their_lines_unless_filtered() {
        let mut log = MessageLog::new(look(&[]));
        assert!(log.event("hq_attach", &["HQ", "Regiment"], None, 0.0));
        // An event filter the files don't set is off.
        assert!(!log.event("engaged", &["Regiment"], None, 0.0));
        assert!(!log.event("unknown", &[], None, 0.0));
        let texts: Vec<&str> = log.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["HQ: attaching Regiment"]);

        let mut log = MessageLog::new(look(&[
            ("UIMessageFilterGeneric", "false"),
            ("UIEventFilterEngagement", "true"),
        ]));
        assert!(!log.event("engaged", &["Regiment"], None, 0.0));
        assert!(log.post("GENERIC_ERROR", "Cannot unpack".into(), None, 0.0));
    }

    #[test]
    fn lines_stay_up_then_fade_and_the_newest_show() {
        let mut log = MessageLog::new(look(&[
            ("UIMessageLogMaximumLines", "2"),
            ("UIMessageLogFadeTime", "10000"),
            ("MessageLogMaximumLines", "3"),
        ]));
        for (i, at) in [0.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
            log.post("GENERIC_ERROR", format!("{i}"), None, at);
        }
        let shown = |log: &MessageLog, now| -> Vec<(String, f32)> {
            (log.on_screen(now).into_iter())
                .map(|(l, a)| (l.text.clone(), a))
                .collect()
        };
        assert_eq!(shown(&log, 3.0), [("2".into(), 1.0), ("3".into(), 1.0)]);
        assert_eq!(shown(&log, 12.5), [("2".into(), 0.5), ("3".into(), 1.0)]);
        assert_eq!(shown(&log, 13.0), [("3".into(), 1.0)]);
        assert_eq!(shown(&log, 14.0), []);
        // Only so many are kept.
        let kept: Vec<&str> = log.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(kept, ["1", "2", "3"]);
    }

    #[test]
    fn the_key_steps_back_through_placed_lines_and_round() {
        let mut log = MessageLog::new(look(&[("UIMessageLogCycleTimeout", "1.0")]));
        log.post("GENERIC_ERROR", "a".into(), place([1.0, 0.0]), 0.0);
        log.post("GENERIC_ERROR", "unplaced".into(), None, 0.0);
        log.post("GENERIC_ERROR", "b".into(), place([2.0, 0.0]), 0.0);
        let mut at = |now| log.cycle(now).map(|l| l.text.clone());
        assert_eq!(at(10.0).as_deref(), Some("b"));
        assert_eq!(at(10.5).as_deref(), Some("a"));
        // Round to the newest after the oldest.
        assert_eq!(at(11.0).as_deref(), Some("b"));
        assert_eq!(at(11.5).as_deref(), Some("a"));
        // A longer pause starts from the newest again.
        assert_eq!(at(20.0).as_deref(), Some("b"));
    }
}
