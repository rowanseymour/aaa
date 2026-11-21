//! The ability column: a button down the floater for each of the player's
//! faction's special operations, in its list's order, with the
//! operation's icon and a bar of how far it's recharged, greyed while it
//! can't be used. What each shows and does is
//! [`special_ops`](crate::special_ops)', shared with ours.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, RelativeCursorPosition};
use bevy::ui_widgets::Button;
use tgdata::widgets::{Rect, Widget};

use super::object::Shaded;
use super::{Drawer, Face, Step, holding, on_screen, reach};
use crate::Sim;
use crate::intent::Asks;
use crate::special_ops::{Operation, Panel, action, probe};
use crate::ui::{Blocks, Hint, Marked};

/// The floater the column is in.
pub(super) const FLOATER: &str = "FactionAbilitiesFloater";
/// The part of it the buttons are listed down.
const LIST: &str = "Abilities";
/// The widget repeated for each operation, and its parts.
pub(super) const ENTRY: &str = "Ability";
const BUTTON: &str = "AbilityButton";
const ICON: &str = "AbilityButtonDecal";
const RECHARGE: &str = "Recharge";
const BAR: &str = "StatusBar";

/// An operation's button, by its place in the faction's list, and whether
/// the side can use it now.
#[derive(Component)]
pub struct Ability {
    index: usize,
    usable: bool,
}

/// An operation's icon, and the grey one shown while it can't be used.
#[derive(Component)]
pub struct AbilityIcon {
    image: Handle<Image>,
    greyed: Option<Handle<Image>>,
}

/// A button's frame picture, shown in the states its layout names.
#[derive(Component)]
pub struct AbilityFrame(Vec<tgdata::widgets::ViewElement>);

/// The bar of how far an operation's recharged, by its button's.
#[derive(Component)]
pub struct Recharge {
    index: usize,
    shaded: Shaded,
}

/// The part of a [`Recharge`] bar that's filled.
#[derive(Component)]
pub struct Charged;

/// An operation's button ready to spawn.
pub(super) struct Built {
    index: usize,
    at: Rect,
    name: Marked,
    frames: Vec<Face>,
    icon: Option<(Rect, Handle<Image>, Option<Handle<Image>>)>,
    bar: Option<(Rect, Color, Shaded)>,
}

/// The column's buttons for `panel`'s operations, the floater being at
/// the end of `within`: one [`ENTRY`] after another down its [`LIST`].
pub(super) fn build(
    floater: &Widget,
    within: &[Step],
    drawer: &mut Drawer,
    panel: Option<&Panel>,
    bindings: &crate::keys::Bindings,
) -> Vec<Built> {
    let (Some(panel), Some(list)) = (panel, floater.find(LIST)) else {
        return Vec::new();
    };
    let Some(entry) = list.find(ENTRY) else {
        return Vec::new();
    };
    let mut list_at = within.to_vec();
    list_at.extend(list.view.as_ref().map(|v| Step::Into(v.bounds)));
    let height = entry.bounds(reach(&list_at)).height();
    (panel.operations.iter().enumerate())
        .map(|(row, op)| {
            let mut at = list_at.clone();
            at.push(Step::By([0.0, row as f32 * height]));
            at.extend(entry.view.as_ref().map(|v| Step::Into(v.bounds)));
            one(entry, &at, op, drawer, bindings)
        })
        .collect()
}

/// The button for `op`, its [`ENTRY`] at the end of `at`.
fn one(
    entry: &Widget,
    at: &[Step],
    op: &Operation,
    drawer: &mut Drawer,
    bindings: &crate::keys::Bindings,
) -> Built {
    let into = |chain: &[Step], w: &Widget| {
        let mut chain = chain.to_vec();
        chain.extend(w.view.as_ref().map(|v| Step::Into(v.bounds)));
        chain
    };
    let mut frames = Vec::new();
    let button = entry.find(BUTTON);
    if let Some(button) = button
        && let Some(view) = &button.view
    {
        drawer.faces(view, &into(at, button), &[], &mut frames);
    }
    let icon = button.and_then(|b| b.find(ICON)).and_then(|decal| {
        let image = drawer.pictures.get(op.icon.as_deref()?)?;
        let greyed = (drawer.pictures.images.get(&image))
            .and_then(super::grey)
            .map(|g| drawer.pictures.images.add(g));
        let chain = into(&into(at, button?), decal);
        Some((reach(&chain), image, greyed))
    });
    let bar = entry.find(RECHARGE).and_then(|bar| {
        let view = bar.view.as_ref()?;
        let [r, g, b] = view.colour;
        let fill = bar
            .parts
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(BAR))?;
        Some((
            reach(&into(at, bar)),
            Color::srgb(r, g, b),
            Shaded::of(fill),
        ))
    });
    Built {
        index: op.index,
        at: reach(at),
        name: bindings.marked(&action(op.index), &op.name),
        frames,
        icon,
        bar,
    }
}

impl Built {
    pub(super) fn spawn(self, screen: &mut ChildSpawnerCommands) {
        let index = self.index;
        let at = self.at;
        // Placed within the button, as a share of its size.
        let within = |inner: Rect| {
            let share = |v: f32, of: f32| Val::Percent(100.0 * v / of.max(1.0));
            Node {
                position_type: PositionType::Absolute,
                left: share(inner.left - at.left, at.width()),
                top: share(inner.top - at.top, at.height()),
                width: share(inner.width().max(0.0), at.width()),
                height: share(inner.height().max(0.0), at.height()),
                ..default()
            }
        };
        screen
            .spawn((
                Ability {
                    index,
                    usable: true,
                },
                Asks::new(action(index)),
                self.name,
                Hint(String::new()),
                on_screen(at),
                Button,
                crate::ui::Click::COMMAND,
                // As ours are, so a script works with either.
                crate::ui::called(format!("special_op.{}", index + 1)),
                Hovered::default(),
                Blocks,
                RelativeCursorPosition::default(),
            ))
            .with_children(|button| {
                for face in self.frames {
                    button.spawn((
                        within(face.at),
                        ImageNode::new(face.image).with_mode(NodeImageMode::Stretch),
                        AbilityFrame(face.when),
                        Visibility::Hidden,
                        Pickable::IGNORE,
                    ));
                }
                if let Some((inner, image, greyed)) = self.icon {
                    button.spawn((
                        within(inner),
                        ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch),
                        AbilityIcon { image, greyed },
                        Pickable::IGNORE,
                    ));
                }
                if let Some((inner, back, shaded)) = self.bar {
                    button
                        .spawn((
                            within(inner),
                            BackgroundColor(back),
                            Recharge { index, shaded },
                            Pickable::IGNORE,
                        ))
                        .with_children(|bar| {
                            bar.spawn((
                                Node {
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(shaded.colour(1.0)),
                                Charged,
                                Pickable::IGNORE,
                            ));
                        });
                }
            });
    }
}

/// Disables the buttons of operations the side can't use now, which
/// its key still asks for, and keeps each button's tooltip to what it does
/// and how long it has left to recharge.
pub fn refresh(
    mut commands: Commands,
    panel: Option<Res<Panel>>,
    sim: Res<Sim>,
    mut buttons: Query<(Entity, &mut Ability, &mut Hint)>,
) {
    let Some(panel) = panel else {
        return;
    };
    for (entity, mut button, mut hint) in &mut buttons {
        let usable = sim.0.check(panel.player, &probe(button.index)).is_ok();
        if button.usable != usable {
            button.usable = usable;
            match usable {
                true => _ = commands.entity(entity).remove::<InteractionDisabled>(),
                false => _ = commands.entity(entity).insert(InteractionDisabled),
            }
        }
        let Some(op) = panel.operations.iter().find(|o| o.index == button.index) else {
            continue;
        };
        let left = (sim.0.special_op(panel.player, button.index)).map_or(0.0, |o| o.left);
        let said = tip(&panel, op, left);
        if hint.0 != said {
            hint.0 = said;
        }
    }
}

/// What an operation's tooltip says under its name: what its effects do,
/// its Cost and Area, how long it has `left` to recharge while it is, and
/// its description, a blank line between each.
fn tip(panel: &Panel, op: &Operation, left: f32) -> String {
    let mut parts = Vec::new();
    let effects: Vec<String> = (op.effects.iter())
        .map(|line| match line.indented {
            true => format!("  {}", line.text),
            false => line.text.clone(),
        })
        .collect();
    parts.push(effects.join("\n"));
    let figures: Vec<String> = (op.figures.iter())
        .map(|(word, value)| format!("{word}  {value}"))
        .collect();
    parts.push(figures.join("\n"));
    if left > 0.0 {
        parts.push(panel.recharging(left));
    }
    parts.extend(op.description.clone());
    parts.retain(|p| !p.is_empty());
    parts.join("\n\n")
}

/// Shows each button's frame for the states the mouse puts it in, none
/// while it can't be used, and its icon greyed then.
#[allow(clippy::type_complexity)]
pub fn show(
    buttons: Query<(&Ability, &Hovered, Has<bevy::ui::Pressed>, &Children)>,
    mut frames: Query<(&AbilityFrame, &mut Visibility)>,
    mut icons: Query<(&AbilityIcon, &mut ImageNode)>,
) {
    for (button, hovered, pressed, children) in &buttons {
        let holding = holding(false, hovered.get(), pressed, !button.usable);
        for &child in children {
            if let Ok((frame, mut shown)) = frames.get_mut(child) {
                let all = frame.0.iter().all(|e| e.shown(&holding));
                shown.set_if_neq(crate::ui::visible(all));
            }
            if let Ok((icon, mut image)) = icons.get_mut(child) {
                let picture = match (button.usable, &icon.greyed) {
                    (false, Some(greyed)) => greyed,
                    _ => &icon.image,
                };
                if image.image != *picture {
                    image.image = picture.clone();
                }
            }
        }
    }
}

/// Fills each recharge bar as far as its operation's recharged: full when
/// it's ready.
pub fn fill(
    panel: Option<Res<Panel>>,
    sim: Res<Sim>,
    bars: Query<(&Recharge, &Children)>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<Charged>>,
) {
    let Some(panel) = panel else {
        return;
    };
    for (bar, children) in &bars {
        let op = sim.0.special_op(panel.player, bar.index);
        let share = op.map_or(1.0, |o| o.charged());
        for &child in children {
            if let Ok((mut node, mut colour)) = fills.get_mut(child) {
                let width = Val::Percent(100.0 * share);
                if node.width != width {
                    node.width = width;
                }
                colour.set_if_neq(BackgroundColor(bar.shaded.colour(share)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::special_ops::tests::{operating, panel};

    #[test]
    fn a_button_greys_once_used_and_its_bar_refills_as_it_recharges() {
        let mut app = App::new();
        app.insert_resource(Sim(operating(450.0)))
            .insert_resource(panel())
            .add_systems(Update, (refresh, fill));
        let shaded = Shaded::of(&tgdata::widgets::ViewElement {
            name: BAR.into(),
            kind: None,
            bounds: Default::default(),
            when: Vec::new(),
            texture: None,
            colour: [1.0; 3],
            text: None,
            font: None,
            justify: None,
            tiles: None,
            children: Vec::new(),
            fields: Vec::new(),
        });
        let charged = app
            .world_mut()
            .spawn((Node::default(), BackgroundColor::DEFAULT, Charged))
            .id();
        app.world_mut()
            .spawn(Recharge { index: 0, shaded })
            .add_child(charged);
        let button = (
            Ability {
                index: 0,
                usable: false,
            },
            Hint(String::new()),
        );
        let button = app.world_mut().spawn(button).id();
        let look = |app: &mut App| {
            app.update();
            let world = app.world();
            let usable = world.get::<Ability>(button).unwrap().usable;
            assert_eq!(world.get::<InteractionDisabled>(button).is_none(), usable);
            let width = world.get::<Node>(charged).unwrap().width;
            let hint = world.get::<Hint>(button).unwrap().0.clone();
            (usable, width, hint.contains("Recharging"))
        };
        assert_eq!(look(&mut app), (true, Val::Percent(100.0), false));
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
        assert_eq!(look(&mut app), (false, Val::Percent(0.0), true));
    }
}
