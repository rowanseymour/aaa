//! Text in the layouts' fonts, and sizes given in the layouts' pixels,
//! kept to the size the layouts' screen is drawn at.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::{Font as Face, FontSource, LineHeight};

use super::SCREEN;
use crate::Screen;

/// How many of the interface's pixels a pixel of the layouts' screen is,
/// as [`fit_view`](super::fit_view) fits that screen to the window, and
/// how many of the window's own pixels, which fonts are sized in.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct LayoutScale(pub f32, pub f32);

impl Default for LayoutScale {
    fn default() -> Self {
        LayoutScale(1.0, 1.0)
    }
}

impl LayoutScale {
    /// The scale for a screen drawn `height` of the interface's pixels
    /// tall, and `pixels` of the window's.
    pub fn of(height: f32, pixels: f32) -> Self {
        LayoutScale(height / SCREEN[1], pixels / SCREEN[1])
    }
}

/// Text in one of the layouts' fonts, its spans with it.
#[derive(Component, Clone)]
pub struct Lettered {
    own: Drawn,
    /// The font's backup, and the size it's drawn in its backup below.
    backup: Option<(Drawn, f32)>,
    /// The font whose lines its lines are as far apart as, where it's not
    /// its own: a label's, for text in another font within it.
    pitch: Option<Lettering>,
}

/// A face, and its size as the data gives it.
#[derive(Clone)]
struct Drawn {
    face: Option<Read>,
    size: f32,
    /// Whether the size is in points, as it is for a face the data holds,
    /// rather than pixels, as for one of the system's.
    points: bool,
    /// The pixels its shadow or outline adds to its lines.
    effects: f32,
}

impl Drawn {
    /// Its em in pixels where a layout pixel is `s` of them, its size
    /// scaled to whole pixels first.
    fn em(&self, s: f32) -> f32 {
        let size = (self.size * s).round();
        if self.points {
            size * 96.0 / 72.0
        } else {
            size
        }
    }

    /// How far apart its lines are, in the same pixels.
    fn line(&self, s: f32) -> f32 {
        let em = self.em(s);
        let spacing = (self.face.as_ref()).map_or(em, |r| (r.spacing(self.points) * em).ceil());
        spacing + self.effects
    }
}

/// A face as read, and how tall its lines are.
#[derive(Clone)]
struct Read {
    face: Handle<Face>,
    /// Its file, for measuring text in it.
    bytes: std::sync::Arc<[u8]>,
    /// Its height above and below the baseline, in ems, as Windows takes
    /// it, and the gap its file asks for between lines.
    ascent: f32,
    descent: f32,
    gap: f32,
}

impl Read {
    /// `bytes` as a face, if they are one.
    fn new(bytes: Vec<u8>, faces: &mut Assets<Face>) -> Option<Read> {
        let (ascent, descent, gap) = {
            let parsed = ttf_parser::Face::parse(&bytes, 0).ok()?;
            let em = f32::from(parsed.units_per_em());
            let os2 = parsed.tables().os2;
            let (up, down) = os2.map_or((parsed.ascender(), parsed.descender()), |t| {
                (t.windows_ascender(), t.windows_descender())
            });
            // Both from the baseline, the descent below it negative.
            let gap = f32::from(parsed.line_gap()) / em;
            (f32::from(up) / em, -f32::from(down) / em, gap)
        };
        Some(Read {
            bytes: bytes.clone().into(),
            face: faces.add(Face::from_bytes(bytes)),
            ascent,
            descent,
            gap,
        })
    }

    /// How wide `text` is set at `px` pixels, by its characters' advances.
    fn width(&self, text: &str, px: f32) -> f32 {
        let Ok(parsed) = ttf_parser::Face::parse(&self.bytes, 0) else {
            return 0.0;
        };
        let em = f32::from(parsed.units_per_em());
        let advance = |c: char| {
            let glyph = parsed.glyph_index(c);
            glyph.and_then(|g| parsed.glyph_hor_advance(g)).unwrap_or(0)
        };
        text.chars().map(|c| f32::from(advance(c))).sum::<f32>() * px / em
    }

    /// How far apart its lines are, in ems: with the file's line gap for a
    /// face the data holds, without for one standing in for the system's.
    fn spacing(&self, own: bool) -> f32 {
        self.ascent + self.descent + if own { self.gap } else { 0.0 }
    }
}

/// A node this size, and with this padding, in the layouts' pixels.
#[derive(Component, Clone, Copy, Default)]
pub struct Measured {
    pub size: Option<Vec2>,
    pub padding: Option<Vec2>,
    /// Padding whose edges differ, left, right, top and bottom, in place of
    /// `padding`.
    pub edges: Option<[f32; 4]>,
    pub min_width: Option<f32>,
    /// The gap between its children across.
    pub gap: Option<f32>,
    /// The gap between its rows of children.
    pub row_gap: Option<f32>,
    /// The space above it.
    pub top_margin: Option<f32>,
}

/// An image sliced into nine, whose corners and edges are in the layouts'
/// pixels.
#[derive(Component)]
pub struct Sliced;

/// `pixels` of the window's in the interface's, `to` of them each, never so
/// much as to be laid out past that many whole pixels again.
fn within(pixels: f32, to: f32) -> f32 {
    let back = 1.0 / to;
    let mut v = pixels * to;
    while v * back > pixels {
        v = f32::from_bits(v.to_bits() - 1);
    }
    v
}

/// The face a font is drawn in at scale `s`: its backup's where its size,
/// scaled to whole pixels, is below the size the backup takes over at.
fn drawn_at<'a>(own: &'a Drawn, backup: Option<&'a (Drawn, f32)>, s: f32) -> &'a Drawn {
    match backup {
        Some((backup, below)) if (own.size * s).round() < *below => backup,
        _ => own,
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
    // A window with no height, as when it's minimised, has nothing to fit.
    if scale.1 <= 0.0 {
        return;
    }
    // Fonts are sized in the window's pixels, then drawn in the interface's.
    let (p, to) = (scale.1, scale.0 / scale.1);
    for mut image in &mut sliced {
        if let NodeImageMode::Sliced(slicer) = &image.image_mode
            && slicer.max_corner_scale != s
        {
            let mut slicer = slicer.clone();
            slicer.max_corner_scale = s;
            image.image_mode = NodeImageMode::Sliced(slicer);
        }
    }
    let sized =
        |(font, line): (&mut Mut<TextFont>, &mut Mut<LineHeight>), drawn: &Drawn, pitch: &Drawn| {
            let size = FontSize::Px(drawn.em(p) * to);
            if font.font_size != size {
                font.font_size = size;
            }
            if let Some(read) = &drawn.face {
                let face = FontSource::Handle(read.face.clone());
                if font.font != face {
                    font.font = face;
                }
            }
            let height = LineHeight::Px(within(pitch.line(p), to));
            if **line != height {
                **line = height;
            }
        };
    for (lettered, mut font, mut line, mut shadow, children) in &mut texts {
        let drawn = drawn_at(&lettered.own, lettered.backup.as_ref(), p);
        let pitch = (lettered.pitch.as_ref()).map_or(drawn, |pitch| {
            drawn_at(&pitch.own, pitch.backup.as_ref(), p)
        });
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
        if let Some(gap) = measured.row_gap
            && node.row_gap != px(gap * s)
        {
            node.row_gap = px(gap * s);
        }
        if let Some(top) = measured.top_margin
            && node.margin.top != px(top * s)
        {
            node.margin.top = px(top * s);
        }
        let padding = match (measured.edges, measured.padding) {
            (Some([l, r, t, b]), _) => {
                Some(UiRect::new(px(l * s), px(r * s), px(t * s), px(b * s)))
            }
            (None, Some(pad)) => Some(UiRect::axes(px(pad.x * s), px(pad.y * s))),
            (None, None) => None,
        };
        if let Some(padding) = padding
            && node.padding != padding
        {
            node.padding = padding;
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
    backup: Option<(Drawn, f32)>,
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
        let mut drawn = |face: Option<&tgdata::Typeface>, size, effects| {
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
                size,
                points: !face.is_some_and(|f| f.system),
                effects,
            }
        };
        let lettering = match game.font(ids) {
            Some(look) => {
                let effects = match (look.outline, look.shadow) {
                    (true, _) => 2.0,
                    (false, Some(_)) => 1.0,
                    (false, None) => 0.0,
                };
                Lettering {
                    own: drawn(look.face.as_ref(), look.size, effects),
                    shadow: look.shadow.unwrap_or(0.0),
                    backup: (look.backup)
                        .map(|b| (drawn(b.face.as_ref(), b.size, effects), b.below)),
                }
            }
            None => Lettering {
                own: drawn(None, 12.0, 0.0),
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
                    size: 12.0,
                    points: true,
                    effects: 0.0,
                },
                shadow: 0.0,
                backup: None,
            });
        let faces =
            std::iter::once(&mut lettering.own).chain(lettering.backup.as_mut().map(|(b, _)| b));
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

    /// How wide `words` are in the font `ids` names, and how far apart its
    /// lines are, in the layouts' pixels.
    pub fn measure(&self, ids: &str, words: &str) -> (f32, f32) {
        let own = self.lettering(ids).own;
        let (em, line) = (own.em(1.0), own.line(1.0));
        (own.face.as_ref()).map_or((0.0, line), |r| (r.width(words, em), line))
    }

    /// The font `ids` names, as read, at its size on the layouts' screen.
    pub fn font(&self, ids: &str) -> TextFont {
        let own = self.lettering(ids).own;
        let font_size = FontSize::Px(own.em(1.0));
        TextFont {
            font: (own.face).map_or_else(FontSource::default, |r| FontSource::Handle(r.face)),
            font_size,
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
        (
            Text::new(words),
            self.lettered(ids, label),
            TextColor(colour),
            Pickable::IGNORE,
        )
    }

    /// What sets text in the font `ids` names, its lines as far apart as
    /// those of the font `label` names.
    pub fn lettered(&self, ids: &str, label: &str) -> (TextFont, LineHeight, TextShadow, Lettered) {
        let lettering = self.lettering(ids);
        let pitch = (!label.eq_ignore_ascii_case(ids)).then(|| self.lettering(label));
        let lined = pitch.as_ref().map_or(&lettering.own, |p| &p.own);
        let line = LineHeight::Px(lined.line(1.0));
        (
            self.font(ids),
            line,
            TextShadow {
                offset: Vec2::ONE,
                color: Color::BLACK.with_alpha(lettering.shadow),
            },
            Lettered {
                own: lettering.own,
                backup: lettering.backup,
                pitch,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(face: Handle<Face>, ascent: f32, descent: f32) -> Read {
        Read {
            face,
            bytes: Default::default(),
            ascent,
            descent,
            gap: 0.0,
        }
    }

    #[test]
    fn a_font_scaled_below_its_threshold_is_drawn_in_its_backup() {
        let own = Handle::<Face>::default();
        let backup = Handle::<Face>::Uuid(bevy::asset::uuid::Uuid::from_u128(1), default());
        let drawn = |face, size, points| Drawn {
            face: Some(read(face, 0.75, 0.25)),
            size,
            points,
            effects: 0.0,
        };
        // `small`: 12 points, Arial 11 below 12.
        let lettered = Lettered {
            own: drawn(own.clone(), 12.0, true),
            backup: Some((drawn(backup.clone(), 11.0, false), 12.0)),
            pitch: None,
        };
        let mut app = App::new();
        app.init_resource::<LayoutScale>()
            .add_systems(Update, scale);
        let text = (app.world_mut())
            .spawn((Text::new("x"), lettered, TextShadow::default()))
            .id();
        let drawn = |app: &mut App, s| {
            *app.world_mut().resource_mut::<LayoutScale>() = LayoutScale(s, s);
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
        // At 600 high it's 9, so Arial 11 scaled to 9.
        assert_eq!(
            drawn(&mut app, 600.0 / 768.0),
            (
                FontSource::Handle(backup),
                FontSize::Px(9.0),
                LineHeight::Px(9.0)
            )
        );
    }

    #[test]
    fn whole_pixels_lay_out_as_no_more_than_that_many() {
        let to = 768.0 / 900.0;
        for pixels in [19.0, 22.0, 26.0, 1.0] {
            let v = within(pixels, to);
            assert!(v / to <= pixels && v * (1.0 / to) <= pixels);
            assert!((v - pixels * to).abs() < 1e-4);
        }
    }

    #[test]
    fn lines_are_a_whole_pixel_sizes_spacing_rounded_up_and_its_shadow() {
        // `small`: 12 points of A&A, which has a shadow.
        let small = Drawn {
            face: Some(read(Handle::default(), 0.846, 0.25)),
            size: 12.0,
            points: true,
            effects: 1.0,
        };
        assert_eq!(small.line(1.0), 19.0);
        assert_eq!(small.line(900.0 / 768.0), 22.0);
        assert_eq!(small.line(1080.0 / 768.0), 26.0);
        assert_eq!(small.em(900.0 / 768.0), 14.0 * 4.0 / 3.0);
    }

    #[test]
    fn the_matchs_shared_text_takes_the_layouts_face() {
        let mut app = App::new();
        let face = Handle::<Face>::default();
        let mut letters = Letters::default();
        let lettering = Lettering {
            own: Drawn {
                face: Some(read(face.clone(), 1.0, 0.0)),
                size: 12.0,
                points: true,
                effects: 0.0,
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
