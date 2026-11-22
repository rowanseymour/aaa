//! The elapsed game time and a trigger's countdown, each in its layout's
//! label, saying what [`times`](crate::times) gives while it's on screen.

use bevy::prelude::*;
use bevy::text::Font as Face;
use tgdata::widgets::{Rect, Widget};

use super::lettering::{Letters, PLAIN};
use super::{Drawer, Step, on_screen, reach};
use crate::times::Times;

/// The widgets the times are said in, each with which it says.
pub(super) const WIDGETS: [(&str, Which); 2] =
    [("TimeMessage", Which::Elapsed), ("Counter", Which::Counter)];
/// Their label.
const LABEL: &str = "Label";

/// Which of the [`Times`] a label says.
#[derive(Component, Clone, Copy, PartialEq, Debug)]
pub enum Which {
    Elapsed,
    Counter,
}

/// A label ready to spawn.
pub(super) struct Built {
    at: Rect,
    font: String,
    colour: Color,
    which: Which,
}

/// The label `widget` lays out for `which`.
pub(super) fn build(
    widget: &Widget,
    which: Which,
    drawer: &mut Drawer,
    (letters, faces): (&mut Letters, &mut Assets<Face>),
) -> Option<Built> {
    let mut chain = drawer.own(widget, &[])?;
    let label = (widget.parts.iter()).find(|p| p.name.eq_ignore_ascii_case(LABEL))?;
    chain.push(Step::Into(label.bounds));
    let font = label.font.clone().unwrap_or(PLAIN.into());
    letters.read(drawer.pictures.game, faces, &font);
    let [r, g, b] = label.colour;
    Some(Built {
        at: reach(&chain),
        font,
        colour: Color::srgb(r, g, b),
        which,
    })
}

impl Built {
    pub(super) fn spawn(self, screen: &mut ChildSpawnerCommands, letters: &Letters) {
        screen.spawn((
            on_screen(self.at),
            letters.text(&self.font, "", self.colour),
            self.which,
            Visibility::Hidden,
        ));
    }
}

/// Has each label say its time, shown only while there's one to say.
pub fn show(times: Res<Times>, mut labels: Query<(&Which, &mut Text, &mut Visibility)>) {
    for (which, mut text, mut shown) in &mut labels {
        let said = match which {
            Which::Elapsed => &times.elapsed,
            Which::Counter => &times.counter,
        };
        shown.set_if_neq(crate::ui::visible(said.is_some()));
        if let Some(said) = said
            && text.0 != *said
        {
            text.0.clone_from(said);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_label_says_its_time_only_while_there_is_one() {
        let mut app = App::new();
        app.init_resource::<Times>().add_systems(Update, show);
        let spawn = |app: &mut App, which| {
            let label = (which, Text::default(), Visibility::Hidden);
            app.world_mut().spawn(label).id()
        };
        let elapsed = spawn(&mut app, Which::Elapsed);
        let counter = spawn(&mut app, Which::Counter);
        let look = |app: &App, label| {
            let world = app.world();
            let text = world.get::<Text>(label).unwrap().0.clone();
            (
                text,
                world.get::<Visibility>(label) == Some(&Visibility::Inherited),
            )
        };
        app.update();
        assert!(!look(&app, elapsed).1 && !look(&app, counter).1);
        *app.world_mut().resource_mut::<Times>() = Times {
            elapsed: None,
            counter: Some("0:20:00".into()),
        };
        app.update();
        assert!(!look(&app, elapsed).1);
        assert_eq!(look(&app, counter), ("0:20:00".into(), true));
    }
}
