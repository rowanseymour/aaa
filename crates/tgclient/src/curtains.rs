//! The letterbox and screen fade the map's triggers draw over the match:
//! over the map and everything on it, under the interface, which the
//! letterbox hides while it's up.

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use tgsim::Slide;

use crate::displays::PanelWindow;
use crate::ui::layer;
use crate::{Loaded, Screen, Sim};

pub(crate) struct CurtainsPlugin;

impl Plugin for CurtainsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Overridden>()
            .add_systems(OnEnter(Screen::Match), spawn)
            .add_systems(First, restore_interface)
            .add_systems(Update, draw.run_if(in_state(Screen::Match)))
            .add_systems(
                PostUpdate,
                hide_interface
                    .run_if(in_state(Screen::Match))
                    .before(VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// A letterbox bar, as tall when fully in as its share of the screen's
/// height.
#[derive(Component)]
struct Bar {
    share: f32,
}

#[derive(Component)]
struct Fade;

/// The interface's own visibility, kept while the letterbox overrides it.
#[derive(Resource, Default)]
struct Overridden(Vec<(Entity, Visibility)>);

/// The game time now, in ticks, between two where the frame falls.
fn now(sim: &Sim, fixed: &Time<Fixed>) -> f32 {
    sim.0.ticks as f32 + fixed.overstep_fraction()
}

/// How far in a letterbox or fade is, none before a trigger switches it.
fn shown(slide: Option<Slide>, ticks: f32) -> f32 {
    slide.map_or(0.0, |s| s.shown(ticks))
}

/// Whether the letterbox hides the interface: from when it's switched on
/// until its bars have gone after it's switched off.
fn letterboxed(slide: Option<Slide>, ticks: f32) -> bool {
    slide.is_some_and(|s| s.on || s.shown(ticks) > 0.0)
}

/// Whether the interface is up for the player to use, with the map's
/// clicks and hotkeys, rather than hidden by the letterbox.
pub(crate) fn interface_up(sim: Option<Res<Sim>>, fixed: Res<Time<Fixed>>) -> bool {
    sim.is_none_or(|sim| !letterboxed(sim.0.triggers.letterbox, now(&sim, &fixed)))
}

fn spawn(mut commands: Commands, loaded: Res<Loaded>) {
    let share = loaded.game.letterbox_height_ratio();
    let bar = |top| {
        (
            Bar { share },
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: if top { px(0) } else { auto() },
                bottom: if top { auto() } else { px(0) },
                height: px(0),
                ..default()
            },
            BackgroundColor(Color::BLACK),
            Pickable::IGNORE,
        )
    };
    let full = Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    };
    commands.spawn((
        full.clone(),
        GlobalZIndex(layer::CURTAINS),
        Pickable::IGNORE,
        DespawnOnExit(Screen::Match),
        children![
            bar(true),
            bar(false),
            // Last, so over the bars.
            (Fade, full, BackgroundColor(Color::NONE), Pickable::IGNORE),
        ],
    ));
}

fn draw(
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    mut bars: Query<(&Bar, &mut Node)>,
    mut fade: Query<&mut BackgroundColor, With<Fade>>,
) {
    let ticks = now(&sim, &fixed);
    let triggers = &sim.0.triggers;
    let bars_in = shown(triggers.letterbox, ticks);
    for (bar, mut node) in &mut bars {
        let height = percent(100.0 * bars_in * bar.share);
        if node.height != height {
            node.height = height;
        }
    }
    let black = Color::BLACK.with_alpha(shown(triggers.screenfade, ticks));
    for mut colour in &mut fade {
        colour.set_if_neq(BackgroundColor(black));
    }
}

/// Hides the interface for the frame while the letterbox is up, keeping
/// what the interface itself has each part show.
fn hide_interface(
    sim: Res<Sim>,
    fixed: Res<Time<Fixed>>,
    mut overridden: ResMut<Overridden>,
    mut nodes: Query<(Entity, &GlobalZIndex, Has<PanelWindow>, &mut Visibility), With<Node>>,
) {
    if !letterboxed(sim.0.triggers.letterbox, now(&sim, &fixed)) {
        return;
    }
    // The in-match windows share the menus' layer, which stays.
    let interface = layer::HUD..layer::DIALOG;
    for (entity, z, window, mut visibility) in &mut nodes {
        if (interface.contains(&z.0) || window) && *visibility != Visibility::Hidden {
            overridden.0.push((entity, *visibility));
            *visibility = Visibility::Hidden;
        }
    }
}

/// Gives the interface back what it had each part show, before anything
/// looks at it in the next frame.
fn restore_interface(mut overridden: ResMut<Overridden>, mut nodes: Query<&mut Visibility>) {
    for (entity, was) in overridden.0.drain(..) {
        if let Ok(mut visibility) = nodes.get_mut(entity) {
            visibility.set_if_neq(was);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_letterbox_hides_the_interface_until_its_bars_have_gone() {
        let slide = |on| Slide {
            on,
            at: 10,
            seconds: 2.0,
        };
        assert!(!letterboxed(None, 0.0));
        assert!(letterboxed(Some(slide(true)), 10.0), "as it starts in");
        assert!(letterboxed(Some(slide(false)), 25.0), "still going out");
        assert!(!letterboxed(Some(slide(false)), 30.0));
        assert_eq!(shown(None, 0.0), 0.0);
    }

    #[test]
    fn the_interface_is_hidden_under_the_letterbox_and_keeps_what_it_shows() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Overridden>()
            .insert_resource(Sim(crate::orders::testing::fresh()))
            .add_systems(First, restore_interface)
            .add_systems(PostUpdate, hide_interface);
        let hud = || (Node::default(), GlobalZIndex(layer::HUD));
        let shown = app.world_mut().spawn((hud(), Visibility::Inherited)).id();
        let unshown = app.world_mut().spawn((hud(), Visibility::Hidden)).id();
        let menu = (
            Node::default(),
            GlobalZIndex(layer::DIALOG),
            Visibility::Inherited,
        );
        let menu = app.world_mut().spawn(menu).id();
        let seen = |app: &App, e| *app.world().get::<Visibility>(e).unwrap();
        let letterbox = |app: &mut App, on| {
            let mut sim = app.world_mut().resource_mut::<Sim>();
            let at = sim.0.ticks;
            sim.0.triggers.letterbox = Some(Slide {
                on,
                at,
                seconds: 0.0,
            });
        };

        letterbox(&mut app, true);
        app.update();
        assert_eq!(seen(&app, shown), Visibility::Hidden);
        assert_eq!(seen(&app, menu), Visibility::Inherited, "the menus stay");
        // What the interface chooses meanwhile is what comes back.
        app.world_mut()
            .entity_mut(unshown)
            .insert(Visibility::Inherited);
        app.update();
        assert_eq!(seen(&app, unshown), Visibility::Hidden);
        letterbox(&mut app, false);
        app.update();
        assert_eq!(seen(&app, shown), Visibility::Inherited);
        assert_eq!(seen(&app, unshown), Visibility::Inherited);
    }
}
