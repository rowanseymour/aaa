//! Our object display, at the middle of the window's lower edge: what
//! [`Showing`] says, laid out as the game's own but without its chrome. Its
//! title names what's shown and its owner, and a line along the bottom says
//! what it earns and takes to keep, and what it's doing. What the pointer's
//! on says what it is in a box above the display. For the player's own
//! organizations, the formation's icon on the title opens the formations
//! to choose between.

use bevy::ecs::hierarchy::ChildSpawnerCommands;
use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;
use tgdata::Id;
use tgrender::textures;
use tgsim::Target;

use crate::displays::Icon;
use crate::hud::original::Cards;
use crate::object_display::*;
use crate::ui::Blocks;
use crate::ui::{self, Edge, Font, Gauge, Hint, Ink, Shade, Surface, framed, label, meter};
use crate::{Loaded, Local, Sim};

/// Our pixels to one of the game's interface units, in which its layout is
/// given, chosen so the display stands as tall as the action buttons beside it.
const SCALE: f32 = super::actions::SIZE.y / HEIGHT;
/// Narrower than the game's: as wide as the most cards a group shows, with
/// the margin they have on the left, which every other layout fits within.
const WIDTH: f32 = 438.0;

/// The display's size, in logical pixels.
pub const SIZE: Vec2 = Vec2::new(WIDTH * SCALE, HEIGHT * SCALE);
const HEIGHT: f32 = 183.0;

/// A box at `left`, `top`, `width` across and `height` down, in the
/// game's interface units.
fn at(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left * SCALE),
        top: px(top * SCALE),
        width: px(width * SCALE),
        height: px(height * SCALE),
        ..default()
    }
}

/// Where the game's layout puts an HQ's places, and how big the area they
/// share is and each of them.
const SLOTS_AT: Vec2 = Vec2::new(144.0, 44.0);
const SLOTS_AREA: Vec2 = Vec2::new(148.0, 101.0);
const SLOT: Vec2 = Vec2::new(42.0, 50.0);

/// Within a place, where its organization's banner goes and how big it is,
/// and its health bar's box.
const SLOT_BANNER: [f32; 3] = [2.0, 2.0, 38.0];
const SLOT_HEALTH: [f32; 4] = [2.0, 47.0, 38.0, 2.0];

/// The box of the bar a place held for an organization in the queue has
/// instead, of how far along it is.
const SLOT_PROGRESS: [f32; 4] = [2.0, 44.0, 38.0, 5.0];

/// Where the `i`th of an HQ's `count` places goes: across then down,
/// spread evenly over the area they share.
fn slot_at(i: usize, count: usize) -> [f32; 2] {
    (SLOTS_AT + spread(i, count, SLOTS_AREA, SLOT)).to_array()
}

/// The strip across the top: how tall it is, so what's below can start
/// clear of it, and how far its contents are in from its sides.
const HEADER: f32 = 40.0;
const HEADER_IN: f32 = 8.0;

/// The gap between the owner's gem and their name.
const OWNER_GAP: f32 = 6.0;

/// Each property's icon in the strip, and the gap between them.
const PROPERTY: f32 = 24.0;
const PROPERTY_GAP: f32 = 2.0;

/// A formation choice's side, and the margin round its icon and round the
/// choices in their well.
const FORMATION: f32 = 30.0;
const FORMATION_IN: f32 = 3.0;
const FORMATIONS_IN: f32 = 2.0;

/// What holds the properties' icons, so that they change without the
/// rest of the display being laid out again under the pointer.
#[derive(Component)]
pub struct PropertyRow;

/// The button on the title showing the formation chosen, which opens the
/// others to choose from, and its picture.
#[derive(Component)]
pub struct FormationPicker;
#[derive(Component)]
pub struct PickerIcon;

/// The well the formation choices open in, under their picker.
#[derive(Component)]
pub struct FormationChoices;

/// The object display, whose children show what [`Showing`] says.
#[derive(Component)]
pub struct ObjectDisplay;

/// The line saying what an organization or building counts for in its side's
/// economy, with what it says now: see [`economy`](crate::object_display::economy).
#[derive(Component, Default)]
pub struct EconomyLine(Vec<(Id, String)>);

/// The icon our interface draws for the game's at `path`, if there's one
/// to draw, so a missing one leaves no gap.
fn modern_icon(game: &tgdata::GameData, path: &str) -> Option<String> {
    Some(game.modern_icon(path)).filter(|p| game.exists(p))
}

/// The game's layout's place for a lone thing's defence: its icon, with
/// the figure beside it.
const DEFENSE: [f32; 4] = [196.0, 46.0, 16.0, 16.0];

/// The game's layout's place for a lone thing's first weapon: its icon,
/// with the figure beside it, and how far down each next one goes.
const WEAPON: [f32; 4] = [144.0, 46.0, 16.0, 16.0];
const WEAPON_ROW: f32 = 21.0;

/// The box for what a lone thing's template says of it, the game's layout's
/// cut to end with the status line, and how far in its words start.
const DESCRIPTION: [f32; 4] = [244.0, 46.0, 186.0, 104.0];
const DESCRIPTION_IN: f32 = 5.0;

/// How much of the display's height the player's nation's emblem fills,
/// and how faintly it shows.
const EMBLEM_ACROSS: f32 = 0.6;
const EMBLEM_ALPHA: f32 = 0.07;

/// Texels down the image the emblem is drawn in.
const EMBLEM_TEXELS: u32 = 128;

/// Spawns our interface's object display, at the middle of the window's
/// lower edge.
pub fn spawn(mut commands: Commands) {
    // Above the display and as wide, growing upwards as its words wrap.
    let said = commands
        .spawn((
            ui::hint_box(
                Node {
                    width: px(SIZE.x),
                    ..default()
                },
                Val::Percent(100.0),
            ),
            GlobalZIndex(crate::ui::layer::HUD),
        ))
        .id();
    let display = commands
        .spawn((
            ObjectDisplay,
            ui::Docks(said),
            Blocks,
            RelativeCursorPosition::default(),
            ui::shelf(
                Surface::Hud,
                Node {
                    width: px(SIZE.x),
                    height: px(SIZE.y),
                    ..default()
                },
            ),
            Visibility::Hidden,
            GlobalZIndex(crate::ui::layer::HUD),
        ))
        .id();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(super::actions::PAD),
                ..default()
            },
            Pickable::IGNORE,
            DespawnOnExit(super::DRAWN),
        ))
        .add_children(&[said, display]);
}

/// Our icon for organizations in different formations.
const MIXED: &str = "formation_mixed";

/// Whether the formation choices are open under their picker.
#[derive(Resource, Default)]
pub struct ChoosingFormation(bool);

/// Opens or closes the formation choices as their picker is clicked.
fn toggle_formations(_: On<bevy::ui_widgets::Activate>, mut open: ResMut<ChoosingFormation>) {
    open.0 = !open.0;
}

/// Closes the formation choices once one's clicked, even the one already
/// chosen, which changes nothing else.
fn chose_formation(_: On<bevy::ui_widgets::Activate>, mut open: ResMut<ChoosingFormation>) {
    open.0 = false;
}

/// The choices' well, not the picker nor its picture.
type FormationChoicesOnly = (
    With<FormationChoices>,
    Without<FormationPicker>,
    Without<PickerIcon>,
);

type PickerOnly = (With<FormationPicker>, Without<PickerIcon>);

type PickerParts<'a> = (
    &'a Hovered,
    &'a mut Visibility,
    &'a mut Surface,
    &'a mut Hint,
);

/// Shows the formation picker while there are formations to choose
/// between, with the one chosen on it, and the choices under it while
/// open: closed again once one's chosen, by a press anywhere but the
/// picker, or by the selection changing.
#[allow(clippy::too_many_arguments)]
pub fn show_formations(
    formations: Res<crate::formations::Formations>,
    selected: Res<crate::Selected>,
    sim: Res<Sim>,
    mouse: Res<ButtonInput<MouseButton>>,
    loaded: Res<Loaded>,
    looks: Res<Looks>,
    mut open: ResMut<ChoosingFormation>,
    mut cache: ResMut<ui::ImageCache>,
    mut images: ResMut<Assets<Image>>,
    mut pickers: Query<PickerParts, PickerOnly>,
    mut icons: Query<(&mut ImageNode, &mut Visibility), With<PickerIcon>>,
    mut choices: Query<&mut Visibility, FormationChoicesOnly>,
    mut was: bevy::prelude::Local<Option<Id>>,
) {
    let offered = !formations.offered.is_empty();
    let hovered = pickers.iter().any(|(h, ..)| h.get());
    let chosen = *was != formations.lit;
    was.clone_from(&formations.lit);
    let away = mouse.just_pressed(MouseButton::Left) && !hovered;
    if chosen || away || selected.is_changed() || !offered {
        open.0 = false;
    }
    let look = (formations.lit.as_ref()).and_then(|f| looks.formation_looks.get(f));
    let name = |f: &Id| (looks.formation_looks.get(f)).map_or(f.to_string(), |l| l.name.clone());
    // What the lit choice's hint says, or for a mix the formations in it.
    let said = match &formations.lit {
        Some(f) => format!("{}{}", name(f), crate::formations::told(&looks, &sim.0, f)),
        None => (formations.held.iter())
            .map(name)
            .collect::<Vec<_>>()
            .join("\n"),
    };
    for (hovered, mut shown, mut fill, mut hint) in &mut pickers {
        shown.set_if_neq(ui::visible(offered));
        fill.set_if_neq(match (open.0, hovered.get()) {
            (true, _) => Surface::Chosen,
            (false, true) => Surface::Hovered,
            (false, false) => Surface::Button,
        });
        if hint.0 != said {
            hint.0.clone_from(&said);
        }
    }
    let game = &loaded.game;
    // Organizations in different formations have none chosen, so it shows
    // that rather than one they aren't all in.
    let path = match look {
        Some(look) => look.icon.as_ref().map(|path| game.modern_icon(path)),
        None => Some(crate::displays::icon(MIXED)),
    };
    let icon = path.and_then(|path| cache.get(game, &mut images, &path));
    for (mut image, mut shown) in &mut icons {
        shown.set_if_neq(ui::visible(icon.is_some()));
        if let Some(icon) = &icon
            && image.image != *icon
        {
            image.image = icon.clone();
        }
    }
    for mut well in &mut choices {
        well.set_if_neq(ui::visible(open.0));
    }
}

/// Puts the player's nation's emblem behind the display, or takes it away
/// where the nation has none, as the player or the display changes.
#[allow(clippy::too_many_arguments)]
pub fn emblem(
    mut commands: Commands,
    loaded: Res<Loaded>,
    sim: Res<Sim>,
    local: Res<Local>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    display: Single<Entity, With<ObjectDisplay>>,
    mut drawn: bevy::prelude::Local<Option<(Entity, Option<Id>)>>,
) {
    let nation = local.0.and_then(|p| sim.0.sides.player(p)?.nation.clone());
    let now = (*display, nation.map(Id::from));
    if drawn.as_ref() == Some(&now) {
        return;
    }
    let (_, nation) = drawn.insert(now);
    let game = &loaded.game;
    let height = (EMBLEM_TEXELS as f32 * EMBLEM_ACROSS).round() as u32;
    let image = (nation.as_ref())
        .and_then(|n| game.emblems().remove(n))
        .and_then(|path| {
            let Some(bytes) = game.read(&path) else {
                warn!("no {path} in the data");
                return None;
            };
            match path.to_ascii_lowercase().ends_with(".svg") {
                true => textures::svg(&bytes, Some(height), true)
                    .map_err(|e| warn!("{path}: {e}"))
                    .ok(),
                false => textures::decode(&bytes, &path, formats.0, true),
            }
        });
    let Some(image) = image else {
        commands.entity(*display).remove::<ImageNode>();
        return;
    };
    let texels = UVec2::new(
        (EMBLEM_TEXELS as f32 * SIZE.x / SIZE.y).round() as u32,
        EMBLEM_TEXELS,
    );
    let Some(image) = crate::emblem::faint(&image, texels, EMBLEM_ALPHA) else {
        warn!("{nation:?}'s emblem isn't an image drawn a texel at a time");
        commands.entity(*display).remove::<ImageNode>();
        return;
    };
    commands
        .entity(*display)
        .insert(ImageNode::new(images.add(image)));
}

/// Lays the display out afresh when what it should show changes.
pub fn lay_out(
    mut commands: Commands,
    showing: Res<Showing>,
    mut from: ShowsFrom,
    cards: Option<Res<Cards>>,
    formations: Res<crate::formations::Formations>,
    mut display: Single<(Entity, &mut Visibility), With<ObjectDisplay>>,
    row: Query<Entity, With<PropertyRow>>,
) {
    if !showing.is_changed() {
        return;
    }
    // Only the icons, where nothing else has changed.
    let row = row.single().ok().filter(|_| showing.icons_only);
    let (root, visibility) = &mut *display;
    match row {
        Some(row) => drop(commands.entity(row).despawn_related::<Children>()),
        None => {
            commands.entity(*root).despawn_related::<Children>();
            visibility.set_if_neq(crate::ui::visible(showing.now.is_some()));
        }
    }
    let Some((mode, detail)) = &showing.now else {
        return;
    };
    let mut ui = Ui {
        commands: &mut commands,
        root: *root,
        cards: cards.as_deref(),
        choosing: !formations.offered.is_empty(),
        strip: None,
        shows: from.shows(detail),
    };
    match row {
        Some(row) => ui.icons(row),
        None => ui.draw(mode),
    }
}

/// What laying the display out reads, and where it's drawn.
struct Ui<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    /// The display, which what's shown is drawn in.
    root: Entity,
    /// The group display's cards, where the game's layouts have them.
    cards: Option<&'a Cards>,
    /// Whether the formations are there to choose between, which then
    /// stand in for the formation's own icon.
    choosing: bool,
    /// The title's strip, once drawn.
    strip: Option<Entity>,
    shows: Shows<'a>,
}

impl<'a> Parts<'a> for Ui<'a, '_, '_> {
    fn shows(&mut self) -> &mut Shows<'a> {
        &mut self.shows
    }

    /// The relay's stockpile, under its portrait.
    fn stockpile(&mut self, b: usize) {
        let of = Says::Building(b);
        let node = at(PORTRAIT_BOX[0], 176.0, PORTRAIT_BOX[2], 3.0);
        let whole = Node {
            width: percent(100),
            height: percent(100),
            ..default()
        };
        self.commands.entity(self.root).with_children(|display| {
            let seen = WhileSeen(Target::Building(b));
            display
                .spawn((node, seen, Pickable::IGNORE))
                .with_children(|holder| bar(holder, whole, Fill::Stockpile(b), Some(of)));
        });
    }

    /// A block of cards for each kind of slot, as the game's has them.
    fn units(&mut self, r: usize, captain: usize) {
        let (sim, looks) = (self.shows.sim, &*self.shows.looks);
        let groups = card_groups(sim, looks, r, captain);
        let card = Vec2::new(42.0, 47.0);
        let places = card_places(
            &groups,
            card,
            Vec2::splat(2.0),
            5.0,
            Vec2::new(210.0, 110.0),
        );
        let pictures: Vec<_> = places
            .iter()
            .map(|&(u, _)| self.shows.picture(sim.units[u].template.as_str()))
            .collect();
        self.commands.entity(self.root).with_children(|display| {
            display
                .spawn((at(144.0, 44.0, 210.0, 110.0), Pickable::IGNORE))
                .with_children(|cards| {
                    for (n, (&(u, place), picture)) in places.iter().zip(pictures).enumerate() {
                        let card = framed(Node {
                            flex_direction: FlexDirection::Column,
                            ..at(place.x, place.y, card.x, card.y)
                        });
                        let mut card = cards.spawn((card, says(Says::Unit(u))));
                        shows_alone(
                            &mut card,
                            Some(Target::Unit(u)),
                            &format!("object.unit.{}", n + 1),
                        );
                        centres(&mut card, Says::Unit(u));
                        card.with_children(|card| {
                            card.spawn((
                                Node {
                                    width: percent(100),
                                    flex_grow: 1.0,
                                    ..default()
                                },
                                picture_node(picture),
                                UnitPicture(u),
                                Shade::Full,
                            ));
                            bar(
                                card,
                                Node {
                                    width: percent(100),
                                    height: px(4.0 * SCALE),
                                    ..default()
                                },
                                Fill::Unit(u),
                                None,
                            );
                        });
                    }
                });
        });
    }

    /// The original's cards, so without its layouts there are none.
    fn cards(&mut self, rs: &[usize]) {
        let Some(cards) = self.cards else {
            return;
        };
        let mut left = 8.0;
        for r in newest_first(rs) {
            let lone = lone(self.shows.sim, r);
            let (Some(size), Some(top)) = (cards.size(lone), cards.top(lone)) else {
                continue;
            };
            // Under the header, which has the name the card's top is for,
            // and cut off at it so the header keeps the pointer.
            let holder = Node {
                overflow: Overflow::clip(),
                ..at(left, HEADER, size.x, size.y - top)
            };
            left += size.x + 4.0;
            self.commands.entity(self.root).with_children(|display| {
                display
                    .spawn((holder, Pickable::IGNORE))
                    .with_children(|holder| {
                        holder
                            .spawn((at(0.0, -top, size.x, size.y), Pickable::IGNORE))
                            .with_children(|s| cards.draw(s, self.shows.reborrow(), r));
                    });
            });
        }
    }

    /// Ours open from a picker on the title's right.
    fn formation_buttons(&mut self) {
        let Some(strip) = self.strip else {
            return;
        };
        let square = px(PROPERTY * SCALE);
        let side = px(FORMATION * SCALE);
        let picture = || {
            (
                Shade::Full,
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            )
        };
        let picker = (
            FormationPicker,
            Button,
            Hovered::default(),
            Pickable::default(),
            Hint(String::new()),
            ui::called("formation.picker"),
            ui::well(Node {
                width: square,
                height: square,
                padding: UiRect::all(px(FORMATIONS_IN * SCALE)),
                margin: UiRect::left(px(PROPERTY_GAP * SCALE)),
                flex_shrink: 0.0,
                ..default()
            }),
        );
        // Drops down from the picker, centred under it.
        let across = (FORMATION + 2.0 * FORMATIONS_IN) * SCALE + 2.0;
        let choices = (
            FormationChoices,
            Visibility::Hidden,
            GlobalZIndex(ui::layer::HUD_TOP),
            ui::well(Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                top: percent(100),
                right: px((PROPERTY * SCALE - across) / 2.0 - 1.0),
                margin: UiRect::top(px(FORMATIONS_IN * SCALE)),
                row_gap: px(FORMATIONS_IN * SCALE),
                padding: UiRect::all(px(FORMATIONS_IN * SCALE)),
                ..default()
            }),
        );
        self.commands
            .spawn((picker, ChildOf(strip)))
            .observe(toggle_formations)
            .with_children(|picker| {
                picker.spawn((
                    PickerIcon,
                    picture(),
                    ImageNode::default(),
                    Pickable::IGNORE,
                ));
                picker.spawn(choices).with_children(|well| {
                    for n in 0..crate::formations::MOST {
                        well.spawn((
                            crate::formations::button_bundle(n),
                            crate::formations::StandIns,
                            Node {
                                width: side,
                                height: side,
                                border: UiRect::all(px(1)),
                                padding: UiRect::all(px(FORMATION_IN * SCALE)),
                                border_radius: BorderRadius::all(px(ui::WELL_CORNER)),
                                ..default()
                            },
                            Surface::Clear,
                            Edge::Clear,
                            children![(crate::formations::FormationIcon, picture())],
                        ))
                        .observe(chose_formation);
                    }
                });
            });
    }

    /// The strip across the top: the name over the owner, and, where it
    /// has `properties`, the icons for those that show on its right.
    fn title(&mut self, name: String, owner: u32, properties: bool) {
        let owner = self.shows.owner(owner);
        let gem = self.shows.gem;
        let mut row = None;
        let mut drawn = None;
        self.commands.entity(self.root).with_children(|display| {
            let strip = Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: px(0),
                right: px(0),
                height: px(HEADER * SCALE),
                padding: UiRect::horizontal(px(HEADER_IN * SCALE)),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::top(px(ui::CORNER)),
                ..default()
            };
            let mut strip = display.spawn((strip, Surface::Slot, Pickable::IGNORE));
            drawn = Some(strip.id());
            strip.with_children(|strip| {
                // Cut short rather than wrapped, which would outgrow the strip.
                let lines = Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_width: px(0),
                    overflow: Overflow::clip(),
                    ..default()
                };
                let unwrapped = || TextLayout::new(Justify::Left, LineBreak::NoWrap);
                strip
                    .spawn((lines, Pickable::IGNORE))
                    .with_children(|lines| {
                        lines.spawn((label(name, Font::HudTitle, Ink::Body), unwrapped()));
                        let Some(owner) = owner else {
                            return;
                        };
                        let line = Node {
                            column_gap: px(OWNER_GAP * SCALE),
                            ..default()
                        };
                        lines.spawn((line, Pickable::IGNORE)).with_children(|line| {
                            if let Some(rgb) = owner.gem {
                                let size = px(ui::GEM * SCALE);
                                let mark = Node {
                                    width: size,
                                    height: size,
                                    align_self: AlignSelf::Center,
                                    ..default()
                                };
                                line.spawn(ui::side_mark(gem, rgb, mark));
                            }
                            let name = ui::coloured(owner.name, Font::Hud, owner.colour);
                            line.spawn((name, unwrapped()));
                        });
                    });
                if properties {
                    let icons = Node {
                        column_gap: px(PROPERTY_GAP * SCALE),
                        flex_shrink: 0.0,
                        ..default()
                    };
                    row = Some(strip.spawn((PropertyRow, icons, Pickable::IGNORE)).id());
                }
            });
        });
        self.strip = drawn;
        if let Some(row) = row {
            self.icons(row);
        }
    }

    /// The portrait's box, holding `template`'s picture until [`portray`]
    /// has a portrait to show there, over a bar filled as `fill` says, both
    /// giving `of`'s tooltip.
    fn portrait(&mut self, template: &str, fill: Fill, of: Says, alone: Option<Target>) {
        let picture = self.shows.picture(template);
        let [left, top, width, height] = PORTRAIT_BOX;
        let [in_left, in_top, _, in_bottom] = PORTRAIT_IN;
        self.commands.entity(self.root).with_children(|display| {
            display.spawn(framed(at(left, top, width, height)));
            let inside = Node {
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                ..at(left + in_left, top + in_top, PORTRAIT.x, PORTRAIT.y)
            };
            // Black round a portrait that doesn't fill its box.
            let backdrop = BackgroundColor(Color::BLACK);
            let mut portrait = display.spawn((inside, backdrop, says(of)));
            centres(&mut portrait, of);
            shows_alone(&mut portrait, alone, "object.portrait");
            portrait.with_child((
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                picture_node(picture).with_mode(NodeImageMode::Stretch),
                Portrait,
                Pickable::IGNORE,
            ));
            let bar_top = top + height - in_bottom + 1.0;
            bar(
                display,
                at(left + in_left, bar_top, PORTRAIT.x, in_bottom - 3.0),
                fill,
                Some(of),
            );
        });
    }

    /// The line along the bottom saying what it counts for in its side's
    /// economy, and what it's doing.
    fn status(&mut self, status: Status) {
        self.commands.entity(self.root).with_children(|display| {
            display
                .spawn((at(144.0, 158.0, 286.0, 18.0), Surface::Track))
                .with_children(|line| {
                    // Filled in by [`refresh`], as what it counts for changes.
                    line.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(4),
                            top: px(0),
                            bottom: px(0),
                            align_items: AlignItems::Center,
                            column_gap: px(6),
                            ..default()
                        },
                        EconomyLine::default(),
                        status,
                        Pickable::IGNORE,
                    ));
                    line.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            right: px(6),
                            top: px(2),
                            ..default()
                        },
                        label("", Font::Hud, Ink::Body),
                        status,
                    ));
                });
        });
    }

    /// An HQ's places: each with the banner and health of the organization in
    /// it, which a click selects, with the picture and progress of what
    /// `building`'s queue holds it for, which a click calls off, or empty.
    fn places(&mut self, building: Option<usize>) {
        let count = self.shows.detail.slots.len();
        for (i, place) in self.shows.detail.slots.clone().into_iter().enumerate() {
            let [left, top] = slot_at(i, count);
            let node = at(left, top, SLOT.x, SLOT.y);
            let mut card = self.commands.spawn((framed(node), ChildOf(self.root)));
            let [left, top, side] = SLOT_BANNER;
            let inside = at(left, top, side, side);
            let held = self.shows.held(place, building);
            held.hook(&mut card, i);
            match held {
                Holding::Organization(r) => {
                    card.with_children(|card| {
                        banner(&mut self.shows, card, inside, r);
                        let [left, top, width, height] = SLOT_HEALTH;
                        bar(
                            card,
                            at(left, top, width, height),
                            Fill::Organization(r),
                            None,
                        );
                    });
                }
                Holding::Queued {
                    building: b,
                    place: n,
                    picture,
                } => {
                    card.with_children(|card| {
                        card.spawn((inside, picture_node(picture), Pickable::IGNORE));
                        let [left, top, width, height] = SLOT_PROGRESS;
                        let node = at(left, top, width, height);
                        // Only the first is under way.
                        match n {
                            0 => bar(card, node, Fill::Progress(b), None),
                            _ => drop(card.spawn(meter(node, Gauge::Progress))),
                        }
                    });
                }
                Holding::Flown(picture, ..) => {
                    card.with_children(|card| {
                        card.spawn((inside, picture_node(picture), Pickable::IGNORE));
                        let [left, top, width, height] = SLOT_HEALTH;
                        bar(card, at(left, top, width, height), Fill::Flight, None);
                    });
                }
                Holding::Empty(_) => {}
            }
        }
    }

    /// The shield and the defence of `target`, for one with a body, whose
    /// `template` gives the icon.
    fn defense(&mut self, target: Target, template: &Id) {
        let icon =
            (self.shows.defense_icon_path(template)).and_then(|p| modern_icon(self.shows.game, &p));
        let [left, top, width, height] = DEFENSE;
        self.commands.entity(self.root).with_children(|display| {
            let row = Node {
                align_items: AlignItems::Center,
                column_gap: px(2.0 * SCALE),
                ..at(left, top, 40.0, height)
            };
            // Filled in by [`refresh`], as its defence changes.
            let figure = ui::coloured("", Font::Small, Color::WHITE);
            let figure = (figure, Defends(target, None));
            let shield = Node {
                width: px(width * SCALE),
                height: px(height * SCALE),
                ..default()
            };
            display
                .spawn((row, Shield(target), Hint(String::new())))
                .with_children(|row| {
                    if let Some(icon) = icon {
                        row.spawn((shield, Icon::new(icon), Pickable::IGNORE));
                    }
                    row.spawn(figure);
                });
        });
    }

    /// `target`'s weapons, each its icon with its main damage by it, a row
    /// each.
    fn attacks(&mut self, target: Target) {
        let icons = self.shows.weapon_icon_paths(target, MOST_WEAPONS);
        let game = self.shows.game;
        let icons = icons.into_iter().map(|p| modern_icon(game, &p?));
        for (i, icon) in icons.enumerate() {
            let [left, top, width, height] = WEAPON;
            let top = top + WEAPON_ROW * i as f32;
            self.commands.entity(self.root).with_children(|display| {
                let row = Node {
                    align_items: AlignItems::Center,
                    column_gap: px(2.0 * SCALE),
                    ..at(left, top, 50.0, height)
                };
                // Filled in by [`refresh`], as its damage changes.
                let figure = ui::coloured("", Font::Small, Color::WHITE);
                let figure = (figure, Strikes(target, i, None));
                let icon_node = Node {
                    width: px(width * SCALE),
                    height: px(height * SCALE),
                    ..default()
                };
                display
                    .spawn((row, Armed(target, i), Hint(String::new())))
                    .with_children(|row| {
                        if let Some(icon) = icon {
                            row.spawn((icon_node, Icon::new(icon), Pickable::IGNORE));
                        }
                        row.spawn(figure);
                    });
            });
        }
    }

    /// The box of what a lone thing's template says of it.
    fn description(&mut self, says: String) {
        let [left, top, width, height] = DESCRIPTION;
        let node = Node {
            padding: UiRect::all(px(DESCRIPTION_IN * SCALE)),
            ..at(left, top, width, height)
        };
        self.commands.entity(self.root).with_children(|display| {
            display.spawn((framed(node), children![label(says, Font::Hud, Ink::Body)]));
        });
    }

    /// The organization at `r`'s banner, between its efficiency and its
    /// morale.
    fn flag(&mut self, r: usize) {
        self.commands.entity(self.root).with_children(|display| {
            let mut flag = display.spawn((
                framed(at(366.0, 44.0, 63.0, 77.0)),
                says(Says::Organization(r)),
            ));
            centres(&mut flag, Says::Organization(r));
            commands_all(&mut flag, r);
            flag.with_children(|card| {
                efficiency(&mut self.shows, card, at(0.0, 6.0, 63.0, 2.0), r);
                banner(
                    &mut self.shows,
                    card,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(6.0 * SCALE),
                        top: px(12.0 * SCALE),
                        width: px(51.0 * SCALE),
                        height: px(51.0 * SCALE),
                        ..default()
                    },
                    r,
                );
                morale(&mut self.shows, card, at(0.0, 69.0, 63.0, 2.0), r);
            });
        });
    }

    /// What a building is commissioning, the first with its progress.
    fn queue(&mut self, b: usize) {
        let queue = self.shows.sim.buildings[b].queue.iter();
        let queue: Vec<String> = queue.map(|c| c.template().to_string()).collect();
        // Commissions are trucks, which show their building's button.
        let pictures: Vec<_> = queue.iter().map(|t| self.shows.picture(t)).collect();
        self.commands.entity(self.root).with_children(|display| {
            for (i, picture) in pictures.into_iter().enumerate().take(MAX_QUEUED) {
                let node = match i {
                    0 => at(240.0, 44.0, 64.0, 64.0),
                    _ => {
                        let n = (i - 1) as f32;
                        at(
                            313.0 + (n % 3.0) * 34.0,
                            44.0 + (n / 3.0).floor() * 34.0,
                            32.0,
                            32.0,
                        )
                    }
                };
                let of = Says::Queued {
                    building: b,
                    place: i,
                    held: false,
                };
                let card = (framed(node), picture_node(picture), Button, Queued(i));
                let id = ui::called(format!("object.queued.{}", i + 1));
                display.spawn((card, id, says(of))).observe(cancel);
            }
            if !queue.is_empty() {
                let of = Says::Queued {
                    building: b,
                    place: 0,
                    held: false,
                };
                let node = at(240.0, 110.0, 64.0, 4.0);
                bar(display, node, Fill::Progress(b), Some(of));
            }
            // How many more are waiting than there are places for.
            if let Some(more) = queue.len().checked_sub(MAX_QUEUED).filter(|n| *n > 0) {
                let node = Node {
                    justify_content: JustifyContent::End,
                    ..at(313.0, 112.0, 98.0, 14.0)
                };
                let more = label(format!("+{more}"), Font::Small, Ink::Body);
                display.spawn((node, Pickable::IGNORE, children![more]));
            }
        });
    }
}

impl Ui<'_, '_, '_> {
    /// An icon for each property that shows, and a marker listing any
    /// more than there are places for.
    fn icons(&mut self, row: Entity) {
        let formations = &self.shows.looks.formations;
        let choosing = self.choosing;
        let properties: Vec<_> = (self.shows.detail.properties.iter())
            .filter(|(p, _)| !choosing || !formations.values().any(|f| f.contains(p)))
            .cloned()
            .collect();
        let (tips, more) = self.shows.looks.property_tips(&properties);
        let seen = self.shows.detail.seen_as.map(WhileSeen);
        let square = || Node {
            width: px(PROPERTY * SCALE),
            height: px(PROPERTY * SCALE),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        };
        for (icon, tip, tint) in tips {
            let icon = self.shows.image(Some(&self.shows.game.modern_icon(&icon)));
            let picture = picture_node(icon).with_color(tint);
            let mut icon = self
                .commands
                .spawn((square(), picture, Hint(tip), ChildOf(row)));
            if let Some(seen) = seen {
                icon.insert(seen);
            }
        }
        if let Some(tip) = more {
            let mut marker = self.commands.spawn((
                square(),
                Hint(tip),
                ChildOf(row),
                children![(label("+", Font::HudTitle, Ink::Body), Pickable::IGNORE)],
            ));
            if let Some(seen) = seen {
                marker.insert(seen);
            }
        }
    }
}

/// The organization at `r`'s efficiency bar in `node`, on black, with a
/// marker at what it's easing towards.
fn efficiency(shows: &mut Shows, parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
    let (gauge, fill) = (Gauge::Efficiency, Fill::Efficiency(r));
    marked_bar(shows, parent, node, gauge, fill, Marker::Efficiency(r));
}

/// The organization at `r`'s morale bar in `node`, on black, with a
/// marker at its breakpoint.
fn morale(shows: &mut Shows, parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
    let (gauge, fill) = (Gauge::Morale, Fill::Morale(r));
    marked_bar(shows, parent, node, gauge, fill, Marker::Breakpoint(r));
}

/// A bar on black with `marker` on it.
fn marked_bar(
    shows: &mut Shows,
    parent: &mut ChildSpawnerCommands,
    node: Node,
    gauge: Gauge,
    fill: Fill,
    marker: Marker,
) {
    let path = shows.looks.marker.clone();
    let picture = shows.image(path.as_deref());
    let size = (picture.as_ref())
        .and_then(|h| shows.images.get(h))
        .map(Image::size_f32);
    let mut bar = parent.spawn((meter(node, gauge), fill, BackgroundColor(Color::BLACK)));
    if let (Some(picture), Some(size)) = (picture, size) {
        bar.with_child((
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                left: marked(shows.sim, marker),
                width: px(size.x * SCALE),
                height: px(size.y * SCALE),
                margin: UiRect::left(px(-size.x * SCALE)),
                ..default()
            },
            ImageNode::new(picture),
            marker,
            Pickable::IGNORE,
        ));
    }
}

/// An organization's banner, as it flies over the organization, filling
/// `node`.
fn banner(shows: &mut Shows, parent: &mut ChildSpawnerCommands, node: Node, r: usize) {
    let layers = shows.banner(r);
    parent
        .spawn((node, Pickable::IGNORE))
        .with_children(|banner| {
            for (image, colour) in layers {
                let Some(image) = image else {
                    continue;
                };
                banner.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    ImageNode {
                        image,
                        color: colour,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
            }
        });
}

/// A bar in `node`, filled as `fill` says, giving `of`'s tooltip where
/// it has one of its own rather than leaving that to what it's on.
/// The organization at `r`'s morale bar.
fn bar(parent: &mut ChildSpawnerCommands, node: Node, fill: Fill, of: Option<Says>) {
    let gauge = match fill {
        Fill::Progress(_) => Gauge::Progress,
        Fill::Stockpile(_) => Gauge::Stockpile,
        _ => Gauge::Health,
    };
    let mut bar = parent.spawn((meter(node, gauge), fill));
    if let Some(of) = of {
        // A meter takes no pointer of its own, which a tooltip needs.
        bar.insert((says(of), Pickable::default()));
    }
}

/// Keeps the economy line up to date: what the thing it's of counts for
/// in its side's economy, by each resource's glyph.
pub fn economy(
    mut commands: Commands,
    sim: Res<Sim>,
    local: Res<Local>,
    looks: Res<Looks>,
    mut lines: Query<(Entity, &Status, &mut EconomyLine)>,
) {
    for (row, status, mut line) in &mut lines {
        let now = crate::object_display::economy_line(&sim.0, local.0, *status);
        if line.0 == now {
            continue;
        }
        commands.entity(row).despawn_related::<Children>();
        commands.entity(row).with_children(|row| {
            for (kind, figure) in &now {
                let item = Node {
                    align_items: AlignItems::Center,
                    column_gap: px(1),
                    ..default()
                };
                row.spawn((item, Pickable::IGNORE)).with_children(|item| {
                    let node = Node {
                        width: px(14),
                        height: px(14),
                        ..default()
                    };
                    if let Some(path) = looks.icons.get(kind) {
                        item.spawn((Icon::new(path.as_str()), node));
                    } else if let Some(glyph) = looks.glyphs.get(kind) {
                        item.spawn((ImageNode::new(glyph.clone()), node));
                    }
                    item.spawn(label(figure.clone(), Font::Hud, Ink::Body));
                });
            }
        });
        line.0 = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_hqs_places_spread_over_their_area() {
        // Three across, the last flush with the area's right edge.
        assert_eq!(slot_at(0, 6), [144.0, 44.0]);
        assert_eq!(slot_at(1, 6), [197.0, 44.0]);
        assert_eq!(slot_at(2, 6), [250.0, 44.0]);
        assert_eq!(slot_at(2, 6)[0] + SLOT.x, SLOTS_AT.x + SLOTS_AREA.x);
        // Then down, the second row flush with its bottom.
        assert_eq!(slot_at(3, 6), [144.0, 95.0]);
        assert_eq!(slot_at(5, 6)[1] + SLOT.y, SLOTS_AT.y + SLOTS_AREA.y);
        // A single row sits where the first of two would.
        assert_eq!(slot_at(2, 3), slot_at(2, 6));
    }
}
