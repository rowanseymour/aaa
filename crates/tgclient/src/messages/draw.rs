//! Drawing the log: its lines faded as they age, the whole log listed,
//! and the icons that look at where a line happened.

use bevy::ecs::system::SystemParam;
use bevy::image::TRANSPARENT_IMAGE_HANDLE;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use tgrender::camera::RtsCamera;
use tgrender::space::world;

use crate::displays::{self, Contents, Displays, Panel, Window};
use crate::hud::Hud;
use crate::ui::{Font, Hint, font};
use crate::{Loaded, Local, Screen, Sim};

use super::log::{Line, Look, MessageLog, Place};
use super::{CYCLE, ICON, ICON_GAP, ICON_MARGIN, LINE_GAP, SHADOW, WIDTH};

/// The log's box, in the top left corner until the interface played
/// places it.
#[derive(Component)]
pub struct LogBox;

/// A line on screen, by its serial.
#[derive(Component)]
pub struct Row(u64);

/// A row's icon and text, faded with it.
#[derive(Component)]
pub struct Faded(Color);

/// How opaque a row text's shadow is unfaded, where the interface played
/// letters it in a font of its own rather than the log's.
#[derive(Component)]
pub struct Shade(pub f32);

/// A row's parts, to fade.
pub(crate) type Parts<'a> = (
    &'a Faded,
    Option<&'a Shade>,
    Option<&'a mut TextColor>,
    Option<&'a mut TextShadow>,
    Option<&'a mut ImageNode>,
);

/// The icon of the line with this serial, which looks where it happened.
#[derive(Component)]
pub(crate) struct LookAt(u64);

pub fn spawn(mut commands: Commands, loaded: Res<Loaded>, mut log: ResMut<MessageLog>) {
    log.look.name_things(&loaded);
    let said = crate::ui::said(&log.look.strings);
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
    commands.spawn((
        LogBox,
        log_node(),
        Pickable::IGNORE,
        GlobalZIndex(crate::ui::layer::HUD),
        DespawnOnExit(Screen::Match),
    ));
}

/// How the log's box is laid out, in the top left corner.
pub fn log_node() -> Node {
    Node {
        position_type: PositionType::Absolute,
        top: px(crate::ui_scale::MARGIN),
        left: px(crate::ui_scale::MARGIN),
        width: px(WIDTH),
        flex_direction: FlexDirection::Column,
        row_gap: px(LINE_GAP),
        ..default()
    }
}

/// Lays out the lines on screen, and fades each as it goes.
pub fn show(
    mut commands: Commands,
    time: Res<Time>,
    (log, hud): (Res<MessageLog>, Res<Hud>),
    root: Single<(Entity, Option<&Children>), With<LogBox>>,
    rows: Query<(&Row, &Children)>,
    tree: Query<&Children>,
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
    if laid != wanted || hud.is_changed() {
        for &c in children {
            commands.entity(c).despawn();
        }
        for &(line, _) in &shown {
            spawn_row(&mut commands, root, &log.look, line, *hud);
        }
        return;
    }
    for (&row, &(_, opacity)) in children.iter().zip(&shown) {
        // A line naming a side has its words and gems further down.
        for part in tree.iter_descendants(row) {
            let Ok((Faded(colour), shade, text, shadow, image)) = faded.get_mut(part) else {
                continue;
            };
            let colour = colour.with_alpha(colour.alpha() * opacity);
            if let Some(mut t) = text {
                t.set_if_neq(TextColor(colour));
            }
            if let Some(mut shadow) = shadow {
                let unfaded = shade.map_or(SHADOW.color.alpha(), |s| s.0);
                let faded = SHADOW.color.with_alpha(unfaded * opacity);
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
    hud: Res<Hud>,
    mut contents: Query<(Entity, &Contents, &mut ScrollPosition)>,
) {
    if !displays.is_open(Panel::MessageLog) {
        listed.0 = None;
        return;
    }
    let span = (log.lines.front())
        .zip(log.lines.back())
        .map(|(a, b)| (a.serial, b.serial));
    if listed.0 == Some(span) && !hud.is_changed() {
        return;
    }
    let Some((list, _, mut scroll)) = contents
        .iter_mut()
        .find(|(_, c, _)| c.0 == Panel::MessageLog)
    else {
        return;
    };
    list_all(&mut commands, list, &log, &mut scroll, *hud);
    listed.0 = Some(span);
}

/// A list of the whole log, oldest first and unfaded, kept scrolled to
/// the newest, by the serials of the oldest and newest it shows once it
/// shows any.
#[derive(Component, Default)]
pub struct WholeLog(Option<Option<(u64, u64)>>);

/// Keeps each [`WholeLog`] up with the log.
pub fn relist(
    mut commands: Commands,
    log: Res<MessageLog>,
    hud: Res<Hud>,
    mut lists: Query<(Entity, &mut WholeLog, &mut ScrollPosition)>,
) {
    let span = (log.lines.front())
        .zip(log.lines.back())
        .map(|(a, b)| (a.serial, b.serial));
    for (list, mut shown, mut scroll) in &mut lists {
        if shown.0 != Some(span) || hud.is_changed() {
            list_all(&mut commands, list, &log, &mut scroll, *hud);
            shown.0 = Some(span);
        }
    }
}

/// Lists every line of the log in `list`, scrolled to the newest.
fn list_all(
    commands: &mut Commands,
    list: Entity,
    log: &MessageLog,
    scroll: &mut ScrollPosition,
    hud: Hud,
) {
    commands.entity(list).despawn_children();
    for line in &log.lines {
        spawn_row(commands, list, &log.look, line, hud);
    }
    // Layout holds it to the bottom of the list.
    scroll.y = f32::MAX;
}

/// Spawns a line in `root`, its icon the one `hud` draws for its kind.
pub(super) fn spawn_row(commands: &mut Commands, root: Entity, look: &Look, line: &Line, hud: Hud) {
    let kind = look.kinds.get(&line.kind);
    let colour = kind.map_or(Color::WHITE, |k| k.colour);
    let modern = kind
        .and_then(|k| k.modern.clone())
        .filter(|_| hud == Hud::Modern);
    let icon = match modern {
        Some(_) => None,
        None => kind.and_then(|k| k.icon.clone()),
    };
    let icon = icon.unwrap_or(TRANSPARENT_IMAGE_HANDLE);
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
                    margin: UiRect::top(px(ICON_MARGIN)),
                    ..default()
                },
            ));
            if let Some(path) = modern {
                icon.insert(displays::Icon::new(path));
            }
            match line.place {
                Some(_) => {
                    let id = crate::ui::called(format!("message.{}.look", line.serial));
                    icon.insert((
                        LookAt(line.serial),
                        Button,
                        id,
                        Hint(look.to_location.clone()),
                    ))
                    .observe(look_at);
                }
                None => {
                    icon.insert(Pickable::IGNORE);
                }
            }
            let words = |words: String, colour: Color| {
                (
                    Faded(colour),
                    Text::new(words),
                    font(Font::Hud),
                    TextColor(colour),
                    SHADOW,
                    Pickable::IGNORE,
                )
            };
            // The rest of the row, wrapping only at the log's edge.
            let rest = Node {
                flex_grow: 1.0,
                flex_basis: px(0),
                ..default()
            };
            if line.marks.is_empty() && line.inks.is_empty() {
                row.spawn((words(line.text.clone(), colour), rest));
                return;
            }
            // Text can't hold a picture, so a line naming a side is laid
            // out a word at a time, wrapping between words, each gem kept
            // with the first word of the name it marks.
            let rest = Node {
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignItems::Center,
                column_gap: px(WORD_GAP),
                ..rest
            };
            row.spawn((rest, Pickable::IGNORE)).with_children(|parts| {
                for Part { gem, word, ink } in parts_of(line) {
                    let colour = ink.map_or(colour, |[r, g, b]| Color::srgb(r, g, b));
                    let Some(rgb) = gem else {
                        parts.spawn(words(word, colour));
                        continue;
                    };
                    let mark = Node {
                        width: px(LINE_GEM),
                        height: px(LINE_GEM),
                        margin: UiRect::right(px(GEM_GAP)),
                        ..default()
                    };
                    let [red, green, blue] = rgb;
                    let together = Node {
                        align_items: AlignItems::Center,
                        ..default()
                    };
                    parts
                        .spawn((together, Pickable::IGNORE))
                        .with_children(|together| {
                            together.spawn((
                                Faded(Color::srgb(red, green, blue)),
                                crate::ui::side_mark(&look.gem, rgb, mark),
                            ));
                            together.spawn(words(word, colour));
                        });
                }
            });
        });
}

/// How big a side's gem is in a line, the space after it, and the space
/// between a line's words where it's laid out a word at a time.
const LINE_GEM: f32 = 15.0;
const GEM_GAP: f32 = 3.0;
const WORD_GAP: f32 = 3.0;

/// A word of a line as it's laid out, with the gem in a side's colour
/// before it where it starts a side's name, and its own colour if it has
/// one.
#[derive(Debug, PartialEq)]
pub(crate) struct Part {
    pub(crate) gem: Option<[f32; 3]>,
    pub(crate) word: String,
    pub(crate) ink: Option<[f32; 3]>,
}

/// A line's words, each that starts a side's name with its gem, and
/// each in the colour its first byte is inked in.
pub(super) fn parts_of(line: &Line) -> Vec<Part> {
    let ink = |at: usize| {
        let before = line.inks.iter().rev().find(|i| i.at <= at);
        before.and_then(|i| i.rgb)
    };
    let mut parts = Vec::new();
    let mut cuts = line.marks.iter().map(|m| (m.at, Some(m.rgb))).peekable();
    let mut from = (0, None);
    loop {
        let to = cuts.peek().map_or(line.text.len(), |&(at, _)| at);
        let segment = line.text.get(from.0..to).unwrap_or_default();
        let mut gem = from.1;
        let words = segment.split_whitespace().map(|word| {
            // Each word is a slice of the segment, so where it starts.
            let at = from.0 + (word.as_ptr() as usize - segment.as_ptr() as usize);
            (at, word)
        });
        let mut words = words.peekable();
        // A side with no name still has its gem.
        if gem.is_some() && words.peek().is_none() {
            parts.push(Part {
                gem,
                word: String::new(),
                ink: ink(from.0),
            });
        }
        for (at, word) in words {
            parts.push(Part {
                gem: gem.take(),
                word: word.to_string(),
                ink: ink(at),
            });
        }
        match cuts.next() {
            Some(cut) => from = cut,
            None => break,
        }
    }
    parts
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
pub(crate) fn look_at(
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

/// Looks at the next line back when asked.
pub fn cycle(
    mut intents: MessageReader<crate::intent::Intent>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut viewer: Viewer,
) {
    if !crate::intent::asked(&mut intents, CYCLE) {
        return;
    }
    if let Some(place) = log.cycle(time.elapsed_secs()).and_then(|l| l.place) {
        viewer.look_at(place);
    }
}
