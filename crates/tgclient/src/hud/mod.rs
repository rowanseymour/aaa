//! The interfaces a match can be played with. What they show, and the
//! [`Intent`](crate::intent::Intent)s they ask for, are shared through the
//! subjects' own modules; each interface only draws them.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub mod modern;
pub mod original;

/// Which interface matches are played with, as chosen in the options.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Hash, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hud {
    /// This project's own, over a view as wide as the window.
    #[default]
    Modern,
    /// The original's, from its layouts, over a view of their 4:3 shape.
    Original,
}

/// The [`Hud`] chosen, as a state for [`InMatch`] to follow.
#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Default, Debug)]
pub struct Chosen(pub Hud);

/// The [`Hud`] the match under way is played with, which exists only
/// during one and changes with the choice.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct InMatch(pub Hud);

impl ComputedStates for InMatch {
    type SourceStates = (crate::Screen, Chosen);

    fn compute((screen, chosen): (crate::Screen, Chosen)) -> Option<Self> {
        (screen == crate::Screen::Match).then_some(InMatch(chosen.0))
    }
}

/// Both interfaces, each drawing only while it's the one chosen.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((modern::ModernHudPlugin, original::OriginalHudPlugin))
            .configure_sets(
                Update,
                (
                    LayOut
                        .after(crate::object_display::choose)
                        .before(crate::object_display::refresh),
                    Draw.after(crate::clock::disable_buttons)
                        .after(crate::displays::Keys)
                        .after(crate::actions::tell)
                        .after(crate::object_display::portray)
                        .after(crate::resource_bar::show),
                ),
            )
            .add_systems(
                Update,
                (crate::object_display::show_portrait)
                    .in_set(Draw)
                    .run_if(in_state(crate::Screen::Match)),
            );
    }
}

/// Where either interface lays its object display out afresh: once
/// what's shown has been chosen, before what it shows is refreshed.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct LayOut;

/// Where either interface draws what the shared modules have worked out
/// to show this frame.
#[derive(SystemSet, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Draw;

/// Has [`Chosen`] follow the [`Hud`] chosen in the options.
pub fn follow(hud: Res<Hud>, chosen: Res<State<Chosen>>, mut next: ResMut<NextState<Chosen>>) {
    if chosen.0 != *hud {
        next.set(Chosen(*hud));
    }
}

/// Registers [`InMatch`], once the screens are.
pub fn add_states(app: &mut App) {
    app.init_state::<Chosen>()
        .add_computed_state::<InMatch>()
        .add_systems(PreUpdate, follow);
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;
    use crate::Screen;

    fn app(screen: Screen, hud: Hud) -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(screen)
            .insert_resource(hud);
        add_states(&mut app);
        app.update();
        app
    }

    fn in_match(app: &App) -> Option<Hud> {
        let state = app.world().get_resource::<State<InMatch>>();
        state.map(|s| s.get().0)
    }

    #[test]
    fn follows_the_choice_during_a_match() {
        let mut app = app(Screen::Match, Hud::Original);
        assert_eq!(in_match(&app), Some(Hud::Original));
        *app.world_mut().resource_mut::<Hud>() = Hud::Modern;
        app.update();
        assert_eq!(in_match(&app), Some(Hud::Modern));
        *app.world_mut().resource_mut::<Hud>() = Hud::Original;
        app.update();
        assert_eq!(in_match(&app), Some(Hud::Original));
    }

    #[test]
    fn exists_only_during_a_match() {
        let mut app = app(Screen::MainMenu, Hud::Original);
        assert_eq!(in_match(&app), None);
        let next = Screen::Match;
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(next);
        app.update();
        assert_eq!(in_match(&app), Some(Hud::Original));
        let next = Screen::MainMenu;
        app.world_mut()
            .resource_mut::<NextState<Screen>>()
            .set(next);
        app.update();
        assert_eq!(in_match(&app), None);
    }
}
