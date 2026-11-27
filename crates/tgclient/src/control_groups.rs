//! The player's control groups: what's selected saved under a digit and
//! selected again by it, and the Group Behavior switch on the selection's
//! current group, which has orders go to the selection as one group. Also
//! what the group strip's buttons show and do, which each interface
//! draws its own way.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgrender::camera::RtsCamera;
use tgrender::space::world;
use tgsim::World;

use crate::intent::{Asked, Intent};
use crate::keys::Bindings;
use crate::object_display::Looks;
use crate::ui::{Hint, Marked};
use crate::{Drawn, Loaded, Local, Selected, SelectedBuilding, Sim};

/// What the hotkey files ask for to save and select group n, as
/// `SaveGroup group_1`; the last is `group_10`.
const SAVE: &str = "SaveGroup group_";
const SELECT: &str = "SelectGroup group_";

/// How many groups the game's keys save, each with a button on the strip.
pub const GROUPS: usize = 10;

/// What asks to open or close the group strip.
pub const TOGGLE_STRIP: &str = "ToggleGroupStrip";

/// What saves group `n`, from 0.
fn save_group(n: usize) -> String {
    format!("{SAVE}{}", n + 1)
}

/// What selects group `n`, from 0.
pub fn select_group(n: usize) -> String {
    format!("{SELECT}{}", n + 1)
}

/// How soon a second press selecting a group still selected looks at it
/// instead, in seconds (`UICenterGroupTimeMS`).
const CENTRE_AGAIN: f64 = 1.0;

/// The commands that turn Group Behavior on and off.
pub const ENABLE: &str = "group_behavior_enable";
pub const DISABLE: &str = "group_behavior_disable";

/// One of the player's saved groups.
#[derive(Clone, Default, PartialEq, Debug)]
struct Saved {
    /// By index into the sim's organizations.
    organizations: Vec<usize>,
    /// A building saved on its own.
    building: Option<usize>,
    /// Its Group Behavior switch.
    behavior: bool,
}

impl Saved {
    fn is_empty(&self) -> bool {
        self.organizations.is_empty() && self.building.is_none()
    }

    /// Whether it holds exactly the organizations `selected`.
    fn is(&self, selected: &[usize]) -> bool {
        self.building.is_none()
            && self.organizations.len() == selected.len()
            && selected.iter().all(|r| self.organizations.contains(r))
    }
}

/// The player's saved groups, and the switch of the one group that stands
/// in for any other selection of two or more.
#[derive(Resource, Clone, Default, Debug)]
pub struct ControlGroups {
    saved: [Saved; GROUPS],
    stand_in: bool,
    /// The group last selected, and when.
    selected: Option<(usize, f64)>,
}

impl ControlGroups {
    /// What's left of group `n`: its organizations, or its building; `None`
    /// once there's nothing.
    fn left(&self, sim: &World, n: usize) -> Option<(Vec<usize>, Option<usize>)> {
        let group = &self.saved[n];
        let organizations: Vec<usize> = (group.organizations.iter())
            .copied()
            .filter(|&r| !sim.destroyed(r))
            .collect();
        let building = group.building.filter(|&b| sim.buildings[b].standing());
        (!organizations.is_empty() || building.is_some()).then_some((organizations, building))
    }

    /// The Group Behavior switch of `selected`'s current group: the saved
    /// group that's exactly it, else the stand-in; `None` for fewer than
    /// two, which have no group.
    pub fn behavior(&self, selected: &[usize]) -> Option<bool> {
        (selected.len() >= 2).then(|| match self.saved.iter().find(|g| g.is(selected)) {
            Some(g) => g.behavior,
            None => self.stand_in,
        })
    }

    /// The number of the saved group that's exactly `selected`, if any;
    /// otherwise its group is the stand-in.
    pub fn selection(&self, selected: &[usize]) -> Option<u8> {
        (self.saved.iter().position(|g| g.is(selected))).and_then(|n| u8::try_from(n).ok())
    }

    /// Whether orders to `selected` go to it as one group.
    pub fn grouping(&self, selected: &[usize]) -> bool {
        self.behavior(selected) == Some(true)
    }

    /// Flips the switch of `selected`'s current group.
    pub fn toggle(&mut self, selected: &[usize]) {
        if selected.len() < 2 {
            return;
        }
        match self.saved.iter_mut().find(|g| g.is(selected)) {
            Some(g) => g.behavior = !g.behavior,
            None => self.stand_in = !self.stand_in,
        }
    }

    /// Saves the player's own `organizations`, or else their `building`,
    /// as group `n`: whatever it held goes, and what's saved leaves any
    /// other group, a group left empty going. It takes the switch of the
    /// group it was saved from.
    fn save(&mut self, n: usize, organizations: Vec<usize>, building: Option<usize>) {
        let behavior = self.behavior(&organizations).unwrap_or(false);
        let building = building.filter(|_| organizations.is_empty());
        for g in &mut self.saved {
            g.organizations.retain(|r| !organizations.contains(r));
            if building.is_some() && g.building == building {
                g.building = None;
            }
            if g.is_empty() {
                *g = Saved::default();
            }
        }
        self.saved[n] = Saved {
            organizations,
            building,
            behavior,
        };
    }
}

/// What saving and selecting groups reads and changes.
#[derive(SystemParam)]
pub struct Recall<'w, 's> {
    groups: ResMut<'w, ControlGroups>,
    selected: ResMut<'w, Selected>,
    chosen: ResMut<'w, SelectedBuilding>,
    sim: Res<'w, Sim>,
    local: Res<'w, Local>,
    loaded: Res<'w, Loaded>,
    time: Res<'w, Time<Real>>,
    cameras: Query<'w, 's, &'static mut RtsCamera>,
}

/// The group, from 0, that `intent` names after `prefix`.
fn numbered(intent: &Intent, prefix: &str) -> Option<usize> {
    let n: usize = intent.0.strip_prefix(prefix)?.parse().ok()?;
    (1..=GROUPS).contains(&n).then_some(n - 1)
}

/// Saves and selects groups as the keys ask.
pub fn recall(mut intents: MessageReader<Intent>, mut recall: Recall) {
    let asked = Asked::read(&mut intents);
    let Some(player) = recall.local.0 else {
        return;
    };
    for intent in asked.all() {
        if let Some(n) = numbered(intent, SAVE) {
            let sim = &recall.sim.0;
            let own = |&r: &usize| sim.organizations[r].owner == player;
            let organizations = recall.selected.0.iter().copied().filter(own).collect();
            let building = recall
                .chosen
                .0
                .filter(|&b| sim.buildings[b].owner == player);
            recall.groups.save(n, organizations, building);
        } else if let Some(n) = numbered(intent, SELECT) {
            recall.select(n);
        }
    }
}

impl Recall<'_, '_> {
    /// Selects what's left of group `n`, or a second time soon after
    /// while it's still exactly what's selected, looks at it instead.
    fn select(&mut self, n: usize) {
        let sim = &self.sim.0;
        let now = self.time.elapsed_secs_f64();
        let group = &mut self.groups.saved[n];
        group.organizations.retain(|&r| !sim.destroyed(r));
        if group.building.is_some_and(|b| !sim.buildings[b].standing()) {
            group.building = None;
        }
        if group.is_empty() {
            return;
        }
        let again = self
            .groups
            .selected
            .is_some_and(|(m, at)| m == n && now - at <= CENTRE_AGAIN);
        let group = &self.groups.saved[n];
        let still =
            group.is(&self.selected.0) || group.building.is_some_and(|b| self.chosen.0 == Some(b));
        if again && still {
            let n = group.organizations.len() + usize::from(group.building.is_some());
            let at = (group.organizations.iter().map(|&r| sim.centre(r)))
                .chain(group.building.map(|b| sim.buildings[b].position))
                .fold([0.0, 0.0], |[x, y], [a, b]| {
                    [x + a / n as f32, y + b / n as f32]
                });
            let [x, y] = at;
            for mut camera in &mut self.cameras {
                camera.focus = world(x, y, self.loaded.map.ground.height_at([x, y]));
            }
        } else {
            self.selected.0 = group.organizations.clone();
            self.chosen.0 = group.building;
        }
        self.groups.selected = Some((n, now));
    }
}

/// What a button on the group strip shows.
#[derive(Clone, Default, PartialEq, Debug)]
pub enum Shown {
    /// There's no such group.
    #[default]
    Unassigned,
    /// The archive path of the icon standing for the group, if any of its
    /// things has one.
    Group(Option<String>),
}

/// What each button on the group strip shows, in group order.
#[derive(Resource, Default, PartialEq, Debug)]
pub struct Strip(pub [Shown; GROUPS]);

/// Whether the group strip is open, kept from one match to the next; as
/// the game's settings say until the first.
#[derive(Resource, Clone, Copy, PartialEq, Debug)]
pub struct StripOpen(pub bool);

/// Opens the strip as the game's settings say, for the first match.
pub fn open_strip(mut commands: Commands, open: Option<Res<StripOpen>>, loaded: Res<Loaded>) {
    if open.is_none() {
        commands.insert_resource(StripOpen(loaded.game.group_strip_open()));
    }
}

/// A button on the group strip, by its group from 0: a left click selects
/// the group as its key does, so a double-click looks at it, and a right
/// click saves the selection as it.
#[derive(Component, Clone, Copy, Debug)]
#[require(Hint = Hint(String::new()), Marked)]
pub struct GroupButton(pub usize);

impl GroupButton {
    /// What it's [`called`](crate::ui::called), the same in either
    /// interface so a script works with both.
    pub fn called(n: usize) -> crate::ui::ButtonId {
        crate::ui::called(format!("group.{}", n + 1))
    }
}

/// Opens or closes the strip as asked.
pub fn toggle_strip(mut intents: MessageReader<Intent>, open: Option<ResMut<StripOpen>>) {
    let Some(mut open) = open else { return };
    if Asked::read(&mut intents).all().any(|i| i.0 == TOGGLE_STRIP) {
        open.0 = !open.0;
    }
}

/// Saves or selects a group as its button's pressed: let go of by the
/// right mouse button, it saves, and by the middle, nothing.
pub fn press(
    activate: On<Activate>,
    buttons: Query<&GroupButton>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut intents: MessageWriter<Intent>,
) {
    let Ok(&GroupButton(n)) = buttons.get(activate.entity) else {
        return;
    };
    if mouse.just_released(MouseButton::Middle) {
        return;
    }
    match mouse.just_released(MouseButton::Right) {
        true => intents.write(Intent(save_group(n))),
        false => intents.write(Intent(select_group(n))),
    };
}

/// Works out what each button on the strip shows, afresh each frame, so
/// it follows deaths and groups saved over at once.
pub fn picture(
    groups: Res<ControlGroups>,
    sim: Res<Sim>,
    looks: Option<Res<Looks>>,
    drawn: Option<Res<Drawn>>,
    mut strip: ResMut<Strip>,
) {
    let (Some(looks), Some(drawn)) = (looks, drawn) else {
        return;
    };
    let sim = &sim.0;
    let shown = std::array::from_fn(|n| match groups.left(sim, n) {
        None => Shown::Unassigned,
        Some((organizations, building)) => {
            let things = (organizations
                .iter()
                .map(|&r| organization_template(sim, &looks, r)))
            .chain(building.map(|b| Some(sim.buildings[b].template.clone())));
            let offered: Vec<Option<&str>> = things
                .map(|t| t.and_then(|t| drawn.0.icon(t.as_str())))
                .collect();
            Shown::Group(chosen(&offered).map(str::to_string))
        }
    });
    if strip.0 != shown {
        strip.0 = shown;
    }
}

/// The template whose icon an organization puts forward: a lone unit's
/// own, else that of the unit type standing for its template.
fn organization_template(sim: &World, looks: &Looks, r: usize) -> Option<tgdata::Id> {
    let organization = &sim.organizations[r];
    match crate::object_display::lone(sim, r) {
        true => Some(organization.template.clone()),
        false => looks.standing_for.get(&organization.template).cloned(),
    }
}

/// Of the pictures `offered` by a group's things, in the group's order,
/// the first offered by at least half of them, else the first offered.
fn chosen<'a>(offered: &[Option<&'a str>]) -> Option<&'a str> {
    let half = offered.len().div_ceil(2);
    let count = |p: &str| offered.iter().filter(|o| **o == Some(p)).count();
    let pictures = || offered.iter().flatten().copied();
    pictures()
        .find(|p| count(p) >= half)
        .or_else(|| pictures().next())
}

/// Keeps each strip button's tooltip to its keys: the one that selects its
/// group, while there is one, and the one that saves it.
pub fn tips(
    strip: Res<Strip>,
    bindings: Res<Bindings>,
    loaded: Res<Loaded>,
    mut buttons: Query<(Ref<GroupButton>, &mut Marked, &mut Hint)>,
) {
    let fresh = buttons.iter().any(|(b, ..)| b.is_added());
    if !(fresh || strip.is_changed() || bindings.is_changed()) {
        return;
    }
    let strings = loaded.game.strings();
    let said = crate::ui::said(&strings);
    let title = Marked::plain(said("groups_tooltip_title", "Groups"));
    let key = |action: &str| bindings.key(action).map(|k| k.bracketed());
    for (button, mut marked, mut hint) in &mut buttons {
        let n = button.0;
        let assigned = strip.0[n] != Shown::Unassigned;
        let says = tip(said, key, n, assigned);
        marked.set_if_neq(title.clone());
        if hint.0 != says {
            hint.0 = says;
        }
    }
}

/// What group `n`'s button's tooltip says under its title, given the
/// game's text and how each key's written: the key selecting it while
/// it's `assigned`, the key saving it, then how the mouse works it.
fn tip(
    said: impl Fn(&str, &str) -> String,
    key: impl Fn(&str) -> Option<String>,
    n: usize,
    assigned: bool,
) -> String {
    let keyed =
        |text: String, action: String| text.replacen("%s", &key(&action).unwrap_or_default(), 1);
    let mut lines = Vec::new();
    if assigned {
        lines.push(keyed(
            said("hotkey_select_group|s", "%s Select Group"),
            select_group(n),
        ));
    }
    lines.push(keyed(
        said("hotkey_save_group|s", "%s Save Group"),
        save_group(n),
    ));
    lines.push(said(
        "groups_tooltip_description",
        "Right-Click on a group button to save.  Double-Click on a group button to center.",
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_group_has_its_own_switch_and_any_other_selection_the_stand_ins() {
        let mut groups = ControlGroups::default();
        assert_eq!(groups.behavior(&[3]), None);
        assert_eq!(groups.behavior(&[3, 4]), Some(false));
        groups.toggle(&[3, 4]);
        // One switch for every unsaved selection.
        assert!(groups.grouping(&[5, 6, 7]));
        // Saved from it, a group takes its switch, then keeps its own.
        groups.save(0, vec![3, 4], None);
        assert!(groups.grouping(&[4, 3]));
        groups.toggle(&[3, 4]);
        assert!(!groups.grouping(&[3, 4]) && groups.grouping(&[5, 6]));
        // Orders to it go to that saved group, and any other's to the stand-in.
        assert_eq!(groups.selection(&[4, 3]), Some(0));
        assert_eq!(groups.selection(&[3]), None);
    }

    #[test]
    fn a_thing_is_in_one_group_at_most() {
        let mut groups = ControlGroups::default();
        groups.save(0, vec![1, 2], None);
        groups.save(1, vec![2, 3], None);
        assert_eq!(groups.saved[0].organizations, [1]);
        groups.save(2, vec![1], None);
        assert!(groups.saved[0].is_empty());
        assert_eq!(
            numbered(&Intent::from("SelectGroup group_10"), SELECT),
            Some(9)
        );
        assert_eq!(
            numbered(&Intent::from("SelectGroup group_11"), SELECT),
            None
        );
        assert_eq!(numbered(&Intent(save_group(9)), SAVE), Some(9));
    }

    #[test]
    fn a_group_shows_the_picture_half_its_things_put_forward() {
        let (a, b, c) = (Some("a"), Some("b"), Some("c"));
        // Half, rounded up, of three is two, wherever they come.
        assert_eq!(chosen(&[a, b, b]), Some("b"));
        assert_eq!(chosen(&[a, b, a, b]), Some("a"));
        // A thing with none still counts towards the half.
        assert_eq!(chosen(&[None, a, b, b]), Some("b"));
        // With none offered by half, the first offered.
        assert_eq!(chosen(&[None, None, b, c, a]), Some("b"));
        assert_eq!(chosen(&[None]), None);
    }

    #[test]
    fn a_buttons_tooltip_gives_the_keys_selecting_its_group_and_saving_it() {
        let said = |_: &str, or: &str| or.to_string();
        let key = |action: &str| match action {
            "SelectGroup group_2" => Some("(2)".to_string()),
            "SaveGroup group_2" => Some("(Ctrl-2)".to_string()),
            _ => None,
        };
        let tip = |assigned| tip(said, key, 1, assigned);
        let lines = |t: String| t.lines().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(
            lines(tip(true))[..2],
            ["(2) Select Group", "(Ctrl-2) Save Group"]
        );
        // Without a group, nothing to select.
        assert_eq!(lines(tip(false))[0], "(Ctrl-2) Save Group");
        assert_eq!(lines(tip(false)).len(), 2);
    }

    #[test]
    fn a_strip_button_selects_its_group_but_saves_it_let_go_of_by_the_right() {
        let mut app = App::new();
        app.add_message::<Intent>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_observer(press);
        let entity = app.world_mut().spawn(GroupButton(2)).id();
        let pressed = |app: &mut App| {
            app.world_mut().trigger(Activate { entity });
            let intents = app.world_mut().resource_mut::<Messages<Intent>>();
            intents
                .iter_current_update_messages()
                .map(|i| i.0.clone())
                .last()
        };
        assert_eq!(pressed(&mut app).as_deref(), Some("SelectGroup group_3"));
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Right);
        mouse.release(MouseButton::Right);
        assert_eq!(pressed(&mut app).as_deref(), Some("SaveGroup group_3"));
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.clear();
        mouse.press(MouseButton::Middle);
        mouse.release(MouseButton::Middle);
        app.world_mut().resource_mut::<Messages<Intent>>().clear();
        assert_eq!(pressed(&mut app), None);
    }

    #[test]
    fn a_group_is_gone_once_nothing_in_it_is_left() {
        let mut sim = crate::orders::testing::fresh();
        let mut groups = ControlGroups::default();
        groups.save(0, vec![0], None);
        assert_eq!(groups.left(&sim, 0), Some((vec![0], None)));
        assert_eq!(groups.left(&sim, 1), None);
        let units = sim.organizations[0].units.clone();
        for u in units {
            sim.units[u].died = Some(0);
        }
        assert_eq!(groups.left(&sim, 0), None);
    }
}
