//! Working the interface without a mouse or keyboard, for the console, so
//! a script can reach every screen.
//!
//! A button goes by its [`ButtonId`]. Clicking one activates it directly,
//! with nothing hovered, but only where a person could: not one under
//! whatever [`Covers`] the screen.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::PrimaryWindow;

use crate::keys::Chord;
use crate::ui::{ButtonId, Chosen, Covers};

/// A button on screen that can be pressed.
pub struct Seen {
    entity: Entity,
    pub id: String,
    /// The current choice of its group, or switched on.
    pub chosen: bool,
    middle: Vec2,
    height: f32,
}

/// What's on screen to press: the buttons, in reading order, and how many
/// more have no id to press them by.
pub fn buttons(world: &mut World) -> (Vec<Seen>, usize) {
    // Only what's on the topmost cover, if anything covers the screen.
    let mut covers =
        world.query_filtered::<(Entity, &GlobalZIndex, &InheritedVisibility), With<Covers>>();
    let shown = covers.iter(world).filter(|(.., shown)| shown.get());
    let cover = shown.max_by_key(|(_, z, _)| z.0).map(|(e, ..)| e);
    type Parts<'a> = (
        Entity,
        &'a ComputedNode,
        &'a UiGlobalTransform,
        &'a InheritedVisibility,
        Option<&'a ButtonId>,
        Option<&'a Chosen>,
    );
    let mut query = world.query_filtered::<Parts, With<Button>>();
    let world = &*world;
    let on = |cover: Entity, button: Entity| {
        let parent = |e: Entity| world.get::<ChildOf>(e).map(ChildOf::parent);
        std::iter::successors(parent(button), |&e| parent(e)).any(|e| e == cover)
    };
    let mut unnamed = 0;
    let mut seen = Vec::new();
    for (entity, node, at, shown, id, chosen) in query.iter(world) {
        let reached = cover.is_none_or(|c| on(c, entity));
        if !shown.get() || node.size.min_element() <= 0.0 || !reached {
            continue;
        }
        let Some(id) = id else {
            unnamed += 1;
            continue;
        };
        seen.push(Seen {
            entity,
            id: id.0.clone(),
            chosen: chosen.is_some_and(|c| c.0),
            middle: at.translation,
            height: node.size.y,
        });
    }
    // Down the screen, then along each row: those whose middles are within
    // half the height of the row's first.
    seen.sort_by(|a, b| a.middle.y.total_cmp(&b.middle.y));
    let mut from = 0;
    for at in 1..=seen.len() {
        let first = &seen[from];
        if at == seen.len() || seen[at].middle.y - first.middle.y > first.height / 2.0 {
            seen[from..at].sort_by(|a, b| a.middle.x.total_cmp(&b.middle.x));
            from = at;
        }
    }
    (seen, unnamed)
}

/// Activates the button with this id.
pub fn click(world: &mut World, id: &str) -> Result<(), String> {
    let (seen, _) = buttons(world);
    let Some(button) = seen.iter().find(|b| b.id.eq_ignore_ascii_case(id)) else {
        return Err(format!("no button {id:?} to press; ui lists them"));
    };
    world.trigger(Activate {
        entity: button.entity,
    });
    world.flush();
    Ok(())
}

/// The keys a chord holds down, written as a hotkey file does, such as
/// `ctrl-s`: pressed as if on the keyboard, for [`release`] to let go a
/// frame later, once whatever answers to them has seen them down together.
pub fn press(world: &mut World, chord: &str) -> Result<Vec<KeyCode>, String> {
    let chord = chord.to_lowercase();
    let parsed = Chord::parse(&chord).ok_or_else(|| format!("{chord:?} isn't a key"))?;
    let held = [
        (parsed.ctrl, KeyCode::ControlLeft),
        (parsed.alt, KeyCode::AltLeft),
        (parsed.shift, KeyCode::ShiftLeft),
        (true, parsed.key),
    ];
    let keys: Vec<KeyCode> = (held.iter()).filter(|(on, _)| *on).map(|k| k.1).collect();
    // What it types alone or shifted, for the console's own prompt.
    let typed = chord.rsplit('-').next().filter(|k| k.chars().count() == 1);
    let typed = typed
        .filter(|_| !parsed.ctrl && !parsed.alt)
        .map(|k| match parsed.shift {
            true => k.to_uppercase(),
            false => k.to_string(),
        });
    for &key in &keys {
        let logical = match (key, &typed) {
            (KeyCode::Space, _) => Key::Space,
            (k, Some(c)) if k == parsed.key => Key::Character(c.as_str().into()),
            _ => Key::Unidentified(NativeKey::Unidentified),
        };
        send(world, key, logical, ButtonState::Pressed);
    }
    Ok(keys)
}

/// Lets go of what [`press`] pressed.
pub fn release(world: &mut World, keys: &[KeyCode]) {
    for &key in keys.iter().rev() {
        let logical = Key::Unidentified(NativeKey::Unidentified);
        send(world, key, logical, ButtonState::Released);
    }
}

fn send(world: &mut World, key_code: KeyCode, logical_key: Key, state: ButtonState) {
    let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
    let window = windows.iter(world).next().unwrap_or(Entity::PLACEHOLDER);
    world.write_message(KeyboardInput {
        key_code,
        logical_key,
        state,
        text: None,
        repeat: false,
        window,
    });
}

#[cfg(test)]
mod tests {
    use bevy::ui::ComputedNode;

    use super::*;
    use crate::ui::{called, scrim};

    #[derive(Resource, Default)]
    struct Clicked(Vec<&'static str>);

    fn spawn(world: &mut World, id: &'static str, at: Vec2) -> Entity {
        let node = ComputedNode {
            size: Vec2::splat(10.0),
            ..default()
        };
        let place = UiGlobalTransform::from_translation(at);
        let mut button = world.spawn((Button, node, place, InheritedVisibility::VISIBLE));
        if !id.is_empty() {
            button.insert(called(id));
        }
        button
            .observe(move |_: On<Activate>, mut clicked: ResMut<Clicked>| clicked.0.push(id))
            .id()
    }

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<Clicked>();
        spawn(&mut world, "launch", Vec2::new(5.0, 50.0));
        spawn(&mut world, "second", Vec2::new(60.0, 9.0));
        spawn(&mut world, "first", Vec2::new(20.0, 10.0));
        spawn(&mut world, "", Vec2::new(30.0, 90.0));
        let hidden = spawn(&mut world, "gone", Vec2::ZERO);
        world.entity_mut(hidden).insert(InheritedVisibility::HIDDEN);
        world.flush();
        world
    }

    fn ids(world: &mut World) -> Vec<String> {
        buttons(world).0.into_iter().map(|b| b.id).collect()
    }

    #[test]
    fn buttons_are_listed_in_reading_order() {
        let mut world = world();
        assert_eq!(ids(&mut world), ["first", "second", "launch"]);
        assert_eq!(buttons(&mut world).1, 1, "one has no id");
    }

    #[test]
    fn clicking_activates_the_one_named() {
        let mut world = world();
        click(&mut world, "launch").unwrap();
        click(&mut world, "Second").unwrap();
        assert_eq!(world.resource::<Clicked>().0, ["launch", "second"]);
        assert!(click(&mut world, "gone").is_err(), "it's hidden");
        assert!(click(&mut world, "nothing").is_err());
    }

    #[test]
    fn only_what_is_on_the_top_cover_can_be_pressed() {
        let mut world = world();
        let under = world.spawn((scrim(10), InheritedVisibility::VISIBLE)).id();
        let over = world.spawn((scrim(25), InheritedVisibility::VISIBLE)).id();
        let paused = spawn(&mut world, "pause.quit", Vec2::new(5.0, 5.0));
        let asked = spawn(&mut world, "confirm.ok", Vec2::new(5.0, 5.0));
        world.entity_mut(paused).insert(ChildOf(under));
        world.entity_mut(asked).insert(ChildOf(over));
        assert_eq!(ids(&mut world), ["confirm.ok"]);
        assert!(click(&mut world, "launch").is_err(), "it's covered");
        world.entity_mut(over).despawn();
        assert_eq!(ids(&mut world), ["pause.quit"]);
    }

    #[test]
    fn a_chord_types_only_what_it_would_on_a_keyboard() {
        let typed = |chord: &str| {
            let mut world = World::new();
            world.init_resource::<Messages<KeyboardInput>>();
            press(&mut world, chord).unwrap();
            let mut sent = world.resource_mut::<Messages<KeyboardInput>>();
            let sent = sent.drain();
            let typed = sent.filter_map(|k| match k.logical_key {
                Key::Character(c) => Some(c.to_string()),
                _ => None,
            });
            typed.collect::<Vec<_>>().join("")
        };
        assert_eq!(typed("s"), "s");
        assert_eq!(typed("shift-s"), "S");
        assert_eq!(typed("ctrl-s"), "");
        assert_eq!(typed("f1"), "");
    }
}
