//! Boxes to type in on the screens around a match: one has the keyboard
//! at a time, taken by pressing it, and Enter in it tells whatever
//! listens.

use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::ui::{Font, card, clickable, padded, text};

pub struct FieldsPlugin;

impl Plugin for FieldsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Focused>()
            .add_observer(focus)
            .add_systems(Update, (type_in, show).chain());
    }
}

/// A box to type in.
#[derive(Component, Debug, Clone, Default)]
pub struct Field {
    pub text: String,
    /// The most characters it takes.
    pub longest: usize,
    /// Whether it takes digits alone.
    pub digits: bool,
    /// Whether what's typed is hidden, as for a password.
    pub masked: bool,
}

/// The field that has the keyboard, if one has.
#[derive(Resource, Default)]
pub struct Focused(pub Option<Entity>);

/// Told of a field as Enter is pressed in it.
#[derive(EntityEvent)]
pub struct Entered {
    pub entity: Entity,
}

/// A field `width` wide holding `text`, taking up to `longest`
/// characters.
pub fn field(text_now: impl Into<String>, longest: usize, width: Val) -> impl Bundle {
    let field = Field {
        text: text_now.into(),
        longest,
        ..default()
    };
    (
        field,
        clickable(card(Node { width, ..padded() })),
        children![text("", Font::Body)],
    )
}

/// Gives the keyboard to the field pressed.
fn focus(pressed: On<Activate>, fields: Query<(), With<Field>>, mut focused: ResMut<Focused>) {
    if fields.contains(pressed.entity) {
        focused.0 = Some(pressed.entity);
    }
}

/// Types what's pressed into the field that has the keyboard.
fn type_in(
    mut commands: Commands,
    mut presses: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focused: ResMut<Focused>,
    mut fields: Query<&mut Field>,
    console: Option<Res<crate::console::Console>>,
) {
    // What's typed in the console is the console's.
    let under = console.is_some_and(|c| !crate::console::closed(c));
    let field = focused.0.filter(|_| !under);
    let Some(mut typed) = field.and_then(|f| fields.get_mut(f).ok()) else {
        // Gone with its screen.
        if focused.0.is_some_and(|f| !fields.contains(f)) {
            focused.0 = None;
        }
        presses.clear();
        return;
    };
    let entity = field.expect("a field has the keyboard");
    if keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]) {
        presses.clear();
        commands.trigger(Entered { entity });
        return;
    }
    let mut now = typed.text.clone();
    crate::chat::type_into(&mut now, typed.longest, &mut presses);
    if typed.digits {
        now.retain(|c| c.is_ascii_digit());
    }
    if now != typed.text {
        typed.text = now;
    }
}

/// Shows what's in each field, with a caret in the one that has the
/// keyboard.
fn show(
    focused: Res<Focused>,
    fields: Query<(Entity, &Field, &Children, Ref<Field>)>,
    mut texts: Query<&mut Text>,
) {
    for (entity, field, children, changed) in &fields {
        if !changed.is_changed() && !focused.is_changed() {
            continue;
        }
        let mut shown = match field.masked {
            true => "*".repeat(field.text.chars().count()),
            false => field.text.clone(),
        };
        if focused.0 == Some(entity) {
            shown.push('_');
        }
        if let Some(Ok(mut text)) = texts.iter_many_mut(children).fetch_next() {
            text.0 = shown;
        }
    }
}
