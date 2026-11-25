//! Working the interface without a mouse or keyboard, for the console, so
//! a script can reach every screen.
//!
//! A button goes by its [`ButtonId`]. Clicking one activates it directly,
//! with nothing hovered; right-clicking one has it take a right click
//! where it stands. Either only where a person could: not one under
//! whatever [`Covers`] the screen, nor one that's disabled.

use bevy::camera::NormalizedRenderTarget;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseButtonInput;
use bevy::picking::backend::HitData;
use bevy::picking::events::Pointer;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::{Location, PointerId};
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::PrimaryWindow;

use crate::keys::Chord;
use crate::ui::{ButtonId, Chosen, Covers, Hint};

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
    let cover = topmost_cover(world);
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
    let mut unnamed = 0;
    let mut seen = Vec::new();
    for (entity, node, at, shown, id, chosen) in query.iter(world) {
        let reached = cover.is_none_or(|c| on(world, c, entity));
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
    let (button, _) = pressable(world, id)?;
    world.trigger(Activate { entity: button });
    world.flush();
    Ok(())
}

/// Clicks the button with id `id` with the right mouse button, as a
/// person would: it's pressed and clicked where it stands, with the right
/// button just let go, so it activates as a button does for any click and
/// whatever answers a right click gets one.
pub fn right_click(world: &mut World, id: &str) -> Result<(), String> {
    let (entity, middle) = pressable(world, id)?;
    let mut mouse = (world.get_resource_mut::<ButtonInput<MouseButton>>())
        .ok_or("there's no mouse to right-click with")?;
    mouse.press(MouseButton::Right);
    mouse.release(MouseButton::Right);
    world.entity_mut(entity).insert(bevy::ui::Pressed);
    let target = NormalizedRenderTarget::None {
        width: 0,
        height: 0,
    };
    let location = Location {
        target,
        position: middle,
    };
    world.trigger(PointerClick {
        entity,
        pointer: Pointer::new(PointerId::Mouse, location),
        button: PointerButton::Secondary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: std::time::Duration::ZERO,
        count: 1,
    });
    world.flush();
    if let Ok(mut button) = world.get_entity_mut(entity) {
        button.remove::<bevy::ui::Pressed>();
    }
    Ok(())
}

/// The button on screen with id `id`, and where its middle is.
fn seen(world: &mut World, id: &str) -> Result<(Entity, Vec2), String> {
    let (seen, _) = buttons(world);
    let button = seen.iter().find(|b| b.id.eq_ignore_ascii_case(id));
    let button = button.ok_or_else(|| format!("no button {id:?} to press; ui lists them"))?;
    Ok((button.entity, button.middle))
}

/// [`seen`], if it can be pressed.
fn pressable(world: &mut World, id: &str) -> Result<(Entity, Vec2), String> {
    let (button, middle) = seen(world, id)?;
    if world.get::<bevy::ui::InteractionDisabled>(button).is_some() {
        return Err(format!("button {id:?} is disabled"));
    }
    Ok((button, middle))
}

/// What covers the screen topmost, which only what's on it can be reached
/// past, if anything does.
fn topmost_cover(world: &mut World) -> Option<Entity> {
    let mut covers =
        world.query_filtered::<(Entity, &GlobalZIndex, &InheritedVisibility), With<Covers>>();
    let shown = covers.iter(world).filter(|(.., shown)| shown.get());
    shown.max_by_key(|(_, z, _)| z.0).map(|(e, ..)| e)
}

/// Whether `entity` is on `cover`.
fn on(world: &World, cover: Entity, entity: Entity) -> bool {
    let parent = |e: Entity| world.get::<ChildOf>(e).map(ChildOf::parent);
    std::iter::successors(parent(entity), |&e| parent(e)).any(|e| e == cover)
}

/// The button with id `id`, or else the part of the interface with that
/// id that says something when hovered, such as the object display's
/// economy line.
fn hoverable(world: &mut World, id: &str) -> Result<Entity, String> {
    if let Ok((button, _)) = seen(world, id) {
        return Ok(button);
    }
    // Only where a person could reach, as for a button.
    let cover = topmost_cover(world);
    type Parts<'a> = (
        Entity,
        &'a ButtonId,
        &'a ComputedNode,
        &'a InheritedVisibility,
    );
    let mut hints = world.query_filtered::<Parts, (With<Hint>, Without<Button>)>();
    let world = &*world;
    let mut found = hints.iter(world).filter(|(entity, named, node, shown)| {
        shown.get()
            && node.size.min_element() > 0.0
            && cover.is_none_or(|c| on(world, c, *entity))
            && named.0.eq_ignore_ascii_case(id)
    });
    match found.next() {
        Some((entity, ..)) => Ok(entity),
        None => Err(format!("no button {id:?} to press; ui lists them")),
    }
}

/// The button the pointer is held over, by [`hover`], as if the mouse were
/// on it.
#[derive(Resource, Default)]
pub struct Over(Option<Entity>);

/// Holds the pointer over the button with id `id`, showing what hovering
/// on it would, or with none, takes it off.
pub fn hover(world: &mut World, id: Option<&str>) -> Result<(), String> {
    let button = id.map(|id| hoverable(world, id)).transpose()?;
    world.resource_mut::<Over>().0 = button;
    Ok(())
}

/// Keeps the button [`hover`] chose hovered, after picking has found the
/// mouse over nothing.
pub fn hold_over(mut commands: Commands, over: Res<Over>, hovered: Query<&Hovered>) {
    let Some(button) = over.0 else {
        return;
    };
    if hovered.get(button).is_ok_and(|h| !h.0) {
        commands.entity(button).insert(Hovered(true));
    }
}

/// Presses or lets go of a mouse button, wherever the cursor is.
pub fn mouse(world: &mut World, button: MouseButton, state: ButtonState) {
    let window = primary(world);
    world.write_message(MouseButtonInput {
        button,
        state,
        window,
    });
}

fn primary(world: &mut World) -> Entity {
    let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
    windows.iter(world).next().unwrap_or(Entity::PLACEHOLDER)
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

/// Holds `keys` down, with nothing typed, for [`release`] to let go.
pub fn hold(world: &mut World, keys: &[KeyCode]) {
    for &key in keys {
        let logical = Key::Unidentified(NativeKey::Unidentified);
        send(world, key, logical, ButtonState::Pressed);
    }
}

/// Lets go of what [`press`] pressed.
pub fn release(world: &mut World, keys: &[KeyCode]) {
    for &key in keys.iter().rev() {
        let logical = Key::Unidentified(NativeKey::Unidentified);
        send(world, key, logical, ButtonState::Released);
    }
}

fn send(world: &mut World, key_code: KeyCode, logical_key: Key, state: ButtonState) {
    let window = primary(world);
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
    fn a_right_click_is_the_right_buttons_and_lands_where_its_pressed() {
        let mut world = world();
        world.init_resource::<ButtonInput<MouseButton>>();
        let first = buttons(&mut world).0.remove(0).entity;
        world.entity_mut(first).observe(
            |click: On<PointerClick>,
             mut clicked: ResMut<Clicked>,
             mouse: Res<ButtonInput<MouseButton>>| {
                let right = click.button == PointerButton::Secondary;
                let there = click.pointer.position == Vec2::new(20.0, 10.0);
                if right && there && mouse.just_released(MouseButton::Right) {
                    clicked.0.push("right");
                }
            },
        );
        right_click(&mut world, "first").unwrap();
        assert_eq!(world.resource::<Clicked>().0, ["right"]);
        assert!(
            world.get::<bevy::ui::Pressed>(first).is_none(),
            "let go after"
        );
        assert!(right_click(&mut world, "gone").is_err(), "it's hidden");
    }

    #[test]
    fn a_button_hovered_stays_hovered_until_let_go() {
        let mut world = world();
        world.init_resource::<Over>();
        let mut named = world.query::<(Entity, &ButtonId)>();
        let first = named.iter(&world).find(|(_, id)| id.0 == "first");
        let first = first.map(|(e, _)| e).unwrap();
        world.entity_mut(first).insert(Hovered(false));
        let hovered = |world: &mut World| {
            world.run_system_cached(hold_over).unwrap();
            world.get::<Hovered>(first).unwrap().0
        };
        hover(&mut world, Some("FIRST")).unwrap();
        assert!(hovered(&mut world));
        world.entity_mut(first).insert(Hovered(false));
        assert!(hovered(&mut world), "picking found the mouse elsewhere");
        hover(&mut world, None).unwrap();
        world.entity_mut(first).insert(Hovered(false));
        assert!(!hovered(&mut world));
        assert!(hover(&mut world, Some("gone")).is_err(), "it's hidden");
    }

    #[test]
    fn a_disabled_button_cannot_be_clicked() {
        let mut world = world();
        let launch = ids(&mut world)
            .iter()
            .position(|id| id == "launch")
            .unwrap();
        let entity = buttons(&mut world).0[launch].entity;
        world
            .entity_mut(entity)
            .insert(bevy::ui::InteractionDisabled);
        assert!(click(&mut world, "launch").is_err());
        assert!(world.resource::<Clicked>().0.is_empty());
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
