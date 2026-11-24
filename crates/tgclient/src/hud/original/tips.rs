//! The one tooltip pane the original's match shows every tooltip in,
//! anchored above the control panel's right end (see the interface spec's
//! tooltips), for the grid's buttons and every [`Hint`](crate::ui::Hint)
//! within the layouts' screen alike.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use tgdata::widgets::{SCREEN, Tiles, Widget};

use super::lettering::{Letters, Measured, PLAIN, Sliced};
use super::{GAME, Pictures};
use crate::actions::{Said, Tip};
use crate::ui::{Hinted, Marked};

/// The pane's layout.
const PANE: &str = "ToolTipGame";
/// Its text's.
const TEXT: &str = "LeftLabel";
/// How big a glyph in a tooltip's table is drawn, in the layouts' pixels.
const GLYPH: f32 = 13.0;
/// How far from the text's left a table's amounts start, and the gap
/// between amounts, in the layouts' pixels, as the game's tooltips measure.
const TAB: f32 = 99.0;
const SPACE: f32 = 4.0;

/// The pane, and what it says.
#[derive(Component)]
pub struct Pane {
    font: String,
    /// The font a title is in.
    title: String,
    body: Color,
    key: Color,
    short: Color,
    said: Option<Saying>,
    /// The node within its padding that its lines are in.
    lines: Entity,
}

/// What the pane says: the grid's tooltip, or a hint docked in it.
#[derive(Clone, PartialEq, Debug)]
enum Saying {
    Tip(Tip),
    Hint(Option<Marked>, String),
}

/// The pane ready to spawn.
pub struct Built {
    node: Node,
    padding: Vec2,
    frame: Option<(Handle<Image>, f32)>,
    pane: Pane,
}

impl Built {
    /// The pane `widgets` lay out, anchored where the match's layer says.
    pub fn new(
        widgets: &[Widget],
        pictures: &mut Pictures,
        (letters, faces): (&mut Letters, &mut Assets<bevy::text::Font>),
        (key, short): (Color, Color),
    ) -> Option<Built> {
        let named = |name: &str| widgets.iter().find(|w| w.name.eq_ignore_ascii_case(name));
        let pane = named(PANE)?;
        let game = named(GAME);
        let number = |w: Option<&Widget>, key: &str| {
            w.and_then(|w| w.value(key))
                .and_then(|v| v.trim().parse::<f32>().ok())
        };
        let x = number(game, "anchored_tooltip_x").unwrap_or(SCREEN[0]);
        let y = number(game, "anchored_tooltip_y").unwrap_or(SCREEN[1]);
        let width = number(game, "anchored_tooltip_width").unwrap_or(SCREEN[0] / 3.0);
        let padding = Vec2::new(
            number(Some(pane), "padding_x").unwrap_or(0.0),
            number(Some(pane), "padding_y").unwrap_or(0.0),
        );
        let label = pane.find(TEXT).and_then(|l| l.parts.first());
        let font = label.and_then(|l| l.font.clone()).unwrap_or(PLAIN.into());
        letters.read(pictures.game, faces, &font);
        let title = (pictures.game.tooltip_title_font()).unwrap_or_else(|| font.clone());
        letters.read(pictures.game, faces, &title);
        let [r, g, b] = label.map_or([1.0; 3], |l| l.colour);
        let tiles = (pane.view.iter())
            .flat_map(|v| std::iter::once(v).chain(&v.children))
            .find_map(|v| v.tiles.as_ref());
        let percent = |v: f32, of: f32| Val::Percent(100.0 * v / of);
        Some(Built {
            node: Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                right: percent(SCREEN[0] - x, SCREEN[0]),
                bottom: percent(SCREEN[1] - y, SCREEN[1]),
                width: percent(width, SCREEN[0]),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            padding,
            frame: tiles.and_then(|t| framed(pictures, t)),
            pane: Pane {
                font,
                title,
                body: Color::srgb(r, g, b),
                key,
                short,
                said: None,
                lines: Entity::PLACEHOLDER,
            },
        })
    }

    /// Spawns it, giving its entity.
    pub fn spawn(mut self, screen: &mut ChildSpawnerCommands) -> Entity {
        // Within a node of its own, as a node's image is drawn within its
        // padding.
        let lines = (screen.commands().spawn((
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Measured {
                padding: Some(self.padding),
                ..default()
            },
            Pickable::IGNORE,
        )))
        .id();
        self.pane.lines = lines;
        let mut pane = screen.spawn((self.node, self.pane, Pickable::IGNORE));
        pane.add_child(lines);
        if let Some((image, border)) = self.frame {
            let tile = SliceScaleMode::Tile { stretch_value: 1.0 };
            let slicer = TextureSlicer {
                border: BorderRect::all(border),
                center_scale_mode: tile,
                sides_scale_mode: tile,
                max_corner_scale: 1.0,
            };
            pane.insert((
                ImageNode::new(image).with_mode(NodeImageMode::Sliced(slicer)),
                Sliced,
            ));
        }
        pane.id()
    }
}

/// One image of a tiled decal's six, for slicing into nine: the corners'
/// quarters at the corners, the edge of each border's image that's
/// outermost along its side, and the center's in the middle, each a
/// quarter of the corners' image across.
pub(super) fn framed(pictures: &mut Pictures, tiles: &Tiles) -> Option<(Handle<Image>, f32)> {
    let mut rgba = |path: &str| pictures.rgba(path);
    let corners = rgba(&tiles.corners)?;
    let (left, right, top, bottom, center) = (
        rgba(&tiles.left)?,
        rgba(&tiles.right)?,
        rgba(&tiles.top)?,
        rgba(&tiles.bottom)?,
        rgba(&tiles.center)?,
    );
    let c = corners.0.x.min(corners.0.y) / 2;
    if c == 0 {
        return None;
    }
    let side = 3 * c;
    let mut out = vec![0u8; (side * side * 4) as usize];
    // Copies a c by c square at `from` in `image` to `to` in `out`.
    let mut copy = |(size, data): &(UVec2, Vec<u8>), from: UVec2, to: UVec2| {
        for y in 0..c {
            for x in 0..c {
                let (sx, sy) = ((from.x + x).min(size.x - 1), (from.y + y).min(size.y - 1));
                let src = ((sy * size.x + sx) * 4) as usize;
                let dst = (((to.y + y) * side + to.x + x) * 4) as usize;
                if let Some(px) = data.get(src..src + 4) {
                    out[dst..dst + 4].copy_from_slice(px);
                }
            }
        }
    };
    let far = |size: UVec2| size.saturating_sub(UVec2::splat(c));
    let corner_far = far(corners.0);
    copy(&corners, UVec2::ZERO, UVec2::ZERO);
    copy(&corners, UVec2::new(corner_far.x, 0), UVec2::new(2 * c, 0));
    copy(&corners, UVec2::new(0, corner_far.y), UVec2::new(0, 2 * c));
    copy(&corners, corner_far, UVec2::splat(2 * c));
    copy(&left, UVec2::ZERO, UVec2::new(0, c));
    copy(&right, UVec2::new(far(right.0).x, 0), UVec2::new(2 * c, c));
    copy(&top, UVec2::ZERO, UVec2::new(c, 0));
    copy(
        &bottom,
        UVec2::new(0, far(bottom.0).y),
        UVec2::new(c, 2 * c),
    );
    copy(&center, UVec2::ZERO, UVec2::splat(c));
    let image = Image::new(
        Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    Some((pictures.images.add(image), c as f32))
}

/// Shows in the pane what the grid's button under the cursor says, or else
/// the hint under it, if the layouts' screen docks it there.
pub fn show(
    said: Res<Said>,
    hinted: Res<Hinted>,
    letters: Res<Letters>,
    pane: Single<(Entity, &mut Node, &mut Pane)>,
    mut commands: Commands,
) {
    let (entity, mut node, mut pane) = pane.into_inner();
    let docked = (hinted.0.as_ref()).filter(|h| h.dock == entity);
    crate::ui::show(&mut node, said.0.is_some() || docked.is_some());
    // Compared before it's copied, as it's mostly unchanged.
    let same = match (&pane.said, &said.0, docked) {
        (Some(Saying::Tip(was)), Some(tip), _) => was == tip,
        (Some(Saying::Hint(name, hint)), None, Some(h)) => *name == h.name && *hint == h.hint,
        (None, None, None) => true,
        _ => false,
    };
    if same {
        return;
    }
    let saying = match (&said.0, docked) {
        (Some(tip), _) => Some(Saying::Tip(tip.clone())),
        (None, Some(h)) => Some(Saying::Hint(h.name.clone(), h.hint.clone())),
        (None, None) => None,
    };
    let mut lines = commands.entity(pane.lines);
    lines.despawn_children();
    if let Some(saying) = &saying {
        lines.with_children(|lines| write(lines, &pane, &letters, saying));
    }
    pane.said = saying;
}

/// The pane's lines for what it says.
fn write(lines: &mut ChildSpawnerCommands, pane: &Pane, letters: &Letters, saying: &Saying) {
    let text = |words: &str, colour: Color| letters.text(&pane.font, words, colour);
    // A title in the title's font but as far apart as the rest's lines, and
    // for a tooltip, the blank line the game puts after it.
    let title = |lines: &mut ChildSpawnerCommands, name: &Marked, more: bool| {
        lines
            .spawn(letters.text_within(&pane.title, &pane.font, &name.before, pane.body))
            .with_children(|line| {
                let span = |words: &str, colour| {
                    (
                        TextSpan::new(words),
                        letters.font(&pane.title),
                        TextColor(colour),
                    )
                };
                line.spawn(span(&name.key, pane.key));
                line.spawn(span(&name.after, pane.body));
            });
        if more {
            // A space alone would be laid out as no line at all.
            lines.spawn(text("\u{a0}", pane.body));
        }
    };
    match saying {
        Saying::Hint(name, hint) => {
            if let Some(name) = name {
                title(lines, name, false);
            }
            if !hint.is_empty() {
                lines.spawn(text("", pane.body)).with_children(|line| {
                    for (words, colour) in crate::ui::stretches(hint) {
                        line.spawn((
                            TextSpan::new(words),
                            letters.font(&pane.font),
                            TextColor(colour.unwrap_or(pane.body)),
                        ));
                    }
                });
            }
        }
        Saying::Tip(tip) => {
            let more = tip.help.is_some()
                || !tip.table.is_empty()
                || !tip.notes.is_empty()
                || tip.why.is_some();
            title(lines, &tip.title, more);
            if let Some(help) = &tip.help {
                lines.spawn(text(help, pane.body));
            }
            if !tip.table.is_empty() {
                let table = Node {
                    display: Display::Grid,
                    grid_template_columns: vec![GridTrack::auto(), GridTrack::flex(1.0)],
                    ..default()
                };
                // Apart by a space, so a label past the tab doesn't run into
                // its amounts.
                let apart = Measured {
                    gap: Some(SPACE),
                    ..default()
                };
                lines
                    .spawn((table, apart, Pickable::IGNORE))
                    .with_children(|table| {
                        for row in &tip.table {
                            let label = Measured {
                                min_width: Some(TAB - SPACE),
                                ..default()
                            };
                            table.spawn((text(&row.label, pane.body), Node::default(), label));
                            let amounts = Measured {
                                gap: Some(SPACE),
                                ..default()
                            };
                            let line = (Node::default(), amounts, Pickable::IGNORE);
                            table.spawn(line).with_children(|line| {
                                for q in &row.amounts {
                                    let amount = Node {
                                        align_items: AlignItems::Center,
                                        ..default()
                                    };
                                    line.spawn((amount, Pickable::IGNORE)).with_children(
                                        |amount| {
                                            glyph(amount, q);
                                            amount.spawn(text(&q.label(), pane.body));
                                        },
                                    );
                                }
                            });
                        }
                    });
            }
            for note in &tip.notes {
                lines.spawn(text(note, pane.body));
            }
            if let Some(why) = &tip.why {
                lines.spawn(text(why, pane.short));
            }
        }
    }
}

/// The glyph of an amount's resource, where it has one.
fn glyph(line: &mut ChildSpawnerCommands, q: &crate::actions::Quantity) {
    if let Some(glyph) = &q.glyph {
        line.spawn((
            ImageNode::new(glyph.clone()),
            Node::default(),
            Measured {
                size: Some(Vec2::splat(GLYPH)),
                ..default()
            },
            Pickable::IGNORE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Docked;

    fn said(app: &App, pane: Entity) -> Option<Saying> {
        app.world().get::<Pane>(pane).unwrap().said.clone()
    }

    #[test]
    fn the_pane_says_the_grids_tip_before_a_hint_docked_in_it() {
        let mut app = App::new();
        app.init_resource::<Said>()
            .init_resource::<Hinted>()
            .init_resource::<Letters>()
            .add_systems(Update, show);
        let lines = app.world_mut().spawn(Node::default()).id();
        let pane = Pane {
            font: PLAIN.into(),
            title: PLAIN.into(),
            body: Color::WHITE,
            key: Color::WHITE,
            short: Color::WHITE,
            said: None,
            lines,
        };
        let pane = app.world_mut().spawn((Node::default(), pane)).id();
        let shown = |app: &App| app.world().get::<Node>(pane).unwrap().display;
        app.update();
        assert_eq!(shown(&app), Display::None);

        let docked = |dock| Docked {
            dock,
            name: None,
            hint: "Oil".into(),
        };
        app.world_mut().resource_mut::<Hinted>().0 = Some(docked(Entity::PLACEHOLDER));
        app.update();
        assert_eq!(shown(&app), Display::None);
        app.world_mut().resource_mut::<Hinted>().0 = Some(docked(pane));
        app.update();
        assert_eq!(shown(&app), Display::Flex);
        assert_eq!(said(&app, pane), Some(Saying::Hint(None, "Oil".into())));

        let tip = Tip {
            title: Marked::plain("Unpack"),
            ..default()
        };
        app.world_mut().resource_mut::<Said>().0 = Some(tip.clone());
        app.update();
        assert_eq!(said(&app, pane), Some(Saying::Tip(tip)));
        let written = app.world().get::<Children>(lines).map_or(0, |c| c.len());
        assert_eq!(written, 1);
    }
}
