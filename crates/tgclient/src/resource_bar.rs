//! The resource bar's figures: the player's money and what they net of
//! each resource a minute, and what the order under the cursor would make
//! of them, for either interface to lay out.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use tgdata::{Id, ResourceRole};
use tgmap::Organization;
use tgsim::{Balance, Change};

use crate::ui::{Hint, Ink, Marked, Theme};
use crate::{Loaded, Local, Sim};

/// What the order under the cursor would change, set by the action grid.
#[derive(Resource, Default)]
pub struct Preview(pub Option<Previewed>);

#[derive(Clone)]
pub enum Previewed {
    /// Commissioning this organization at the building at this index into
    /// the sim's buildings.
    Commission(usize, Box<Organization>),
    Research(tgmap::Research),
    /// The packed building at this index into the sim's organizations
    /// unpacking.
    Unpack(usize),
    /// Putting up a building of this template.
    Build(tgmap::Id),
}

/// Whether the preview bar has something to show: a [`Preview`] that
/// still applies, as [`show`] last found.
#[derive(Resource, Default)]
pub struct Previewing(pub bool);

/// The text of a cell of a bar, drawn in a span for each run of it, in
/// its own font. Its parent holds it and the resource's glyph, hidden
/// on the preview bar while the order leaves the figure alone.
#[derive(Component)]
pub struct Cell {
    preview: bool,
    resource: Id,
    /// Whether it's the stockpiled resource's, which shows the stockpile.
    stockpiled: bool,
}

impl Cell {
    /// A cell of the bar, or of the preview, for `resource`, and on the
    /// bar, what it needs to say what it shows when hovered.
    pub fn new(
        game: &tgdata::GameData,
        preview: bool,
        resource: &Id,
    ) -> (Cell, Option<(Hint, Marked, About)>) {
        let resources = game.resources();
        let kind = resources.iter().find(|r| &r.id == resource);
        let cell = Cell {
            preview,
            resource: resource.clone(),
            stockpiled: stockpile(game).as_ref() == Some(resource),
        };
        let name = kind.map_or(resource.to_string(), |k| k.name.clone());
        let about = kind.and_then(|k| k.description.clone());
        let hint = (!preview).then(|| (Hint(String::new()), Marked::plain(name), About(about)));
        (cell, hint)
    }
}

/// What the game says of a cell's resource, under its figures when hovered.
#[derive(Component)]
pub struct About(Option<String>);

/// The colours a cell's text is drawn in, for an interface that doesn't
/// draw it in the theme's [`Ink`]s: normally, and for a shortfall.
#[derive(Component, Clone, Copy)]
pub struct Colours {
    pub body: Color,
    pub short: Color,
}

/// The resource the sim keeps a stockpile of: the first stockpiled one.
pub(crate) fn stockpile(game: &tgdata::GameData) -> Option<Id> {
    let resources = game.resources();
    let first = resources
        .iter()
        .find(|r| r.role == ResourceRole::Stockpiled);
    first.map(|r| r.id.clone())
}

/// Takes the game's colour for what's short, and clears any preview, for
/// either interface.
pub fn setup(mut commands: Commands, loaded: Res<Loaded>, mut theme: ResMut<Theme>) {
    theme.short = (loaded.game.invalid_colour()).map(|[r, g, b]| Color::srgb(r, g, b));
    commands.init_resource::<Preview>();
    commands.init_resource::<Previewing>();
}

/// Fills the cells of both bars for the player, the preview's only while
/// there's something to preview.
#[allow(clippy::too_many_arguments)]
pub fn show(
    sim: Res<Sim>,
    local: Res<Local>,
    preview: Res<Preview>,
    mut previewing: ResMut<Previewing>,
    mut commands: Commands,
    mut cells: Query<CellParts>,
    mut holders: Query<&mut Visibility>,
    mut spans: Query<SpanParts>,
) {
    let Some(player) = local.0 else {
        previewing.0 = false;
        return;
    };
    let sim = &sim.0;
    let now = (sim.stockpile(player), sim.balance(player));
    let previewed = preview.0.as_ref().and_then(|p| {
        let change = match p {
            Previewed::Commission(b, r) if *b < sim.buildings.len() => Change::Commission(*b, r),
            Previewed::Commission(..) => return None,
            Previewed::Research(r) => Change::Research(r),
            Previewed::Unpack(r) if *r < sim.organizations.len() => Change::Unpack(*r),
            Previewed::Unpack(_) => return None,
            Previewed::Build(t) => Change::Build(t),
        };
        Some(sim.preview(player, change))
    });
    if previewing.0 != previewed.is_some() {
        previewing.0 = previewed.is_some();
    }
    for (entity, cell, children, style, holder, hint) in &mut cells {
        let shown = match cell.preview {
            true => previewed.as_ref(),
            false => Some(&now),
        };
        let Some((stockpile, balance)) = shown else {
            continue;
        };
        let parts = cell_parts(&cell.resource, cell.stockpiled, *stockpile, balance);
        // The preview shows only what the order would change.
        let changed =
            !cell.preview || parts != cell_parts(&cell.resource, cell.stockpiled, now.0, &now.1);
        if let Ok(mut visibility) = holders.get_mut(holder.parent()) {
            visibility.set_if_neq(match changed {
                true => Visibility::Inherited,
                false => Visibility::Hidden,
            });
        }
        // Only what's hovered is said.
        if let Some((mut hint, about, hovered)) = hint
            && hovered.get()
        {
            let said = said(&parts, about.0.as_deref());
            if hint.0 != said {
                hint.0 = said;
            }
        }
        paint(&mut commands, entity, children, style, parts, &mut spans);
    }
}

/// A cell's parts: its text's, the parent holding it and its glyph, and,
/// on the bar rather than the preview, its hint.
type CellParts = (
    Entity,
    &'static Cell,
    Option<&'static Children>,
    (&'static TextFont, Option<&'static Colours>),
    &'static ChildOf,
    Option<(&'static mut Hint, &'static About, &'static Hovered)>,
);

/// What a cell's hint says: its figures, then what the game says of its
/// resource.
fn said(parts: &[(String, bool)], about: Option<&str>) -> String {
    let figures: String = parts.iter().map(|(words, _)| words.as_str()).collect();
    match about {
        Some(about) => format!("{figures}\n{about}"),
        None => figures,
    }
}

/// A span of a cell's text's parts: what it says, and how it's coloured.
type SpanParts = (
    &'static mut TextSpan,
    Option<&'static mut Ink>,
    Option<&'static mut TextColor>,
);

/// Puts each run of a cell's text in a span of its own, in the cell's
/// font and in its ink or [`Colours`], adding spans it lacks and emptying
/// those it has spare.
fn paint(
    commands: &mut Commands,
    cell: Entity,
    children: Option<&Children>,
    (font, colours): (&TextFont, Option<&Colours>),
    parts: Vec<(String, bool)>,
    spans: &mut Query<SpanParts>,
) {
    let children = children.map_or(&[][..], |c| &c[..]);
    let ink = |short| match short {
        true => Ink::Short,
        false => Ink::Body,
    };
    let colour = |colours: &Colours, short| match short {
        true => colours.short,
        false => colours.body,
    };
    let mut parts = parts.into_iter();
    for &child in children {
        let Ok((mut text, was_ink, was_colour)) = spans.get_mut(child) else {
            continue;
        };
        let (words, short) = parts.next().unwrap_or_default();
        if text.0 != words {
            text.0 = words;
        }
        match (colours, was_ink, was_colour) {
            (Some(c), _, Some(mut was)) => _ = was.set_if_neq(TextColor(colour(c, short))),
            (None, Some(mut was), _) => _ = was.set_if_neq(ink(short)),
            _ => {}
        }
    }
    for (words, short) in parts {
        let span = commands
            .spawn((TextSpan(words), font.clone(), ChildOf(cell)))
            .id();
        match colours {
            Some(c) => commands.entity(span).insert(TextColor(colour(c, short))),
            None => commands.entity(span).insert(ink(short)),
        };
    }
}

/// What a cell says, in runs that are each short or not: the stockpile and
/// its rate, a resource's rate, or a limit's use against what's provided.
fn cell_parts(
    kind: &Id,
    stockpiled: bool,
    stockpile: f32,
    balance: &Balance,
) -> Vec<(String, bool)> {
    // The sign comes from the rate, not its rounding, so a rate just under
    // zero reads -0.
    let rate = |r: f32| {
        let sign = if r < 0.0 { '-' } else { '+' };
        (format!("{sign}{}", r.abs().round()), r < 0.0)
    };
    if let Some(limit) = balance.limits.iter().find(|l| l.consumed == *kind) {
        let (used, provided) = (limit.used.round(), limit.provided.round());
        return vec![
            (used.to_string(), used > provided),
            (format!("/{provided}"), false),
        ];
    }
    let net = balance.net.get(kind.as_str());
    match stockpiled {
        true => vec![
            // Adding zero turns a -0 into 0.
            ((stockpile.floor() + 0.0).to_string(), stockpile < 0.0),
            ("(".into(), false),
            rate(net),
            (")".into(), false),
        ],
        false => vec![rate(net)],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Font, label};
    use tgmap::ResourceKind;
    use tgsim::{Ledger, economy::EconomyRules};

    fn resources() -> Vec<ResourceKind> {
        vec![
            ResourceKind::stockpiled("money"),
            ResourceKind::limited("unit_limit_consumed", "unit_limit_provided"),
            ResourceKind::limited("building_limit_consumed", "building_limit_provided"),
        ]
    }

    #[test]
    fn cells_show_rates_and_limits_as_the_game_does() {
        let mut ledger = Ledger::default();
        ledger.production.set("money", 25.0);
        ledger.production.set("ammo", 20.0);
        ledger.upkeep.set("oil", 4.0);
        ledger.production.set("unit_limit_provided", 6.0);
        ledger.upkeep.set("building_limit_consumed", 1.0);
        let rules = EconomyRules {
            resources: resources(),
            limited_max: 20.0,
            ..Default::default()
        };
        let balance = ledger.balance(&rules);
        let cells = [
            "money",
            "ammo",
            "oil",
            "building_limit_consumed",
            "unit_limit_consumed",
        ];
        let said: Vec<String> = cells
            .iter()
            .map(|&k| {
                cell_parts(&Id::from(k), k == "money", 518.7, &balance)
                    .into_iter()
                    .map(|p| p.0)
                    .collect()
            })
            .collect();
        assert_eq!(said, ["518(+25)", "+20", "-4", "1/0", "0/6"]);
    }

    #[test]
    fn only_shortfalls_are_short() {
        let mut ledger = Ledger::default();
        ledger.upkeep.set("money", 0.3);
        ledger.production.set("ammo", 0.2);
        ledger.production.set("unit_limit_provided", 5.0);
        ledger.upkeep.set("unit_limit_consumed", 6.0);
        ledger.upkeep.set("building_limit_consumed", 5.0);
        ledger.production.set("building_limit_provided", 5.0);
        let rules = EconomyRules {
            resources: resources(),
            limited_max: 20.0,
            ..Default::default()
        };
        let balance = ledger.balance(&rules);
        let parts = |kind: &str, stockpile| {
            cell_parts(&Id::from(kind), kind == "money", stockpile, &balance)
        };
        let short = |s: &str| (s.to_string(), true);
        let body = |s: &str| (s.to_string(), false);
        assert_eq!(
            parts("money", -12.5),
            [short("-13"), body("("), short("-0"), body(")")]
        );
        assert_eq!(parts("money", 0.0)[0], body("0"));
        assert_eq!(parts("money", -0.0)[0], body("0"));
        assert_eq!(parts("ammo", 0.0), [body("+0")]);
        assert_eq!(parts("unit_limit_consumed", 0.0), [short("6"), body("/5")]);
        assert_eq!(
            parts("building_limit_consumed", 0.0),
            [body("5"), body("/5")]
        );
    }

    #[test]
    fn a_cells_hint_is_its_figures_then_what_the_game_says_of_it() {
        let parts = [
            ("512".into(), false),
            ("(".into(), false),
            ("+25".into(), true),
            (")".into(), false),
        ];
        assert_eq!(
            said(&parts, Some("Money is gained.")),
            "512(+25)\nMoney is gained."
        );
        assert_eq!(said(&parts[..1], None), "512");
    }

    #[test]
    fn runs_are_painted_in_spans_of_the_hud_font() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let cell = world.spawn(label("", Font::Hud, Ink::Body)).id();
        let mut paint_with = |parts: Vec<(String, bool)>| {
            world
                .run_system_once(
                    move |mut commands: Commands,
                          cells: Query<(Option<&Children>, &TextFont)>,
                          mut spans: Query<SpanParts>| {
                        let (children, font) = cells.get(cell).unwrap();
                        let style = (font, None);
                        paint(
                            &mut commands,
                            cell,
                            children,
                            style,
                            parts.clone(),
                            &mut spans,
                        );
                    },
                )
                .unwrap();
        };
        paint_with(vec![("6".into(), true), ("/5".into(), false)]);
        paint_with(vec![("5".into(), false)]);
        let children: Vec<Entity> = world.get::<Children>(cell).unwrap().to_vec();
        let spans: Vec<(String, Ink, f32)> = children
            .iter()
            .map(|&c| {
                let e = world.entity(c);
                let size = match e.get::<TextFont>().unwrap().font_size {
                    FontSize::Px(px) => px,
                    _ => 0.0,
                };
                (
                    e.get::<TextSpan>().unwrap().0.clone(),
                    *e.get::<Ink>().unwrap(),
                    size,
                )
            })
            .collect();
        let hud = Font::Hud.px();
        assert_eq!(
            spans,
            [
                ("5".into(), Ink::Body, hud),
                (String::new(), Ink::Body, hud)
            ]
        );
    }
}
