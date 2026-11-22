//! The group strip along the top: a button for each control group, in
//! fixed places across the floater, showing the group's picture or the
//! layout's own for a group there isn't, with a toggle that opens and
//! closes it. What each shows and does is
//! [`control_groups`](crate::control_groups)', shared with ours.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui_widgets::Button;
use tgdata::widgets::{Rect, Widget};

use super::{Drawer, Press, Step, on_screen, picture, reach};
use crate::Loaded;
use crate::control_groups::{GROUPS, GroupButton, Shown, Strip, StripOpen, TOGGLE_STRIP};
use crate::ui::{Blocks, ImageCache};

/// The floater the strip is in, and its parts.
pub(super) const FLOATER: &str = "GroupFloater";
const TOGGLE: &str = "ShowButton";
const END: &str = "EndDecal";
const GROUP_LIST: &str = "Groups";
const BUTTON: &str = "GroupButton";
/// The field giving the picture of a group there isn't.
const EMPTY: &str = "empty_group_texture";

/// The toggle that opens and closes the strip.
static TOGGLE_PRESS: Press = Press::toggle(
    TOGGLE,
    TOGGLE_STRIP,
    "groups.toggle",
    super::Follows::GroupStrip,
);

/// What's drawn only while the strip is open.
#[derive(Component)]
pub struct WhileOpen;

/// A strip button's picture, and the one it shows for a group there
/// isn't.
#[derive(Component)]
pub struct GroupPicture {
    empty: Option<Handle<Image>>,
}

/// The strip ready to spawn.
pub(super) struct Built {
    buttons: Vec<Rect>,
    empty: Option<Handle<Image>>,
    /// The end cap's pictures.
    cap: Vec<(Vec<Step>, Handle<Image>)>,
}

/// Draws the floater's parts but its buttons, which [`Built`] spawns: its
/// toggle as a button, its end cap where the game has one, and the rest
/// as they are. The floater's own bounds are at the end of `within`.
pub(super) fn build(floater: &Widget, within: &[Step], drawer: &mut Drawer) -> Built {
    let mut built = Built {
        buttons: Vec::new(),
        empty: None,
        cap: Vec::new(),
    };
    for child in &floater.children {
        let name = child.name.as_str();
        if name.eq_ignore_ascii_case(TOGGLE) {
            drawer.pressable(&TOGGLE_PRESS, child, within);
        } else if name.eq_ignore_ascii_case(END) {
            if !drawer.flags.floater_end_cap {
                continue;
            }
            let from = drawer.drawn.len();
            drawer.widget(child, within);
            built.cap = drawer.drawn.split_off(from);
        } else if name.eq_ignore_ascii_case(GROUP_LIST) {
            let Some(chain) = drawer.own(child, within) else {
                continue;
            };
            built.empty = (child.image(EMPTY)).and_then(|path| drawer.pictures.get(&path));
            built.buttons = places(reach(&chain), child.find(BUTTON));
        } else {
            drawer.widget(child, within);
        }
    }
    built
}

/// Where each group's button goes in the list's bounds `at`: a tenth of its
/// width each from its left edge, each as big as `button` says.
fn places(at: Rect, button: Option<&Widget>) -> Vec<Rect> {
    let origin = Rect {
        left: 0.0,
        top: 0.0,
        right: at.width(),
        bottom: at.height(),
    };
    let size = button.map_or(origin, |b| b.bounds(origin));
    let step = at.width() / GROUPS as f32;
    (0..GROUPS)
        .map(|n| {
            let left = at.left + n as f32 * step + size.left;
            let top = at.top + size.top;
            Rect {
                left,
                top,
                right: left + size.width(),
                bottom: top + size.height(),
            }
        })
        .collect()
}

impl Built {
    pub(super) fn spawn(self, screen: &mut ChildSpawnerCommands) {
        for (chain, image) in self.cap {
            screen.spawn((picture(&chain, image), WhileOpen));
        }
        for (n, at) in self.buttons.into_iter().enumerate() {
            screen
                .spawn((
                    GroupButton(n),
                    GroupButton::called(n),
                    on_screen(at),
                    Button,
                    crate::ui::Click::COMMAND,
                    Hovered::default(),
                    Blocks,
                    RelativeCursorPosition::default(),
                    WhileOpen,
                ))
                .with_child((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    ImageNode::default().with_mode(NodeImageMode::Stretch),
                    GroupPicture {
                        empty: self.empty.clone(),
                    },
                    Visibility::Hidden,
                    Pickable::IGNORE,
                ));
        }
    }
}

/// Shows what's drawn only while the strip's open while it is.
pub fn open(open: Option<Res<StripOpen>>, mut shown: Query<&mut Visibility, With<WhileOpen>>) {
    let on = open.is_some_and(|o| o.0);
    for mut visibility in &mut shown {
        visibility.set_if_neq(crate::ui::visible(on));
    }
}

/// Shows each button's group's picture, or the layout's own for a group
/// there isn't.
pub fn show(
    strip: Res<Strip>,
    loaded: Res<Loaded>,
    mut cache: ResMut<ImageCache>,
    mut images: ResMut<Assets<Image>>,
    buttons: Query<(&GroupButton, &Children)>,
    mut pictures: Query<(&GroupPicture, &mut ImageNode, &mut Visibility)>,
) {
    for (button, children) in &buttons {
        for &child in children {
            let Ok((picture, mut image, mut visibility)) = pictures.get_mut(child) else {
                continue;
            };
            let handle = match &strip.0[button.0] {
                Shown::Unassigned => picture.empty.clone(),
                Shown::Group(path) => {
                    (path.as_deref()).and_then(|path| cache.get(&loaded.game, &mut images, path))
                }
            };
            visibility.set_if_neq(crate::ui::visible(handle.is_some()));
            if let Some(handle) = handle
                && image.image != handle
            {
                image.image = handle;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_take_a_tenth_of_the_list_each_from_its_left() {
        use tgdata::widgets::{Bounds, Edge, ViewElement};
        let view = ViewElement {
            name: "View".into(),
            kind: None,
            bounds: Bounds {
                right: Edge::Past(32.0),
                bottom: Edge::Past(32.0),
                ..Bounds::default()
            },
            when: Vec::new(),
            texture: None,
            colour: [1.0; 3],
            text: None,
            font: None,
            justify: None,
            tiles: None,
            children: Vec::new(),
            fields: Vec::new(),
        };
        let button = Widget {
            name: BUTTON.into(),
            kind: None,
            visible: true,
            view: Some(view),
            parts: Vec::new(),
            children: Vec::new(),
            fields: Vec::new(),
        };
        let list = Rect {
            left: 134.0,
            top: 3.0,
            right: 134.0 + 339.0,
            bottom: 40.0,
        };
        let places = places(list, Some(&button));
        assert_eq!(places.len(), GROUPS);
        assert_eq!((places[0].left, places[0].top), (134.0, 3.0));
        assert!((places[9].left - (134.0 + 9.0 * 33.9)).abs() < 1e-3);
        assert_eq!((places[9].width(), places[9].height()), (32.0, 32.0));
    }
}
