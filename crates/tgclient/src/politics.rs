//! The politics panel, as either interface shows it: a row for each side
//! played in the match, in side order, with how the player here stands to
//! it, as the politics spec has it. Unlike the display panels it's no
//! window: it doesn't hold the match still or close them.

use bevy::prelude::*;
use tgdata::GameData;
use tgmap::Relation;

use crate::intent::{Asked, Intent};
use crate::messages::MessageLog;
use crate::{Leaders, Loaded, Local, Screen, Sim};

/// The button that opens and closes it, which its key presses too.
pub(crate) const BUTTON: &str = "ButtonPolitics";
/// Its layout, and where the hotkey files bind the keys heard while it's
/// up.
pub(crate) const WIDGET: &str = "PoliticsDisplay";
/// What either interface's button is [`called`](crate::ui::called).
pub(crate) const CALLED: &str = "panel.politics";

/// Whether it's open.
#[derive(Resource, Default, Debug)]
pub struct Politics {
    pub open: bool,
}

/// One side's row.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub name: String,
    /// The side's colour, which its gem's drawn in.
    pub colour: [f32; 3],
    /// Whether it's out of the match, which greys its name.
    pub out: bool,
    /// How the player here stands to it; none on their own row, or for
    /// someone watching without a side.
    pub relation: Option<Relation>,
    /// What its tooltip says under its name; none for someone watching
    /// without a side.
    pub tip: Option<String>,
}

/// The rows, kept to the match's every frame.
#[derive(Resource, Default, PartialEq, Debug)]
pub struct Rows(pub Vec<Row>);

/// How the rows look, from the panel's layout.
#[derive(Resource, Clone, Debug)]
pub struct Look {
    /// The font's images of the relation glyphs: ally, neutral, enemy.
    pub glyphs: [Option<String>; 3],
    /// How far across from the start of a row's text its marker sits.
    pub relation_at: f32,
    /// A side out of the match's name colour.
    pub out: [f32; 3],
}

impl Look {
    pub fn new(game: &GameData) -> Look {
        let widgets = game.game_widgets();
        let widget = widgets.iter().find(|w| w.name.eq_ignore_ascii_case(WIDGET));
        let value = |key: &str| widget.and_then(|w| w.value(key)).map(str::trim);
        let glyph = |key: &str| value(key).and_then(|id| game.glyph_named(id));
        Look {
            glyphs: [
                glyph("glyph_ally"),
                glyph("glyph_neutral"),
                glyph("glyph_enemy"),
            ],
            relation_at: value("offset_relation")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            out: (game.interface_vars())
                .rgb("UIColorDefeatedKingdom")
                .unwrap_or([0.5; 3]),
        }
    }
}

/// Where in [`Look::glyphs`] the marker for `relation` is, if it's marked.
pub fn marker(relation: Relation) -> Option<usize> {
    match relation {
        Relation::Ally => Some(0),
        Relation::Neutral => Some(1),
        Relation::Enemy => Some(2),
        Relation::Itself => None,
    }
}

/// What a row's tooltip is made of.
#[derive(Resource, Clone, Debug)]
pub struct Words {
    team: String,
    relation: String,
    /// Self, ally, neutral and enemy, as the relation line and the
    /// description say them.
    relations: [String; 4],
    descriptions: [String; 4],
}

impl Words {
    pub fn of(strings: &tgdata::Strings) -> Words {
        let said = crate::ui::said(strings);
        let kinds = ["self", "ally", "neutral", "enemy"];
        let stock = ["Self", "Ally", "Neutral", "Enemy"];
        let described = [
            ("ui_tooltip_politics_display_self", "Your Side."),
            (
                "ui_tooltip_politics_display_ally_generic",
                "Supply and visibility are shared with allied sides.",
            ),
            (
                "ui_tooltip_politics_display_neutral_generic",
                "Neutral sides will not attack you and cannot be attacked.",
            ),
            (
                "ui_tooltip_politics_display_enemy_generic",
                "Enemy sides will try to destroy your side.",
            ),
        ];
        Words {
            team: said("ui_tooltip_politics_team", "Team"),
            relation: said("ui_tooltip_politics_relation", "Relation"),
            relations: std::array::from_fn(|i| {
                said(
                    &format!("ui_tooltip_politics_relation_{}", kinds[i]),
                    stock[i],
                )
            }),
            descriptions: described.map(|(key, or)| said(key, or)),
        }
    }

    /// What a row's tooltip says under its name, for a side in `team`, if
    /// any, standing so to the viewer.
    fn tip(&self, team: Option<&str>, relation: Relation) -> String {
        let at = match relation {
            Relation::Itself => 0,
            Relation::Ally => 1,
            Relation::Neutral => 2,
            Relation::Enemy => 3,
        };
        let mut lines = Vec::new();
        if let Some(team) = team {
            lines.push(format!("{}: {team}", self.team));
        }
        lines.push(format!("{}: {}", self.relation, self.relations[at]));
        lines.push(String::new());
        lines.push(self.descriptions[at].clone());
        lines.join("\n")
    }
}

pub struct PoliticsPlugin;

impl Plugin for PoliticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Politics>()
            .init_resource::<Rows>()
            .add_systems(OnEnter(Screen::Match), setup)
            .add_systems(OnExit(Screen::Match), |mut commands: Commands| {
                commands.insert_resource(Politics::default());
                commands.insert_resource(Rows::default());
            })
            .add_systems(
                Update,
                (
                    keys.after(crate::keys::send).before(crate::displays::Keys),
                    rows,
                )
                    .before(crate::hud::Draw)
                    .run_if(in_state(Screen::Match)),
            );
    }
}

fn setup(mut commands: Commands, loaded: Res<Loaded>) {
    commands.insert_resource(Look::new(&loaded.game));
    commands.insert_resource(Words::of(&loaded.game.strings()));
}

/// Opens or closes it as asked: its button flips it, and closing whatever
/// panel's up closes it too.
pub fn keys(mut intents: MessageReader<Intent>, mut politics: ResMut<Politics>) {
    let asked = Asked::read(&mut intents);
    if asked.wants(BUTTON) {
        politics.open = !politics.open;
    } else if politics.open && asked.wants(crate::displays::CLOSE_ALL) {
        politics.open = false;
    }
}

/// Keeps the [`Rows`] to the match's sides.
pub fn rows(
    sim: Res<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    log: Option<Res<MessageLog>>,
    words: Option<Res<Words>>,
    mut rows: ResMut<Rows>,
) {
    let Some(words) = words else {
        return;
    };
    let map = log.map(|l| l.map_strings()).unwrap_or_default();
    let named = |player: &tgmap::Player| leaders.owner(player, &sim.0.sides, &map);
    let team = |name: &str| map.get(name).cloned().unwrap_or_else(|| name.to_string());
    let fresh = Rows(made(&sim.0, local.0, &words, named, team));
    rows.set_if_neq(fresh);
}

/// The rows of `world`'s sides as `viewer` sees them, each side named by
/// `name` and each team by `team`.
fn made(
    world: &tgsim::World,
    viewer: Option<u32>,
    words: &Words,
    name: impl Fn(&tgmap::Player) -> String,
    team: impl Fn(&str) -> String,
) -> Vec<Row> {
    let sides = &world.sides;
    (sides.players.iter())
        .filter(|p| !p.independent)
        .map(|p| {
            let relation = viewer.map(|v| match v == p.id {
                true => Relation::Itself,
                false => sides.relation(v, p.id),
            });
            let team = (p.team)
                .and_then(|t| sides.teams.get(t))
                .map(|t| team(&t.name));
            let out = (world.fates.get(&p.id)).is_some_and(|f| f.outcome == tgmap::Outcome::Lost);
            Row {
                name: name(p),
                colour: p.rgb,
                out,
                relation: relation.filter(|r| *r != Relation::Itself),
                tip: relation.map(|r| words.tip(team.as_deref(), r)),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words() -> Words {
        Words::of(&tgdata::Strings::default())
    }

    /// A match of sides 1 and 2, allies, and 3, their enemy.
    fn allied_sides() -> tgsim::World {
        let mut players: Vec<_> = [1, 2, 3].map(crate::tests::player).into();
        for p in &mut players {
            p.relations = vec![Relation::Enemy; 3];
        }
        players[0].relations[1] = Relation::Ally;
        players[1].relations[0] = Relation::Ally;
        let sides = tgsim::Sides {
            names: Default::default(),
            computers: Default::default(),
            players,
            teams: Vec::new(),
        };
        let pathing = tgmap::Pathing::uniform(8, tgmap::Domain::Land);
        tgsim::World::from_organizations(sides, pathing, &[])
    }

    #[test]
    fn rows_list_the_played_sides_with_how_the_viewer_stands_to_each() {
        let mut world = allied_sides();
        world.sides.players[2].independent = true;
        world.fates.get_mut(&2).expect("a fate").outcome = tgmap::Outcome::Lost;
        let rows = made(
            &world,
            Some(1),
            &words(),
            |p| format!("side {}", p.id),
            str::to_string,
        );
        let seen: Vec<_> = (rows.iter())
            .map(|r| (r.name.as_str(), r.relation, r.out))
            .collect();
        assert_eq!(
            seen,
            [
                ("side 1", None, false),
                ("side 2", Some(Relation::Ally), true),
            ]
        );
        let own = rows[0].tip.as_deref().unwrap();
        assert_eq!(own, "Relation: Self\n\nYour Side.");
        assert!(
            rows[1]
                .tip
                .as_deref()
                .unwrap()
                .starts_with("Relation: Ally\n\n")
        );
        // Watching without a side, no row has a marker or a tooltip.
        let watched = made(
            &world,
            None,
            &words(),
            |p| format!("side {}", p.id),
            str::to_string,
        );
        assert!(
            watched
                .iter()
                .all(|r| r.relation.is_none() && r.tip.is_none())
        );
    }

    #[test]
    fn its_button_flips_it_and_closing_all_closes_it() {
        let mut app = App::new();
        app.init_resource::<Politics>()
            .add_message::<Intent>()
            .add_systems(Update, keys);
        let ask = |app: &mut App, name: &str| {
            app.world_mut().write_message(Intent::from(name));
            app.update();
            app.world().resource::<Politics>().open
        };
        assert!(ask(&mut app, BUTTON));
        assert!(!ask(&mut app, crate::displays::CLOSE_ALL));
        assert!(!ask(&mut app, crate::displays::CLOSE_ALL));
        assert!(ask(&mut app, BUTTON));
        assert!(!ask(&mut app, BUTTON));
    }
}
