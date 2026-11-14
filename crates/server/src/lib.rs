//! Setting a match up from the game's data, and playing one with no window:
//! what a server of matches is built from.

mod client;
mod film;
mod host;
mod link;
pub mod protocol;
#[cfg(test)]
mod testing;

use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, RandomState};

use aaa_data::{GameData, General, Handicap, Id, Nation};
use aaa_map::{FogSetting, Map, Player, Recruits};
use aaa_sim::World;
use log::{debug, warn};
use serde::{Deserialize, Serialize};

pub use client::{Client, welcome};
pub use film::{Film, NotFilm};
pub use host::{Event, Host, Refusal};
pub use link::{Link, Local, Lost, pair};

/// What a match is played on and by, as its lobby leaves it: all that
/// every copy of the match has to agree on before it starts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    /// The map's name, as the game's data has it.
    pub map: String,
    /// What the match's chances are drawn from, and the picks for sides
    /// left to chance.
    pub seed: u64,
    /// The teams the sides are put in, by player id, for [`Map::arrange`];
    /// none to play the map as made.
    pub teams: Option<Vec<Vec<u32>>>,
    /// The nation chosen for each side, by player id; one at random where
    /// the map fixes none and none is chosen.
    pub nations: BTreeMap<u32, Id>,
    /// The general chosen for each side, by player id; one of its nation's
    /// at random where none is.
    pub generals: BTreeMap<u32, Id>,
    /// The handicap given each side, by player id, in place of the map's.
    pub handicaps: BTreeMap<u32, Id>,
    /// How much of the map the fog of war hides, if not as the game's
    /// world rule has it.
    pub fog: Option<FogSetting>,
}

/// A match set up and yet to be played.
pub struct Started {
    pub map: Map,
    /// What raises the regiments and buildings the match adds.
    pub raising: Recruits,
    pub world: World,
    /// The general who leads each side with one, by player id.
    pub generals: HashMap<u32, General>,
}

/// Sets up the match `setup` describes from the game's data, calling
/// `mapped` once its map is read and before its objects are.
pub fn start(game: &GameData, setup: &Setup, mapped: impl FnOnce()) -> Result<Started, String> {
    let (name, seed) = (&setup.map, setup.seed);
    let all = game.nations();
    let chosen = aaa_map::Brief::load(game, name).map_or_else(
        |e| {
            warn!("{name}: sides play as no nation: {e}");
            HashMap::new()
        },
        |b| nations(&b.players, &setup.nations, &all, seed),
    );
    let raising = Recruits::new(game);
    let mut map = Map::load_as(game, &raising, name, &chosen)?;
    for w in &map.warnings {
        warn!("{name}: {w}");
    }
    let generals = generals(&map.players, &chosen, &setup.generals, seed);
    if let Some(teams) = &setup.teams {
        map.arrange(teams);
    }
    mapped();
    let operations = game.operations();
    let dropped = operations.values().filter_map(|o| match o {
        aaa_map::Operation::Drop(template) => Some(template.clone()),
        aaa_map::Operation::Enchant { .. } => None,
    });
    let drops = (dropped.filter_map(|t| Some((t.clone(), raising.building(&t)?)))).collect();
    let recruit_lists = game.recruit_lists();
    let raisable = raising.raisable();
    for t in recruit_lists.values().flatten() {
        if !raisable.contains_key(t)
            && let Err(e) = raising.raise(t.as_str(), 0, 0, [0.0, 0.0], [0.0, -1.0])
        {
            debug!("can't commission {t}: {e}");
        }
    }
    let experience = game.experience_rules();
    let mut rules = aaa_sim::Rules {
        seed,
        terrain_rules: game.terrain_rules(),
        packing: raising.packings(),
        requires: game.requirements(),
        economies: game.economies(),
        sellable: game.sellable(),
        economy: game.economy_rules(),
        supplies: game.supplies(),
        supply_rules: game.supply_rules(),
        guard_rules: game.guard_rules(),
        supply_effects: game.supply_effects(),
        kinds: game.kinds(),
        sort_objectives: game.sort_objectives(),
        operations,
        drops,
        decays: game.decays(),
        logistics: game.logistics(),
        resupply_rates: game.resupply_rates(),
        recruit_lists,
        raisable,
        airlifts: game.airlifts(),
        falls: game.falls(),
        recruit_times: game.recruit_times(),
        research_lists: game.research_lists(),
        experience_per_second: experience.per_second,
        sights: game.sights(),
        numbered: game.numbered(),
        sustaining: game.sustaining(),
        team_victory: game.team_victory(),
        ..Default::default()
    };
    if let Some(gravity) = game.gravity() {
        rules.gravity = gravity;
    }
    if let Some(seconds) = game.siege_seconds() {
        rules.siege_seconds = seconds;
    }
    let mut world = World::new(&map, rules);
    world.set_fog(setup.fog.unwrap_or_else(|| game.fog_setting()));
    for (&player, general) in &generals {
        world.lead(player, experience.starting, general.experience_max);
    }
    let given = handicap_technologies(&map.players, &setup.handicaps, &game.handicaps());
    for (player, technology) in given {
        world.gain(player, technology);
    }
    Ok(Started {
        map,
        raising,
        world,
        generals,
    })
}

/// The technology each side starts with from its handicap, by player id:
/// the one `chosen` for it, else the one the map gives it.
pub fn handicap_technologies(
    players: &[Player],
    chosen: &BTreeMap<u32, Id>,
    all: &[Handicap],
) -> HashMap<u32, Id> {
    aaa_map::sides(players)
        .filter_map(|p| {
            let id = chosen.get(&p.id).or(p.handicap.as_ref())?;
            let handicap = all.iter().find(|h| h.id == *id)?;
            Some((p.id, handicap.property.clone()?))
        })
        .collect()
}

/// The nation each side plays as, by player id: the one the map fixes, else
/// the one `chosen`, else one at random.
pub fn nations(
    players: &[Player],
    chosen: &BTreeMap<u32, Id>,
    nations: &[Nation],
    seed: u64,
) -> HashMap<u32, Nation> {
    let find = |id: &str| nations.iter().find(|n| n.id.as_str() == id);
    aaa_map::sides(players)
        .filter_map(|p| {
            let nation = match (&p.nation, chosen.get(&p.id)) {
                (Some(fixed), _) => find(fixed),
                (None, Some(id)) => find(id.as_str()),
                (None, None) => pick_at_random(seed, Pick::Nation, p.id, nations),
            };
            Some((p.id, nation?.clone()))
        })
        .collect()
}

/// A seed no match has had, for one that isn't given its own.
pub fn fresh_seed() -> u64 {
    RandomState::new().hash_one(std::time::SystemTime::now())
}

/// What's picked for a side left to chance, so that each is its own pick.
#[derive(Clone, Copy)]
enum Pick {
    Nation = 1,
    General = 2,
}

/// One of `items` for `player`, at random but the same wherever the match
/// has the same `seed`; none where there are none.
fn pick_at_random<T>(seed: u64, pick: Pick, player: u32, items: &[T]) -> Option<&T> {
    let mut z = seed
        .wrapping_add(u64::from(player) << 8 | pick as u64)
        .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let roll = ((z ^ (z >> 31)) >> 16) as usize;
    items.get(roll % items.len().max(1))
}

/// The general who leads each side with a nation, by player id: the one the
/// map fixes, else the one `chosen` where they lead its nation, else one of
/// those the lobby offers at random.
pub fn generals(
    players: &[Player],
    nations: &HashMap<u32, Nation>,
    chosen: &BTreeMap<u32, Id>,
    seed: u64,
) -> HashMap<u32, General> {
    nations
        .iter()
        .filter_map(|(&p, nation)| {
            let all = &nation.generals;
            let find = |id: &Id| all.iter().find(|g| g.id == *id);
            let player = players.iter().find(|q| q.id == p);
            let fixed = player.and_then(|q| q.general.as_deref());
            let general = (fixed.and_then(|id| find(&Id::from(id))))
                .or_else(|| chosen.get(&p).and_then(find).filter(|g| g.public))
                .or_else(|| {
                    let offered: Vec<&General> = all.iter().filter(|g| g.public).collect();
                    pick_at_random(seed, Pick::General, p, &offered).copied()
                });
            Some((p, general?.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::testing::player;
    use super::*;

    fn players() -> Vec<Player> {
        vec![
            player(10, false, false),
            player(11, true, false),
            player(12, false, true),
            player(13, false, true),
        ]
    }

    #[test]
    fn sides_play_the_fixed_nation_else_the_chosen_else_any() {
        let nation = |id: &str| Nation {
            id: Id::from(id),
            name: id.to_uppercase(),
            placeholders: HashMap::new(),
            generals: Vec::new(),
            flag: None,
            music: None,
        };
        let all = [nation("nation_usa"), nation("nation_ger")];
        let mut players = players();
        players[0].nation = Some("nation_ger".into());
        let chosen = BTreeMap::from([(10, Id::from("nation_usa")), (12, Id::from("nation_usa"))]);
        let nations = nations(&players, &chosen, &all, 7);
        let id = |p| nations[&p].id.as_str();
        assert_eq!(id(10), "nation_ger");
        assert_eq!(id(12), "nation_usa");
        assert!(["nation_usa", "nation_ger"].contains(&id(13)));
        // Independent players play as no nation.
        assert!(!nations.contains_key(&11));
    }

    #[test]
    fn picks_left_to_chance_follow_the_seed() {
        let items = [0, 1, 2, 3, 4, 5, 6, 7];
        let picked = |seed, pick| -> Vec<i32> {
            (1..=8)
                .map(|p| *pick_at_random(seed, pick, p, &items).unwrap())
                .collect()
        };
        for pick in [Pick::Nation, Pick::General] {
            assert_eq!(picked(7, pick), picked(7, pick));
            assert_ne!(picked(7, pick), picked(8, pick));
            // Each side's is its own.
            assert!(picked(7, pick).iter().any(|&n| n != picked(7, pick)[0]));
        }
        assert_ne!(picked(7, Pick::Nation), picked(7, Pick::General));
        assert!(pick_at_random::<i32>(7, Pick::Nation, 1, &[]).is_none());
    }

    #[test]
    fn sides_are_led_by_the_chosen_general_of_their_nation_else_any() {
        let general = |id: &str| General {
            id: Id::from(id),
            name: format!("General {id}"),
            short_name: id.into(),
            operations: Vec::new(),
            experience_max: None,
            public: id != "patton_tutorial",
            portrait: None,
        };
        let nation = |id: &str, generals: &[&str]| Nation {
            id: Id::from(id),
            name: id.into(),
            placeholders: HashMap::new(),
            generals: generals.iter().map(|g| general(g)).collect(),
            flag: None,
            music: None,
        };
        let usa = nation("nation_usa", &["patton", "patton_tutorial", "nimitz"]);
        let nations = HashMap::from([
            (1, usa.clone()),
            (2, usa.clone()),
            (3, usa.clone()),
            (4, nation("nation_ger", &["rommel"])),
            (5, nation("nation_none", &[])),
            (6, usa.clone()),
            (7, usa),
        ]);
        let mut players: Vec<Player> = (1..=7).map(|id| player(id, false, true)).collect();
        players[5].general = Some("patton_tutorial".into());
        let chosen = BTreeMap::from([
            (1, Id::from("Nimitz")),
            (2, Id::from("rommel")),
            (6, Id::from("nimitz")),
            (7, Id::from("patton_tutorial")),
        ]);
        let generals = generals(&players, &nations, &chosen, 7);
        let id = |p| generals[&p].id.as_str();
        assert_eq!(id(1), "nimitz");
        // A general of another nation's leads none of this one's sides, and
        // the lobby offers none of those only scenarios use.
        for p in [2, 3, 7] {
            assert!(["patton", "nimitz"].contains(&id(p)));
        }
        assert_eq!(id(4), "rommel");
        assert!(!generals.contains_key(&5));
        // The map's own general leads over the one chosen, offered or not.
        assert_eq!(id(6), "patton_tutorial");
    }
}
