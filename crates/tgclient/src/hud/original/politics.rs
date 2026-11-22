//! The politics panel in its layout: the [rows](crate::politics::Rows) in
//! the tiny font down a tooltip-style frame against the screen's top right,
//! which grows downward with them, each row's tooltip in the docked pane.

use bevy::prelude::*;
use bevy::text::Font as Face;
use bevy::ui::RelativeCursorPosition;
use tgdata::widgets::{Rect, SCREEN, Widget};

use super::lettering::{Letters, Measured, PLAIN, Sliced};
use super::{Drawer, Step, reach};
use crate::politics::{Politics, Rows, WIDGET};
use crate::ui::{Blocks, Hint, Marked};

/// The panel, holding its rows.
#[derive(Component)]
pub struct Panel {
    font: String,
    colour: Color,
    /// A side out of the match's name colour.
    out: Color,
    /// From the panel's left to the rows' text.
    inset: f32,
    row: Vec2,
    top: f32,
    bottom: f32,
    separation: f32,
    relation_at: f32,
    gem: Option<(Handle<Image>, Vec2)>,
    /// The ally, neutral and enemy markers.
    markers: [Option<(Handle<Image>, Vec2)>; 3],
}

/// The panel ready to spawn.
pub(super) struct Built {
    at: Rect,
    frame: Option<(Handle<Image>, f32)>,
    panel: Panel,
}

/// The panel `widgets` lay out, if the game has one.
pub(super) fn build(
    widgets: &[Widget],
    drawer: &mut Drawer,
    (letters, faces): (&mut Letters, &mut Assets<Face>),
) -> Option<Built> {
    let widget = (widgets.iter()).find(|w| w.name.eq_ignore_ascii_case(WIDGET))?;
    let number = |key: &str| {
        (widget.value(key))
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    let mut chain = Vec::new();
    if let Some(view) = &widget.view {
        chain.push(Step::Into(view.bounds));
    }
    let at = reach(&chain);
    let label = widget.find("Label");
    let row = label.and_then(|l| l.view.as_ref()).map_or(at, |v| {
        let mut chain = chain.clone();
        chain.push(Step::Into(v.bounds));
        reach(&chain)
    });
    let part = label.and_then(|l| l.parts.first());
    let font = part.and_then(|p| p.font.clone()).unwrap_or(PLAIN.into());
    letters.read(drawer.pictures.game, faces, &font);
    let [r, g, b] = part.map_or([1.0; 3], |p| p.colour);
    let game = drawer.pictures.game;
    let mut glyph = |name: Option<String>| {
        let image = drawer
            .pictures
            .get(&format!("Fonts/{font}/{}.png", name?))?;
        let size = drawer.pictures.images.get(&image)?.size_f32();
        Some((image, size))
    };
    let look = crate::politics::Look::new(game);
    let gem = glyph(game.side_glyph());
    let markers = look.glyphs.clone().map(&mut glyph);
    let tiles = (widget.find("Background").and_then(|b| b.view.as_ref()))
        .into_iter()
        .flat_map(|v| std::iter::once(v).chain(&v.children))
        .find_map(|v| v.tiles.as_ref());
    let frame = tiles.and_then(|t| super::tips::framed(&mut drawer.pictures, t));
    Some(Built {
        at,
        frame,
        panel: Panel {
            font,
            colour: Color::srgb(r, g, b),
            out: Color::srgb(look.out[0], look.out[1], look.out[2]),
            inset: row.left - at.left,
            row: Vec2::new(row.width(), row.height()),
            top: number("offset_top"),
            bottom: number("offset_bottom"),
            separation: number("separation"),
            relation_at: look.relation_at,
            gem,
            markers,
        },
    })
}

impl Built {
    pub(super) fn spawn(self, screen: &mut ChildSpawnerCommands) {
        let percent = |v: f32, of: f32| Val::Percent(100.0 * v / of);
        let mut panel = screen.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(self.at.left, SCREEN[0]),
                top: percent(self.at.top, SCREEN[1]),
                width: percent(self.at.width().max(0.0), SCREEN[0]),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            self.panel,
            Visibility::Hidden,
            Blocks,
            RelativeCursorPosition::default(),
        ));
        if let Some((image, border)) = self.frame {
            let tile = SliceScaleMode::Tile { stretch_value: 1.0 };
            let slicer = TextureSlicer {
                border: BorderRect::all(border),
                center_scale_mode: tile,
                sides_scale_mode: tile,
                max_corner_scale: 1.0,
            };
            panel.insert((
                ImageNode::new(image).with_mode(NodeImageMode::Sliced(slicer)),
                Sliced,
            ));
        }
    }
}

/// A node `size` in the layouts' pixels.
fn sized(size: Vec2) -> impl Bundle {
    (
        Node {
            flex_shrink: 0.0,
            ..default()
        },
        Measured {
            size: Some(size),
            ..default()
        },
        Pickable::IGNORE,
    )
}

/// Shows the panel while it's open, its rows written afresh whenever they
/// change.
pub fn show(
    mut commands: Commands,
    politics: Res<Politics>,
    rows: Res<Rows>,
    letters: Option<Res<Letters>>,
    mut panels: Query<(Entity, Ref<Panel>, &mut Visibility)>,
) {
    let Some(letters) = letters else {
        return;
    };
    for (entity, panel, mut shown) in &mut panels {
        shown.set_if_neq(crate::ui::visible(politics.open));
        if !rows.is_changed() && !panel.is_added() {
            continue;
        }
        commands.entity(entity).despawn_children();
        commands.entity(entity).with_children(|lines| {
            lines.spawn(sized(Vec2::new(0.0, panel.top)));
            for (n, row) in rows.0.iter().enumerate() {
                if n > 0 {
                    lines.spawn(sized(Vec2::new(0.0, panel.separation)));
                }
                write(lines, &panel, &letters, row);
            }
            lines.spawn(sized(Vec2::new(0.0, panel.bottom)));
        });
    }
}

/// Writes `row`: the gem in the side's colour and its name, then the
/// marker at the panel's offset from the text's start.
fn write(
    lines: &mut ChildSpawnerCommands,
    panel: &Panel,
    letters: &Letters,
    row: &crate::politics::Row,
) {
    let ink = match row.out {
        true => panel.out,
        false => panel.colour,
    };
    let mut line = lines.spawn((
        Node {
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
    ));
    if let Some(tip) = &row.tip {
        line.insert((
            Marked::plain(row.name.clone()),
            Hint(tip.clone()),
            RelativeCursorPosition::default(),
            Pickable::default(),
        ));
    }
    line.with_children(|line| {
        line.spawn(sized(Vec2::new(panel.inset, panel.row.y)));
        line.spawn((
            Node {
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                flex_shrink: 0.0,
                ..default()
            },
            Measured {
                size: Some(Vec2::new(panel.relation_at, panel.row.y)),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|text| {
            if let Some((gem, size)) = &panel.gem {
                let [r, g, b] = row.colour;
                text.spawn((
                    ImageNode::new(gem.clone()).with_color(Color::srgb(r, g, b)),
                    sized(*size),
                ));
            }
            text.spawn(letters.text(&panel.font, &format!(" {}", row.name), ink));
        });
        let marker = row
            .relation
            .and_then(crate::politics::marker)
            .and_then(|at| panel.markers[at].as_ref());
        if let Some((image, size)) = marker {
            line.spawn((ImageNode::new(image.clone()), sized(*size)));
        }
    });
}
