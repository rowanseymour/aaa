//! Text in the layouts' fonts, and sizes given in the layouts' pixels,
//! kept to the size the layouts' screen is drawn at.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::{Font as Face, FontSource, LineHeight};

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

/// Text in one of the layouts' fonts, its spans with it.
#[derive(Component, Clone)]
pub struct Lettered {
    own: Drawn,
    /// The font's backup, and the font's size and the size it's drawn in
    /// its backup below, as the data gives them.
    backup: Option<(Drawn, f32, f32)>,
    /// The font whose lines its lines are as far apart as, where it's not
    /// its own: a label's, for text in another font within it.
    pitch: Option<Lettering>,
}

/// A face, and its size: in the layouts' pixels for a font's own, in
/// the interface's for a backup.
#[derive(Clone)]
struct Drawn {
    face: Option<Read>,
    px: f32,
}

/// A face as read, and how tall its lines are.
#[derive(Clone)]
struct Read {
    face: Handle<Face>,
    /// Its height above and below the baseline, in ems, as Windows takes it.
    ascent: f32,
    descent: f32,
}

impl Read {
    /// `bytes` as a face, if they are one.
    fn new(bytes: Vec<u8>, faces: &mut Assets<Face>) -> Option<Read> {
        let (ascent, descent) = {
            let parsed = ttf_parser::Face::parse(&bytes, 0).ok()?;
            let em = f32::from(parsed.units_per_em());
            let os2 = parsed.tables().os2;
            let (up, down) = os2.map_or((parsed.ascender(), parsed.descender()), |t| {
                (t.windows_ascender(), t.windows_descender())
            });
            // Both from the baseline, the descent below it negative.
            (f32::from(up) / em, -f32::from(down) / em)
        };
        Some(Read {
            face: faces.add(Face::from_bytes(bytes)),
            ascent,
            descent,
        })
    }

    /// How far apart its lines are at `px` pixels, each part rounded up to
    /// whole pixels as Windows does.
    fn line(&self, px: f32) -> f32 {
        (self.ascent * px).ceil() + (self.descent * px).ceil()
    }
}

/// A node this size, and with this padding, in the layouts' pixels.
#[derive(Component, Clone, Copy, Default)]
pub struct Measured {
    pub size: Option<Vec2>,
    pub padding: Option<Vec2>,
    pub min_width: Option<f32>,
    /// The gap between its children across.
    pub gap: Option<f32>,
}

/// An image sliced into nine, whose corners and edges are in the layouts'
/// pixels.
#[derive(Component)]
pub struct Sliced;

/// The face a font is drawn in at scale `s`, and its size in the
/// interface's pixels: its backup's where it's scaled below the size the
/// backup takes over at. The size compared is the one the font's scaled
/// to; the backup's is the screen's own, as it's there to keep small text
/// legible.
fn drawn_at<'a>(own: &'a Drawn, backup: Option<&'a (Drawn, f32, f32)>, s: f32) -> (&'a Drawn, f32) {
    match backup {
        Some((backup, size, below)) if size * s < *below => (backup, backup.px),
        _ => (own, own.px * s),
    }
}

/// A span's font and line height, kept to its text's.
type Spanned = (&'static mut TextFont, &'static mut LineHeight);

/// Keeps [`Lettered`] text, [`Measured`] nodes and [`Sliced`] images to the
/// [`LayoutScale`].
pub fn scale(
    scale: Res<LayoutScale>,
    mut texts: Query<(
        &Lettered,
        &mut TextFont,
        &mut LineHeight,
        &mut TextShadow,
        Option<&Children>,
    )>,
    mut spans: Query<Spanned, (With<TextSpan>, Without<Lettered>)>,
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
    let sized = |(font, line): (&mut Mut<TextFont>, &mut Mut<LineHeight>),
                 (drawn, px): (&Drawn, f32),
                 (pitch, at): (&Drawn, f32)| {
        let size = FontSize::Px(px);
        if font.font_size != size {
            font.font_size = size;
        }
        if let Some(read) = &drawn.face {
            let face = FontSource::Handle(read.face.clone());
            if font.font != face {
                font.font = face;
            }
        }
        if let Some(read) = &pitch.face {
            let height = LineHeight::Px(read.line(at));
            if **line != height {
                **line = height;
            }
        }
    };
    for (lettered, mut font, mut line, mut shadow, children) in &mut texts {
        let drawn = drawn_at(&lettered.own, lettered.backup.as_ref(), s);
        let pitch =
            (lettered.pitch.as_ref()).map_or(drawn, |p| drawn_at(&p.own, p.backup.as_ref(), s));
        sized((&mut font, &mut line), drawn, pitch);
        let offset = Vec2::splat(s);
        if shadow.offset != offset {
            shadow.offset = offset;
        }
        for &child in children.into_iter().flatten() {
            if let Ok((mut font, mut line)) = spans.get_mut(child) {
                sized((&mut font, &mut line), drawn, pitch);
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
        if let Some(width) = measured.min_width
            && node.min_width != px(width * s)
        {
            node.min_width = px(width * s);
        }
        if let Some(gap) = measured.gap
            && node.column_gap != px(gap * s)
        {
            node.column_gap = px(gap * s);
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
    faces: HashMap<String, Read>,
    /// The first face the data holds read, for fonts with no face, or
    /// failing that the first face read at all.
    first: Option<Read>,
    any: Option<Read>,
}

/// How text in one of the fonts is drawn.
#[derive(Clone)]
struct Lettering {
    own: Drawn,
    /// How opaque the shadow behind it is: none where it has none.
    shadow: f32,
    /// As [`Lettered`]'s.
    backup: Option<(Drawn, f32, f32)>,
}

/// The font a label that names none is in.
pub const PLAIN: &str = "small";

/// How many pixels tall a font of `size` in `face` is. The game sizes a
/// face its data holds in points of a 96 dots-per-inch screen, and one of
/// the system's in pixels, as text in each measures on its screen.
fn pixels(face: Option<&tgdata::Typeface>, size: f32) -> f32 {
    if face.is_some_and(|f| f.system) {
        size
    } else {
        size * 96.0 / 72.0
    }
}

impl Letters {
    /// Reads the font `ids` names, if it hasn't been.
    pub fn read(&mut self, game: &tgdata::GameData, faces: &mut Assets<Face>, ids: &str) {
        let key = ids.to_ascii_lowercase();
        if self.fonts.contains_key(&key) {
            return;
        }
        let mut drawn = |face: Option<&tgdata::Typeface>, size| {
            let read = face.and_then(|face| {
                let path = face.file.as_ref()?;
                let file = path.to_ascii_lowercase();
                let known = self.faces.get(&file).cloned();
                let read = known.or_else(|| {
                    let read = Read::new(game.read(path)?, faces)?;
                    self.faces.insert(file, read.clone());
                    Some(read)
                })?;
                if !face.system {
                    self.first.get_or_insert_with(|| read.clone());
                }
                self.any.get_or_insert_with(|| read.clone());
                Some(read)
            });
            Drawn {
                face: read,
                px: pixels(face, size),
            }
        };
        let lettering = match game.font(ids) {
            Some(look) => Lettering {
                own: drawn(look.face.as_ref(), look.size),
                shadow: look.shadow.unwrap_or(0.0),
                backup: (look.backup).map(|b| (drawn(b.face.as_ref(), b.size), look.size, b.below)),
            },
            None => Lettering {
                own: drawn(None, 12.0),
                shadow: 0.0,
                backup: None,
            },
        };
        self.fonts.insert(key, lettering);
    }

    /// How the font `ids` names is drawn, as read, a face the data holds
    /// standing in for one it has none for.
    fn lettering(&self, ids: &str) -> Lettering {
        let mut lettering =
            (self.fonts.get(&ids.to_ascii_lowercase()).cloned()).unwrap_or(Lettering {
                own: Drawn {
                    face: None,
                    px: 16.0,
                },
                shadow: 0.0,
                backup: None,
            });
        let faces =
            std::iter::once(&mut lettering.own).chain(lettering.backup.as_mut().map(|(b, _, _)| b));
        for drawn in faces {
            if drawn.face.is_none() {
                drawn.face = self.first.clone().or_else(|| self.any.clone());
            }
        }
        lettering
    }

    /// The face of the font `ids` names, as read, or one the data does hold.
    fn face(&self, ids: &str) -> Option<Handle<Face>> {
        self.lettering(ids).own.face.map(|r| r.face)
    }

    /// The font `ids` names, as read, at its size on the layouts' screen.
    pub fn font(&self, ids: &str) -> TextFont {
        let own = self.lettering(ids).own;
        TextFont {
            font: (own.face).map_or_else(FontSource::default, |r| FontSource::Handle(r.face)),
            font_size: FontSize::Px(own.px),
            ..default()
        }
    }

    /// Text saying `words` in the font `ids` names, in `colour`.
    pub fn text(&self, ids: &str, words: &str, colour: Color) -> impl Bundle + use<> {
        self.text_within(ids, ids, words, colour)
    }

    /// As [`text`](Self::text), its lines as far apart as those of the
    /// font `label` names, the font of the label it's within.
    pub fn text_within(
        &self,
        ids: &str,
        label: &str,
        words: &str,
        colour: Color,
    ) -> impl Bundle + use<> {
        let lettering = self.lettering(ids);
        let pitch = (!label.eq_ignore_ascii_case(ids)).then(|| self.lettering(label));
        let lined = pitch.as_ref().map_or(&lettering.own, |p| &p.own);
        let line = (lined.face.as_ref())
            .map_or_else(LineHeight::default, |r| LineHeight::Px(r.line(lined.px)));
        (
            Text::new(words),
            self.font(ids),
            line,
            TextColor(colour),
            TextShadow {
                offset: Vec2::ONE,
                color: Color::BLACK.with_alpha(lettering.shadow),
            },
            Lettered {
                own: lettering.own,
                backup: lettering.backup,
                pitch,
            },
            Pickable::IGNORE,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_font_scaled_below_its_threshold_is_drawn_in_its_backup() {
        let read = |face| Read {
            face,
            ascent: 0.75,
            descent: 0.25,
        };
        let own = Handle::<Face>::default();
        let backup = Handle::<Face>::Uuid(bevy::asset::uuid::Uuid::from_u128(1), default());
        let lettered = Lettered {
            own: Drawn {
                face: Some(read(own.clone())),
                px: 16.0,
            },
            backup: Some((
                Drawn {
                    face: Some(read(backup.clone())),
                    px: 11.0,
                },
                12.0,
                12.0,
            )),
            pitch: None,
        };
        let mut app = App::new();
        app.init_resource::<LayoutScale>()
            .add_systems(Update, scale);
        let text = (app.world_mut())
            .spawn((Text::new("x"), lettered, TextShadow::default()))
            .id();
        let drawn = |app: &mut App, s| {
            app.world_mut().resource_mut::<LayoutScale>().0 = s;
            app.update();
            let font = app.world().get::<TextFont>(text).unwrap();
            let line = *app.world().get::<LineHeight>(text).unwrap();
            (font.font.clone(), font.font_size, line)
        };
        assert_eq!(
            drawn(&mut app, 1.0),
            (
                FontSource::Handle(own),
                FontSize::Px(16.0),
                LineHeight::Px(16.0)
            )
        );
        assert_eq!(
            drawn(&mut app, 0.75),
            (
                FontSource::Handle(backup),
                FontSize::Px(11.0),
                LineHeight::Px(12.0)
            )
        );
    }

    #[test]
    fn lines_are_as_far_apart_as_a_faces_rounded_height() {
        let read = Read {
            face: Handle::default(),
            ascent: 0.846,
            descent: 0.25,
        };
        // 13.5 and 4 pixels, rounded up apart.
        assert_eq!(read.line(16.0), 18.0);
        assert_eq!(read.line(12.0), 14.0);
    }

    #[test]
    fn the_matchs_shared_text_takes_the_layouts_face() {
        let mut app = App::new();
        let face = Handle::<Face>::default();
        let mut letters = Letters::default();
        let lettering = Lettering {
            own: Drawn {
                face: Some(Read {
                    face: face.clone(),
                    ascent: 1.0,
                    descent: 0.0,
                }),
                px: 16.0,
            },
            shadow: 0.0,
            backup: None,
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
