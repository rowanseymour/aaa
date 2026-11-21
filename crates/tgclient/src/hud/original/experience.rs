//! The commander's experience, in the layouts' label for it: the font's
//! experience glyph, then the figure as
//! [`special_ops`](crate::special_ops) gives it, with its tooltip in the
//! docked pane. There only for a side with a general.

use bevy::prelude::*;
use bevy::text::Font as Face;
use bevy::ui::RelativeCursorPosition;
use tgdata::widgets::{Rect, Widget};

use super::lettering::{Letters, Measured, PLAIN};
use super::{Drawer, Step, on_screen, reach};
use crate::Sim;
use crate::special_ops::Panel;
use crate::ui::{Blocks, Hint, Marked};

/// The widget the experience is said in.
pub(super) const WIDGET: &str = "FactionExperience";
/// Its label.
const LABEL: &str = "Label";

/// The experience's figure: whether the glyph's before it, and what it was
/// last shown as, to redo only on a change.
#[derive(Component, Default)]
pub struct Figure {
    after_glyph: bool,
    shown: Option<i64>,
}

/// What holds the experience, whose tooltip tells it too.
#[derive(Component)]
pub struct Holder;

/// The experience ready to spawn.
pub(super) struct Built {
    at: Rect,
    font: String,
    colour: Color,
    glyph: Option<(Handle<Image>, Vec2)>,
    title: String,
}

/// The experience `widget` lays out, at the end of `within`, for a side
/// with a general.
pub(super) fn build(
    widget: &Widget,
    within: &[Step],
    drawer: &mut Drawer,
    panel: Option<&Panel>,
    (letters, faces): (&mut Letters, &mut Assets<Face>),
) -> Option<Built> {
    let panel = panel?;
    let label = (widget.parts.iter()).find(|p| p.name.eq_ignore_ascii_case(LABEL))?;
    let mut chain = within.to_vec();
    chain.push(Step::Into(label.bounds));
    let font = label.font.clone().unwrap_or(PLAIN.into());
    let game = drawer.pictures.game;
    letters.read(game, faces, &font);
    // As the font draws it.
    let glyph = game.experience_glyph().and_then(|name| {
        let image = drawer.pictures.get(&format!("Fonts/{font}/{name}.png"))?;
        let size = drawer.pictures.images.get(&image)?.size_f32();
        Some((image, size))
    });
    let [r, g, b] = label.colour;
    Some(Built {
        at: reach(&chain),
        font,
        colour: Color::srgb(r, g, b),
        glyph,
        title: panel.words.title.clone(),
    })
}

impl Built {
    pub(super) fn spawn(self, screen: &mut ChildSpawnerCommands, letters: &Letters) {
        screen
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..on_screen(self.at)
                },
                Holder,
                Marked::plain(self.title),
                Hint(String::new()),
                Blocks,
                RelativeCursorPosition::default(),
            ))
            .with_children(|holder| {
                let after_glyph = self.glyph.is_some();
                if let Some((glyph, size)) = self.glyph {
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
                let figure = Figure {
                    after_glyph,
                    shown: None,
                };
                holder.spawn((letters.text(&self.font, "", self.colour), figure));
            });
    }
}

/// Keeps the experience and its tooltip to the side's.
pub fn refresh(
    panel: Option<Res<Panel>>,
    sim: Res<Sim>,
    mut figures: Query<(&mut Text, &mut Figure)>,
    mut holders: Query<&mut Hint, With<Holder>>,
) {
    let Some(panel) = panel else {
        return;
    };
    let experience = sim.0.experience(panel.player).unwrap_or(0.0);
    let rounded = experience.round() as i64;
    for (mut text, mut figure) in &mut figures {
        if figure.shown != Some(rounded) {
            figure.shown = Some(rounded);
            let space = if figure.after_glyph { " " } else { "" };
            text.0 = format!("{space}{}", panel.experience(rounded));
        }
    }
    let hint = Hint(panel.experience_tip(rounded));
    for mut h in &mut holders {
        h.set_if_neq(hint.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::special_ops::tests::{operating, panel};

    #[test]
    fn the_experience_and_its_tooltip_follow_the_sides() {
        let mut app = App::new();
        app.insert_resource(Sim(operating(199.6)))
            .insert_resource(panel())
            .add_systems(Update, refresh);
        let after_glyph = Figure {
            after_glyph: true,
            shown: None,
        };
        let figure = app.world_mut().spawn((Text::default(), after_glyph)).id();
        let alone = (Text::default(), Figure::default());
        let alone = app.world_mut().spawn(alone).id();
        let holder = app.world_mut().spawn((Holder, Hint(String::new()))).id();
        let look = |app: &mut App| {
            app.update();
            let world = app.world();
            let text = world.get::<Text>(figure).unwrap().0.clone();
            let hint = world.get::<Hint>(holder).unwrap().0.clone();
            (text, hint)
        };
        let panel = panel();
        assert_eq!(
            look(&mut app),
            (
                format!(" {}", panel.experience(200)),
                panel.experience_tip(200)
            )
        );
        app.world_mut().resource_mut::<Sim>().0 = operating(450.0);
        assert_eq!(look(&mut app).0, format!(" {}", panel.experience(450)));
        // With no glyph before it, no space either.
        let text = app.world().get::<Text>(alone).unwrap().0.clone();
        assert_eq!(text, panel.experience(450));
    }
}
