//! The special operations panel in the top left corner: the player's
//! side's experience, and a button for each of its general's special
//! operations, greyed out while it can't be used. Pressing one, or its key,
//! arms the cursor with it, and the next left click in the world gives it.

use std::collections::HashMap;

use aaa_data::{Decal, SpecialOperation};
use aaa_render::decals::DecalCache;
use aaa_render::terrain::world;
use aaa_render::{MapAssets, textures};
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use bevy::ecs::system::SystemParam;
use bevy::picking::hover::Hovered;
use bevy::ui_widgets::{Activate, Button};

use crate::actions::{self, BUTTON, Blocks, GAP, GLYPH_DIR, PAD, Page, fill};
use crate::keys::Bindings;
use crate::messages::Failed;
use crate::orders::Orders;
use crate::ui::{
    self, Click, Edge, Font, Gauge, Hint, Ink, Marked, Meter, Shade, Surface, label, meter, panel,
    tooltip,
};
use crate::{Leaders, Loaded, Local, Screen, Sim};

/// An icon's size within its button, which is the action palette's, and
/// the bar under it.
const ICON: Vec2 = Vec2::new(46.0, 40.0);
const BAR: f32 = 3.0;
const ROW: f32 = 22.0;
/// The tray of buttons, padded as the action palette is.
const TRAY: f32 = BUTTON + 2.0 * PAD;
const MEDAL: f32 = 16.0;
const MEDAL_GLYPH: &str = "FactionExperience";

/// How far right of the left edge, and down from the top, the panel
/// leaves clear for what shares the corner with it.
const CLEAR: Vec2 = Vec2::new(
    crate::MARGIN + TRAY + 2.0 * GAP,
    crate::MARGIN + ROW + 2.0 * GAP,
);

/// Where what shares the top left corner starts: clear of the panel,
/// which only a side with a general has.
pub fn corner(local: &Local, leaders: &Leaders) -> Vec2 {
    let led = local.0.is_some_and(|p| leaders.0.contains_key(&p));
    match led {
        true => CLEAR,
        false => Vec2::splat(crate::MARGIN),
    }
}

/// The key action that uses the operation in the `n`th button.
fn action(n: usize) -> String {
    format!("UseFactionAbility {n}")
}

pub struct SpecialOpsPlugin;

impl Plugin for SpecialOpsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Match), spawn.after(crate::keys::load))
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.remove_resource::<Panel>();
            })
            .init_resource::<Armed>()
            .add_systems(OnExit(Screen::Match), |mut armed: ResMut<Armed>| {
                armed.0 = None
            })
            .add_systems(
                Update,
                (refresh, tips, mark)
                    .run_if(in_state(Screen::Match))
                    .run_if(resource_exists::<Panel>),
            );
    }
}

/// The special operation the cursor's armed with, by its place in the
/// general's list: the next left click in the world gives it.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Armed(pub Option<usize>);

pub fn armed(armed: Res<Armed>) -> bool {
    armed.0.is_some()
}

/// What the panel shows of the player's side, which has a general.
#[derive(Resource)]
pub(crate) struct Panel {
    player: u32,
    general: String,
    max: Option<f32>,
    words: Words,
    /// What the experience was last shown as, to redo only on a change.
    shown: Option<i64>,
}

/// The tooltips' and messages' words from the game's strings.
pub(crate) struct Words {
    title: String,
    general: String,
    experience: String,
    help: String,
    /// "Special Operation %s has not yet recharged".
    unready: String,
    /// "Insufficient experience to use Special Operation %s".
    short: String,
    /// "Recharging (%d:%.2d left)".
    recharging: String,
}

#[derive(Component)]
struct ExperienceText;

/// The experience's box, whose tooltip tells it too.
#[derive(Component)]
struct ExperienceBox;

/// An operation's button.
#[derive(Component)]
pub(crate) struct OperationButton {
    /// Its operation's place in the general's list.
    index: usize,
    name: String,
    /// It's used as soon as it's pressed, with no aiming.
    on_self: bool,
    /// Its tooltip, beside the tray while the pointer's on the button, and
    /// the tooltip's line saying how long it has left to recharge.
    tip: Entity,
    recharge_line: Entity,
    /// What's laid on the ground at the cursor while it's armed, sized to
    /// the area it would land on.
    marker: Option<Decal>,
}

#[derive(Component)]
struct OperationIcon;

/// The bar under an operation's icon, showing how far it's recharged.
#[derive(Component)]
struct RechargeBar;

fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    bindings: Res<Bindings>,
) {
    let Some(player) = local.0 else {
        return;
    };
    let Some(general) = leaders.0.get(&player) else {
        return;
    };
    let game = &loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let mut cache: HashMap<String, Option<Handle<Image>>> = HashMap::new();
    let mut image = |game: &aaa_data::GameData, path: &str| {
        (cache.entry(path.to_string()))
            .or_insert_with(|| {
                let decoded = textures::decode(&game.read(path)?, path, formats.0, true)?;
                Some(images.add(decoded))
            })
            .clone()
    };
    let medal = image(game, &format!("{GLYPH_DIR}/{MEDAL_GLYPH}.png"));
    let operations = game.special_operations();
    let vfx = game.vfx();
    let words = Words {
        title: said("game_interface_FactionExperience_ToolTip", "Experience"),
        general: said("ui_tooltip_item_general", "General"),
        experience: said("ui_tooltip_item_experience", "Experience"),
        help: said(
            "game_interface_FactionExperience_ToolTip_2",
            "Spend experience to activate Special Operations.",
        ),
        unready: said(
            "ui_error_faction_ability_recharge_time|s",
            "Special Operation %s has not yet recharged",
        ),
        short: said(
            "ui_error_faction_ability_cost|s",
            "Insufficient experience to use Special Operation %s",
        ),
        recharging: said(
            "ui_floater_ability_recharge_time|dd",
            "Recharging (%d:%.2d left)",
        ),
    };
    let cost = said("ui_tooltip_item_title_cost", "Cost");
    let area = said("ui_tooltip_item_title_area", "Area");
    let buttons: Vec<_> = (general.operations.iter().enumerate())
        .filter_map(|(n, o)| {
            let op = operations.get(&o.id)?;
            let icon = (op.icon.as_deref()).and_then(|path| image(game, path));
            let name = bindings.marked(&action(n), &op.name);
            let figures = figures(op, o.cost, &cost, &area);
            let marker = op.marker.as_ref().and_then(|m| {
                let decal = vfx.get(&m.vfx)?.decal.clone()?;
                Some(Decal {
                    size: m.size,
                    ..decal
                })
            });
            Some((n, icon, name, op.clone(), figures, marker))
        })
        .collect();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(crate::MARGIN),
                left: px(crate::MARGIN),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Start,
                row_gap: px(GAP),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(2),
            DespawnOnExit(Screen::Match),
        ))
        .with_children(|corner| {
            corner
                .spawn((
                    ExperienceBox,
                    Blocks,
                    Marked::plain(words.title.clone()),
                    Hint(String::new()),
                    panel(
                        Surface::Hud,
                        Node {
                            height: px(ROW),
                            padding: UiRect::horizontal(px(GAP)),
                            column_gap: px(GAP),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ),
                    Edge::Border,
                ))
                .with_children(|row| {
                    if let Some(medal) = medal {
                        row.spawn((
                            ImageNode::new(medal),
                            Node {
                                width: px(MEDAL),
                                height: px(MEDAL),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    row.spawn((ExperienceText, label("", Font::Hud, Ink::Body)));
                });
            if buttons.is_empty() {
                return;
            }
            corner
                .spawn((
                    Blocks,
                    panel(
                        Surface::Hud,
                        Node {
                            width: px(TRAY),
                            padding: UiRect::all(px(PAD)),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(GAP),
                            ..default()
                        },
                    ),
                ))
                .with_children(|tray| {
                    for (index, icon, name, op, figures, marker) in buttons {
                        let (tip, recharge_line) = pane(tray, &name, &op, &figures);
                        tray.spawn((
                            OperationButton {
                                index,
                                name: op.name.clone(),
                                on_self: op.on_self,
                                tip,
                                recharge_line,
                                marker,
                            },
                            Button,
                            ui::called(format!("special_op.{}", index + 1)),
                            Click::COMMAND,
                            Hovered::default(),
                            Pickable::default(),
                            panel(
                                Surface::Slot,
                                Node {
                                    width: px(BUTTON),
                                    height: px(BUTTON),
                                    border: UiRect::all(px(2)),
                                    flex_direction: FlexDirection::Column,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::SpaceEvenly,
                                    ..default()
                                },
                            ),
                            Edge::Border,
                        ))
                        .with_children(|button| {
                            let size = Node {
                                width: px(ICON.x),
                                height: px(ICON.y),
                                ..default()
                            };
                            match icon {
                                Some(icon) => {
                                    button.spawn((
                                        OperationIcon,
                                        ImageNode::new(icon),
                                        Shade::Dimmed,
                                        size,
                                        Pickable::IGNORE,
                                    ));
                                }
                                None => {
                                    button.spawn((size, Pickable::IGNORE));
                                }
                            }
                            let bar = Node {
                                width: px(ICON.x),
                                height: px(BAR),
                                ..default()
                            };
                            button.spawn((meter(bar, Gauge::Recharge), RechargeBar));
                        })
                        .observe(press);
                    }
                });
        });
    commands.insert_resource(Panel {
        player,
        general: general.name.clone(),
        max: general.experience_max,
        words,
        shown: None,
    });
}

/// The Cost and Area rows of an operation's tooltip, where they apply.
fn figures(
    op: &SpecialOperation,
    cost: f32,
    cost_word: &str,
    area_word: &str,
) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if cost > 0.0 {
        rows.push((cost_word.to_string(), cost.round().to_string()));
    }
    if let Some(area) = &op.area {
        rows.push((area_word.to_string(), area.clone()));
    }
    rows
}

/// Spawns an operation's tooltip, hidden until its button is hovered: its
/// name, what its effects do, its Cost and Area, how long it has left to
/// recharge while it is, then its description. It sits a little way right
/// of the tray, level with its top, as the action palette's sits above the
/// palette. Gives the tooltip and its recharge line.
fn pane(
    tray: &mut ChildSpawnerCommands,
    name: &Marked,
    op: &SpecialOperation,
    figures: &[(String, String)],
) -> (Entity, Entity) {
    let mut line = Entity::PLACEHOLDER;
    let node = Node {
        display: Display::None,
        position_type: PositionType::Absolute,
        left: percent(100),
        top: px(0),
        margin: UiRect::left(px(PAD)),
        width: px(actions::SIZE.x),
        ..default()
    };
    let tip = tray
        .spawn(tooltip(node))
        .with_children(|tip| {
            ui::marked(tip, name, Font::HudTitle, Ink::Body);
            if !op.effects.is_empty() {
                tip.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    ..default()
                })
                .with_children(|lines| {
                    for line in &op.effects {
                        let indent = if line.indented { 2.0 * PAD } else { 0.0 };
                        lines.spawn((
                            label(line.text.clone(), Font::Small, Ink::Body),
                            Node {
                                margin: UiRect::left(px(indent)),
                                ..default()
                            },
                        ));
                    }
                });
            }
            if !figures.is_empty() {
                tip.spawn(Node {
                    display: Display::Grid,
                    grid_template_columns: vec![GridTrack::auto(), GridTrack::flex(1.0)],
                    column_gap: px(PAD * 2.0),
                    row_gap: px(2),
                    ..default()
                })
                .with_children(|table| {
                    for (word, value) in figures {
                        table.spawn(label(word.clone(), Font::Small, Ink::Help));
                        table.spawn(label(value.clone(), Font::Small, Ink::Body));
                    }
                });
            }
            line = (tip.spawn((
                label("", Font::Small, Ink::Body),
                Node {
                    display: Display::None,
                    ..default()
                },
            )))
            .id();
            if let Some(description) = &op.description {
                tip.spawn(label(description.clone(), Font::Small, Ink::Help));
            }
        })
        .id();
    (tip, line)
}

/// Shows the tooltip of the operation under the pointer, and only that.
fn tips(buttons: Query<(&OperationButton, &Hovered)>, mut nodes: Query<&mut Node>) {
    for (button, hovered) in &buttons {
        let display = match hovered.get() {
            true => Display::Flex,
            false => Display::None,
        };
        if let Ok(mut node) = nodes.get_mut(button.tip)
            && node.display != display
        {
            node.display = display;
        }
    }
}

/// "Recharging (4:05 left)", the seconds `left` rounded up.
fn recharging(text: &str, left: f32) -> String {
    let left = left.ceil() as u32;
    let (minutes, seconds) = (left / 60, left % 60);
    (text.replacen("%d", &minutes.to_string(), 1)).replacen("%.2d", &format!("{seconds:02}"), 1)
}

/// The order to use the `i`th operation, where it's aimed being no part of
/// whether it can be used.
fn probe(i: usize) -> aaa_sim::Command {
    aaa_sim::Command::Operate {
        operation: i,
        at: [0.0; 2],
        on: None,
    }
}

/// What pressing an operation needs: the side, the sim, the cursor and
/// somewhere to say why one's refused.
#[derive(SystemParam)]
pub(crate) struct User<'w> {
    panel: Option<Res<'w, Panel>>,
    sim: Res<'w, Sim>,
    orders: Option<ResMut<'w, Orders>>,
    armed: ResMut<'w, Armed>,
    page: ResMut<'w, Page>,
    failed: MessageWriter<'w, Failed>,
}

impl User<'_> {
    /// Arms the cursor with `button`'s operation, or uses one that needs no
    /// aiming at once, unless it can't be used now.
    fn press(&mut self, button: &OperationButton) {
        let Some(panel) = &self.panel else {
            return;
        };
        let player = panel.player;
        match self.sim.0.check(player, &probe(button.index)) {
            Ok(()) if button.on_self => {
                let given = (self.orders.as_mut())
                    .map(|o| o.give(&self.sim.0, player, probe(button.index)));
                if let Some(Err(e)) = given {
                    self.refuse(&button.name, e);
                }
            }
            Ok(()) => {
                self.armed.0 = Some(button.index);
                // Not aiming an order too.
                self.page.set_if_neq(Page::Orders);
            }
            Err(e) => self.refuse(&button.name, e),
        }
    }

    /// Tells the player why the operation `name` can't be used.
    fn refuse(&mut self, name: &str, refused: aaa_sim::Refused) {
        let Some(panel) = &self.panel else {
            return;
        };
        let said = match refused {
            aaa_sim::Refused::Recharging => &panel.words.unready,
            aaa_sim::Refused::Unaffordable => &panel.words.short,
            e => {
                debug!("can't use {name}: {e:?}");
                return;
            }
        };
        let text = fill(said, &[name]);
        self.failed.write(Failed { text, about: None });
    }
}

/// An operation's button clicked.
fn press(activate: On<Activate>, buttons: Query<&OperationButton>, mut user: User) {
    if let Ok(button) = buttons.get(activate.entity) {
        user.press(button);
    }
}

/// Presses the operation a key just pressed is bound to.
pub fn hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    buttons: Query<&OperationButton>,
    mut user: User,
) {
    if keys.get_just_pressed().next().is_none() {
        return;
    }
    let pressed = buttons
        .iter()
        .find(|b| bindings.pressed(&keys, &action(b.index)));
    if let Some(button) = pressed {
        user.press(button);
    }
}

/// While the cursor's armed, the next left click in the world gives the
/// operation: on what's under the cursor where the operation works on it,
/// else on the ground for one aimed at a place. Either way the click
/// disarms it, as do a right click and aiming an order (Escape is the pause
/// menu's to handle).
#[allow(clippy::too_many_arguments)]
pub fn give(
    buttons: Res<ButtonInput<MouseButton>>,
    pointer: crate::Pointer,
    loaded: Res<Loaded>,
    focus: Res<crate::Focus>,
    named: Query<&OperationButton>,
    mut user: User,
) {
    let Some(i) = user.armed.0 else {
        return;
    };
    let aiming = !matches!(*user.page, Page::Orders | Page::Menu(_));
    if buttons.just_pressed(MouseButton::Right) || aiming {
        user.armed.0 = None;
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(at) = pointer.at(&loaded.map.ground) else {
        return;
    };
    user.armed.0 = None;
    let Some(player) = user.panel.as_ref().map(|p| p.player) else {
        return;
    };
    let Some(command) = clicked(&user.sim.0, player, i, at, *focus) else {
        return;
    };
    let given = (user.orders.as_mut()).map(|o| o.give(&user.sim.0, player, command));
    // What can be used may have changed since it was armed.
    if let Some(Err(e)) = given {
        let name = named.iter().find(|b| b.index == i).map(|b| b.name.clone());
        user.refuse(&name.unwrap_or_default(), e);
    }
}

/// The order a click `at` gives with `player`'s `i`th operation armed, with
/// `focus` under the cursor, if it gives one. Only a click on the map
/// itself is on what's under the cursor.
fn clicked(
    sim: &aaa_sim::World,
    player: u32,
    i: usize,
    at: crate::Pointing,
    focus: crate::Focus,
) -> Option<aaa_sim::Command> {
    let on = match (at, focus) {
        (crate::Pointing::Ground(_), crate::Focus::Regiment(r)) => {
            Some(aaa_sim::Piece::Regiment(r))
        }
        (crate::Pointing::Ground(_), crate::Focus::Building(b)) => {
            Some(aaa_sim::Piece::Building(b))
        }
        _ => None,
    };
    let to = sim.operation_aim(player, i, on, at.point())?;
    Some(aaa_sim::Command::Operate {
        operation: i,
        at: to,
        on,
    })
}

/// The marker laid at the cursor while an operation is armed.
#[derive(Component)]
struct CursorMarker;

/// What [`mark`] laid last: for which operation and where, and the
/// pictures it's read.
#[derive(Default)]
struct Marking {
    at: Option<(usize, [f32; 2])>,
    cache: DecalCache,
}

/// Which operation's marker goes where: the armed one's, under the cursor,
/// when it has a marker and the cursor's over the map itself.
fn marked(
    armed: Option<usize>,
    buttons: &[&OperationButton],
    at: Option<crate::Pointing>,
) -> Option<(usize, [f32; 2])> {
    let i = armed?;
    buttons.iter().find(|b| b.index == i)?.marker.as_ref()?;
    match at? {
        crate::Pointing::Ground(p) => Some((i, p)),
        crate::Pointing::Minimap(_) => None,
    }
}

/// Lays the armed operation's marker on the ground under the cursor, while
/// the cursor's over the map itself, its top toward the top of the map
/// whatever's under it.
#[allow(clippy::too_many_arguments)]
fn mark(
    mut commands: Commands,
    armed: Res<Armed>,
    buttons: Query<&OperationButton>,
    pointer: crate::Pointer,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
    mut marking: bevy::prelude::Local<Marking>,
    laid: Query<Entity, With<CursorMarker>>,
) {
    let ground = &loaded.map.ground;
    let listed: Vec<&OperationButton> = buttons.iter().collect();
    let at = marked(armed.0, &listed, pointer.at(ground));
    // What was laid may have gone with the last match.
    if at == marking.at && at.is_some() == !laid.is_empty() {
        return;
    }
    marking.at = at;
    let Some((i, [x, y])) = at else {
        for e in &laid {
            commands.entity(e).despawn();
        }
        return;
    };
    let Some(marker) = listed
        .iter()
        .find(|b| b.index == i)
        .and_then(|b| b.marker.as_ref())
    else {
        return;
    };
    let placed = Transform::from_translation(world(x, y, ground.height_at([x, y])))
        .looking_to(world(0.0, 1.0, 0.0), Vec3::Y);
    let bundle = assets.decal(
        &loaded.game,
        &mut marking.cache,
        ground,
        marker,
        &placed,
        1.0,
    );
    let Some(bundle) = bundle else {
        return;
    };
    // The one marker moves, rather than one being made for each place.
    match laid.iter().next() {
        Some(e) => {
            commands.entity(e).insert(bundle);
        }
        None => {
            commands.spawn((bundle, CursorMarker, DespawnOnExit(Screen::Match)));
        }
    }
}

/// "474/5000": the experience rounded, and the ceiling when there is one.
fn experience_text(experience: i64, max: Option<f32>) -> String {
    match max {
        Some(max) => format!("{experience}/{}", max.round()),
        None => experience.to_string(),
    }
}

/// Keeps the experience and its tooltip up to date, greys out the
/// operations the side can't use now, and shows how far each has
/// recharged.
#[allow(clippy::too_many_arguments)]
fn refresh(
    mut panel: ResMut<Panel>,
    sim: Res<Sim>,
    mut text: Query<&mut Text, With<ExperienceText>>,
    mut boxes: Query<&mut Hint, With<ExperienceBox>>,
    buttons: Query<(&OperationButton, &Children)>,
    mut icons: Query<&mut Shade, With<OperationIcon>>,
    mut bars: Query<&mut Meter, With<RechargeBar>>,
    mut lines: Query<(&mut Text, &mut Node), Without<ExperienceText>>,
) {
    let sim = &sim.0;
    let experience = sim.experience(panel.player).unwrap_or(0.0);
    for (button, children) in &buttons {
        let shade = match sim.check(panel.player, &probe(button.index)) {
            Ok(()) => Shade::Full,
            Err(_) => Shade::Dimmed,
        };
        let op = sim.special_op(panel.player, button.index);
        let charged = op.map_or(1.0, |o| o.charged());
        for &child in children {
            if let Ok(mut s) = icons.get_mut(child) {
                s.set_if_neq(shade);
            }
            if let Ok(mut bar) = bars.get_mut(child)
                && bar.share != charged
            {
                bar.share = charged;
            }
        }
        if let Ok((mut t, mut node)) = lines.get_mut(button.recharge_line) {
            let left = op.map_or(0.0, |o| o.left);
            let display = match left > 0.0 {
                true => Display::Flex,
                false => Display::None,
            };
            if node.display != display {
                node.display = display;
            }
            if left > 0.0 {
                let said = recharging(&panel.words.recharging, left);
                if t.0 != said {
                    t.0 = said;
                }
            }
        }
    }
    let rounded = experience.round() as i64;
    if panel.shown == Some(rounded) {
        return;
    }
    panel.shown = Some(rounded);
    for mut t in &mut text {
        t.0 = experience_text(rounded, panel.max);
    }
    let w = &panel.words;
    let mut said = Vec::new();
    if !panel.general.trim().is_empty() {
        said.push(format!("{}  {}", w.general, panel.general));
    }
    said.push(format!("{}  {rounded}", w.experience));
    let hint = format!("{}\n\n{}", said.join("\n"), w.help);
    for mut h in &mut boxes {
        h.set_if_neq(Hint(hint.clone()));
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;
    use aaa_data::Id;

    fn words() -> Words {
        Words {
            title: "Experience".into(),
            general: "General".into(),
            experience: "Experience".into(),
            help: String::new(),
            unready: "Special Operation %s has not yet recharged".into(),
            short: "Insufficient experience to use Special Operation %s".into(),
            recharging: "Recharging (%d:%.2d left)".into(),
        }
    }

    fn button(index: usize, tip: Entity, recharge_line: Entity) -> OperationButton {
        OperationButton {
            index,
            name: "Resupply".into(),
            on_self: false,
            tip,
            recharge_line,
            marker: None,
        }
    }

    /// A side of `experience` whose one operation, a resupply, costs 200
    /// and recharges in 10 seconds.
    fn operating(experience: f32) -> aaa_sim::World {
        let pathing = aaa_map::Pathing::uniform(8, aaa_map::Domain::Land);
        let resupply = aaa_map::Operation::Enchant {
            targets: Default::default(),
            area: 15.0,
            max_targets: 4,
            property: Id::from("property_resupply"),
            seconds: 30.0,
        };
        let mut rules = aaa_sim::Rules::default();
        rules.operations.insert(Id::from("resupply"), resupply);
        let mut world = aaa_sim::World::from_regiments_by(Default::default(), pathing, rules, &[]);
        let op = aaa_sim::SpecialOp::new(Id::from("resupply"), 200.0, 10.0);
        world.lead(1, experience, Some(5000.0), vec![op]);
        world
    }

    fn panel() -> Panel {
        Panel {
            player: 1,
            general: "Marshal Zhukov".into(),
            max: Some(5000.0),
            words: words(),
            shown: None,
        }
    }

    #[test]
    fn buttons_light_once_experience_reaches_their_cost_however_it_shows() {
        let mut app = App::new();
        app.insert_resource(Sim(operating(199.5)))
            .add_systems(Update, refresh);
        app.insert_resource(panel());
        let icon = app.world_mut().spawn((OperationIcon, Shade::Dimmed)).id();
        let bar = app
            .world_mut()
            .spawn((meter(Node::default(), Gauge::Recharge), RechargeBar))
            .id();
        let line = app
            .world_mut()
            .spawn((Text::default(), Node::default()))
            .id();
        app.world_mut()
            .spawn(button(0, Entity::PLACEHOLDER, line))
            .add_children(&[icon, bar]);
        app.update();
        // Shown as 200, but short of it.
        assert_eq!(app.world().get::<Shade>(icon), Some(&Shade::Dimmed));
        assert_eq!(app.world().get::<Meter>(bar).map(|m| m.share), Some(1.0));
        let display = |app: &App| app.world().get::<Node>(line).map(|n| n.display);
        assert_eq!(display(&app), Some(Display::None));
        app.world_mut().resource_mut::<Sim>().0 = operating(450.0);
        app.update();
        assert_eq!(app.world().get::<Shade>(icon), Some(&Shade::Full));
        // Used, it greys out and its bar empties, its tooltip saying how
        // long it has left.
        let used = aaa_sim::Command::Operate {
            operation: 0,
            at: [4.0, 4.0],
            on: None,
        };
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .apply(1, used)
            .unwrap();
        app.update();
        assert_eq!(app.world().get::<Shade>(icon), Some(&Shade::Dimmed));
        assert_eq!(app.world().get::<Meter>(bar).map(|m| m.share), Some(0.0));
        assert_eq!(display(&app), Some(Display::Flex));
        let said = app.world().get::<Text>(line).map(|t| t.0.clone());
        assert_eq!(said.as_deref(), Some("Recharging (0:10 left)"));
    }

    #[test]
    fn a_click_gives_the_operation_on_the_ground_but_not_on_what_it_cant_work_on() {
        let pathing = aaa_map::Pathing::uniform(8, aaa_map::Domain::Land);
        let mut rules = aaa_sim::Rules::default();
        let resupply = aaa_map::Operation::Enchant {
            targets: aaa_map::Targets {
                required: vec![Id::from("organization")],
                excluded: Vec::new(),
            },
            area: 15.0,
            max_targets: 4,
            property: Id::from("property_resupply"),
            seconds: 30.0,
        };
        rules.operations.insert(Id::from("resupply"), resupply);
        let mut sim = aaa_sim::World::from_regiments_by(Default::default(), pathing, rules, &[]);
        let op = aaa_sim::SpecialOp::new(Id::from("resupply"), 0.0, 0.0);
        sim.lead(1, 0.0, None, vec![op]);
        let ground = crate::Pointing::Ground([3.0, 4.0]);
        let operate = |at| {
            Some(aaa_sim::Command::Operate {
                operation: 0,
                at,
                on: None,
            })
        };
        assert_eq!(
            clicked(&sim, 1, 0, ground, crate::Focus::Nothing),
            operate([3.0, 4.0])
        );
        // A building isn't a regiment, so the click gives nothing.
        assert_eq!(clicked(&sim, 1, 0, ground, crate::Focus::Building(0)), None);
        // On the minimap, what's under the cursor on the map plays no part.
        let minimap = crate::Pointing::Minimap([5.0, 6.0]);
        assert_eq!(
            clicked(&sim, 1, 0, minimap, crate::Focus::Building(0)),
            operate([5.0, 6.0])
        );
        // Nor does a side with no such operation give one.
        assert_eq!(clicked(&sim, 1, 1, ground, crate::Focus::Nothing), None);
    }

    #[test]
    fn the_armed_operations_marker_goes_under_the_cursor_on_the_map_only() {
        let mut with = button(0, Entity::PLACEHOLDER, Entity::PLACEHOLDER);
        with.marker = Some(Decal {
            texture: "crosshair.dds".into(),
            size: 30.0,
            colour: [1.0; 3],
            lit: false,
            additive: false,
            fade: [0.0; 2],
            layer: 4,
        });
        let without = button(1, Entity::PLACEHOLDER, Entity::PLACEHOLDER);
        let buttons = [&with, &without];
        let ground = Some(crate::Pointing::Ground([3.0, 4.0]));
        assert_eq!(marked(Some(0), &buttons, ground), Some((0, [3.0, 4.0])));
        assert_eq!(marked(None, &buttons, ground), None);
        assert_eq!(marked(Some(1), &buttons, ground), None);
        let minimap = Some(crate::Pointing::Minimap([3.0, 4.0]));
        assert_eq!(marked(Some(0), &buttons, minimap), None);
        assert_eq!(marked(Some(0), &buttons, None), None);
    }

    #[test]
    fn recharge_time_shows_in_minutes_and_seconds_rounded_up() {
        let text = "Recharging (%d:%.2d left)";
        assert_eq!(recharging(text, 244.2), "Recharging (4:05 left)");
        assert_eq!(recharging(text, 0.5), "Recharging (0:01 left)");
    }

    /// An app with the operation armed or refused by a press of its
    /// button, and the messages it posted.
    fn pressed(experience: f32) -> (App, Vec<String>) {
        let mut app = App::new();
        app.add_message::<Failed>()
            .init_resource::<Armed>()
            .init_resource::<Page>()
            .insert_resource(Sim(operating(experience)))
            .insert_resource(panel());
        let b = app
            .world_mut()
            .spawn(button(0, Entity::PLACEHOLDER, Entity::PLACEHOLDER))
            .id();
        app.world_mut()
            .run_system_once(move |buttons: Query<&OperationButton>, mut user: User| {
                user.press(buttons.get(b).unwrap());
            })
            .unwrap();
        let said = app
            .world_mut()
            .resource_mut::<Messages<Failed>>()
            .drain()
            .map(|f| f.text)
            .collect();
        (app, said)
    }

    #[test]
    fn a_press_arms_the_cursor_or_says_why_it_cant() {
        let (app, said) = pressed(200.0);
        assert_eq!(app.world().resource::<Armed>(), &Armed(Some(0)));
        assert!(said.is_empty());
        let (app, said) = pressed(100.0);
        assert_eq!(app.world().resource::<Armed>(), &Armed(None));
        assert_eq!(
            said,
            ["Insufficient experience to use Special Operation Resupply"]
        );
    }

    #[test]
    fn experience_shows_its_ceiling_where_there_is_one() {
        assert_eq!(experience_text(474, Some(5000.0)), "474/5000");
        assert_eq!(experience_text(474, None), "474");
    }

    #[test]
    fn tooltips_give_cost_and_area_where_they_apply() {
        let op = SpecialOperation {
            name: "Armored Resupply".into(),
            description: None,
            icon: None,
            recharge: 300.0,
            on_self: false,
            marker: None,
            area: Some("Large".into()),
            effects: Vec::new(),
        };
        let row = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            figures(&op, 300.0, "Cost", "Area"),
            [row("Cost", "300"), row("Area", "Large")]
        );
        let free = SpecialOperation { area: None, ..op };
        assert!(figures(&free, 0.0, "Cost", "Area").is_empty());
    }

    #[test]
    fn only_the_hovered_operation_shows_its_tooltip() {
        let mut app = App::new();
        app.add_systems(Update, tips);
        let hidden = || Node {
            display: Display::None,
            ..default()
        };
        let tips: Vec<Entity> = (0..2)
            .map(|_| app.world_mut().spawn(hidden()).id())
            .collect();
        let on = Hovered(true);
        let line = Entity::PLACEHOLDER;
        app.world_mut().spawn((button(0, tips[0], line), on));
        let off = Hovered(false);
        app.world_mut().spawn((button(1, tips[1], line), off));
        app.update();
        let shown = |e: Entity| app.world().get::<Node>(e).map(|n| n.display);
        assert_eq!(shown(tips[0]), Some(Display::Flex));
        assert_eq!(shown(tips[1]), Some(Display::None));
    }
}
