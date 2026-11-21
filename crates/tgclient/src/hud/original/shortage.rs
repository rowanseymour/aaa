//! The upkeep shortage icon: its layout's pictures, shown only while the
//! player's side is short of upkeep (see the economy spec), with its
//! tooltip in the docked pane.

use bevy::prelude::*;
use tgdata::widgets::Widget;

use super::{Drawer, Step, picture};
use crate::ui::{Hint, Marked};
use crate::{Local, Sim};

/// The icon's widget.
pub(super) const WIDGET: &str = "UpkeepShortageIcon";

/// What holds the icon's pictures, shown while the side's short.
#[derive(Component)]
pub struct Shortage;

/// The icon ready to spawn: its pictures, and what its tooltip says.
pub(super) struct Built {
    pictures: Vec<(Vec<Step>, Handle<Image>)>,
    name: Option<String>,
    help: String,
}

/// The icon `widget` lays out.
pub(super) fn build(widget: &Widget, drawer: &mut Drawer) -> Built {
    let kept = std::mem::take(&mut drawer.drawn);
    drawer.widget(widget, &[]);
    let pictures = std::mem::replace(&mut drawer.drawn, kept);
    let (name, help) = widget.tooltip(&drawer.pictures.game.strings());
    Built {
        pictures,
        name,
        help: help.unwrap_or_default(),
    }
}

impl Built {
    pub(super) fn spawn(self, screen: &mut ChildSpawnerCommands) {
        let mut holder = screen.spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            Shortage,
            Visibility::Hidden,
            Pickable::IGNORE,
        ));
        holder.with_children(|holder| {
            for (chain, image) in self.pictures {
                let mut icon = holder.spawn((picture(&chain, image), Hint(self.help.clone())));
                if let Some(name) = &self.name {
                    icon.insert(Marked::plain(name.clone()));
                }
            }
        });
    }
}

/// Shows the icon while the player's side is short of upkeep.
pub fn show(sim: Res<Sim>, local: Res<Local>, mut icons: Query<&mut Visibility, With<Shortage>>) {
    let short = local.0.is_some_and(|p| sim.0.short(p).is_some());
    for mut shown in &mut icons {
        shown.set_if_neq(crate::ui::visible(short));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_shows_only_while_the_side_is_short() {
        let mut app = App::new();
        let mut world = crate::special_ops::tests::operating(0.0);
        app.insert_resource(Local(Some(1)))
            .add_systems(Update, show);
        let icon = app.world_mut().spawn((Shortage, Visibility::Hidden)).id();
        let shown = |app: &mut App, world: &tgsim::World| {
            app.insert_resource(Sim(world.clone()));
            app.update();
            app.world().get::<Visibility>(icon) == Some(&Visibility::Inherited)
        };
        assert!(!shown(&mut app, &world));
        world.treasuries.entry(1).or_default().shortage = Some(12.0);
        assert!(shown(&mut app, &world));
    }
}
