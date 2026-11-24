//! The log itself: the lines told, how they're filled in and marked, and
//! how long each stays up.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use bevy::image::{CompressedImageFormatSupport, CompressedImageFormats};
use bevy::prelude::*;
use tgdata::{GameEvent, Id, InterfaceVars};
use tgsim::World;

use crate::Loaded;
use crate::sound::Sounds;

use super::{CYCLE_TIMEOUT, FADE_SECONDS, KEPT, SHOWN, UP_MS};

/// A kind of message, as its lines look.
pub(crate) struct Kind {
    pub(crate) icon: Option<Handle<Image>>,
    /// The path of the icon this project's own interface draws instead,
    /// if it has one.
    pub(crate) modern: Option<String>,
    pub(crate) colour: Color,
    pub(crate) filter: Option<String>,
    pub(crate) sound: Option<String>,
}

/// What lines are made from, read as the match starts.
pub(crate) struct Look {
    pub(crate) kinds: HashMap<Id, Kind>,
    pub(crate) events: HashMap<Id, GameEvent>,
    pub(crate) vars: InterfaceVars,
    pub(crate) strings: Arc<tgdata::Strings>,
    pub(crate) names: HashMap<Id, String>,
    /// The map's own text, for the names it gives things and sides.
    pub(crate) map: Arc<tgdata::Strings>,
    /// What an icon that looks somewhere says.
    pub(crate) to_location: String,
    /// What marks a side named in a line.
    pub(crate) gem: crate::ui::Gem,
}

impl Look {
    pub(crate) fn name(&self, template: &str) -> String {
        (self.names.get(&Id::from(template)).cloned()).unwrap_or_else(|| template.to_string())
    }

    pub(crate) fn organization(&self, sim: &World, r: usize) -> String {
        crate::object_display::organization_name(sim, &self.names, &self.strings, &self.map, r)
    }

    pub(crate) fn building(&self, sim: &World, b: usize) -> String {
        crate::object_display::building_name(sim, &self.names, &self.strings, &self.map, b)
    }

    /// An HQ's name: its building's, numbered as its organization is,
    /// packed or not.
    pub(crate) fn hq(&self, sim: &World, hq: &tgsim::Hq) -> String {
        let standing = (sim.buildings.iter())
            .position(|b| !b.gone() && b.organization.as_ref().is_some_and(|o| o.id == hq.id));
        if let Some(b) = standing {
            return self.building(sim, b);
        }
        let number = sim
            .organizations
            .iter()
            .find(|r| r.id == hq.id)
            .and_then(|r| r.number);
        crate::object_display::numbered(&self.strings, number, self.name(hq.building.as_str()))
    }

    /// The text under `key`, or `or` where the strings lack it.
    pub(crate) fn said(&self, key: &str, or: &str) -> String {
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
    Organization(usize),
    Building(usize),
}

impl From<tgsim::Piece> for About {
    fn from(piece: tgsim::Piece) -> About {
        match piece {
            tgsim::Piece::Organization(r) => About::Organization(r),
            tgsim::Piece::Building(b) => About::Building(b),
        }
    }
}

/// Where a line happened: at what it's about, while that's there, else
/// where it was when the line was posted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub about: Option<About>,
    pub at: [f32; 2],
}

impl Place {
    pub(crate) fn of(sim: &World, about: About) -> Place {
        let at = match about {
            About::Organization(r) => sim.leader_position(r),
            About::Building(b) => sim.buildings[b].position,
        };
        Place {
            about: Some(about),
            at,
        }
    }

    /// Where it is now, as `player` sees it.
    pub(crate) fn now(&self, sim: &World, player: Option<u32>) -> [f32; 2] {
        let now = match self.about {
            Some(About::Organization(r)) if r < sim.organizations.len() && !sim.destroyed(r) => {
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
    /// Where it names sides.
    pub marks: Vec<Mark>,
    /// Where its words take a colour of their own.
    pub inks: Vec<Ink>,
    pub place: Option<Place>,
    /// When it was posted, in seconds of the app's time.
    pub posted: f32,
}

/// Where a line names a side: the byte of its text the name starts at,
/// and the side's colour, which the side's gem before the name is drawn
/// in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub at: usize,
    pub rgb: [f32; 3],
}

/// Where a line's words change colour: from the byte of its text at
/// `at`, to `rgb`, or back to the colour of its kind if none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub at: usize,
    pub rgb: Option<[f32; 3]>,
}

/// A line's text, where it names sides, and where its words take a
/// colour of their own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Words {
    pub text: String,
    pub marks: Vec<Mark>,
    pub inks: Vec<Ink>,
}

impl From<String> for Words {
    fn from(text: String) -> Words {
        Words {
            text,
            ..Default::default()
        }
    }
}

/// What fills one of a line's placeholders: words, or a side's name.
#[derive(Clone, Debug, PartialEq)]
pub struct Fill {
    words: String,
    /// The colour of the side the words name.
    pub(crate) side: Option<[f32; 3]>,
    /// The colour the words are written in, if not the line's.
    ink: Option<[f32; 3]>,
}

impl Fill {
    /// Words written in the colour `rgb`.
    pub(crate) fn inked(words: String, rgb: [f32; 3]) -> Fill {
        Fill {
            ink: Some(rgb),
            ..Fill::from(words)
        }
    }

    /// The name of a side of colour `rgb`.
    pub(crate) fn side(name: String, rgb: [f32; 3]) -> Fill {
        Fill {
            side: Some(rgb),
            ..Fill::from(name)
        }
    }

    /// The name of `player`'s side, marked unless it's independent, as
    /// the selection panel marks an owner.
    pub(crate) fn side_of(name: String, player: &tgmap::Player) -> Fill {
        match player.independent {
            true => Fill::from(name),
            false => Fill::side(name, player.rgb),
        }
    }
}

impl From<String> for Fill {
    fn from(words: String) -> Fill {
        Fill {
            words,
            side: None,
            ink: None,
        }
    }
}

impl From<&str> for Fill {
    fn from(words: &str) -> Fill {
        Fill::from(words.to_string())
    }
}

impl From<&String> for Fill {
    fn from(words: &String) -> Fill {
        Fill::from(words.clone())
    }
}

/// Every line the player has been told, oldest first, as many as the
/// settings keep.
#[derive(Resource)]
pub struct MessageLog {
    pub(crate) look: Look,
    pub(crate) lines: VecDeque<Line>,
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
    pub(crate) fn new(look: Look) -> MessageLog {
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
    pub fn post(
        &mut self,
        kind: &str,
        words: impl Into<Words>,
        place: Option<Place>,
        now: f32,
    ) -> bool {
        let kind = Id::from(kind);
        if !self.look.shows(&kind) {
            return false;
        }
        let Words { text, marks, inks } = words.into();
        info!("{text}");
        self.lines.push_back(Line {
            serial: self.next,
            kind,
            text,
            marks,
            inks,
            place,
            posted: now,
        });
        self.next += 1;
        while self.lines.len() > self.kept.max(1) {
            self.lines.pop_front();
        }
        true
    }

    /// The map's own text, for the names it gives sides.
    pub(crate) fn map_strings(&self) -> std::sync::Arc<tgdata::Strings> {
        self.look.map.clone()
    }

    /// Posts the warning line the string `key` says, or `stock` where the
    /// strings don't, its placeholders filled with `with` in turn.
    pub(crate) fn warn(&mut self, key: &str, stock: &str, with: &[String], now: f32) {
        let said = self.look.said(key, stock);
        self.post("GENERIC_WARNING", fill_all(&said, with), None, now);
    }

    /// Posts what the player here whispers to the person `to`, as they
    /// send it.
    pub(crate) fn whispering(&mut self, to: String, words: String, now: f32) {
        let said = self
            .look
            .said("ui_lobby_whispering_to|ss", "Whispering to %s: %s");
        let name = self.look.vars.rgb("UIChatPlayerColorNormal");
        let with = [Fill::inked(to, name.unwrap_or([1.0; 3])), words.into()];
        self.post("CHAT_PRIVATE", fill_all(&said, &with), None, now);
    }

    /// Posts a line of `kind`, heard as its kind sounds.
    pub(crate) fn say(
        &mut self,
        kind: &str,
        words: impl Into<Words>,
        place: Option<Place>,
        time: &Time,
        to: &mut Sounds,
    ) {
        if self.post(kind, words, place, time.elapsed_secs()) {
            self.sound(time, to);
        }
    }

    /// Plays the sound of the kind of line just posted.
    pub(crate) fn sound(&self, time: &Time, to: &mut Sounds) {
        let kind = self.lines.back().and_then(|l| self.look.kinds.get(&l.kind));
        if let Some(sound) = kind.and_then(|k| k.sound.as_deref()) {
            to.interface(time, sound);
        }
    }

    /// Posts the line of the game event `event`, its placeholders filled
    /// with `with` in turn, unless the event's filters or its kind's drop
    /// it.
    pub fn event(
        &mut self,
        event: &str,
        with: &[impl Into<Fill> + Clone],
        place: Option<Place>,
        now: f32,
    ) -> bool {
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
    pub(crate) fn on_screen(&self, now: f32) -> Vec<(&Line, f32)> {
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
    pub(crate) fn cycle(&mut self, now: f32) -> Option<&Line> {
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

/// [`fill_all`]'s words alone, for text shown outside the message log.
pub(crate) fn filled(text: &str, with: &[impl Into<Fill> + Clone]) -> String {
    fill_all(text, with).text
}

/// Fills the `%s`, `%d`, `%i` and `%f` placeholders of a line in turn,
/// the last also as `%.2f` and the like, though what fills it is written
/// as its caller has it; reads `%%` as `%`, and takes `<color=r,g,b>` as
/// the colour of what follows.
pub(super) fn fill_all(text: &str, with: &[impl Into<Fill> + Clone]) -> Words {
    let mut out = String::new();
    let mut marks = Vec::new();
    let mut inks = Vec::new();
    let mut with = with.iter().cloned().map(Into::into);
    let mut rest = text;
    while let Some(i) = rest.find(['%', '<']) {
        out.push_str(&rest[..i]);
        if let Some((rgb, after)) = colour_markup(&rest[i..]) {
            inks.push(Ink {
                at: out.len(),
                rgb: Some(rgb),
            });
            rest = after;
            continue;
        }
        if rest[i..].starts_with('<') {
            out.push('<');
            rest = &rest[i + 1..];
            continue;
        }
        let after = &rest[i + 1..];
        // A placeholder for a number with its places given, e.g. `%.0f`.
        let places = after
            .strip_prefix('.')
            .map(|p| p.trim_start_matches(|c: char| c.is_ascii_digit()))
            .filter(|p| p.starts_with('f'));
        let after = places.unwrap_or(after);
        match after.chars().next() {
            Some('%') => {
                out.push('%');
                rest = &after[1..];
            }
            Some('s' | 'd' | 'i' | 'f') => {
                if let Some(Fill { words, side, ink }) = with.next() {
                    if let Some(rgb) = side {
                        marks.push(Mark { at: out.len(), rgb });
                    }
                    if ink.is_some() {
                        inks.push(Ink {
                            at: out.len(),
                            rgb: ink,
                        });
                    }
                    out.push_str(&words);
                    if ink.is_some() {
                        let at = out.len();
                        inks.push(Ink { at, rgb: None });
                    }
                }
                rest = &after[1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Words {
        text: out,
        marks,
        inks,
    }
}

impl Look {
    /// What `game` has lines look like, short of what the map has things
    /// called, which [`Look::name_things`] gives once the match is set up.
    fn of(
        game: &tgdata::GameData,
        images: &mut Assets<Image>,
        formats: CompressedImageFormats,
        gem: &crate::ui::Gem,
    ) -> Look {
        let mut modern = game.message_kind_icons();
        modern.retain(|kind, path| {
            let there = game.exists(path);
            if !there {
                warn!("{kind}'s icon: no {path} in the data");
            }
            there
        });
        let kinds = game
            .message_kinds()
            .into_iter()
            .map(|k| {
                let icon = k.icon.as_deref().and_then(|path| {
                    let image = crate::ui::read_image(game, path, formats)?;
                    Some(images.add(image))
                });
                let [r, g, b] = k.colour;
                let kind = Kind {
                    icon,
                    modern: modern.remove(&k.id),
                    colour: Color::srgb(r, g, b),
                    filter: k.filter,
                    sound: k.sound,
                };
                (k.id, kind)
            })
            .collect();
        for kind in modern.keys() {
            warn!("{kind} has an icon but isn't a kind of message");
        }
        let strings = game.strings();
        Look {
            kinds,
            events: game.game_events(),
            vars: game.interface_vars(),
            to_location: (strings.get("ui_tooltip_message_location").cloned())
                .unwrap_or_else(|| "Move camera to message location".into()),
            gem: gem.clone(),
            strings,
            map: Default::default(),
            names: HashMap::new(),
        }
    }

    /// Has it call things by what `loaded`'s data and map call them.
    pub(crate) fn name_things(&mut self, loaded: &Loaded) {
        let game = &loaded.game;
        self.map = loaded.map.strings.clone();
        self.names = (game.listings().iter())
            .map(|(id, l)| (id.clone(), l.name.clone()))
            .chain((game.technologies().iter()).map(|(id, t)| (id.clone(), t.name.clone())))
            .collect();
    }
}

/// Starts the log as a match begins loading, so that who goes and what's
/// said while it loads is told then and kept.
pub fn start(
    mut commands: Commands,
    data: Res<crate::Data>,
    mut images: ResMut<Assets<Image>>,
    formats: Option<Res<CompressedImageFormatSupport>>,
    gem: Res<crate::ui::Gem>,
) {
    let formats = formats.map(|f| f.0).unwrap_or_default();
    let look = Look::of(&data.0, &mut images, formats, &gem);
    commands.insert_resource(MessageLog::new(look));
}

/// The colour `<color=r,g,b>` at the start of `text` sets, and what
/// follows it.
fn colour_markup(text: &str) -> Option<([f32; 3], &str)> {
    let (inside, after) = text.strip_prefix("<color=")?.split_once('>')?;
    let mut parts = inside.split(',').map(|p| p.trim().parse::<f32>().ok());
    let rgb = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((rgb, after))
}
