//! Where our interface places the on-screen message log.

use bevy::prelude::*;

use super::special_ops::corner;
use crate::messages::LogBox;
use crate::{Leaders, Local};

/// Places the log in the top left corner, clear of the special operations
/// panel.
pub fn place(local: Res<Local>, leaders: Res<Leaders>, mut logs: Query<&mut Node, With<LogBox>>) {
    let corner = corner(&local, &leaders);
    for mut node in &mut logs {
        node.top = px(corner.y);
        node.left = px(corner.x);
    }
}
