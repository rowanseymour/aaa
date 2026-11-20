//! The resource bars: the player's economy, and the bar previewing what
//! the order under the cursor would make of it, which the first slides
//! away from (see the interface spec's resource bars).

use bevy::prelude::*;
use bevy::text::Font as Face;
use bevy::ui::RelativeCursorPosition;
use tgdata::{Id, widgets::Widget};

use super::lettering::{Letters, Measured, PLAIN};
use super::{Drawer, Step, on_screen, placed, point2, reach};
use crate::actions::Blocks;
use crate::resource_bar::{Cell, Colours, Previewing};

/// The bars, under the one that slides away from them first.
pub const BARS: [&str; 2] = ["EconomicImpact", "EconomyBar"];

/// What a bar's resource's figures are drawn within.
const RESOURCE: &str = "resource_IDS";
const LABEL: &str = "ResourceLabel";

/// A bar ready to spawn.
pub struct Bar {
    slide: Slide,
    pictures: Vec<(Vec<Step>, Handle<Image>)>,
    cells: Vec<BarCell>,
}

/// One of a bar's cells ready to spawn.
struct BarCell {
    resource: Id,
    at: tgdata::widgets::Rect,
    font: String,
    colour: [f32; 3],
    glyph: Option<(Handle<Image>, Vec2)>,
}

/// How a bar moves as previews start and end.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    /// Where it rests while there's no preview, from its bounds.
    hidden: [f32; 2],
    /// How long it takes to slide to its bounds or back, in seconds.
    time: f32,
    /// Whether it's the preview, seen only while there's one.
    preview: bool,
    /// How far it's slid to its bounds, from 0 to 1.
    out: f32,
}

impl Drawer<'_> {
    /// The bar `widget` lays out, with a cell for each of `cells`.
    pub fn bar(
        &mut self,
        widget: &Widget,
        cells: &[Id],
        (letters, faces): (&mut Letters, &mut Assets<Face>),
    ) -> Bar {
        let number = |key: &str| widget.value(key).and_then(|v| v.trim().parse::<f32>().ok());
        let flag = |key: &str| {
            widget
                .value(key)
                .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        };
        let slide = Slide {
            hidden: widget
                .value("hidden_offset")
                .and_then(point2)
                .unwrap_or_default(),
            time: number("slide_time").unwrap_or(0.0),
            preview: flag("slide_economy_bar"),
            out: 0.0,
        };
        let kept = std::mem::take(&mut self.drawn);
        self.widget(widget, &[]);
        let pictures = std::mem::replace(&mut self.drawn, kept);
        let within: Vec<Step> = widget.view.iter().map(|v| Step::Into(v.bounds)).collect();
        let game = self.pictures.game;
        let resources = game.resources();
        let cells = (cells.iter())
            .filter_map(|resource| {
                let child = widget
                    .children
                    .iter()
                    .find(|c| c.value(RESOURCE).is_some_and(|r| Id::from(r) == *resource))?;
                let mut chain = within.clone();
                chain.extend(child.view.iter().map(|v| Step::Into(v.bounds)));
                let label = child.find(LABEL).and_then(|l| l.parts.first());
                chain.extend(label.map(|l| Step::Into(l.bounds)));
                let font = label.and_then(|l| l.font.clone()).unwrap_or(PLAIN.into());
                letters.read(game, faces, &font);
                // The resource's glyph, as the font draws it.
                let kind = resources.iter().find(|r| &r.id == resource);
                let glyph = kind.and_then(|k| k.glyph.as_ref()).and_then(|name| {
                    let image = self.pictures.get(&format!("Fonts/{font}/{name}.png"))?;
                    let size = self.pictures.images.get(&image)?.size_f32();
                    Some((image, size))
                });
                Some(BarCell {
                    resource: resource.clone(),
                    at: reach(&chain),
                    font,
                    colour: label.map_or([1.0; 3], |l| l.colour),
                    glyph,
                })
            })
            .collect();
        Bar {
            slide,
            pictures,
            cells,
        }
    }
}

impl Bar {
    pub fn spawn(
        self,
        screen: &mut ChildSpawnerCommands,
        game: &tgdata::GameData,
        letters: &Letters,
        short: Color,
    ) {
        let preview = self.slide.preview;
        screen
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                self.slide,
                Pickable::IGNORE,
                match preview {
                    true => Visibility::Hidden,
                    false => Visibility::Inherited,
                },
            ))
            .with_children(|bar| {
                for (chain, image) in self.pictures {
                    bar.spawn((
                        placed(&chain),
                        ImageNode::new(image).with_mode(NodeImageMode::Stretch),
                        Blocks,
                        RelativeCursorPosition::default(),
                    ));
                }
                for cell in self.cells {
                    let [r, g, b] = cell.colour;
                    let body = Color::srgb(r, g, b);
                    bar.spawn((
                        // From the top, as some cells' bounds run far below
                        // the bar.
                        Node {
                            align_items: AlignItems::FlexStart,
                            ..on_screen(cell.at)
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|holder| {
                        if let Some((glyph, size)) = cell.glyph {
                            holder.spawn((
                                ImageNode::new(glyph),
                                Node::default(),
                                Measured {
                                    size: Some(size),
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ));
                        }
                        let (text, hint) = Cell::new(game, preview, &cell.resource);
                        // Across the rest of the cell, which is hovered whole.
                        let fill = Node {
                            flex_grow: 1.0,
                            align_self: AlignSelf::Stretch,
                            ..default()
                        };
                        let mut text = holder.spawn((
                            text,
                            letters.text(&cell.font, "", body),
                            fill,
                            Colours { body, short },
                        ));
                        if let Some(hint) = hint {
                            text.insert((hint, Pickable::default()));
                        }
                    });
                }
            });
    }
}

/// Slides the bar away while a preview's shown, and shows the preview.
pub fn slide(
    time: Res<Time>,
    previewing: Res<Previewing>,
    mut bars: Query<(&mut Slide, &mut Node, &mut Visibility)>,
) {
    let previewing = previewing.0;
    for (mut slide, mut node, mut visibility) in &mut bars {
        let target = if previewing { 1.0 } else { 0.0 };
        let step = match slide.time > 0.0 {
            true => time.delta_secs() / slide.time,
            false => 1.0,
        };
        let out = match target > slide.out {
            true => (slide.out + step).min(target),
            false => (slide.out - step).max(target),
        };
        if slide.out != out {
            slide.out = out;
        }
        let [x, y] = slide.hidden.map(|h| h * (1.0 - out));
        let (left, top) = (
            percent(100.0 * x / super::SCREEN[0]),
            percent(100.0 * y / super::SCREEN[1]),
        );
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
        if slide.preview {
            visibility.set_if_neq(match previewing {
                true => Visibility::Inherited,
                false => Visibility::Hidden,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_slides_to_its_bounds_while_a_preview_shows() {
        let mut app = App::new();
        app.init_resource::<Time>()
            .init_resource::<Previewing>()
            .add_systems(Update, slide);
        let still = Slide {
            hidden: [0.0, 25.0],
            time: 0.0,
            preview: false,
            out: 0.0,
        };
        let bar = app
            .world_mut()
            .spawn((still, Node::default(), Visibility::Inherited))
            .id();
        let preview = (app.world_mut())
            .spawn((
                Slide {
                    hidden: [0.0, 0.0],
                    preview: true,
                    ..still
                },
                Node::default(),
                Visibility::Hidden,
            ))
            .id();
        app.update();
        let top = |app: &App, e| app.world().get::<Node>(e).unwrap().top;
        let seen = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        assert_eq!(top(&app, bar), percent(100.0 * 25.0 / 768.0));
        assert_eq!(seen(&app, preview), Visibility::Hidden);
        app.world_mut().resource_mut::<Previewing>().0 = true;
        app.update();
        assert_eq!(top(&app, bar), percent(0.0));
        assert_eq!(seen(&app, preview), Visibility::Inherited);
    }
}
