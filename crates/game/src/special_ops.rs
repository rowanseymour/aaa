//! The special operations panel in the top left corner: the player's
//! side's experience, and a button for each of its general's special
//! operations, greyed out while it can't be used.

use std::collections::HashMap;

use aaa_data::SpecialOperation;
use aaa_render::textures;
use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;

use bevy::picking::hover::Hovered;

use crate::actions::{self, BUTTON, Blocks, GAP, GLYPH_DIR, PAD};
use crate::keys::Bindings;
use crate::ui::{
    self, Edge, Font, Gauge, Hint, Ink, Marked, Meter, Shade, Surface, label, meter, panel, tooltip,
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
        app.add_systems(OnEnter(Screen::Match), spawn)
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.remove_resource::<Panel>();
            })
            .add_systems(
                Update,
                (refresh, tips)
                    .run_if(in_state(Screen::Match))
                    .run_if(resource_exists::<Panel>),
            );
    }
}

/// What the panel shows of the player's side, which has a general.
#[derive(Resource)]
struct Panel {
    player: u32,
    general: String,
    max: Option<f32>,
    words: Words,
    /// What the experience was last shown as, to redo only on a change.
    shown: Option<i64>,
}

/// The tooltips' words from the game's strings.
struct Words {
    title: String,
    general: String,
    experience: String,
    help: String,
}

#[derive(Component)]
struct ExperienceText;

/// The experience's box, whose tooltip tells it too.
#[derive(Component)]
struct ExperienceBox;

/// An operation's button, by what using it costs.
#[derive(Component)]
struct OperationButton {
    cost: f32,
    /// Its tooltip, beside the tray while the pointer's on the button.
    tip: Entity,
}

#[derive(Component)]
struct OperationIcon;

/// The bar under an operation's icon, showing how far it's recharged.
#[derive(Component)]
struct RechargeBar;

fn spawn(
    mut commands: Commands,
    mut loaded: ResMut<Loaded>,
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
    let game = &mut loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let mut cache: HashMap<String, Option<Handle<Image>>> = HashMap::new();
    let mut image = |game: &mut aaa_data::GameData, path: &str| {
        (cache.entry(path.to_string()))
            .or_insert_with(|| {
                let decoded = textures::decode(&game.read(path)?, path, formats.0, true)?;
                Some(images.add(decoded))
            })
            .clone()
    };
    let medal = image(game, &format!("{GLYPH_DIR}/{MEDAL_GLYPH}.png"));
    let operations = game.special_operations();
    let words = Words {
        title: said("game_interface_FactionExperience_ToolTip", "Experience"),
        general: said("ui_tooltip_item_general", "General"),
        experience: said("ui_tooltip_item_experience", "Experience"),
        help: said(
            "game_interface_FactionExperience_ToolTip_2",
            "Spend experience to activate Special Operations.",
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
            Some((o.cost, icon, name, op.clone(), figures))
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
                    for (cost, icon, name, op, figures) in buttons {
                        let tip = pane(tray, &name, &op, &figures);
                        tray.spawn((
                            OperationButton { cost, tip },
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
/// name, what its effects do, its Cost and Area, then its description. It
/// sits a little way right of the tray, level with its top, as the action
/// palette's sits above the palette.
fn pane(
    tray: &mut ChildSpawnerCommands,
    name: &Marked,
    op: &SpecialOperation,
    figures: &[(String, String)],
) -> Entity {
    let node = Node {
        display: Display::None,
        position_type: PositionType::Absolute,
        left: percent(100),
        top: px(0),
        margin: UiRect::left(px(PAD)),
        width: px(actions::SIZE.x),
        ..default()
    };
    tray.spawn(tooltip(node))
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
            if let Some(description) = &op.description {
                tip.spawn(label(description.clone(), Font::Small, Ink::Help));
            }
        })
        .id()
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

/// "474/5000": the experience rounded, and the ceiling when there is one.
fn experience_text(experience: i64, max: Option<f32>) -> String {
    match max {
        Some(max) => format!("{experience}/{}", max.round()),
        None => experience.to_string(),
    }
}

/// Keeps the experience and its tooltip up to date, and greys out the
/// operations the side can't afford.
fn refresh(
    mut panel: ResMut<Panel>,
    sim: Res<Sim>,
    mut text: Query<&mut Text, With<ExperienceText>>,
    mut boxes: Query<&mut Hint, With<ExperienceBox>>,
    buttons: Query<(&OperationButton, &Children)>,
    mut icons: Query<&mut Shade, With<OperationIcon>>,
    mut bars: Query<&mut Meter, With<RechargeBar>>,
) {
    let experience = sim.0.experience(panel.player).unwrap_or(0.0);
    // Against the experience itself, which the text only rounds.
    for (button, children) in &buttons {
        let shade = match experience >= button.cost {
            true => Shade::Full,
            false => Shade::Dimmed,
        };
        for &child in children {
            if let Ok(mut s) = icons.get_mut(child) {
                s.set_if_neq(shade);
            }
            // Every operation starts a match ready.
            if let Ok(mut bar) = bars.get_mut(child)
                && bar.share != 1.0
            {
                bar.share = 1.0;
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
    use super::*;

    #[test]
    fn buttons_light_once_experience_reaches_their_cost_however_it_shows() {
        let pathing = aaa_map::Pathing::uniform(8, aaa_map::Domain::Land);
        let mut world = aaa_sim::World::from_regiments(Default::default(), pathing, &[]);
        world.lead(1, 199.5, Some(5000.0));
        let mut app = App::new();
        app.insert_resource(Sim(world)).add_systems(Update, refresh);
        app.insert_resource(Panel {
            player: 1,
            general: "Marshal Zhukov".into(),
            max: Some(5000.0),
            words: Words {
                title: "Experience".into(),
                general: "General".into(),
                experience: "Experience".into(),
                help: String::new(),
            },
            shown: None,
        });
        let icon = app.world_mut().spawn((OperationIcon, Shade::Dimmed)).id();
        let bar = app
            .world_mut()
            .spawn((meter(Node::default(), Gauge::Recharge), RechargeBar))
            .id();
        app.world_mut()
            .spawn(OperationButton {
                cost: 200.0,
                tip: Entity::PLACEHOLDER,
            })
            .add_children(&[icon, bar]);
        app.update();
        // Shown as 200, but short of it.
        assert_eq!(app.world().get::<Shade>(icon), Some(&Shade::Dimmed));
        assert_eq!(app.world().get::<Meter>(bar).map(|m| m.share), Some(1.0));
        app.world_mut()
            .resource_mut::<Sim>()
            .0
            .lead(1, 200.0, Some(5000.0));
        app.update();
        assert_eq!(app.world().get::<Shade>(icon), Some(&Shade::Full));
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
        app.world_mut().spawn((
            OperationButton {
                cost: 0.0,
                tip: tips[0],
            },
            on,
        ));
        let off = Hovered(false);
        app.world_mut().spawn((
            OperationButton {
                cost: 0.0,
                tip: tips[1],
            },
            off,
        ));
        app.update();
        let shown = |e: Entity| app.world().get::<Node>(e).map(|n| n.display);
        assert_eq!(shown(tips[0]), Some(Display::Flex));
        assert_eq!(shown(tips[1]), Some(Display::None));
    }
}
