//! Our elapsed game time and countdown, each in a pill beside the resource
//! bar while [`times`](crate::times) has it on screen.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::prelude::*;

use crate::times::Times;
use crate::ui::{Font, Ink, Surface, backed, label};

/// A pill, by whether it's the countdown's rather than the elapsed time's.
#[derive(Component)]
pub struct Pill(bool);

/// The text in a [`Pill`].
#[derive(Component)]
pub struct Said;

/// The narrowest a pill is, which leaves room for the time it says, so the
/// top row is as wide with them as [`TopRow`](crate::ui_scale::TopRow)
/// allows for.
pub const PILL: f32 = 80.0;

/// Spawns the pills in `row`, the countdown first, as it's the larger.
pub fn spawn(row: &mut ChildSpawnerCommands, height: f32) {
    for (counter, font) in [(true, Font::HudTitle), (false, Font::Hud)] {
        row.spawn((
            Pill(counter),
            backed(
                Surface::Hud,
                Node {
                    display: Display::None,
                    height: px(height),
                    min_width: px(PILL),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(8)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
            ),
        ))
        .with_child((Said, label("", font, Ink::Body)));
    }
}

/// Has each pill say its time, shown only while there's one to say.
pub fn show(
    times: Res<Times>,
    mut pills: Query<(&Pill, &Children, &mut Node)>,
    mut texts: Query<&mut Text, With<Said>>,
) {
    for (pill, children, mut node) in &mut pills {
        let said = match pill.0 {
            true => &times.counter,
            false => &times.elapsed,
        };
        if (node.display == Display::None) == said.is_some() {
            crate::ui::show(&mut node, said.is_some());
        }
        let Some(said) = said else {
            continue;
        };
        for &child in children {
            if let Ok(mut text) = texts.get_mut(child)
                && text.0 != *said
            {
                text.0.clone_from(said);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_pill_says_its_time_only_while_there_is_one() {
        let mut app = App::new();
        app.init_resource::<Times>().add_systems(Update, show);
        let spawn = |app: &mut App, counter| {
            let world = app.world_mut();
            let text = world.spawn((Said, Text::default())).id();
            let mut pill = world.spawn((Pill(counter), Node::default()));
            pill.add_child(text);
            (pill.id(), text)
        };
        let (elapsed, elapsed_text) = spawn(&mut app, false);
        let (counter, _) = spawn(&mut app, true);
        *app.world_mut().resource_mut::<Times>() = Times {
            elapsed: Some("0:01:00".into()),
            counter: None,
        };
        app.update();
        let world = app.world();
        let display = |e| world.get::<Node>(e).unwrap().display;
        assert_eq!(display(elapsed), Display::Flex);
        assert_eq!(display(counter), Display::None);
        assert_eq!(world.get::<Text>(elapsed_text).unwrap().0, "0:01:00");
    }
}
