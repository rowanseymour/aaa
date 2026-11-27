//! The subtitles of the lines of speech the map's triggers play: shown a
//! part at a time in the layouts' subtitle panel, over the bottom
//! letterbox bar, and as a line in the message log when the letterbox is
//! off.

use bevy::prelude::*;
use bevy::text::Font as Face;
use tgdata::Id;
use tgdata::widgets::{Rect, SCREEN};

use crate::hud::original::lettering::Letters;
use crate::messages::MessageLog;
use crate::paused::{LayoutFont, Stage};
use crate::sound::Sounds;
use crate::{Loaded, Orders, Screen, Sim};

/// The layout file the panel's in, the widget that holds it there, and
/// its name and the label without a portrait within it.
const FILE: &str = "UI/Game/game_interface.tgi";
const HOLDER: &str = "Game";
const PANEL: &str = "Subtitles";
const LABEL: &str = "LabelNoModel";

/// The kind of message a subtitle's line in the message log is.
const KIND: &str = "TUTORIAL";

pub(crate) struct SubtitlesPlugin;

impl Plugin for SubtitlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<Started>()
            .add_systems(OnEnter(Screen::Match), spawn)
            .add_systems(
                Update,
                (start, show.in_set(Shown))
                    .chain()
                    .run_if(in_state(Screen::Match))
                    .run_if(resource_exists::<Panel>),
            );
    }
}

/// Where the subtitle showing is worked out, and so what it has the
/// banner show.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Shown;

/// A line of speech started playing, or would have but for being
/// `played`.
#[derive(Message)]
pub(crate) struct Started {
    pub(crate) audio: Id,
    pub(crate) played: bool,
}

/// The panel's text box, in the layouts' pixels, and the font it's in.
#[derive(Resource)]
struct Panel {
    width: f32,
    height: f32,
    font: String,
    /// The banner's label, which a subtitle is copied to outside the
    /// letterbox, cut to its own size: its width, height and font.
    banner: Option<(f32, f32, String)>,
    letters: Letters,
    showing: Option<Showing>,
}

/// A subtitle on screen: its parts, the one showing, how long that one
/// has left, and whether its line is playing rather than unplayed.
struct Showing {
    parts: Vec<Part>,
    /// Its parts as cut for the banner, and how long it's been showing.
    banner: Vec<Part>,
    elapsed: f32,
    at: usize,
    left: f32,
    played: bool,
}

/// Some of a subtitle's text, shown for its share of the subtitle's time.
#[derive(Debug, PartialEq)]
struct Part {
    text: String,
    seconds: f32,
}

/// The panel's text.
#[derive(Component)]
struct Words;

/// Splits `text` into the parts shown in turn: as much as fits `lines`
/// lines, each as long as `fits` lets it be, ending after the last full
/// stop within that where there is one. Each part lasts its share of
/// `length` by characters.
fn parts(text: &str, lines: usize, fits: impl Fn(&str) -> bool, length: f32) -> Vec<Part> {
    let total = text.trim().chars().count().max(1) as f32;
    let words: Vec<&str> = text.split_whitespace().collect();
    let stop = |w: &&str| w.trim_end_matches(['"', '\'', ')']).ends_with('.');
    let mut out = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut j = i;
        for _ in 0..lines.max(1) {
            if j == words.len() {
                break;
            }
            // A line holds at least one word, however long.
            let mut k = j + 1;
            while k < words.len() && fits(&words[j..=k].join(" ")) {
                k += 1;
            }
            j = k;
        }
        let end = match j < words.len() {
            true => (words[i..j].iter().rposition(stop)).map_or(j, |p| i + p + 1),
            false => j,
        };
        let part = words[i..end].join(" ");
        let seconds = length * part.chars().count() as f32 / total;
        out.push(Part {
            text: part,
            seconds,
        });
        i = end;
    }
    // The last takes what's left, so the subtitle lasts its whole length.
    let before: f32 = out.iter().rev().skip(1).map(|p| p.seconds).sum();
    if let Some(last) = out.last_mut() {
        last.seconds = (length - before).max(0.0);
    }
    out
}

/// The part of `parts` due `elapsed` seconds in; the last once they've
/// all run.
fn at(parts: &[Part], elapsed: f32) -> Option<&str> {
    let mut until = 0.0;
    for part in parts {
        until += part.seconds;
        if elapsed < until {
            return Some(&part.text);
        }
    }
    parts.last().map(|p| p.text.as_str())
}

/// Puts the panel up, empty, where the layouts have it.
fn spawn(mut commands: Commands, loaded: Res<Loaded>, mut faces: ResMut<Assets<Face>>) {
    let game = &loaded.game;
    let screen = Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    };
    let panel = game.screen_widget(FILE, HOLDER).and_then(|holder| {
        let panel = holder.children.into_iter().find(|w| w.name == PANEL)?;
        let bounds = panel.bounds(screen);
        let label = panel.children.into_iter().find(|w| w.name == LABEL)?;
        let at = label.bounds(bounds);
        let text = label
            .parts
            .into_iter()
            .find(|p| p.kind.as_deref() == Some("LABEL"))?;
        Some((text.bounds.place(at), text))
    });
    let Some((at, label)) = panel else {
        warn!("{FILE}: no {PANEL} panel");
        return;
    };
    let mut letters = Letters::default();
    let font = label.font.clone().unwrap_or_default();
    letters.read(game, &mut faces, &font);
    let text_font = letters.font(&font);
    let FontSize::Px(size) = text_font.font_size else {
        return;
    };
    let [r, g, b] = label.colour;
    let percent = |v: f32, of: f32| Val::Percent(100.0 * v / of);
    let stage = commands
        .spawn((
            Stage,
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(crate::ui::layer::SUBTITLES),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ))
        .id();
    let holder = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(at.left, SCREEN[0]),
                top: percent(at.top, SCREEN[1]),
                width: percent(at.width(), SCREEN[0]),
                height: percent(at.height(), SCREEN[1]),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(stage),
        ))
        .id();
    commands.spawn((
        Words,
        LayoutFont(size),
        Text::default(),
        text_font,
        TextColor(Color::srgb(r, g, b)),
        TextLayout::new(Justify::Center, LineBreak::WordBoundary),
        Visibility::Hidden,
        Pickable::IGNORE,
        ChildOf(holder),
    ));
    let banner = crate::banner::layout(game).map(|(panel, label)| {
        let at = crate::banner::label_bounds(&panel, &label);
        let font = label.font.unwrap_or_default();
        letters.read(game, &mut faces, &font);
        (at.width(), at.height(), font)
    });
    commands.insert_resource(Panel {
        width: at.width(),
        height: at.height(),
        font,
        banner,
        letters,
        showing: None,
    });
}

/// Starts the subtitle of a line that's started playing, in place of any
/// showing, and posts its line in the message log outside the letterbox.
#[allow(clippy::too_many_arguments)]
fn start(
    mut started: MessageReader<Started>,
    mut panel: ResMut<Panel>,
    loaded: Res<Loaded>,
    mut sounds: ResMut<Sounds>,
    mut log: ResMut<MessageLog>,
    sim: Option<Res<Sim>>,
    fixed: Res<Time<Fixed>>,
    time: Res<Time>,
) {
    let Some(Started { audio, played }) = started.read().last() else {
        return;
    };
    // A line that names no audio does nothing, leaving the one playing.
    let Some(audio) = sounds.audio(audio) else {
        return;
    };
    panel.showing = None;
    if !shown(&log) {
        return;
    }
    let Some(subtitle) = audio.subtitle.as_ref() else {
        return;
    };
    let game = &loaded.game;
    let strings = game.strings();
    let text = match subtitle.text.strip_prefix('#') {
        Some(key) => strings.get(key).cloned().unwrap_or_default(),
        None => subtitle.text.clone(),
    };
    let length = match (*played, subtitle.duration > 0.0) {
        // One that can't be played shows for its noted length, at once.
        (false, _) => subtitle.noted,
        (true, true) => Some(subtitle.duration),
        (true, false) => (audio.files.first()).and_then(|f| game.audio_seconds(f)),
    };
    let Some(length) = length.filter(|l| *l > 0.0) else {
        warn!("{}: no length for its subtitle", audio.files.join(", "));
        return;
    };
    let cut = |width: f32, height: f32, font: &str| {
        let (_, line) = panel.letters.measure(font, "");
        let lines = ((height / line.max(1.0)) as usize).max(1);
        let fits = |words: &str| panel.letters.measure(font, words).0 <= width;
        parts(&text, lines, fits, length)
    };
    let shown = cut(panel.width, panel.height, &panel.font);
    let banner = (panel.banner.as_ref()).map_or_else(Vec::new, |(w, h, font)| cut(*w, *h, font));
    if let Some(first) = shown.first() {
        let left = first.seconds;
        let played = *played;
        panel.showing = Some(Showing {
            parts: shown,
            banner,
            elapsed: 0.0,
            at: 0,
            left,
            played,
        });
    }
    if crate::curtains::interface_up(sim, fixed) && log.post(KIND, text, None, time.elapsed_secs())
    {
        log.sound(&time, &mut sounds);
    }
}

/// Whether subtitles are on.
fn shown(log: &MessageLog) -> bool {
    log.look.vars.flag("ViewShowSubtitles", true)
}

/// Steps the subtitle on through its parts in real time, standing still
/// while the match is paused, and shows the part it's at while the
/// letterbox is up.
#[allow(clippy::too_many_arguments)]
fn show(
    mut panel: ResMut<Panel>,
    mut copied: ResMut<crate::banner::Copied>,
    log: Res<MessageLog>,
    orders: Option<Res<Orders>>,
    sim: Option<Res<Sim>>,
    fixed: Res<Time<Fixed>>,
    real: Res<Time<Real>>,
    mut words: Query<(&mut Text, &mut Visibility), With<Words>>,
) {
    let paused = orders.as_ref().is_some_and(|o| o.paused());
    let skipping = orders.is_some_and(|o| o.skipping());
    let letterboxed = !crate::curtains::interface_up(sim, fixed);
    // Hurrying a scene on under the letterbox stops the line playing,
    // and its subtitle goes with it.
    let hushed = skipping && letterboxed && panel.showing.as_ref().is_some_and(|s| s.played);
    if !shown(&log) || hushed {
        panel.showing = None;
    }
    if let Some(showing) = &mut panel.showing
        && !paused
    {
        showing.left -= real.delta_secs();
        showing.elapsed += real.delta_secs();
        while showing.left <= 0.0 {
            showing.at += 1;
            match showing.parts.get(showing.at) {
                Some(part) => showing.left += part.seconds,
                None => break,
            }
        }
    }
    if panel
        .showing
        .as_ref()
        .is_some_and(|s| s.at >= s.parts.len())
    {
        panel.showing = None;
    }
    let part = panel.showing.as_ref().map(|s| s.parts[s.at].text.as_str());
    let banner = panel
        .showing
        .as_ref()
        .and_then(|s| at(&s.banner, s.elapsed));
    if copied.0.as_deref() != banner {
        copied.0 = banner.map(str::to_string);
    }
    for (mut text, mut visibility) in &mut words {
        let says = part.unwrap_or_default();
        if text.0 != says {
            text.0 = says.to_string();
        }
        visibility.set_if_neq(crate::ui::visible(part.is_some() && letterboxed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether `words` fit a line of `width` characters.
    fn within(width: usize) -> impl Fn(&str) -> bool {
        move |words| words.chars().count() <= width
    }

    fn texts(parts: &[Part]) -> Vec<&str> {
        parts.iter().map(|p| p.text.as_str()).collect()
    }

    #[test]
    fn a_subtitle_is_split_into_parts_that_fill_the_box() {
        let text = "one two three four five six";
        // Two lines of nine characters: "one two" "three" then "four five" "six".
        let parts = parts(text, 2, within(9), 27.0);
        assert_eq!(texts(&parts), ["one two three", "four five six"]);
        assert_eq!(parts[0].seconds, 13.0);
        assert_eq!(parts[1].seconds, 14.0, "the rest of the length");
    }

    #[test]
    fn a_part_ends_after_the_last_full_stop_that_fits() {
        // "Go now. The" and "rest follows" would fill two lines; then
        // "The rest" and "follows here".
        let parts = parts("Go now. The rest follows here soon", 2, within(12), 10.0);
        assert_eq!(texts(&parts), ["Go now.", "The rest follows here", "soon"]);
    }

    #[test]
    fn a_short_subtitle_is_one_part_for_its_whole_length() {
        let parts = parts("Hold the line.", 3, within(40), 4.0);
        assert_eq!(texts(&parts), ["Hold the line."]);
        assert!((parts[0].seconds - 4.0).abs() < 1e-6);
    }

    #[test]
    fn the_banners_copy_shows_the_part_due_then_the_last() {
        let cut = parts("one two three four", 1, within(7), 4.0);
        assert_eq!(texts(&cut), ["one two", "three", "four"]);
        assert_eq!(at(&cut, 0.0), Some("one two"));
        assert_eq!(at(&cut, 2.5), Some("three"));
        assert_eq!(at(&cut, 99.0), Some("four"));
        assert_eq!(at(&[], 0.0), None);
    }

    #[test]
    fn a_word_too_long_for_a_line_has_one_to_itself() {
        let parts = parts("a enormousword b", 1, within(4), 1.0);
        assert_eq!(texts(&parts), ["a", "enormousword", "b"]);
    }
}
