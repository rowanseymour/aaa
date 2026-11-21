//! What marks the match as paused over the map, in either interface: the
//! layouts' paused label, blinking, as the interface spec has it under
//! "While paused". The darkened light is [`sunlight`](crate::sunlight)'s.

use bevy::prelude::*;
use bevy::text::Font as Face;
use tgdata::widgets::{Rect, SCREEN};

use crate::hud::original::lettering::Letters;
use crate::{Loaded, Orders, Screen};

/// The layout file the label's in, and its name there.
const FILE: &str = "UI/Shared/display_banner.tgi";
const LABEL: &str = "DisplayPause";

/// How long the label is shown, then hidden, in turn, in real time.
const BLINK: f32 = 1.0;

/// The layouts' screen, fitted to the window as the original interface
/// fits it, which the label's placed within.
#[derive(Component)]
struct Stage;

/// The label, and its size in the layouts' pixels.
#[derive(Component)]
pub(crate) struct Banner(f32);

/// When the match was last seen to pause, in real seconds.
#[derive(Resource, Default)]
struct Since(Option<f32>);

pub struct PausedPlugin;

impl Plugin for PausedPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Since>()
            .add_systems(OnEnter(Screen::Match), spawn)
            .add_systems(
                Update,
                (fit, blink.run_if(resource_exists::<Orders>)).run_if(in_state(Screen::Match)),
            );
    }
}

/// Puts the label up, hidden, where the layouts have it.
fn spawn(mut commands: Commands, loaded: Res<Loaded>, mut faces: ResMut<Assets<Face>>) {
    let game = &loaded.game;
    let Some(widget) = game.screen_widget(FILE, LABEL) else {
        return;
    };
    let label = (widget.parts.iter()).find(|p| p.kind.as_deref() == Some("LABEL"));
    let Some(label) = label else {
        return;
    };
    let screen = Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    };
    let at = label.bounds.place(widget.bounds(screen));
    let key = label.text.as_deref().unwrap_or_default();
    let strings = game.strings();
    let words = match key.strip_prefix('#') {
        Some(key) => strings.get(key).cloned().unwrap_or_default(),
        None => key.to_string(),
    };
    let ids = label
        .font
        .as_deref()
        .unwrap_or(crate::hud::original::lettering::PLAIN);
    let mut letters = Letters::default();
    letters.read(game, &mut faces, ids);
    let font = letters.font(ids);
    let FontSize::Px(size) = font.font_size else {
        return;
    };
    let [r, g, b] = label.colour;
    let percent = |v: f32, of: f32| Val::Percent(100.0 * v / of);
    let stage = commands
        .spawn((
            Stage,
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(crate::ui::layer::SELECTING),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ))
        .id();
    // As wide as the label's bounds, its words centred within them, as
    // the layouts justify it.
    let holder = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(at.left, SCREEN[0]),
                top: percent(at.top, SCREEN[1]),
                width: percent(at.width(), SCREEN[0]),
                height: percent(at.height(), SCREEN[1]),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                overflow: Overflow::visible(),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(stage),
        ))
        .id();
    commands.spawn((
        Banner(size),
        Text::new(words),
        font,
        TextColor(Color::srgb(r, g, b)),
        TextLayout::new(Justify::Center, LineBreak::NoWrap),
        Visibility::Hidden,
        Pickable::IGNORE,
        ChildOf(holder),
    ));
}

/// Keeps the layouts' screen fitted to what the map's drawn to, and the
/// label's words to its size.
fn fit(
    cameras: Query<&Camera, With<tgrender::camera::RtsCamera>>,
    ui: Res<UiScale>,
    mut stages: Query<&mut Node, With<Stage>>,
    mut banners: Query<(&Banner, &mut TextFont)>,
) {
    let Some(size) = cameras.iter().find_map(|c| {
        let scale = c.target_scaling_factor()?;
        Some(c.physical_target_size()?.as_vec2() / scale)
    }) else {
        return;
    };
    let scale = crate::hud::original::layout_scale(size);
    let fitted = Vec2::new(SCREEN[0], SCREEN[1]) * scale;
    for mut node in &mut stages {
        let mut placed = node.clone();
        crate::ui_scale::place_on_map(&mut placed, (size - fitted) / 2.0, fitted, &ui);
        node.set_if_neq(placed);
    }
    for (banner, mut font) in &mut banners {
        let px = FontSize::Px(banner.0 * scale / ui.0);
        if font.font_size != px {
            font.font_size = px;
        }
    }
}

/// Shows the label while the match is paused, but not under the game
/// menu, blinking from when the pause began.
fn blink(
    orders: Res<Orders>,
    menu: Res<crate::pause::PauseMenu>,
    time: Res<Time<Real>>,
    mut since: ResMut<Since>,
    mut banners: Query<&mut Visibility, With<Banner>>,
) {
    let now = time.elapsed_secs();
    let paused = orders.paused();
    match (paused, since.0) {
        // So it shows at once as the menu closes over the pause.
        (true, _) if menu.is_open() => since.0 = Some(now),
        (true, None) => since.0 = Some(now),
        (false, Some(_)) => since.0 = None,
        _ => {}
    }
    let on = since.0.is_some_and(|at| shown(now - at)) && !menu.is_open();
    for mut visibility in &mut banners {
        visibility.set_if_neq(crate::ui::visible(on));
    }
}

/// Whether the label's shown `seconds` into a pause.
fn shown(seconds: f32) -> bool {
    ((seconds / BLINK) as u64).is_multiple_of(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_blinks_a_second_on_then_a_second_off() {
        assert!(shown(0.0) && shown(0.99));
        assert!(!shown(1.0) && !shown(1.99));
        assert!(shown(2.0));
    }
}
