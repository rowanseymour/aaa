//! Our special operations panel in the window's upper left corner: the
//! side's experience, and a tray of the faction's operations, each with a
//! tooltip beside the tray and a bar showing how far it's recharged.

use std::collections::HashMap;

use bevy::image::CompressedImageFormatSupport;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use tgdata::SpecialOperation;
use tgrender::textures;

use super::actions::{BUTTON, GAP, PAD};
use crate::actions::{Blocks, GLYPH_DIR};
use crate::intent::Asks;
use crate::keys::Bindings;
use crate::special_ops::{Panel, action, offered, probe};
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
/// which only a side with a faction has.
pub fn corner(local: &Local, leaders: &Leaders) -> Vec2 {
    let led = local.0.is_some_and(|p| leaders.0.contains_key(&p));
    match led {
        true => CLEAR,
        false => Vec2::splat(crate::MARGIN),
    }
}

/// The experience's figure, with what it was last shown as, to redo only
/// on a change.
#[derive(Component, Default)]
pub struct ExperienceText(Option<i64>);

/// The experience's box, whose tooltip tells it too.
#[derive(Component)]
pub struct ExperienceBox;

/// An operation's button.
#[derive(Component)]
pub struct OperationButton {
    /// Its operation's place in the faction's list.
    index: usize,
    /// Its tooltip, beside the tray while the pointer's on the button, and
    /// the tooltip's line saying how long it has left to recharge.
    tip: Entity,
    recharge_line: Entity,
}

#[derive(Component)]
pub struct OperationIcon;

/// The bar under an operation's icon, showing how far it's recharged.
#[derive(Component)]
pub struct RechargeBar;

/// Spawns our interface's experience and operations, in the window's upper
/// left corner.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    mut commands: Commands,
    loaded: Res<Loaded>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    bindings: Res<Bindings>,
    shared: Option<Res<Panel>>,
) {
    let (Some(shared), Some(faction)) = (shared, (local.0).and_then(|p| leaders.0.get(&p))) else {
        return;
    };
    let game = &loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let mut cache: HashMap<String, Option<Handle<Image>>> = HashMap::new();
    let mut image = |game: &tgdata::GameData, path: &str| {
        (cache.entry(path.to_string()))
            .or_insert_with(|| {
                let decoded = textures::decode(&game.read(path)?, path, formats.0, true)?;
                Some(images.add(decoded))
            })
            .clone()
    };
    let medal = image(game, &format!("{GLYPH_DIR}/{MEDAL_GLYPH}.png"));
    let operations = game.special_operations();
    let cost = said("ui_tooltip_item_title_cost", "Cost");
    let area = said("ui_tooltip_item_title_area", "Area");
    let buttons: Vec<_> = offered(&faction.operations, &operations)
        .map(|(n, o, op)| {
            let icon = (op.icon.as_deref()).and_then(|path| image(game, path));
            let name = bindings.marked(&action(n), &op.name);
            let figures = figures(op, o.cost, &cost, &area);
            (n, icon, name, op.clone(), figures)
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
                    Marked::plain(shared.words.title.clone()),
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
                    row.spawn((ExperienceText::default(), label("", Font::Hud, Ink::Body)));
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
                    for (index, icon, name, op, figures) in buttons {
                        let (tip, recharge_line) = pane(tray, &name, &op, &figures);
                        tray.spawn((
                            OperationButton {
                                index,
                                tip,
                                recharge_line,
                            },
                            Button,
                            ui::called(format!("special_op.{}", index + 1)),
                            Asks::new(action(index)),
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
                        });
                    }
                });
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
        width: px(super::actions::SIZE.x),
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
pub fn tips(buttons: Query<(&OperationButton, &Hovered)>, mut nodes: Query<&mut Node>) {
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
pub fn refresh(
    panel: Res<Panel>,
    sim: Res<Sim>,
    mut text: Query<(&mut Text, &mut ExperienceText)>,
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
    let mut changed = false;
    for (mut t, mut shown) in &mut text {
        if shown.0 != Some(rounded) {
            shown.0 = Some(rounded);
            t.0 = experience_text(rounded, panel.max);
            changed = true;
        }
    }
    if !changed {
        return;
    }
    let w = &panel.words;
    let mut said = Vec::new();
    if !panel.faction.trim().is_empty() {
        said.push(format!("{}  {}", w.faction, panel.faction));
    }
    said.push(format!("{}  {rounded}", w.experience));
    let hint = format!("{}\n\n{}", said.join("\n"), w.help);
    for mut h in &mut boxes {
        h.set_if_neq(Hint(hint.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::special_ops::tests::{operating, panel};

    fn button(index: usize, tip: Entity, recharge_line: Entity) -> OperationButton {
        OperationButton {
            index,
            tip,
            recharge_line,
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
        let used = tgsim::Command::Operate {
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
    fn recharge_time_shows_in_minutes_and_seconds_rounded_up() {
        let text = "Recharging (%d:%.2d left)";
        assert_eq!(recharging(text, 244.2), "Recharging (4:05 left)");
        assert_eq!(recharging(text, 0.5), "Recharging (0:01 left)");
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
            summons: None,
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
