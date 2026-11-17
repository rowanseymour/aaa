//! What the player asks the interface for, by the names the game's hotkey
//! files give it: a command such as `ActorCommand move`, or a widget
//! pressed, such as `ActionButtonsButton_3`. Keys, widgets and the console
//! all send an [`Intent`], so what it asks for is done the same way
//! whichever sent it, and either interface's widgets can send it.

use bevy::prelude::*;

/// Something asked for, by its name in the hotkey files.
#[derive(Message, Clone, PartialEq, Eq, Debug)]
pub struct Intent(pub String);

impl Intent {
    pub fn is(&self, name: &str) -> bool {
        self.0 == name
    }
}

impl From<&str> for Intent {
    fn from(name: &str) -> Self {
        Intent(name.to_string())
    }
}

/// A button that, pressed, asks for this.
#[derive(Component, Clone, Debug)]
pub struct Asks(pub String);

impl Asks {
    pub fn new(name: impl Into<String>) -> Self {
        Asks(name.into())
    }
}

/// Sends what a pressed button asks for.
pub fn send(
    activate: On<bevy::ui_widgets::Activate>,
    asks: Query<&Asks>,
    mut intents: MessageWriter<Intent>,
) {
    if let Ok(asks) = asks.get(activate.entity) {
        intents.write(Intent(asks.0.clone()));
    }
}

/// What's been asked for since a handler last read its intents.
pub struct Asked(Vec<Intent>);

impl Asked {
    /// Reads them all.
    pub fn read(intents: &mut MessageReader<Intent>) -> Asked {
        Asked(intents.read().cloned().collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `name` is among them.
    pub fn wants(&self, name: &str) -> bool {
        self.0.iter().any(|i| i.is(name))
    }
}

/// Whether any of `intents` asks for `name`, reading them all.
pub fn asked(intents: &mut MessageReader<Intent>, name: &str) -> bool {
    Asked::read(intents).wants(name)
}
