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
