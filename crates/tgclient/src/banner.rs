//! The banner panel the map's triggers put text up in, which every player
//! sees: shown at once, held for its time, then faded out. A subtitle
//! holds it while it runs, outside the letterbox.

use bevy::prelude::*;
use bevy::text::Font as Face;
use tgdata::widgets::{Rect, SCREEN, ViewElement, Widget};

use crate::hud::original::lettering::Letters;
use crate::paused::{LayoutFont, Stage};
use crate::{Loaded, Local, Orders, Screen, Sim};

/// The layout file the panel's in, and its name and its parts' there.
const FILE: &str = "UI/Shared/display_banner.tgi";
const PANEL: &str = "DisplayBannerPanel";
const BACKDROP: &str = "DisplayBannerBackground";
const LABEL: &str = "DisplayBanner";

pub(crate) struct BannerPlugin;

impl Plugin for BannerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Copied>()
            .add_systems(OnEnter(Screen::Match), spawn)
            .add_systems(
                Update,
                (take, run, draw)
                    .chain()
                    .after(crate::subtitles::Shown)
                    .run_if(in_state(Screen::Match))
                    .run_if(resource_exists::<Banner>),
            );
    }
}

/// What a subtitle has the banner show this frame, which holds it.
#[derive(Resource, Default)]
pub(crate) struct Copied(pub(crate) Option<String>);

/// The banner: how many of the sim's it has looked at, the one up, and
/// how long one takes to fade.
#[derive(Resource)]
struct Banner {
    seen: usize,
    up: Option<Up>,
    fade: f32,
}

/// Banner text up: its words, the seconds it's still held for, none for
/// ever, and how far into its fade it is.
#[derive(Debug, PartialEq)]
struct Up {
    text: String,
    held: Option<f32>,
    faded: f32,
}

impl Up {
    /// Text put up for `seconds`, counted to the thousandth; below 0 for
    /// ever.
    fn new(text: String, seconds: f32) -> Up {
        let held = (seconds >= 0.0).then(|| (seconds * 1000.0).trunc() / 1000.0);
        Up {
            text,
            held,
            faded: 0.0,
        }
    }

    /// Runs it `by` seconds on, saying whether it's still up after a fade
    /// of `fade` seconds.
    fn run(&mut self, by: f32, fade: f32) -> bool {
        let over = match &mut self.held {
            None => return true,
            Some(held) if *held > by => {
                *held -= by;
                return true;
            }
            Some(held) => by - std::mem::replace(held, 0.0),
        };
        self.faded += over;
        self.faded < fade
    }

    /// How opaque it is, after a fade of `fade` seconds.
    fn opacity(&self, fade: f32) -> f32 {
        match fade > 0.0 {
            true => (1.0 - self.faded / fade).clamp(0.0, 1.0),
            false => f32::from(self.faded <= 0.0),
        }
    }
}

/// The panel's backdrop.
#[derive(Component)]
struct Backdrop;

/// The panel's words.
#[derive(Component)]
struct Words([f32; 3]);

/// The panel as the layouts have it: its own widget, and its label's
/// text part.
pub(crate) fn layout(game: &tgdata::GameData) -> Option<(Widget, ViewElement)> {
    let panel = game.screen_widget(FILE, PANEL)?;
    let label = (panel.children.iter()).find(|w| w.name == LABEL)?;
    let text = (label.parts.iter()).find(|p| p.kind.as_deref() == Some("LABEL"))?;
    Some((panel.clone(), text.clone()))
}

/// The layouts' screen, which the panel's placed on.
fn screen() -> Rect {
    Rect {
        left: 0.0,
        top: 0.0,
        right: SCREEN[0],
        bottom: SCREEN[1],
    }
}

/// Where the panel's label sits, on the layouts' screen.
pub(crate) fn label_bounds(panel: &Widget, label: &ViewElement) -> Rect {
    label.bounds.place(panel.bounds(screen()))
}

/// Puts the panel up, hidden: as wide as the layouts have it and as tall
/// as its words.
#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut faces: ResMut<Assets<Face>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<bevy::image::CompressedImageFormatSupport>,
    mut copied: ResMut<Copied>,
) {
    copied.0 = None;
    let game = &loaded.game;
    let Some((panel, label)) = layout(game) else {
        warn!("{FILE}: no {PANEL}");
        return;
    };
    let at = panel.bounds(screen());
    let words_at = label_bounds(&panel, &label);
    let backdrop = (panel.children.iter())
        .find(|w| w.name == BACKDROP)
        .and_then(|w| w.parts.iter().find_map(|p| p.texture.clone()));
    let skin = crate::hud::original::skin(game, &sim, &local);
    let backdrop = backdrop.and_then(|path| {
        let bytes = skin
            .as_ref()
            .and_then(|s| s.read(&path))
            .or_else(|| game.read(&path))?;
        let image = tgrender::textures::decode(&bytes, &path, formats.0, true)?;
        Some(images.add(image))
    });
    let mut letters = Letters::default();
    let font = label.font.clone().unwrap_or_default();
    letters.read(game, &mut faces, &font);
    let text_font = letters.font(&font);
    let FontSize::Px(size) = text_font.font_size else {
        return;
    };
    // Lengths across the layouts' screen, as shares of its width, which
    // padding is measured against too.
    let across = |v: f32| Val::Percent(100.0 * v / SCREEN[0]);
    let down = |v: f32| Val::Percent(100.0 * v / SCREEN[1]);
    let stage = commands
        .spawn((
            Stage,
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            GlobalZIndex(crate::ui::layer::HUD),
            Pickable::IGNORE,
            DespawnOnExit(Screen::Match),
        ))
        .id();
    let mut panel_node = commands.spawn((
        Backdrop,
        Node {
            position_type: PositionType::Absolute,
            left: across(at.left),
            top: down(at.top),
            width: across(at.width()),
            padding: UiRect {
                left: across(words_at.left - at.left),
                right: across(at.right - words_at.right),
                top: across(words_at.top - at.top),
                bottom: across(words_at.top - at.top),
            },
            ..default()
        },
        Visibility::Hidden,
        Pickable::IGNORE,
        ChildOf(stage),
    ));
    if let Some(backdrop) = backdrop {
        panel_node.insert(ImageNode::new(backdrop).with_mode(NodeImageMode::Stretch));
    }
    let holder = panel_node.id();
    commands.spawn((
        Words(label.colour),
        LayoutFont(size),
        Text::default(),
        text_font,
        TextColor(Color::srgb(
            label.colour[0],
            label.colour[1],
            label.colour[2],
        )),
        TextLayout::new(Justify::Left, LineBreak::WordBoundary),
        Node {
            width: Val::Percent(100.0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(holder),
    ));
    commands.insert_resource(Banner {
        seen: sim.0.triggers.banners.len(),
        up: None,
        fade: game.banner_fade_seconds(),
    });
}

/// Puts up the last banner text the triggers have put up since last
/// looked, in place of any; one with no words takes the banner down.
fn take(sim: Res<Sim>, loaded: Res<Loaded>, mut banner: ResMut<Banner>) {
    let banners = &sim.0.triggers.banners;
    let new = banners.get(banner.seen..).unwrap_or_default();
    banner.seen = banners.len();
    let Some(last) = new.last() else {
        return;
    };
    let text = loaded
        .map
        .strings
        .get(&last.text)
        .cloned()
        .unwrap_or_default();
    banner.up = (!text.is_empty()).then(|| Up::new(text, last.seconds));
}

/// Runs the banner up on in real time, standing still while the match is
/// paused; a subtitle holding the banner takes the place of one up.
fn run(
    mut banner: ResMut<Banner>,
    copied: Res<Copied>,
    orders: Option<Res<Orders>>,
    real: Res<Time<Real>>,
) {
    if copied.0.is_some() {
        banner.up = None;
        return;
    }
    if orders.is_some_and(|o| o.paused()) {
        return;
    }
    let fade = banner.fade;
    if let Some(up) = &mut banner.up
        && !up.run(real.delta_secs(), fade)
    {
        banner.up = None;
    }
}

fn draw(
    banner: Res<Banner>,
    copied: Res<Copied>,
    mut panels: Query<(&mut Visibility, Option<&mut ImageNode>), With<Backdrop>>,
    mut words: Query<(&Words, &mut Text, &mut TextColor)>,
) {
    let (says, opacity) = match (&copied.0, &banner.up) {
        (Some(copy), _) => (Some(copy.as_str()), 1.0),
        (None, Some(up)) => (Some(up.text.as_str()), up.opacity(banner.fade)),
        (None, None) => (None, 0.0),
    };
    for (mut visibility, image) in &mut panels {
        visibility.set_if_neq(crate::ui::visible(says.is_some()));
        if let Some(mut image) = image {
            let tint = Color::WHITE.with_alpha(opacity);
            if image.color != tint {
                image.color = tint;
            }
        }
    }
    for (words, mut text, mut colour) in &mut words {
        let says = says.unwrap_or_default();
        if text.0 != says {
            text.0 = says.to_string();
        }
        let [r, g, b] = words.0;
        colour.set_if_neq(TextColor(Color::srgba(r, g, b, opacity)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_text_holds_its_time_then_fades() {
        let mut up = Up::new("Hold".into(), 2.0004);
        assert_eq!(up.held, Some(2.0), "to the thousandth");
        assert!(up.run(1.5, 1.0));
        assert_eq!(up.opacity(1.0), 1.0);
        assert!(up.run(1.0, 1.0));
        assert!((up.opacity(1.0) - 0.5).abs() < 1e-6, "half faded");
        assert!(!up.run(0.5, 1.0), "gone");
    }

    #[test]
    fn banner_text_for_no_time_fades_at_once_and_below_none_never() {
        let mut now = Up::new("Now".into(), 0.0);
        assert!(now.run(0.25, 1.0));
        assert!(now.opacity(1.0) < 1.0);
        let mut ever = Up::new("Ever".into(), -1.0);
        assert!(ever.run(1000.0, 1.0));
        assert_eq!(ever.opacity(1.0), 1.0);
    }
}
