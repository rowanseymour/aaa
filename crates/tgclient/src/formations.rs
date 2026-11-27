//! The formation buttons: one for each
//! formation the player's selected organizations can be switched to,
//! top to bottom, the one they're in lit. Each interface draws the
//! buttons as [`FormationButton`]s; what they offer, which is lit and what
//! a press does is worked out here, for either.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use tgmap::Id;

use crate::intent::Intent;
use crate::orders::Orders;
use crate::ui::{Edge, Hint, Marked, Shade, Surface};
use crate::{Loaded, Local, Selected, Sim};

/// What the hotkey files call the buttons beside one organization's
/// display and beside several's, numbered from the top, e.g.
/// `CompanyFormationButtons_0`.
pub const COMPANY: &str = "CompanyFormationButtons";
pub const GROUP: &str = "GroupFormationButtons";

/// The most buttons there are, and the most formations of each
/// organization they offer.
pub const MOST: usize = 3;
/// How many of the selection, from its first, have their formations
/// offered.
const COUNTED: usize = 6;

/// The name of the `n`th button from the top in the display of `several`
/// organizations or one.
pub fn button(n: usize, several: bool) -> String {
    let display = if several { GROUP } else { COMPANY };
    format!("{display}_{n}")
}

/// A formation button of either interface, by its place from the top.
#[derive(Component)]
#[require(Hint = Hint(String::new()), Marked, Hovered)]
pub struct FormationButton(pub usize);

/// The `n`th [`FormationButton`] from the top, hidden until there's a
/// formation for it.
pub fn button_bundle(n: usize) -> impl Bundle {
    (
        FormationButton(n),
        crate::ui::called(format!("formation.{}", n + 1)),
        bevy::ui_widgets::Button,
        Pickable::default(),
        Visibility::Hidden,
    )
}

/// A [`FormationButton`] pressed with the pointer, whose activation as it's
/// let go is that same press.
#[derive(Component)]
pub struct Held;

/// Asks for a formation button's formation as the left mouse button goes
/// down on it, as the original's do.
pub fn on_press(
    press: On<PointerPress>,
    buttons: Query<&FormationButton>,
    formations: Res<Formations>,
    mut intents: MessageWriter<Intent>,
    mut commands: Commands,
) {
    let Ok(place) = buttons.get(press.entity) else {
        return;
    };
    if press.button == PointerButton::Primary {
        intents.write(Intent(button(place.0, formations.several)));
        commands.entity(press.entity).insert(Held);
    }
}

/// Asks for a formation button's formation when it's activated other than
/// by the pointer it was pressed with, such as by the console.
pub fn on_activate(
    activate: On<bevy::ui_widgets::Activate>,
    buttons: Query<(&FormationButton, Has<Held>)>,
    formations: Res<Formations>,
    mut intents: MessageWriter<Intent>,
    mut commands: Commands,
) {
    match buttons.get(activate.entity) {
        Ok((_, true)) => _ = commands.entity(activate.entity).remove::<Held>(),
        Ok((place, false)) => _ = intents.write(Intent(button(place.0, formations.several))),
        Err(_) => {}
    }
}

/// Forgets a press the mouse button has been let go of away from its
/// button, which no activation follows.
pub fn let_go(
    mouse: Res<ButtonInput<MouseButton>>,
    held: Query<Entity, With<Held>>,
    mut commands: Commands,
) {
    if !mouse.pressed(MouseButton::Left) {
        held.iter()
            .for_each(|b| _ = commands.entity(b).remove::<Held>());
    }
}

/// On a [`FormationButton`], draws it as this project's own interface
/// does: a [`choice`](crate::ui::choice), with the stand-ins it has for
/// the game's pictures.
#[derive(Component)]
pub struct StandIns;

/// The picture of the formation on a [`FormationButton`].
#[derive(Component)]
#[require(ImageNode, Pickable = Pickable::IGNORE)]
pub struct FormationIcon;

/// What the buttons offer the selection and which of it is lit.
#[derive(Resource, Default, Debug)]
pub struct Formations {
    /// The formations offered, top to bottom; none at all for fewer than
    /// two.
    pub offered: Vec<Id>,
    /// Whether they're offered to several organizations rather than one.
    pub several: bool,
    pub lit: Option<Id>,
    /// The offered formations any of the counted organizations are in.
    pub held: Vec<Id>,
    /// The formation the counted organizations were last all seen in,
    /// which lights its button only as they come to be in it.
    seen: Option<Id>,
    /// What they were in when last looked at.
    was: Option<Shared>,
    /// The selection they were worked out for.
    selection: Vec<usize>,
    /// The tick a button was pressed on, until the match has played it.
    pressed: Option<u64>,
}

/// What formation the counted organizations are in, together.
#[derive(Clone, PartialEq, Debug)]
enum Shared {
    All(Id),
    Mixed,
    /// None of them has a formation, as while routing.
    Broken,
}

/// Those of the selection whose formations are offered: of its first
/// [`COUNTED`], the player's own that would take a formation order now.
fn counted<'a>(
    sim: &'a tgsim::World,
    player: Option<u32>,
    selected: &'a [usize],
) -> impl Iterator<Item = usize> + 'a {
    let takes = move |r: usize| {
        let order = tgsim::Command::Formation {
            organization: r,
            formation: sim.organizations[r].formation,
        };
        player.is_some_and(|p| sim.check(p, &order).is_ok())
    };
    selected
        .iter()
        .take(COUNTED)
        .copied()
        .filter(move |&r| takes(r))
}

/// The formations the buttons offer `selected`, top to bottom: the first
/// [`MOST`] each counted organization's layout lists, in its order, each
/// once; none at all where that's fewer than two.
pub fn offered(sim: &tgsim::World, player: Option<u32>, selected: &[usize]) -> Vec<Id> {
    let mut out: Vec<Id> = Vec::new();
    for r in counted(sim, player, selected) {
        for f in sim.organizations[r].formations.iter().take(MOST) {
            let id = Id::from(f.id.as_str());
            if out.len() < MOST && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    if out.len() < 2 {
        out.clear();
    }
    out
}

/// The formation each of the counted organizations of `selected` is in,
/// those that are in one.
fn in_formation<'a>(
    sim: &'a tgsim::World,
    player: Option<u32>,
    selected: &'a [usize],
) -> impl Iterator<Item = Id> + 'a {
    counted(sim, player, selected)
        .map(|r| &sim.organizations[r])
        .filter(|r| !r.broken())
        .filter_map(|r| r.formations.get(r.current_formation()))
        .map(|f| Id::from(f.id.as_str()))
}

/// What formation the counted organizations of `selected` are in now.
fn shared(sim: &tgsim::World, player: Option<u32>, selected: &[usize]) -> Shared {
    let mut now = in_formation(sim, player, selected);
    match now.next() {
        None => Shared::Broken,
        Some(first) => match now.all(|f| f == first) {
            true => Shared::All(first),
            false => Shared::Mixed,
        },
    }
}

/// Works out what the buttons offer the selection, and lights the one
/// for the formation they've come to be in: none while they're in
/// different ones, and the same as before while they have none.
pub fn light(
    sim: Res<Sim>,
    local: Res<Local>,
    selected: Res<Selected>,
    mut formations: ResMut<Formations>,
) {
    let sim = &sim.0;
    let state = &mut *formations;
    if state.selection != selected.0 {
        state.selection.clone_from(&selected.0);
        state.seen = None;
        state.was = None;
        state.pressed = None;
    }
    let offered = offered(sim, local.0, &selected.0);
    if state.offered != offered {
        state.offered = offered;
    }
    state.several = selected.0.len() > 1;
    if state.offered.is_empty() {
        state.held.clear();
        return;
    }
    let held: Vec<Id> = (state.offered.iter())
        .filter(|f| in_formation(sim, local.0, &selected.0).any(|g| g == **f))
        .cloned()
        .collect();
    if state.held != held {
        state.held = held;
    }
    let now = shared(sim, local.0, &selected.0);
    let opened = state.was.is_none();
    let played = state.pressed.is_some_and(|tick| sim.ticks > tick);
    if played {
        state.pressed = None;
    }
    if state.was.as_ref() != Some(&now) || played {
        match &now {
            Shared::All(f) if state.seen.as_ref() != Some(f) => {
                state.seen = Some(f.clone());
                state.lit = Some(f.clone());
            }
            Shared::Mixed => state.lit = None,
            _ => {}
        }
        state.was = Some(now);
    }
    // Opened on organizations with no formation, the top button lights,
    // unless the one lit before is there.
    let lit = state.lit.as_ref().filter(|f| state.offered.contains(f));
    if opened && lit.is_none() && state.was == Some(Shared::Broken) {
        state.lit = state.offered.first().cloned();
    }
}

/// Switches the selection to a button's formation, by its key or a click,
/// and lights it; the lit one gives no order.
#[allow(clippy::too_many_arguments)]
pub fn press(
    mut intents: MessageReader<Intent>,
    sim: Res<Sim>,
    local: Res<Local>,
    selected: Res<Selected>,
    mut formations: ResMut<Formations>,
    mut orders: ResMut<Orders>,
    mut sounds: ResMut<crate::sound::Sounds>,
    time: Res<Time>,
) {
    let several = formations.several;
    let asked = intents
        .read()
        .filter_map(|i| (0..MOST).find(|&n| i.is(&button(n, several))));
    // Only the last asked counts, as only one can be lit.
    let n = asked.last();
    let (Some(n), Some(player)) = (n, local.0) else {
        return;
    };
    let Some(formation) = formations.offered.get(n).cloned() else {
        return;
    };
    crate::ui::Click::COMMAND.play(&mut sounds, &time);
    if formations.lit.as_ref() == Some(&formation) {
        return;
    }
    let sim = &sim.0;
    for &r in &selected.0 {
        let organization = &sim.organizations[r];
        let Some(f) = (organization.formations.iter()).position(|f| f.id == formation.as_str())
        else {
            continue;
        };
        let order = tgsim::Command::Formation {
            organization: r,
            formation: f,
        };
        if orders.give(sim, player, order).is_ok() {
            debug!("{} to formation {formation}", organization.template);
        }
    }
    formations.lit = Some(formation);
    formations.pressed = Some(sim.ticks);
}

/// What a formation button's tooltip says under its name: whether the
/// formation takes on what comes near, then what it changes.
pub(crate) fn told(
    looks: &crate::object_display::Looks,
    sim: &tgsim::World,
    formation: &Id,
) -> String {
    let said = |key: &str| looks.strings.get(key).cloned().unwrap_or_default();
    let aggressive = (sim.organizations.iter())
        .flat_map(|r| &r.formations)
        .find(|f| f.id == formation.as_str())
        .is_some_and(|f| f.aggressive);
    let behavior = match aggressive {
        true => said("ui_tooltip_behavior_aggressive"),
        false => said("ui_tooltip_behavior_passive"),
    };
    let mut lines = vec![
        String::new(),
        format!("{} {behavior}", said("ui_tooltip_item_title_behavior")),
    ];
    let look = looks.formation_looks.get(formation);
    let modifiers = look.map_or(&[][..], |l| l.modifiers.as_slice());
    if !modifiers.is_empty() {
        lines.push(String::new());
        lines.extend(modifiers.iter().cloned());
    }
    lines.join("\n")
}

type Parts<'a> = (
    &'a FormationButton,
    &'a Hovered,
    &'a mut Visibility,
    &'a mut Hint,
    &'a mut Marked,
    Option<&'a mut crate::hud::original::Pressable>,
    Option<&'a mut Edge>,
    Option<&'a mut Surface>,
    &'a Children,
    Has<StandIns>,
);

/// Shows each interface's formation buttons as [`Formations`] has them:
/// the offered ones with their formation's picture, name and key, the
/// lit one lit, and the rest hidden.
#[allow(clippy::too_many_arguments)]
pub fn show(
    formations: Res<Formations>,
    sim: Res<Sim>,
    loaded: Res<Loaded>,
    bindings: Res<crate::keys::Bindings>,
    looks: Res<crate::object_display::Looks>,
    mut cache: ResMut<crate::ui::ImageCache>,
    mut images: ResMut<Assets<Image>>,
    mut buttons: Query<Parts>,
    mut icons: Query<(&mut ImageNode, Option<&mut Shade>), With<FormationIcon>>,
) {
    for (
        place,
        hovered,
        mut shown,
        mut hint,
        mut marked,
        pressable,
        edge,
        surface,
        children,
        stand_ins,
    ) in &mut buttons
    {
        let Some(formation) = formations.offered.get(place.0) else {
            shown.set_if_neq(Visibility::Hidden);
            continue;
        };
        shown.set_if_neq(Visibility::Inherited);
        let lit = formations.lit.as_ref() == Some(formation);
        if let Some(mut pressable) = pressable {
            pressable.selected = lit;
        }
        let choice = match (lit, formations.held.contains(formation)) {
            (true, _) => crate::ui::Choice::Current,
            (false, true) => crate::ui::Choice::Partly,
            (false, false) => crate::ui::Choice::Other,
        };
        let (fill, frame, shade) = match stand_ins {
            true => crate::ui::choice(choice, hovered.get()),
            false => (
                Surface::Clear,
                Edge::lit(lit, hovered.get(), Edge::Clear),
                Shade::Full,
            ),
        };
        if let Some(mut edge) = edge {
            edge.set_if_neq(frame);
        }
        if let Some(mut surface) = surface {
            surface.set_if_neq(fill);
        }
        let name = button(place.0, formations.several);
        let look = looks.formation_looks.get(formation);
        let called = look.map_or(formation.as_str(), |l| l.name.as_str());
        marked.set_if_neq(bindings.marked(&name, called));
        let icon = look.and_then(|l| l.icon.clone());
        if hovered.get() {
            let words = told(&looks, &sim.0, formation);
            if hint.0 != words {
                hint.0 = words;
            }
        }
        let game = &loaded.game;
        let icon = icon.map(|path| match stand_ins {
            true => game.modern_icon(&path),
            false => path,
        });
        let icon = icon.and_then(|path| cache.get(game, &mut images, &path));
        for child in children {
            let Ok((mut image, picture)) = icons.get_mut(*child) else {
                continue;
            };
            if let Some(mut picture) = picture {
                picture.set_if_neq(shade);
            }
            if let Some(icon) = &icon
                && image.image != *icon
            {
                image.image = icon.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn formation(id: &str) -> tgmap::Formation {
        tgmap::Formation {
            id: id.into(),
            offsets: vec![[0.0, 0.0]],
            aggressive: false,
        }
    }

    /// The test match, its first two organizations each able to stand in
    /// `firsts` and `seconds`, the first in its first.
    fn with(firsts: &[&str], seconds: &[&str]) -> tgsim::World {
        let mut sim = crate::orders::testing::fresh();
        for (r, ids) in [firsts, seconds].iter().enumerate() {
            sim.organizations[r].formations = ids.iter().map(|id| formation(id)).collect();
        }
        sim
    }

    #[test]
    fn the_players_own_offer_their_first_three_formations_each_once() {
        let sim = with(&["assault", "aggressive", "column", "herding"], &["line"]);
        let assault: Vec<Id> = ["assault", "aggressive", "column"].map(Id::from).into();
        assert_eq!(offered(&sim, Some(1), &[0]), assault);
        // Another's organization isn't counted.
        assert_eq!(offered(&sim, Some(1), &[0, 1]), assault);
        assert_eq!(offered(&sim, Some(2), &[0]), Vec::<Id>::new());
        // One formation alone makes no buttons.
        assert_eq!(offered(&sim, Some(2), &[1]), Vec::<Id>::new());
        assert_eq!(offered(&sim, None, &[0]), Vec::<Id>::new());
    }

    #[test]
    fn several_share_buttons_up_to_three() {
        let mut sim = with(&["assault", "column"], &["wedge", "assault", "line"]);
        sim.organizations[1].owner = 1;
        let ids: Vec<Id> = ["assault", "column", "wedge"].map(Id::from).into();
        assert_eq!(offered(&sim, Some(1), &[0, 1]), ids);
    }

    fn lit(app: &mut App) -> Option<String> {
        app.update();
        let lit = &app.world().resource::<Formations>().lit;
        lit.as_ref().map(|f| f.to_string())
    }

    #[test]
    fn the_formation_they_come_to_be_in_lights() {
        let mut sim = with(&["assault", "column"], &["assault", "column"]);
        sim.organizations[1].owner = 1;
        let mut app = App::new();
        app.insert_resource(Sim(sim))
            .insert_resource(Local(Some(1)))
            .insert_resource(Selected(vec![0, 1]))
            .init_resource::<Formations>()
            .add_systems(Update, light);
        assert_eq!(lit(&mut app).as_deref(), Some("assault"));
        let switch = |app: &mut App, r: usize, f: usize| {
            app.world_mut().resource_mut::<Sim>().0.organizations[r].formation = f;
        };
        // In different formations, none is lit.
        switch(&mut app, 0, 1);
        assert_eq!(lit(&mut app), None);
        // Together again in the one last lit, it stays out...
        switch(&mut app, 0, 0);
        assert_eq!(lit(&mut app), None);
        // ...until they're next all in another.
        switch(&mut app, 0, 1);
        switch(&mut app, 1, 1);
        assert_eq!(lit(&mut app).as_deref(), Some("column"));
    }

    #[test]
    fn a_button_orders_its_formation_unless_its_lit() {
        let sim = with(&["assault", "column"], &["line"]);
        let orders = Orders::hosting(Default::default(), sim.clone(), Some(1), "Player");
        let mut app = App::new();
        app.insert_resource(Sim(sim))
            .insert_resource(Local(Some(1)))
            .insert_resource(Selected(vec![0]))
            .insert_resource(orders)
            .insert_resource(crate::sound::Sounds::of(Default::default(), &[]))
            .init_resource::<Time>()
            .init_resource::<Formations>()
            .add_message::<Intent>()
            .add_systems(Update, (light, press).chain());
        // What the match has done with what was asked, as its formation.
        let formation = |app: &mut App| {
            let world = app.world_mut();
            world.resource_scope(|world, mut orders: Mut<Orders>| {
                orders.step(&mut world.resource_mut::<Sim>().0);
            });
            world.resource::<Sim>().0.organizations[0].formation
        };
        let ask = |app: &mut App, name: &str| {
            app.world_mut().write_message(Intent::from(name));
            lit(app)
        };
        assert_eq!(lit(&mut app).as_deref(), Some("assault"));
        // The lit one, and the group's while one is shown, give nothing.
        assert_eq!(
            ask(&mut app, "CompanyFormationButtons_0").as_deref(),
            Some("assault")
        );
        assert_eq!(
            ask(&mut app, "GroupFormationButtons_1").as_deref(),
            Some("assault")
        );
        assert_eq!(formation(&mut app), 0);
        assert_eq!(
            ask(&mut app, "CompanyFormationButtons_1").as_deref(),
            Some("column")
        );
        assert_eq!(formation(&mut app), 1);
    }

    #[test]
    fn routing_lights_nothing_new() {
        let mut sim = with(&["assault", "column"], &["assault", "column"]);
        sim.organizations[1].owner = 1;
        sim.organizations[0].formation = 1;
        let mut app = App::new();
        app.insert_resource(Sim(sim))
            .insert_resource(Local(Some(1)))
            .insert_resource(Selected(vec![0, 1]))
            .init_resource::<Formations>()
            .add_systems(Update, light);
        assert_eq!(lit(&mut app), None);
        for r in [0, 1] {
            app.world_mut().resource_mut::<Sim>().0.organizations[r].mind = tgsim::Mind::Routing {
                to: None,
                until: u64::MAX,
                retry: u64::MAX,
            };
        }
        assert_eq!(lit(&mut app), None);
        assert_eq!(
            app.world().resource::<Formations>().offered.len(),
            2,
            "still shown"
        );
    }
}
