//! Special operations: the player's side's experience and its faction's
//! operations, for either interface to show. Pressing one's button, or its
//! key, arms the cursor with it, and the next left click in the world
//! gives it.

use std::collections::HashMap;

use bevy::prelude::*;
use tgdata::{Decal, SpecialOperation};
use tgrender::MapAssets;
use tgrender::decals::DecalCache;
use tgrender::terrain::world;

use bevy::ecs::system::SystemParam;

use crate::actions::{Page, fill};
use crate::intent::{Asked, Intent};
use crate::messages::Failed;
use crate::orders::Orders;
use crate::{Leaders, Loaded, Local, Screen, Sim};

/// The key action that uses the operation in the `n`th button.
pub(crate) fn action(n: usize) -> String {
    format!("UseFactionAbility {n}")
}

pub struct SpecialOpsPlugin;

impl Plugin for SpecialOpsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Match), setup.after(crate::keys::load))
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.remove_resource::<Panel>();
            })
            .init_resource::<Armed>()
            .add_systems(OnExit(Screen::Match), |mut armed: ResMut<Armed>| {
                armed.0 = None
            })
            .add_systems(
                Update,
                mark.run_if(in_state(Screen::Match))
                    .run_if(resource_exists::<Panel>),
            );
    }
}

/// The special operation the cursor's armed with, by its place in the
/// faction's list: the next left click in the world gives it.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Armed(pub Option<usize>);

pub fn armed(armed: Res<Armed>) -> bool {
    armed.0.is_some()
}

/// What the panel shows of the player's side, which has a faction.
#[derive(Resource)]
pub(crate) struct Panel {
    pub(crate) player: u32,
    /// The operations the player's faction has, in its list's order.
    operations: Vec<Operation>,
    pub(crate) faction: String,
    pub(crate) max: Option<f32>,
    pub(crate) words: Words,
}

/// The tooltips' and messages' words from the game's strings.
pub(crate) struct Words {
    pub(crate) title: String,
    pub(crate) faction: String,
    pub(crate) experience: String,
    pub(crate) help: String,
    /// "Special Operation %s has not yet recharged".
    pub(crate) unready: String,
    /// "Insufficient experience to use Special Operation %s".
    pub(crate) short: String,
    /// "Special Operation %s can not summon an additional %s".
    pub(crate) summoned: String,
    /// "Invalid location for %s".
    pub(crate) misplaced: String,
    /// "Recharging (%d:%.2d left)".
    pub(crate) recharging: String,
}

impl Panel {
    fn operation(&self, index: usize) -> Option<&Operation> {
        self.operations.iter().find(|o| o.index == index)
    }
}

/// One of the operations a player's faction has, which its key or its
/// button uses, whichever interface draws the button.
#[derive(Clone, Debug)]
pub(crate) struct Operation {
    /// Its place in the faction's list.
    index: usize,
    name: String,
    /// The name of what it summons, for one that does.
    summons: Option<String>,
    /// It's used as soon as it's pressed, with no aiming.
    on_self: bool,
    /// What's laid on the ground at the cursor while it's armed, sized to
    /// the area it would land on.
    marker: Option<Decal>,
}

/// Takes the player's faction's operations and the words they're told of
/// in, for either interface.
fn setup(mut commands: Commands, loaded: Res<Loaded>, local: Res<Local>, leaders: Res<Leaders>) {
    let Some(player) = local.0 else {
        return;
    };
    let Some(faction) = leaders.0.get(&player) else {
        return;
    };
    let game = &loaded.game;
    let strings = game.strings();
    let said = |key: &str, or: &str| strings.get(key).map_or(or, String::as_str).to_string();
    let operations = game.special_operations();
    let vfx = game.vfx();
    let words = Words {
        title: said("game_interface_FactionExperience_ToolTip", "Experience"),
        faction: said("ui_tooltip_item_general", "General"),
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
        summoned: said(
            "ui_error_faction_ability_already_summoned|ss",
            "Special Operation %s can not summon an additional %s",
        ),
        misplaced: said(
            "ability_effect_invalid_location|s",
            "Invalid location for %s",
        ),
        recharging: said(
            "ui_floater_ability_recharge_time|dd",
            "Recharging (%d:%.2d left)",
        ),
    };
    let listed = offered(&faction.operations, &operations)
        .map(|(index, _, op)| Operation {
            index,
            name: op.name.clone(),
            summons: op.summons.clone(),
            on_self: op.on_self,
            marker: op.marker.as_ref().and_then(|m| {
                let decal = vfx.get(&m.vfx)?.decal.clone()?;
                Some(Decal {
                    size: m.size,
                    ..decal
                })
            }),
        })
        .collect();
    commands.insert_resource(Panel {
        player,
        operations: listed,
        faction: faction.name.clone(),
        max: faction.experience_max,
        words,
    });
}

/// The operations of `faction`'s that the game has, with their place in
/// its list.
pub(crate) fn offered<'a>(
    faction: &'a [tgdata::FactionOperation],
    operations: &'a HashMap<tgdata::Id, SpecialOperation>,
) -> impl Iterator<Item = (usize, &'a tgdata::FactionOperation, &'a SpecialOperation)> {
    (faction.iter().enumerate()).filter_map(|(n, o)| Some((n, o, operations.get(&o.id)?)))
}

/// The order to use the `i`th operation, where it's aimed being no part of
/// whether it can be used.
pub(crate) fn probe(i: usize) -> tgsim::Command {
    tgsim::Command::Operate {
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
    /// Arms the cursor with the `index`th operation, or uses one that needs
    /// no aiming at once, unless it can't be used now.
    fn press(&mut self, index: usize) {
        let Some(panel) = &self.panel else {
            return;
        };
        let player = panel.player;
        let Some(on_self) = panel.operation(index).map(|o| o.on_self) else {
            return;
        };
        match self.sim.0.check(player, &probe(index)) {
            Ok(()) if on_self => {
                let given =
                    (self.orders.as_mut()).map(|o| o.give(&self.sim.0, player, probe(index)));
                if let Some(Err(e)) = given {
                    self.refuse(index, e);
                }
            }
            Ok(()) => {
                self.armed.0 = Some(index);
                // Not aiming an order too.
                self.page.set_if_neq(Page::Orders);
            }
            Err(e) => self.refuse(index, e),
        }
    }

    /// Tells the player why the `index`th operation can't be used.
    fn refuse(&mut self, index: usize, refused: tgsim::Refused) {
        let Some(panel) = &self.panel else {
            return;
        };
        let Some(operation) = panel.operation(index) else {
            return;
        };
        let name = operation.name.as_str();
        let summons = operation.summons.as_deref().unwrap_or_default();
        let said = match refused {
            tgsim::Refused::Recharging => &panel.words.unready,
            tgsim::Refused::Unaffordable => &panel.words.short,
            tgsim::Refused::Summoned => &panel.words.summoned,
            tgsim::Refused::Misplaced => &panel.words.misplaced,
            e => {
                debug!("can't use {name}: {e:?}");
                return;
            }
        };
        let text = fill(said, &[name, summons]);
        self.failed.write(Failed { text, about: None });
    }
}

/// An operation's button clicked.
/// Uses the operation asked for, by its button or its key.
pub fn act(mut intents: MessageReader<Intent>, mut user: User) {
    let asked = Asked::read(&mut intents);
    if asked.is_empty() {
        return;
    }
    let pressed = (user.panel.as_ref()).and_then(|panel| {
        (panel.operations.iter())
            .find(|o| asked.wants(&action(o.index)))
            .map(|o| o.index)
    });
    if let Some(index) = pressed {
        user.press(index);
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
    pointer: crate::pointing::Pointer,
    loaded: Res<Loaded>,
    focus: Res<crate::Focus>,
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
        user.refuse(i, e);
    }
}

/// The order a click `at` gives with `player`'s `i`th operation armed, with
/// `focus` under the cursor, if it gives one. Only a click on the map
/// itself is on what's under the cursor.
fn clicked(
    sim: &tgsim::World,
    player: u32,
    i: usize,
    at: crate::pointing::Pointing,
    focus: crate::Focus,
) -> Option<tgsim::Command> {
    let on = match (at, focus) {
        (crate::pointing::Pointing::Ground(_), crate::Focus::Organization(r)) => {
            Some(tgsim::Piece::Organization(r))
        }
        (crate::pointing::Pointing::Ground(_), crate::Focus::Building(b)) => {
            Some(tgsim::Piece::Building(b))
        }
        _ => None,
    };
    let to = sim.operation_aim(player, i, on, at.point())?;
    Some(tgsim::Command::Operate {
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
    panel: &Panel,
    at: Option<crate::pointing::Pointing>,
) -> Option<(usize, [f32; 2])> {
    let i = armed?;
    panel.operation(i)?.marker.as_ref()?;
    match at? {
        crate::pointing::Pointing::Ground(p) => Some((i, p)),
        crate::pointing::Pointing::Minimap(_) => None,
    }
}

/// Lays the armed operation's marker on the ground under the cursor, while
/// the cursor's over the map itself, its top toward the top of the map
/// whatever's under it.
#[allow(clippy::too_many_arguments)]
fn mark(
    mut commands: Commands,
    armed: Res<Armed>,
    panel: Res<Panel>,
    pointer: crate::pointing::Pointer,
    loaded: Res<Loaded>,
    mut assets: MapAssets,
    mut marking: bevy::prelude::Local<Marking>,
    laid: Query<Entity, With<CursorMarker>>,
) {
    let ground = &loaded.map.ground;
    let at = marked(armed.0, &panel, pointer.at(ground));
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
    let Some(marker) = panel.operation(i).and_then(|o| o.marker.as_ref()) else {
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

#[cfg(test)]
pub(crate) mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;
    use tgdata::Id;

    pub(crate) fn words() -> Words {
        Words {
            title: "Experience".into(),
            faction: "General".into(),
            experience: "Experience".into(),
            help: String::new(),
            unready: "Special Operation %s has not yet recharged".into(),
            short: "Insufficient experience to use Special Operation %s".into(),
            summoned: "Special Operation %s can not summon an additional %s".into(),
            misplaced: "Invalid location for %s".into(),
            recharging: "Recharging (%d:%.2d left)".into(),
        }
    }

    /// A side of `experience` whose one operation, a resupply, costs 200
    /// and recharges in 10 seconds.
    pub(crate) fn operating(experience: f32) -> tgsim::World {
        let pathing = tgmap::Pathing::uniform(8, tgmap::Domain::Land);
        let resupply = tgmap::Operation::Enchant(tgmap::Enchantment {
            targets: Default::default(),
            area: 15.0,
            max_targets: 4,
            property: Id::from("property_resupply"),
            seconds: 30.0,
            kind: None,
            morale: 0.0,
            damage: Vec::new(),
            delay: 0.0,
        });
        let mut rules = tgsim::Rules::default();
        rules.operations.insert(Id::from("resupply"), resupply);
        let mut world =
            tgsim::World::from_organizations_by(Default::default(), pathing, rules, &[]);
        let op = tgsim::SpecialOp::new(Id::from("resupply"), 200.0, 10.0);
        world.lead(1, experience, Some(5000.0), vec![op]);
        world
    }

    pub(crate) fn panel() -> Panel {
        Panel {
            player: 1,
            operations: vec![Operation {
                index: 0,
                name: "Resupply".into(),
                summons: None,
                on_self: false,
                marker: None,
            }],
            faction: "Marshal Zhukov".into(),
            max: Some(5000.0),
            words: words(),
        }
    }

    #[test]
    fn a_click_gives_the_operation_on_the_ground_but_not_on_what_it_cant_work_on() {
        let pathing = tgmap::Pathing::uniform(8, tgmap::Domain::Land);
        let mut rules = tgsim::Rules::default();
        let resupply = tgmap::Operation::Enchant(tgmap::Enchantment {
            targets: tgmap::Targets {
                required: vec![Id::from("organization")],
                excluded: Vec::new(),
            },
            area: 15.0,
            max_targets: 4,
            property: Id::from("property_resupply"),
            seconds: 30.0,
            kind: None,
            morale: 0.0,
            damage: Vec::new(),
            delay: 0.0,
        });
        rules.operations.insert(Id::from("resupply"), resupply);
        let mut sim = tgsim::World::from_organizations_by(Default::default(), pathing, rules, &[]);
        let op = tgsim::SpecialOp::new(Id::from("resupply"), 0.0, 0.0);
        sim.lead(1, 0.0, None, vec![op]);
        let ground = crate::pointing::Pointing::Ground([3.0, 4.0]);
        let operate = |at| {
            Some(tgsim::Command::Operate {
                operation: 0,
                at,
                on: None,
            })
        };
        assert_eq!(
            clicked(&sim, 1, 0, ground, crate::Focus::Nothing),
            operate([3.0, 4.0])
        );
        // A building isn't an organization, so the click gives nothing.
        assert_eq!(clicked(&sim, 1, 0, ground, crate::Focus::Building(0)), None);
        // On the minimap, what's under the cursor on the map plays no part.
        let minimap = crate::pointing::Pointing::Minimap([5.0, 6.0]);
        assert_eq!(
            clicked(&sim, 1, 0, minimap, crate::Focus::Building(0)),
            operate([5.0, 6.0])
        );
        // Nor does a side with no such operation give one.
        assert_eq!(clicked(&sim, 1, 1, ground, crate::Focus::Nothing), None);
    }

    #[test]
    fn the_armed_operations_marker_goes_under_the_cursor_on_the_map_only() {
        let mut panel = panel();
        let without = Operation {
            index: 1,
            ..panel.operations[0].clone()
        };
        panel.operations[0].marker = Some(Decal {
            texture: "crosshair.dds".into(),
            size: 30.0,
            colour: [1.0; 3],
            lit: false,
            additive: false,
            fade: [0.0; 2],
            layer: 4,
        });
        panel.operations.push(without);
        let ground = Some(crate::pointing::Pointing::Ground([3.0, 4.0]));
        assert_eq!(marked(Some(0), &panel, ground), Some((0, [3.0, 4.0])));
        assert_eq!(marked(None, &panel, ground), None);
        assert_eq!(marked(Some(1), &panel, ground), None);
        let minimap = Some(crate::pointing::Pointing::Minimap([3.0, 4.0]));
        assert_eq!(marked(Some(0), &panel, minimap), None);
        assert_eq!(marked(Some(0), &panel, None), None);
    }

    /// An app with the operation armed or refused by a press of its key or
    /// button, and the messages it posted. It has no button to press.
    fn pressed(experience: f32) -> (App, Vec<String>) {
        let mut app = App::new();
        app.add_message::<Failed>()
            .init_resource::<Armed>()
            .init_resource::<Page>()
            .insert_resource(Sim(operating(experience)))
            .insert_resource(panel());
        app.world_mut()
            .run_system_once(|mut user: User| user.press(0))
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
}
