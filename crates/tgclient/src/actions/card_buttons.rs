//! The buttons on an organization's card, which give its order at once.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::intent::Intent;
use crate::ui::{Hint, Marked};
use crate::{Local, Sim};

use super::layout::can;
use super::looks::Looks;
use super::{Give, ORDERS};

/// The widget of the button under each regiment's card in the group
/// display, and, with the regiment's id after it, what pressing it asks
/// for, e.g. `RetreatRoutButton 12`.
pub const CARD_BUTTON: &str = "RetreatRoutButton";

/// What the button under a regiment's card gives that regiment alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CardOrder {
    Run,
    Rout,
}

impl CardOrder {
    /// The command whose name and help it borrows.
    fn command(self) -> &'static str {
        match self {
            CardOrder::Run => "retreat",
            CardOrder::Rout => "rout",
        }
    }
}

/// What the button under the card of the organization at `r` gives it:
/// Run while `player` could have it run, else Rout while it could rout,
/// so nothing for another's regiment or one routing or Exhausted.
pub fn card_order(sim: &tgsim::World, player: Option<u32>, r: usize) -> Option<CardOrder> {
    let player = player?;
    if crate::object_display::lone(sim, r) {
        return None;
    }
    // Where to doesn't decide whether it may run.
    let run = tgsim::Command::Run {
        organization: r,
        to: [0.0; 2],
    };
    if sim.check(player, &run).is_ok() {
        Some(CardOrder::Run)
    } else if can(sim, Some(player), Give::Rout, r) {
        Some(CardOrder::Rout)
    } else {
        None
    }
}

/// The button under a regiment's card, of either interface, for the
/// organization at this index.
#[derive(Component)]
#[require(Hint = Hint(String::new()), Marked, Hovered)]
pub struct CardButton(pub usize);

/// The one organization the order being aimed goes to instead of the
/// selection, as one aimed from its card's button.
#[derive(Resource, Default)]
pub struct AimedFor(pub Option<usize>);

/// Shows each [`CardButton`] while it has an order to give, with that
/// order's name and help as its tooltip.
pub fn show_card_buttons(
    sim: Res<Sim>,
    local: Res<Local>,
    looks: Res<Looks>,
    mut buttons: Query<(&CardButton, &mut Visibility, &mut Hint, &mut Marked)>,
) {
    for (button, mut shown, mut hint, mut name) in &mut buttons {
        let order = card_order(&sim.0, local.0, button.0);
        shown.set_if_neq(crate::ui::visible(order.is_some()));
        let Some(order) = order else {
            continue;
        };
        let Some(i) = ORDERS.iter().position(|(id, _)| *id == order.command()) else {
            continue;
        };
        let look = &looks.orders[i].1;
        // A blank line between its name and what it does.
        let help = format!("\n{}", look.help.as_deref().unwrap_or_default());
        if hint.0 != help {
            hint.0 = help;
        }
        name.set_if_neq(Marked::plain(look.name.clone()));
    }
}

/// A [`CardButton`] a mouse button other than the left went down on,
/// whose activation as it's let go gives no order.
#[derive(Component)]
pub struct NotLeft;

/// Notes whether it was the left mouse button that went down on a
/// [`CardButton`].
pub fn hold_card_button(
    press: On<PointerPress>,
    buttons: Query<(), With<CardButton>>,
    mut commands: Commands,
) {
    if buttons.contains(press.entity) {
        let mut button = commands.entity(press.entity);
        match press.button {
            PointerButton::Primary => _ = button.remove::<NotLeft>(),
            _ => _ = button.insert(NotLeft),
        }
    }
}

/// Forgets a press once the mouse buttons are let go, wherever, so one
/// that came to nothing doesn't hold back the next activation, such as
/// the console's.
pub fn let_go_card_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    held: Query<Entity, With<NotLeft>>,
    mut commands: Commands,
) {
    if !mouse.any_pressed([MouseButton::Right, MouseButton::Middle]) {
        held.iter()
            .for_each(|b| _ = commands.entity(b).remove::<NotLeft>());
    }
}

/// Asks for a [`CardButton`]'s order as it's activated, unless by a mouse
/// button other than the left, which gives none.
pub fn press_card_button(
    activate: On<Activate>,
    buttons: Query<(&CardButton, Has<NotLeft>)>,
    // Absent outside a match, where menu buttons still trigger this.
    sim: Option<Res<Sim>>,
    mut intents: MessageWriter<Intent>,
) {
    let Some(sim) = sim else { return };
    if let Ok((button, false)) = buttons.get(activate.entity) {
        let id = sim.0.organizations[button.0].id;
        intents.write(Intent(format!("{CARD_BUTTON} {id}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cards_button_runs_or_else_routs_the_players_own_regiment() {
        let mut sim = crate::orders::testing::fresh();
        assert_eq!(card_order(&sim, Some(1), 0), Some(CardOrder::Run));
        // Not for another's, or with no player.
        assert_eq!(card_order(&sim, Some(1), 1), None);
        assert_eq!(card_order(&sim, None, 0), None);
        // Fighting with too little morale to run, it routs.
        let r = &mut sim.organizations[0];
        (r.morale_max, r.morale, r.speed) = (100.0, 0.5, 2.0);
        r.engaged.push(tgsim::Piece::Organization(1));
        assert_eq!(card_order(&sim, Some(1), 0), Some(CardOrder::Rout));
        // Exhausted, it has neither.
        sim.organizations[0].mind = tgsim::Mind::Exhausted;
        assert_eq!(card_order(&sim, Some(1), 0), None);
    }

    #[test]
    fn a_button_pressed_outside_a_match_is_ignored() {
        let mut app = App::new();
        app.add_message::<Intent>().add_observer(press_card_button);
        let button = app.world_mut().spawn_empty().id();
        app.world_mut().trigger(Activate { entity: button });
        app.world_mut().flush();
        assert!(app.world().resource::<Messages<Intent>>().is_empty());
    }

    #[test]
    fn only_a_left_press_on_a_cards_button_asks_for_its_order() {
        let sim = crate::orders::testing::fresh();
        let id = sim.organizations[0].id;
        let mut app = App::new();
        app.insert_resource(Sim(sim))
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<Intent>()
            .add_observer(press_card_button)
            .add_systems(Update, let_go_card_buttons);
        let button = app.world_mut().spawn(CardButton(0)).id();
        let activate = |app: &mut App| {
            app.world_mut().trigger(Activate { entity: button });
            app.world_mut().flush();
            let mut asked = app.world_mut().resource_mut::<Messages<Intent>>();
            asked.drain().map(|i| i.0).collect::<Vec<_>>()
        };
        assert_eq!(activate(&mut app), [format!("{CARD_BUTTON} {id}")]);
        // A right or middle press gives nothing...
        app.world_mut().entity_mut(button).insert(NotLeft);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        assert!(activate(&mut app).is_empty());
        // ...and once let go, it doesn't hold back the next activation.
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Right);
        app.update();
        assert_eq!(activate(&mut app), [format!("{CARD_BUTTON} {id}")]);
    }
}
