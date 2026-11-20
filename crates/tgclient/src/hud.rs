//! The interfaces a match can be played with. What they show, and the
//! [`Intent`](crate::intent::Intent)s they ask for, are shared through the
//! subjects' own modules; each interface only draws them.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Which interface matches are played with, as chosen in the options.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hud {
    /// This project's own, over a view as wide as the window.
    #[default]
    Modern,
    /// The original's, from its layouts, over a view of their 4:3 shape.
    Original,
}

/// The [`Hud`] the match under way is played with: one chosen during it
/// waits for the next.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct InMatch(pub Hud);

/// Has the match about to be played take the [`Hud`] chosen.
pub fn choose(mut commands: Commands, hud: Res<Hud>) {
    commands.insert_resource(InMatch(*hud));
}

/// A run condition for what only `hud` draws.
pub fn playing(hud: Hud) -> impl Fn(Res<InMatch>) -> bool + Clone {
    move |in_match: Res<InMatch>| in_match.0 == hud
}
