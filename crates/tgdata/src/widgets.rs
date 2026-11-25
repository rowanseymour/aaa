//! The original interface's layouts, from the `.tgi` files under `UI/`.
//!
//! Each screen is a tree of widgets. A widget is a block whose templates
//! lead back to `Widget` in `Templates/UI/template_widget.tgi`; its
//! `[View]` block, a view element, gives its bounds within its parent's and
//! what's drawn there, and other view-element blocks it holds (such as a
//! `DecalWidget`'s `[Decal]`) are drawn within those bounds. View elements
//! lead back to `ViewElement` in `Templates/UI/template_ve.tgi`, and a
//! group's `[Child]` blocks are view elements drawn within the group's.
//!
//! The files say how widgets look, not what they do: which widgets a screen
//! shows, and what each does, goes by their names.
//!
//! Bounds are four edges, `L`, `T`, `R` and `B`, each one of:
//!
//! - `10`: that far from the parent's left or top.
//! - `10r`, `10b`: that far in from the parent's right or bottom.
//! - `20%`: that fraction of the way across or down the parent.
//! - `+50` (right and bottom only): that far past the left or top edge;
//!   `+50%`, that fraction of the parent's width or height past it.
//!
//! Positions are in pixels of a 1024 by 768 screen.
//!
//! A view element's `when` names the states it's drawn in, joined by `+`,
//! all of which must hold, e.g. `FOCUSED + UNSELECTED`.
//!
//! A label (`LabelVE`) names its `font` by the font's `ids` (see
//! [`GameData::font`]) and how its lines `justify`. A tiled decal
//! (`DecalVETiled`) fills its bounds from six images: `corners_texture`,
//! whose quarters are the four corners, the four `*_border_texture`s, each
//! repeated along its edge, and `center_texture`, repeated over the rest.

use std::collections::HashSet;
use std::sync::Arc;

use crate::messages::colour;
use crate::{GameData, ui_path};

/// The size of the screen the layouts are written for.
pub const SCREEN: [f32; 2] = [1024.0, 768.0];

/// A widget, as its layout has it.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub name: String,
    /// What kind of widget it is, e.g. `PUSH_BUTTON`, from its templates.
    pub kind: Option<String>,
    pub visible: bool,
    /// Its bounds, and what's drawn behind everything else of it.
    pub view: Option<ViewElement>,
    /// Its other view elements, such as a decal widget's decal, in the
    /// order written.
    pub parts: Vec<ViewElement>,
    /// Its widgets, in the order written.
    pub children: Vec<Widget>,
    /// Its fields, inherited ones included, for settings of its own kind
    /// such as a resource bar's `hidden_offset`.
    pub fields: Vec<(String, String)>,
}

impl Widget {
    /// One of its [`fields`](Widget::fields), by key, case-insensitive.
    pub fn value(&self, key: &str) -> Option<&str> {
        field(&self.fields, key)
    }

    /// One of its fields naming an image, as an archive path.
    pub fn image(&self, key: &str) -> Option<String> {
        self.value(key).and_then(ui_path)
    }

    /// What a tooltip says of it: its name, and what it does.
    pub fn tooltip(&self, strings: &crate::Strings) -> (Option<String>, Option<String>) {
        let said = |key| crate::strings::localized(strings, self.value(key));
        (said("ToolTip_name"), said("tooltip"))
    }

    /// Its bounds, given its parent's.
    pub fn bounds(&self, parent: Rect) -> Rect {
        (self.view.as_ref()).map_or(parent, |v| v.bounds.place(parent))
    }

    /// The images it and its widgets draw at rest, in the order drawn.
    pub fn resting(&self) -> Vec<&str> {
        fn element<'a>(e: &'a ViewElement, out: &mut Vec<&'a str>) {
            if !e.shown(&RESTING) {
                return;
            }
            out.extend(e.texture.as_deref());
            for child in &e.children {
                element(child, out);
            }
        }
        let mut out = Vec::new();
        for e in self.view.iter().chain(&self.parts) {
            element(e, &mut out);
        }
        for child in self.children.iter().filter(|c| c.visible) {
            out.extend(child.resting());
        }
        out
    }

    /// The widget among its descendants, at any depth, named `name`.
    pub fn find(&self, name: &str) -> Option<&Widget> {
        (self.children.iter()).find_map(|c| match c.name.eq_ignore_ascii_case(name) {
            true => Some(c),
            false => c.find(name),
        })
    }
}

/// Something a widget draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewElement {
    /// Its block's kind, e.g. `View`, `Decal` or `Child`.
    pub name: String,
    /// What kind it is, e.g. `DECAL`, `LABEL` or `GROUP`, from its
    /// templates.
    pub kind: Option<String>,
    pub bounds: Bounds,
    /// The states it's drawn in, each of which must hold; in any, if empty.
    pub when: Vec<String>,
    /// The archive path of a decal's image.
    pub texture: Option<String>,
    pub colour: [f32; 3],
    /// What a label says: a key into the strings if it starts with `#`.
    pub text: Option<String>,
    /// A label's font, by its `ids`.
    pub font: Option<String>,
    /// How a label's lines line up, e.g. `left`.
    pub justify: Option<String>,
    /// A tiled decal's images.
    pub tiles: Option<Tiles>,
    /// A group's view elements, in the order written.
    pub children: Vec<ViewElement>,
    /// Its fields, inherited ones included, for settings of its own kind
    /// such as a status bar's `end_color`.
    pub fields: Vec<(String, String)>,
}

impl ViewElement {
    /// One of its [`fields`](ViewElement::fields), by key, case-insensitive.
    pub fn value(&self, key: &str) -> Option<&str> {
        field(&self.fields, key)
    }

    /// One of its fields naming an image, as an archive path.
    pub fn image(&self, key: &str) -> Option<String> {
        self.value(key).and_then(ui_path)
    }

    /// One of its fields giving a colour.
    pub fn colour_of(&self, key: &str) -> Option<[f32; 3]> {
        self.value(key).and_then(colour)
    }

    /// Whether it's drawn while the states in `holding` hold, upper-case.
    pub fn shown(&self, holding: &[&str]) -> bool {
        (self.when.iter()).all(|w| holding.iter().any(|h| w.eq_ignore_ascii_case(h)))
    }
}

/// The archive paths of a tiled decal's images.
#[derive(Debug, Clone, PartialEq)]
pub struct Tiles {
    pub corners: String,
    pub left: String,
    pub right: String,
    pub top: String,
    pub bottom: String,
    pub center: String,
}

/// The states a widget is in when nothing's been done to it.
pub const RESTING: [&str; 6] = [
    "ENABLED",
    "UNFOCUSED",
    "UNPRESSED",
    "UNSELECTED",
    "UNTARGETED",
    "CLOSED",
];

/// A rectangle on the screen, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

/// Where each of a rectangle's edges goes within its parent's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub left: Edge,
    pub top: Edge,
    pub right: Edge,
    pub bottom: Edge,
}

impl Default for Bounds {
    /// The whole of the parent.
    fn default() -> Self {
        Bounds {
            left: Edge::Start(0.0),
            top: Edge::Start(0.0),
            right: Edge::End(0.0),
            bottom: Edge::End(0.0),
        }
    }
}

impl Bounds {
    /// Where it is, within `parent`.
    pub fn place(&self, parent: Rect) -> Rect {
        let left = self.left.place(parent.left, parent.right, parent.left);
        let top = self.top.place(parent.top, parent.bottom, parent.top);
        Rect {
            left,
            top,
            right: self.right.place(parent.left, parent.right, left),
            bottom: self.bottom.place(parent.top, parent.bottom, top),
        }
    }
}

/// Where one edge goes within its parent's span (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Start(f32),
    End(f32),
    Fraction(f32),
    /// Past the opposite edge, in pixels.
    Past(f32),
    /// Past the opposite edge, as a fraction of the parent's span.
    PastFraction(f32),
}

impl Edge {
    /// Reads a value as written, e.g. `257r` or `+50%`.
    pub fn parse(value: &str) -> Option<Edge> {
        let value = value.trim();
        let (past, value) = match value.strip_prefix('+') {
            Some(rest) => (true, rest.trim()),
            None => (false, value),
        };
        let number = |s: &str| s.trim().parse::<f32>().ok();
        let last = value.chars().last()?;
        let edge = match last.to_ascii_lowercase() {
            '%' => {
                let f = number(&value[..value.len() - 1])? / 100.0;
                match past {
                    true => Edge::PastFraction(f),
                    false => Edge::Fraction(f),
                }
            }
            'r' | 'b' if !past => Edge::End(number(&value[..value.len() - 1])?),
            _ => match past {
                true => Edge::Past(number(value)?),
                false => Edge::Start(number(value)?),
            },
        };
        Some(edge)
    }

    /// Where it is in a span from `start` to `end`, `from` being the
    /// opposite edge.
    fn place(self, start: f32, end: f32, from: f32) -> f32 {
        match self {
            Edge::Start(n) => start + n,
            Edge::End(n) => end - n,
            Edge::Fraction(f) => start + f * (end - start),
            Edge::Past(n) => from + n,
            Edge::PastFraction(f) => from + f * (end - start),
        }
    }
}

/// Which of the in-match interface's optional parts a game has.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InterfaceFlags {
    /// A side panel showing one page at a time, picked with its tabs, in
    /// place of separate windows opened by the control panel's buttons.
    pub side_panel: bool,
    /// A second resource bar previewing what an order would cost.
    pub economic_impact: bool,
    /// A column of what's being recruited.
    pub recruit_item_floater: bool,
    /// A pause button on the control panel.
    pub pause_button: bool,
    /// The commander's experience, beside the abilities.
    pub faction_experience: bool,
    /// A cap at the end of each floater, shown while it's open.
    pub floater_end_cap: bool,
}

/// The lists, under `UI/`, of the files whose templates the in-match
/// interface's layouts use, in the order they load.
const GAME_LISTS: [&str; 2] = ["UI/shared.lst", "UI/game.lst"];

/// The folder of the in-match interface's layouts.
const GAME_FOLDER: &str = "ui/game/";

impl GameData {
    /// Which of the in-match interface's optional parts the game has, from
    /// its settings for how it looks; without where they don't say.
    pub fn interface_flags(&self) -> InterfaceFlags {
        let flag = |of: fn(&crate::defs::AVars) -> Option<bool>| self.avar(of).unwrap_or(false);
        InterfaceFlags {
            side_panel: flag(|v| v.ui_use_side_panel),
            economic_impact: flag(|v| v.ui_use_economic_impact),
            recruit_item_floater: flag(|v| v.ui_use_recruit_item_floater),
            pause_button: flag(|v| v.ui_use_pause_button),
            faction_experience: flag(|v| v.ui_show_faction_experience),
            floater_end_cap: flag(|v| v.ui_floaters_display_end_cap),
        }
    }

    /// The files the in-match interface's lists name, in their order.
    fn game_lists(&self) -> Vec<String> {
        let mut names = Vec::new();
        for list in GAME_LISTS {
            self.take_ui_list(list, &mut Vec::new(), &mut names);
        }
        names
    }

    /// The widgets the in-match interface's layouts define at their top
    /// level, in the order their files and blocks come.
    pub fn game_widgets(&self) -> Arc<Vec<Widget>> {
        self.kept("game_widgets", || {
            let mut names = self.game_lists();
            let mut screens = self.entry_names(|n| {
                n.starts_with(GAME_FOLDER)
                    && n.ends_with(".tgi")
                    && !n[GAME_FOLDER.len()..].contains('/')
            });
            screens.sort_by_key(|n| n.to_ascii_lowercase());
            let mut seen: HashSet<String> = HashSet::new();
            names.retain(|n| seen.insert(n.to_ascii_lowercase()));
            for screen in &screens {
                if seen.insert(screen.to_ascii_lowercase()) {
                    names.push(screen.clone());
                }
            }
            let files = self.parse_named(names.iter().map(String::as_str));
            let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
            let screens: HashSet<String> = screens
                .into_iter()
                .map(|s| s.to_ascii_lowercase())
                .collect();
            (files.iter())
                .filter(|(name, _)| screens.contains(&name.to_ascii_lowercase()))
                .flat_map(|(_, parsed)| parsed.root.blocks())
                .filter(|b| !b.kind().is_some_and(|k| k.eq_ignore_ascii_case("Template")))
                .filter_map(|b| widget(&templates.resolve(b), &templates))
                .collect()
        })
    }

    /// The widget `name` defined at the top level of the layout `file`,
    /// placed against the whole screen as [`game_widgets`](Self::game_widgets)
    /// are, with the in-match interface's templates.
    pub fn screen_widget(&self, file: &str, name: &str) -> Option<Widget> {
        let mut names = self.game_lists();
        names.push(file.to_string());
        let files = self.parse_named(names.iter().map(String::as_str));
        let templates = tgi::Templates::new(files.iter().map(|(_, p)| &p.root));
        let (_, parsed) = files
            .iter()
            .rev()
            .find(|(n, _)| n.eq_ignore_ascii_case(file))?;
        (parsed.root.blocks())
            .find(|b| b.kind().is_some_and(|k| k.eq_ignore_ascii_case(name)))
            .and_then(|b| widget(&templates.resolve(b), &templates))
    }

    /// Adds the archive paths of the `.tgi` files a list of the interface's
    /// names to `out`, in order. Its lines are paths from its own folder,
    /// or from the archive's root where they start with `/`, and may name
    /// other lists, whose files are taken in there and then.
    fn take_ui_list(&self, list: &str, open: &mut Vec<String>, out: &mut Vec<String>) {
        let lower = list.to_ascii_lowercase();
        if open.contains(&lower) {
            return;
        }
        let Some(text) = self.read(list) else {
            return;
        };
        open.push(lower);
        for line in tgi::decode(&text).lines() {
            let path = line.split(';').next().unwrap_or("").trim();
            if path.is_empty() {
                continue;
            }
            let path = crate::beside(list, &path.replace('\\', "/"));
            if path.to_ascii_lowercase().ends_with(".lst") {
                self.take_ui_list(&path, open, out);
            } else {
                out.push(path);
            }
        }
        open.pop();
    }
}

/// A resolved block as a widget, or `None` if it isn't one.
fn widget(block: &tgi::Block, templates: &tgi::Templates) -> Option<Widget> {
    if !is_widget(block) {
        return None;
    }
    let mut view = None;
    let mut parts = Vec::new();
    let mut children = Vec::new();
    for b in block.blocks().filter(|b| !is_slot(b)) {
        let b = templates.resolve(b);
        if let Some(w) = widget(&b, templates) {
            children.push(w);
        } else if let Some(v) = view_element(&b, templates) {
            match b.kind().is_some_and(|k| k.eq_ignore_ascii_case("View")) && view.is_none() {
                true => view = Some(v),
                false => parts.push(v),
            }
        }
    }
    Some(Widget {
        name: block.kind().unwrap_or("").to_string(),
        kind: value(block, "type"),
        visible: value(block, "visible").is_none_or(|v| !v.eq_ignore_ascii_case("false")),
        view,
        parts,
        children,
        fields: fields(block),
    })
}

/// A resolved block as a view element, or `None` if it isn't one.
fn view_element(block: &tgi::Block, templates: &tgi::Templates) -> Option<ViewElement> {
    if is_widget(block) || block.field("when").is_none() {
        return None;
    }
    let edge = |key: &str, or: Edge| {
        value(block, key)
            .and_then(|v| Edge::parse(&v))
            .unwrap_or(or)
    };
    let fallback = Bounds::default();
    let when = value(block, "when").filter(|w| crate::named(w).is_some());
    let children = (block.blocks())
        .filter(|b| !is_slot(b))
        .filter_map(|b| view_element(&templates.resolve(b), templates))
        .collect();
    Some(ViewElement {
        name: block.kind().unwrap_or("").to_string(),
        kind: value(block, "type"),
        bounds: Bounds {
            left: edge("L", fallback.left),
            top: edge("T", fallback.top),
            right: edge("R", fallback.right),
            bottom: edge("B", fallback.bottom),
        },
        when: (when.iter())
            .flat_map(|w| w.split('+'))
            .map(|w| w.trim().to_ascii_uppercase())
            .filter(|w| !w.is_empty())
            .collect(),
        texture: value(block, "texture").and_then(|t| ui_path(&t)),
        colour: value(block, "color")
            .and_then(|c| colour(&c))
            .unwrap_or([1.0; 3]),
        text: value(block, "text").filter(|t| !t.is_empty()),
        font: value(block, "font").filter(|f| crate::named(f).is_some()),
        justify: value(block, "justify"),
        tiles: tiles(block),
        children,
        fields: fields(block),
    })
}

/// A tiled decal's images, if it has them all.
fn tiles(block: &tgi::Block) -> Option<Tiles> {
    let path = |key: &str| value(block, key).and_then(|t| ui_path(&t));
    Some(Tiles {
        corners: path("corners_texture")?,
        left: path("left_border_texture")?,
        right: path("right_border_texture")?,
        top: path("top_border_texture")?,
        bottom: path("bottom_border_texture")?,
        center: path("center_texture")?,
    })
}

/// Whether a resolved block is a widget: only widgets have a guess at how
/// many children they'll have.
fn is_widget(block: &tgi::Block) -> bool {
    block.field("max_child_count").is_some()
}

/// A block's fields that have values, in order, the values trimmed.
fn fields(block: &tgi::Block) -> Vec<(String, String)> {
    (block.fields())
        .filter_map(|f| Some((f.key.clone(), f.value.as_deref()?.trim().to_string())))
        .collect()
}

/// The value of the last of `fields` keyed `key`, ignoring case.
fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    (fields.iter().rev())
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// A field's value, the last given where a block repeats a key.
fn value(block: &tgi::Block, key: &str) -> Option<String> {
    let field = (block.fields())
        .filter(|f| f.key.eq_ignore_ascii_case(key))
        .last()?;
    Some(field.value.as_deref()?.trim().to_string())
}

/// Whether a block declares what a template may hold, as `[list Child]`
/// does, rather than holding it.
fn is_slot(block: &tgi::Block) -> bool {
    block
        .kind()
        .is_some_and(|k| k.eq_ignore_ascii_case("list") || k.eq_ignore_ascii_case("optional"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn edges_read_as_written() {
        assert_eq!(Edge::parse("101"), Some(Edge::Start(101.0)));
        assert_eq!(Edge::parse("257r"), Some(Edge::End(257.0)));
        assert_eq!(Edge::parse("26R"), Some(Edge::End(26.0)));
        assert_eq!(Edge::parse("10b"), Some(Edge::End(10.0)));
        assert_eq!(Edge::parse("25%"), Some(Edge::Fraction(0.25)));
        assert_eq!(Edge::parse("+64"), Some(Edge::Past(64.0)));
        assert_eq!(Edge::parse("+100%"), Some(Edge::PastFraction(1.0)));
        assert_eq!(Edge::parse("wide"), None);
    }

    #[test]
    fn bounds_place_within_the_parent() {
        let parent = rect(100.0, 0.0, 300.0, 400.0);
        let bounds = Bounds {
            left: Edge::Fraction(0.5),
            top: Edge::End(50.0),
            right: Edge::Past(20.0),
            bottom: Edge::End(0.0),
        };
        assert_eq!(bounds.place(parent), rect(200.0, 350.0, 220.0, 400.0));
        assert_eq!(Bounds::default().place(parent), parent);
    }

    fn widgets(text: &str) -> Vec<Widget> {
        let templates = tgi::parse(
            "[Template ViewElement]\n{\n\tstring when = null\n\tstring L = 0\n}\n\
             [Template DecalVE Inherit=ViewElement]\n{\n\ttype = DECAL\n\tstring texture = NULL\n}\n\
             [Template GroupVE Inherit=ViewElement]\n{\n\ttype = GROUP\n\t[list Child Template=ViewElement]\n}\n\
             [Template Widget]\n{\n\tmax_child_count = 15\n\tvisible = true\n\t[View Template=ViewElement]\n}\n\
             [Template DecalWidget Inherit=Widget]\n{\n\ttype = DECAL\n\t[Decal Template=DecalVE]\n}\n",
        );
        let parsed = tgi::parse(text);
        let all = tgi::Templates::new([&templates.root, &parsed.root]);
        (parsed.root.blocks())
            .filter_map(|b| widget(&all.resolve(b), &all))
            .collect()
    }

    #[test]
    fn widgets_hold_their_view_parts_and_children() {
        let read = widgets(
            "[Panel Inherit=Widget]\n{\n\
             \t[View Template=GroupVE]\n\t{\n\t\tT = 257b\n\
             \t\t[Child Template=DecalVE]\n\t\twhen = PRESSED + UNFOCUSED\n\t\ttexture = /UI/%s/Game/a.tga\n\
             \t\t[Child Template=DecalVE]\n\t\ttexture = b.png ;; the rest\n\t}\n\
             \t[Top Template=DecalWidget]\n\t{\n\t\tvisible = false\n\
             \t\t[Decal Template=DecalVE]\n\t\t{\n\t\t\tL = 101\n\t\t\tR = +64\n\t\t}\n\t}\n}\n",
        );
        let [panel] = read.as_slice() else {
            panic!("{read:?}")
        };
        assert_eq!(panel.name, "Panel");
        let view = panel.view.as_ref().unwrap();
        assert_eq!(view.kind.as_deref(), Some("GROUP"));
        assert_eq!(view.bounds.top, Edge::End(257.0));
        let textures: Vec<_> = view.children.iter().map(|c| c.texture.as_deref()).collect();
        assert_eq!(textures, [Some("UI/Game/a.tga"), Some("b.png")]);
        assert_eq!(view.children[0].when, ["PRESSED", "UNFOCUSED"]);
        assert!(!view.children[0].shown(&RESTING));
        assert!(view.children[1].shown(&RESTING));

        let top = panel.find("top").unwrap();
        assert!(!top.visible);
        assert_eq!(top.kind.as_deref(), Some("DECAL"));
        assert_eq!(top.parts.len(), 1);
        assert_eq!(top.parts[0].bounds.left, Edge::Start(101.0));
        assert_eq!(top.parts[0].bounds.right, Edge::Past(64.0));
    }

    #[test]
    fn a_widget_rests_on_the_pictures_drawn_at_rest() {
        let read = widgets(
            "[Button Inherit=Widget]\n{\n\
             \t[View Template=GroupVE]\n\t{\n\
             \t\t[Child Template=DecalVE]\n\t\twhen = UNFOCUSED\n\t\ttexture = border.png\n\
             \t\t[Child Template=DecalVE]\n\t\twhen = FOCUSED\n\t\ttexture = lit.png\n\t}\n\
             \t[Client Template=DecalWidget]\n\t{\n\
             \t\t[View Template=GroupVE]\n\t\t{\n\
             \t\t\t[Child Template=DecalVE]\n\t\t\ttexture = icon.png\n\t\t}\n\t}\n\
             \t[Hidden Template=DecalWidget]\n\t{\n\t\tvisible = false\n\
             \t\t[Decal Template=DecalVE]\n\t\ttexture = no.png\n\t}\n}\n",
        );
        assert_eq!(read[0].resting(), ["border.png", "icon.png"]);
    }
}
