//! The action buttons: a grid of the orders the selection can take, with
//! the game's icons in the places its data gives them, for either
//! interface to draw. Orders given at a place, such as Move, wait for a left click on the
//! map or minimap, with a button to cancel; right click or Escape does too.
//! A selected building that recruits, such as a Corps HQ, has Deploy,
//! opening a menu of what comes out of it, and Deploy to Location, of what
//! a transport carries to where it's sent, each while that menu offers
//! anything; [`crate::object_display`] shows what it's been asked for, and a button
//! sets the rally point what it recruits moves on to. One that researches
//! has Research, opening a menu of the technologies it lists. A building that
//! packs, into a truck say, has Pack, dimmed while it can't. A player with
//! allies has Team Commands whatever is selected: a choice of ally, then
//! the requests that can be made of them. Each button
//! answers to the [`Intent`]s naming what it does or its slot's widget,
//! whatever sends them; its tooltip marks the letter of its key.

mod aim;
mod card_buttons;
mod layout;
mod looks;
mod press;
mod tips;

pub(crate) use aim::aim;
pub(crate) use card_buttons::{
    AimedFor, CARD_BUTTON, CardButton, card_order, hold_card_button, let_go_card_buttons,
    press_card_button, show_card_buttons,
};
pub(crate) use layout::{allied, allies, builder, builds, contents, lit, structure_state};
pub(crate) use looks::{Looks, Quantity, setup};
pub(crate) use press::{act, widget};
pub(crate) use tips::{Said, Tip, fill, hover, tell};

use bevy::prelude::*;
use tgsim::{Lacking, Researchable};

use crate::control_groups::{self, ControlGroups};
use crate::{Local, Selected, SelectedBuilding, Sim};

pub const COLUMNS: usize = 4;
pub const ROWS: usize = 3;
pub const SLOTS: usize = COLUMNS * ROWS;
/// The original's widget for a grid button, repeated once per slot and
/// named by it in the hotkey files, e.g. `ActionButtonsButton_3`.
pub const WIDGET: &str = "ActionButtonsButton";
/// The orders an organization has, by command id, and what each does here;
/// `None` for those the sim can't carry out yet, which are shown dimmed.
/// The game leaves the rest of the commands off an organization's grid.
/// They're in the game's own order of commands, which decides who keeps a
/// slot two want.
const ORDERS: [(&str, Option<Give>); 18] = [
    ("kill", Some(Give::Aim(Aim::Attack))),
    ("bombard", Some(Give::Aim(Aim::Bombard))),
    ("guard", Some(Give::Aim(Aim::Guard))),
    ("repair", Some(Give::Aim(Aim::Repair))),
    (CONSTRUCT, Some(Give::Build)),
    ("move", Some(Give::Aim(Aim::Move))),
    ("stop", Some(Give::Stop)),
    ("retreat", Some(Give::Aim(Aim::Run))),
    ("rout", Some(Give::Rout)),
    ("explore", Some(Give::Aim(Aim::Explore))),
    ("disband", Some(Give::Disband)),
    ("unpack", Some(Give::Aim(Aim::Unpack))),
    ("attach", Some(Give::AttachHero)),
    ("detach", Some(Give::DetachHero)),
    ("hq_attach", Some(Give::Attach)),
    ("hq_detach", Some(Give::Detach)),
    (control_groups::ENABLE, Some(Give::GroupBehavior)),
    (control_groups::DISABLE, Some(Give::GroupBehavior)),
];

/// The commands whose buttons open a recruiter's two menus.
const DEPLOY: &str = "recruit";
const DEPLOY_TO_LOCATION: &str = "recruit_location";
/// The command whose button opens a regiment's Build menu.
const CONSTRUCT: &str = "construct";
/// The command whose button opens a building's research menu.
const RESEARCH: &str = "research";
/// The command that packs a building up, such as into a truck.
const PACK: &str = "pack";
/// The command that sells a building.
const SELL: &str = "sell";
/// The command that sends out an airfield's air attack.
pub const AIR_ATTACK: &str = "air_attack";
/// The commands that switch an airfield's escort on and off.
const ESCORT_ON: &str = "escort_on";
const ESCORT_OFF: &str = "escort_off";
/// The command that sets where what a building commissions moves on to.
pub const RALLY: &str = "set_rally_point";

/// Where the game puts an HQ's button that selects the organizations attached
/// to it, the image it shows, and its strings' keys.
const COMMAND_ALL: usize = 4;
const COMMAND_ALL_ICON: &str = "UI/Game/ActionButtons/IconCommandAll.png";
const COMMAND_ALL_NAME: &str = "action_buttons_CommandAll_name";
const COMMAND_ALL_HELP: &str = "action_buttons_CommandAll_tooltip";

/// The layouts' names for the button that opens the team commands, the
/// requests made of allies, the ally page's buttons for all allies and
/// for one, and the way back from either page.
const TEAM_COMMANDS: &str = "GoToTeamCommands";
const ALL_ALLIES: &str = "SelectTeamKingdomAlly";
const SELECT_ALLY: &str = "SelectTeamKingdom";
const TEAM_CANCEL: &str = "CancelTeamCommands";
/// How many allies the ally page has buttons for, after the one for all.
const MOST_ALLIES: usize = 10;
/// The team commands offered only for one ally, never for all of them.
const ONE_ALLY: [&str; 4] = [
    "team_give_resources",
    "team_give_actor",
    "team_request_resources",
    "team_request_actor",
];
/// The team command that opens the chat bar, offered for one ally only
/// when a person plays it.
const TEAM_CHAT: &str = "team_chat";

/// Where the game puts the button that cancels an order being aimed.
const CANCEL: usize = 8;
/// Where the commission menu's way back goes.
const MENU_CANCEL: usize = 11;
/// The command whose look the cancel button borrows, as the game gives
/// none of its own for backing out of an order or a menu.
const CANCEL_LOOK: &str = "construction_cancel";
/// What an order does once its button is pressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Give {
    /// Waits for a place on the map.
    Aim(Aim),
    Stop,
    Rout,
    Attach,
    Detach,
    /// Asks first.
    Disband,
    /// Needs a hero chosen first.
    AttachHero,
    DetachHero,
    /// Opens the Build menu.
    Build,
    /// Flips the Group Behavior switch.
    GroupBehavior,
}

impl Give {
    /// The order to rout, attach, detach or disband `organization`, or
    /// detach its hero; none for what waits on a place or a choice first.
    fn command(self, organization: usize) -> Option<tgsim::Command> {
        match self {
            Give::Rout => Some(tgsim::Command::Rout { organization }),
            Give::Attach => Some(tgsim::Command::Attach { organization }),
            Give::Detach => Some(tgsim::Command::Detach { organization }),
            Give::Disband => Some(tgsim::Command::Disband { organization }),
            Give::DetachHero => Some(tgsim::Command::DetachHero { organization }),
            Give::Aim(_) | Give::Stop | Give::AttachHero | Give::Build | Give::GroupBehavior => {
                None
            }
        }
    }
}

/// The way buildings the player unpacks face, for the order and its
/// preview alike.
pub const UNPACK_FACING: [f32; 2] = tgmap::UNPACKED_FACING;

/// An order given at a place.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Aim {
    Move,
    Attack,
    /// Given on a thing or a place.
    Bombard,
    Run,
    Guard,
    Unpack,
    /// Given on a thing, not a place.
    Repair,
    Explore,
}

impl Aim {
    pub const ALL: [Aim; 8] = [
        Aim::Move,
        Aim::Attack,
        Aim::Bombard,
        Aim::Run,
        Aim::Guard,
        Aim::Unpack,
        Aim::Repair,
        Aim::Explore,
    ];
}

/// What the grid is showing.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Page {
    #[default]
    Orders,
    /// Waiting for a click on the map to give an order there.
    Aiming(Aim),
    /// Waiting for a click on the map to set the selected building's
    /// rally point there.
    Rally,
    /// Waiting for a click on the map, or a thing on it, to send the
    /// selected airfield's air attack there.
    AirAttack,
    /// Waiting for a click on the map to have the selected building
    /// deliver there the entry at this index into its menu.
    Delivering(usize),
    /// Waiting for a click on the map, or a drag to turn it, to have the
    /// selection put up there the entry at this index into its Build
    /// menu.
    Placing(usize),
    /// One of the selected building's menus of what it recruits or
    /// researches.
    Menu(Menu),
    /// The choice of whom the team commands are for, whatever is
    /// selected.
    Allies,
    /// The team commands for them.
    Team(Target),
}

/// Whom the team commands are for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// All the player's allies.
    All,
    /// One ally, by player id.
    Side(u32),
}

/// Which of a building's menus something is in: Deploy for what comes out
/// of it, Deploy to Location for what names a transport, and Research; or
/// a regiment's Build menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Deploy,
    Location,
    Research,
    Build,
}

/// How a recruiter's menu shows one of its entries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Offer {
    Offered,
    /// Shown, but not pressable.
    Greyed,
    /// Left out, its slot kept empty.
    Hidden,
}

impl From<&Lacking> for Offer {
    fn from(lacking: &Lacking) -> Offer {
        if lacking.hidden {
            Offer::Hidden
        } else if lacking.is_empty() {
            Offer::Offered
        } else {
            Offer::Greyed
        }
    }
}

/// What a button is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// Index into [`ORDERS`].
    Order(usize),
    Cancel,
    Open(Menu),
    /// Index into everything the selected building recruits.
    Recruit(usize),
    /// Index into what the selected building researches.
    Research(usize),
    /// Packs the selected building up.
    Pack,
    /// Sells the selected building, once the player's sure.
    Sell,
    /// Waits for a place to set the selected building's rally point.
    Rally,
    /// Waits for a target for the selected airfield's air attack.
    AirAttack,
    /// Switches the selected airfield's escort on or off.
    Escort,
    /// Selects the organizations attached to the selected HQ.
    CommandAll,
    /// Index into what the selection can build.
    Build(usize),
    /// Calls off the selected building site.
    CancelSite,
    /// Opens the ally page.
    TeamCommands,
    /// Opens the team commands for all allies.
    AllAllies,
    /// Opens them for one ally, by index into [`allies`].
    Ally(usize),
    /// Index into the team commands.
    Team(usize),
    /// Goes back from the ally page or the team commands.
    TeamCancel,
}

/// A grid button, of either interface, at this slot.
#[derive(Component)]
pub struct Slot(pub usize);

/// What the grid holds now, as [`fill_slots`] works it out for the keys
/// and whichever interface draws it.
#[derive(Resource, Default, PartialEq)]
pub struct Shown {
    /// Whether the selection is the player's to command, so has a grid.
    pub(crate) commanded: bool,
    /// What each slot holds.
    pub(crate) entries: [Option<Entry>; SLOTS],
    /// How each slot's button shows.
    pub faces: [Option<Face>; SLOTS],
    /// What the player lacks for each recruit the menu offers, and how
    /// each research it lists stands.
    lacks: Vec<Lacking>,
    researches: Vec<Researchable>,
}

/// How a slot's button shows, whichever interface draws it.
#[derive(Clone, PartialEq, Debug)]
pub struct Face {
    pub icon: Option<Handle<Image>>,
    /// Whether pressing it can do anything now; its icon's greyed if not.
    pub ready: bool,
    /// The colour its icon is drawn in, where it's an ally's.
    pub tint: Option<[f32; 3]>,
}

/// The slot whose press was just refused and why, said while the cursor
/// stays on it.
#[derive(Resource, Default)]
pub struct Refusal(Option<(usize, tgsim::Refused)>);

pub fn aiming(page: Res<Page>) -> bool {
    matches!(
        *page,
        Page::Aiming(_) | Page::Rally | Page::AirAttack | Page::Delivering(_) | Page::Placing(_)
    )
}

/// Goes back to the orders on Escape, and when what the page is for is
/// no longer selected.
pub fn back_out(
    keys: Res<ButtonInput<KeyCode>>,
    selected: Res<Selected>,
    chosen: Res<SelectedBuilding>,
    (sim, local): (Res<Sim>, Res<crate::Local>),
    mut page: ResMut<Page>,
    mut seen: bevy::ecs::system::Local<Seen>,
    displays: Res<crate::displays::Displays>,
) {
    let picked = (selected.0.clone(), chosen.0);
    let repicked = seen.picked != picked;
    seen.picked = picked;
    let cleared = selected.0.is_empty() && chosen.0.is_none();
    // A menu is the building's, or the builder's, it was opened on.
    let switched = std::mem::replace(&mut seen.chosen, chosen.0) != chosen.0;
    let builder = builder(&sim.0, &selected.0);
    let rebuilt = std::mem::replace(&mut seen.builder, builder) != builder;
    let theirs = !crate::selection::commanded(&sim.0, local.0, &selected.0, chosen.0);
    let gone = match *page {
        Page::Orders => false,
        // Requests of allies are open whoever's things are selected, while
        // there are allies to ask, until something else is selected.
        Page::Allies | Page::Team(_) if !allied(&sim.0, local.0) => true,
        Page::Team(Target::Side(side)) if !allies(&sim.0, local.0).contains(&side) => true,
        Page::Allies | Page::Team(_) => repicked && !cleared,
        _ if theirs => true,
        Page::Placing(_) | Page::Menu(Menu::Build) => builder.is_none() || rebuilt,
        Page::Aiming(_) => selected.0.is_empty(),
        Page::Menu(_) => chosen.0.is_none() || switched,
        Page::Rally | Page::AirAttack => chosen.0.is_none(),
        Page::Delivering(_) => chosen.0.is_none() || switched,
    };
    // Escape closes a display panel that's up, not the page under it.
    let escaped = keys.just_pressed(KeyCode::Escape) && crate::displays::closed(Some(displays));
    // A team page that gave way to a selection comes back when that's
    // cleared.
    match *page {
        Page::Allies | Page::Team(_) if gone && !escaped => seen.left = Some(*page),
        Page::Orders if repicked && cleared => {
            if let Some(left) = seen.left.take().filter(|_| allied(&sim.0, local.0)) {
                page.set_if_neq(left);
                return;
            }
        }
        Page::Orders => {}
        _ => seen.left = None,
    }
    if escaped {
        seen.left = None;
    }
    if gone || escaped {
        page.set_if_neq(Page::Orders);
    }
}

/// What [`back_out`] saw last: the building selected, the builder, and
/// the whole selection; and the team page that gave way to a selection
/// since.
#[derive(Default)]
pub struct Seen {
    chosen: Option<usize>,
    builder: Option<usize>,
    picked: (Vec<usize>, Option<usize>),
    left: Option<Page>,
}

/// Has the grid's buttons, of either interface, sound when pressed only
/// while a press can do something.
pub fn sound_slots(
    shown: Res<Shown>,
    slots: Query<(Entity, &Slot, Has<crate::ui::Click>)>,
    mut commands: Commands,
) {
    for (id, slot, clicks) in &slots {
        let ready = shown.faces[slot.0].as_ref().is_some_and(|f| f.ready);
        match (ready, clicks) {
            (true, false) => _ = commands.entity(id).insert(crate::ui::Click::COMMAND),
            (false, true) => _ = commands.entity(id).remove::<crate::ui::Click>(),
            _ => {}
        }
    }
}

/// Fills the grid's slots for the page and the selection, for its keys
/// and whichever interface draws it.
pub fn fill_slots(
    page: Res<Page>,
    (selected, chosen): (Res<Selected>, Res<SelectedBuilding>),
    sim: Res<Sim>,
    looks: Res<Looks>,
    local: Res<Local>,
    (groups, orders): (Res<ControlGroups>, Res<crate::Orders>),
    mut shown: ResMut<Shown>,
) {
    let people = crate::chat::people(&orders, local.0);
    let building = chosen.0.filter(|_| selected.0.is_empty());
    let behavior = groups.behavior(&selected.0);
    // Others' things have no buttons of their own, nor anything for a key
    // to press, but a player with allies has the team commands whatever
    // is selected.
    let now = match crate::selection::commanded(&sim.0, local.0, &selected.0, building) {
        true => contents(
            *page,
            &sim.0,
            &looks,
            &selected.0,
            building,
            local.0,
            behavior,
            &people,
        ),
        false if allied(&sim.0, local.0) => {
            contents(*page, &sim.0, &looks, &[], None, local.0, None, &people)
        }
        false => Shown::default(),
    };
    shown.set_if_neq(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A match of the player, 1, allied to 2, whose building is 0, with
    /// nothing selected and the grid on `page`.
    fn app(page: Page) -> App {
        let mut players: Vec<_> = [1, 2].map(crate::tests::player).into();
        for p in &mut players {
            p.relations = vec![tgmap::Relation::Ally; 2];
        }
        let sides = tgsim::Sides {
            players,
            ..Default::default()
        };
        let pathing = tgmap::Pathing::uniform(8, tgmap::Domain::Land);
        let mut world = tgsim::World::from_organizations(sides, pathing, &[]);
        world.add_buildings(&[tgmap::Building {
            id: 9,
            template: "hq".into(),
            owner: 2,
            position: [20.0, 20.0],
            facing: [0.0, -1.0],
            fighter: Default::default(),
            health: 1.0,
            zone: None,
            members: Vec::new(),
            obstacle: None,
            deploy: None,
            organization: None,
            awake: false,
            stockpile: 0.0,
        }]);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Selected>()
            .init_resource::<SelectedBuilding>()
            .init_resource::<crate::displays::Displays>()
            .insert_resource(Sim(world))
            .insert_resource(Local(Some(1)))
            .insert_resource(page)
            .add_systems(Update, back_out);
        app.update();
        app
    }

    fn pick(app: &mut App, building: Option<usize>) {
        app.world_mut().resource_mut::<SelectedBuilding>().0 = building;
        app.update();
    }

    fn page(app: &App) -> Page {
        *app.world().resource::<Page>()
    }

    #[test]
    fn a_team_page_gives_way_to_a_selection_and_comes_back_when_it_s_cleared() {
        let team = Page::Team(Target::Side(2));
        let mut app = app(team);
        assert_eq!(page(&app), team);
        // A building of someone else's: the selection's commands.
        pick(&mut app, Some(0));
        assert_eq!(page(&app), Page::Orders);
        pick(&mut app, None);
        assert_eq!(page(&app), team);
        // Clearing what's already clear leaves the page as it is.
        pick(&mut app, None);
        assert_eq!(page(&app), team);
    }

    #[test]
    fn a_team_page_goes_with_the_ally_it_was_for() {
        let mut app = app(Page::Team(Target::Side(2)));
        let mut sim = app.world_mut().resource_mut::<Sim>();
        sim.0.sides.players[0].relations[1] = tgmap::Relation::Enemy;
        app.update();
        assert_eq!(page(&app), Page::Orders);
    }
}
