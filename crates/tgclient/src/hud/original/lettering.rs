//! Text in the layouts' fonts, and sizes given in the layouts' pixels,
//! kept to the size the layouts' screen is drawn at.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::{Font as Face, FontSource};

use super::SCREEN;
use crate::Screen;

/// How many of the interface's pixels a pixel of the layouts' screen is,
/// as [`fit_view`](super::fit_view) fits that screen to the window.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct LayoutScale(pub f32);

impl Default for LayoutScale {
    fn default() -> Self {
        LayoutScale(1.0)
    }
}

impl LayoutScale {
    /// The scale for a screen drawn `height` of the interface's pixels tall.
    pub fn of(height: f32) -> Self {
        LayoutScale(height / SCREEN[1])
    }
}

/// Text in one of the layouts' fonts, this many of their pixels tall,
/// its spans with it.
#[derive(Component, Clone, Copy)]
pub struct Lettered(f32);

/// A node this size, and with this padding, in the layouts' pixels.
#[derive(Component, Clone, Copy, Default)]
pub struct Measured {
    pub size: Option<Vec2>,
    pub padding: Option<Vec2>,
}

/// An image sliced into nine, whose corners and edges are in the layouts'
/// pixels.
#[derive(Component)]
pub struct Sliced;

/// Keeps [`Lettered`] text, [`Measured`] nodes and [`Sliced`] images to the
/// [`LayoutScale`].
pub fn scale(
    scale: Res<LayoutScale>,
    mut texts: Query<(&Lettered, &mut TextFont, &mut TextShadow, Option<&Children>)>,
    mut spans: Query<&mut TextFont, (With<TextSpan>, Without<Lettered>)>,
    mut nodes: Query<(&Measured, &mut Node)>,
    mut sliced: Query<&mut ImageNode, With<Sliced>>,
) {
    let s = scale.0;
    for mut image in &mut sliced {
        if let NodeImageMode::Sliced(slicer) = &image.image_mode
            && slicer.max_corner_scale != s
        {
            let mut slicer = slicer.clone();
            slicer.max_corner_scale = s;
            image.image_mode = NodeImageMode::Sliced(slicer);
        }
    }
    let sized = |font: &mut Mut<TextFont>, size: FontSize| {
        if font.font_size != size {
            font.font_size = size;
        }
    };
    for (lettered, mut font, mut shadow, children) in &mut texts {
        let size = FontSize::Px(lettered.0 * s);
        sized(&mut font, size);
        let offset = Vec2::splat(s);
        if shadow.offset != offset {
            shadow.offset = offset;
        }
        for &child in children.into_iter().flatten() {
            if let Ok(mut font) = spans.get_mut(child) {
                sized(&mut font, size);
            }
        }
    }
    for (measured, mut node) in &mut nodes {
        if let Some(size) = measured.size {
            let (width, height) = (px(size.x * s), px(size.y * s));
            if node.width != width || node.height != height {
                node.width = width;
                node.height = height;
            }
        }
        if let Some(pad) = measured.padding {
            let padding = UiRect::axes(px(pad.x * s), px(pad.y * s));
            if node.padding != padding {
                node.padding = padding;
            }
        }
    }
}

/// Sets the text over the map that both interfaces share in the face of the
/// layouts' [`PLAIN`] font, as the layouts' own text is. Only text that goes
/// with the match: what outlasts it, such as the hint by the pointer, is
/// also the menus' and keeps their face.
pub fn share_face(
    letters: Res<Letters>,
    mut texts: Query<(Entity, &mut TextFont)>,
    parents: Query<&ChildOf>,
    with_match: Query<(), With<DespawnOnExit<Screen>>>,
) {
    let Some(face) = letters.face(PLAIN) else {
        return;
    };
    let shared = FontSource::Handle(crate::ui::HUD);
    for (text, mut font) in &mut texts {
        if (letters.is_changed() || font.is_changed())
            && font.font == shared
            && (with_match.contains(text)
                || parents.iter_ancestors(text).any(|a| with_match.contains(a)))
        {
            font.font = FontSource::Handle(face.clone());
        }
    }
}

/// The layouts' fonts, as a label names them, read as they're asked for.
#[derive(Resource, Default)]
pub struct Letters {
    fonts: HashMap<String, Lettering>,
    faces: HashMap<String, Handle<Face>>,
    /// The first face read, for fonts whose face the data doesn't hold.
    first: Option<Handle<Face>>,
}

/// How text in one of the fonts is drawn.
#[derive(Clone)]
struct Lettering {
    face: Option<Handle<Face>>,
    /// Its size in the layouts' pixels.
    px: f32,
    /// How opaque the shadow behind it is: none where it has none.
    shadow: f32,
}

/// The font a label that names none is in.
pub const PLAIN: &str = "small";

impl Letters {
    /// Reads the font `ids` names, if it hasn't been.
    pub fn read(&mut self, game: &tgdata::GameData, faces: &mut Assets<Face>, ids: &str) {
        let key = ids.to_ascii_lowercase();
        if self.fonts.contains_key(&key) {
            return;
        }
        let look = game.font(ids);
        let face = (look.as_ref().and_then(|l| l.file.clone())).and_then(|file| {
            let known = self.faces.get(&file.to_ascii_lowercase()).cloned();
            known.or_else(|| {
                let face = faces.add(Face::from_bytes(game.read(&file)?));
                self.faces.insert(file.to_ascii_lowercase(), face.clone());
                self.first.get_or_insert_with(|| face.clone());
                Some(face)
            })
        });
        // A Windows size is in points, of 96 dots to the inch.
        let px = look.as_ref().map_or(12.0, |l| l.size) * 96.0 / 72.0;
        let shadow = look.and_then(|l| l.shadow).unwrap_or(0.0);
        self.fonts.insert(key, Lettering { face, px, shadow });
    }

    /// The face of the font `ids` names, as read, or one the data does hold.
    fn face(&self, ids: &str) -> Option<Handle<Face>> {
        let lettering = self.fonts.get(&ids.to_ascii_lowercase());
        (lettering.and_then(|l| l.face.clone())).or_else(|| self.first.clone())
    }

    /// The font `ids` names, as read, at its size on the layouts' screen.
    /// A font whose face the data doesn't hold is drawn in one it does.
    pub fn font(&self, ids: &str) -> TextFont {
        let lettering = self.fonts.get(&ids.to_ascii_lowercase());
        TextFont {
            font: self
                .face(ids)
                .map_or_else(FontSource::default, FontSource::Handle),
            font_size: FontSize::Px(lettering.map_or(16.0, |l| l.px)),
            ..default()
        }
    }

    /// Text saying `words` in the font `ids` names, in `colour`.
    pub fn text(&self, ids: &str, words: &str, colour: Color) -> impl Bundle + use<> {
        let lettering = self.fonts.get(&ids.to_ascii_lowercase());
        let px = lettering.map_or(16.0, |l| l.px);
        let shadow = lettering.map_or(0.0, |l| l.shadow);
        (
            Text::new(words),
            self.font(ids),
            TextColor(colour),
            TextShadow {
                offset: Vec2::ONE,
                color: Color::BLACK.with_alpha(shadow),
            },
            Lettered(px),
            Pickable::IGNORE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matchs_shared_text_takes_the_layouts_face() {
        let mut app = App::new();
        let face = Handle::<Face>::default();
        let mut letters = Letters::default();
        let lettering = Lettering {
            face: Some(face.clone()),
            px: 16.0,
            shadow: 0.0,
        };
        letters.fonts.insert(PLAIN.into(), lettering);
        app.insert_resource(letters).add_systems(Update, share_face);
        let hud = || crate::ui::font(crate::ui::Font::Hud);
        let world = app.world_mut();
        let window = world.spawn(DespawnOnExit(Screen::Match)).id();
        let shared = world.spawn((hud(), ChildOf(window))).id();
        let menu = world
            .spawn((crate::ui::font(crate::ui::Font::Body), ChildOf(window)))
            .id();
        let lasting = world.spawn(hud()).id();
        app.update();
        // Text spawned once the layouts' fonts are read is set too.
        let later = app.world_mut().spawn((hud(), ChildOf(window))).id();
        app.update();
        let font = |e| app.world().get::<TextFont>(e).unwrap().font.clone();
        assert_eq!(font(shared), FontSource::Handle(face.clone()));
        assert_eq!(font(later), FontSource::Handle(face));
        assert_eq!(font(menu), crate::ui::font(crate::ui::Font::Body).font);
        assert_eq!(font(lasting), hud().font);
    }
}
