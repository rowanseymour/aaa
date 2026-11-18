//! Setting a match up from the game's data, and playing one with no window:
//! what a server of matches is built from.

mod client;
mod clock;
mod film;
mod host;
pub mod lan;
mod link;
mod lobby;
mod names;
mod pausing;
pub mod presets;
pub mod protocol;
pub mod quic;
#[cfg(test)]
mod testing;

use std::collections::{BTreeMap, HashMap};
use std::hash::{BuildHasher, RandomState};

use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tgdata::{Faction, GameData, Handicap, Id, Nation};
use tgmap::{FogSetting, Map, Player, Recruits};
use tgsim::World;

pub use client::{Client, NotJoined, enter, join, welcome};
pub use clock::Clock;
pub use film::{Film, NotFilm};
pub use host::{Event, Host, Refusal, allows, refuse, sayable};
pub use link::{Link, Local, Lost, pair};
pub use lobby::{COUNTDOWN, Choice, Entry, Guest, Lobby, Staging, Told, enter_lobby};
pub use names::{NAME_LONGEST, NUMBERED, Naming, TAKEN_OVER, default_name, name, unique};

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
    /// The faction chosen for each side, by player id; one of its nation's
    /// at random where none is.
    pub factions: BTreeMap<u32, Id>,
    /// The handicap given each side, by player id, in place of the map's.
    pub handicaps: BTreeMap<u32, Id>,
    /// How much of the map the fog of war hides, if not as the game's
    /// world rule has it.
    pub fog: Option<FogSetting>,
}

/// A match set up and yet to be played.
pub struct Started {
    pub map: Map,
    /// What raises the organizations and buildings the match adds.
    pub raising: Recruits,
    pub world: World,
    /// The faction that leads each side with one, by player id.
    pub factions: HashMap<u32, Faction>,
}

/// Sets up the match `setup` describes from the game's data, calling
/// `mapped` once its map is read and before its objects are.
pub fn start(game: &GameData, setup: &Setup, mapped: impl FnOnce()) -> Result<Started, String> {
    let (name, seed) = (&setup.map, setup.seed);
    let all = game.nations();
    let chosen = tgmap::Brief::load(game, name).map_or_else(
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
    let factions = factions(&map.players, &chosen, &setup.factions, seed);
    // What the match goes by, such as which heroes are a side's.
    for player in &mut map.players {
        if let Some(faction) = factions.get(&player.id) {
            player.faction = Some(faction.id.to_string());
        }
    }
    if let Some(teams) = &setup.teams {
        map.arrange(teams);
    }
    mapped();
    let mut operations = game.operations();
    let raisable = raising.raisable();
    let airlifts = game.airlifts();
    // Only summons of regiments and units can be carried out.
    operations.retain(|id, o| match o {
        tgmap::Operation::Summon(s) => {
            let kept = raisable.contains_key(&s.template);
            if !kept {
                debug!("can't carry out {id}: can't summon {}", s.template);
            }
            kept
        }
        _ => true,
    });
    let dropped = operations.values().filter_map(|o| match o {
        tgmap::Operation::Drop(template) => Some(template),
        tgmap::Operation::Enchant { .. }
        | tgmap::Operation::Reveal { .. }
        | tgmap::Operation::Strike(_)
        | tgmap::Operation::Summon(_) => None,
    });
    let drops = (dropped.filter_map(|t| Some((t.clone(), raising.building(t)?)))).collect();
    let mut recruit_lists = game.recruit_lists();
    // A settlement recruits its companies itself.
    recruit_lists.extend(game.settlement_recruits());
    for t in recruit_lists.values().flatten() {
        if !raisable.contains_key(t)
            && let Err(e) = raising.raise(t.as_str(), 0, 0, [0.0, 0.0], [0.0, -1.0])
        {
            debug!("can't commission {t}: {e}");
        }
    }
    let experience = game.experience_rules();
    let heroes = game.heroes();
    let hero_templates = heroes.iter().map(|h| h.id.clone()).collect();
    let combat_profiles = game.combat_profiles();
    let combat_value_rules = game.combat_value_rules();
    let combat_means =
        tgsim::CombatMeans::of(&combat_profiles, &combat_value_rules, &hero_templates);
    let mut rules = tgsim::Rules {
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
        guard_zones: game.guard_zones(),
        remains: game.remains(),
        deaths: game.deaths(),
        damage_filters: game.damage_filters(),
        morale_rules: game.morale_rules(),
        morale_values: game.morale_values(),
        morale_breakpoints: game.morale_breakpoints(),
        morale_effects: game.morale_effects(),
        combat_value_rules,
        combat_profiles,
        combat_means,
        combat_effects: game.combat_effects(),
        property_types: game.property_types(),
        formation_effects: game.formation_effects(),
        broken_effects: game.broken_effects(),
        experience_gains: game.experience_gains(),
        experience_levels: game.experience_levels(),
        supply_effects: game.supply_effects(),
        kinds: game.kinds(),
        sort_objectives: game.sort_objectives(),
        heroes,
        faction_groups: game.faction_groups(),
        operations,
        drops,
        decays: game.decays(),
        logistics: game.logistics(),
        resupply_rates: game.resupply_rates(),
        recruit_lists,
        raisable,
        airlifts,
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
    let [climbing, falling] = game.efficiency_rates();
    rules.efficiency_rates = [
        climbing.unwrap_or(rules.efficiency_rates[0]),
        falling.unwrap_or(rules.efficiency_rates[1]),
    ];
    if let Some(seconds) = game.siege_seconds() {
        rules.siege_seconds = seconds;
    }
    if let Some(seconds) = game.kill_credit_seconds() {
        rules.kill_credit_seconds = seconds;
    }
    if let Some(bonus) = game.attacker_bonus() {
        rules.attacker_bonus = bonus;
    }
    if let Some(first) = game.attack_target_first() {
        rules.attack_target_first = first;
    }
    if let Some(seconds) = game.engage_reaction() {
        rules.engage_reaction = seconds;
    }
    let mut world = World::new(&map, rules);
    world.set_fog(setup.fog.unwrap_or_else(|| game.fog_setting()));
    let recharges = game.operation_recharges();
    for (&player, faction) in &factions {
        let ops = (faction.operations.iter())
            .map(|o| {
                let recharge = recharges.get(&o.id).copied().unwrap_or(0.0);
                tgsim::SpecialOp::new(o.id.clone(), o.cost, recharge)
            })
            .collect();
        world.lead(player, experience.starting, faction.experience_max, ops);
    }
    let given = handicap_technologies(&map.players, &setup.handicaps, &game.handicaps());
    for (player, technology) in given {
        world.gain(player, technology);
    }
    Ok(Started {
        map,
        raising,
        world,
        factions,
    })
}

/// The technology each side starts with from its handicap, by player id:
/// the one `chosen` for it, else the one the map gives it.
pub fn handicap_technologies(
    players: &[Player],
    chosen: &BTreeMap<u32, Id>,
    all: &[Handicap],
) -> HashMap<u32, Id> {
    tgmap::sides(players)
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
    tgmap::sides(players)
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
    Faction = 2,
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

/// The faction that leads each side with a nation, by player id: the one the
/// map fixes, else the one `chosen` where they lead its nation, else one of
/// those the lobby offers at random.
pub fn factions(
    players: &[Player],
    nations: &HashMap<u32, Nation>,
    chosen: &BTreeMap<u32, Id>,
    seed: u64,
) -> HashMap<u32, Faction> {
    nations
        .iter()
        .filter_map(|(&p, nation)| {
            let all = &nation.factions;
            let find = |id: &Id| all.iter().find(|g| g.id == *id);
            let player = players.iter().find(|q| q.id == p);
            let fixed = player.and_then(|q| q.faction.as_deref());
            let faction = (fixed.and_then(|id| find(&Id::from(id))))
                .or_else(|| chosen.get(&p).and_then(find).filter(|g| g.public))
                .or_else(|| {
                    let offered: Vec<&Faction> = all.iter().filter(|g| g.public).collect();
                    pick_at_random(seed, Pick::Faction, p, &offered).copied()
                });
            Some((p, faction?.clone()))
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
            factions: Vec::new(),
            flag: None,
            music: None,
            skin: Vec::new(),
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
        for pick in [Pick::Nation, Pick::Faction] {
            assert_eq!(picked(7, pick), picked(7, pick));
            assert_ne!(picked(7, pick), picked(8, pick));
            // Each side's is its own.
            assert!(picked(7, pick).iter().any(|&n| n != picked(7, pick)[0]));
        }
        assert_ne!(picked(7, Pick::Nation), picked(7, Pick::Faction));
        assert!(pick_at_random::<i32>(7, Pick::Nation, 1, &[]).is_none());
    }

    #[test]
    fn sides_are_led_by_the_chosen_faction_of_their_nation_else_any() {
        let faction = |id: &str| Faction {
            id: Id::from(id),
            name: format!("General {id}"),
            short_name: id.into(),
            operations: Vec::new(),
            experience_max: None,
            public: id != "patton_tutorial",
            portrait: None,
            computer_name: None,
            banner: None,
        };
        let nation = |id: &str, factions: &[&str]| Nation {
            id: Id::from(id),
            name: id.into(),
            placeholders: HashMap::new(),
            factions: factions.iter().map(|g| faction(g)).collect(),
            flag: None,
            music: None,
            skin: Vec::new(),
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
        players[5].faction = Some("patton_tutorial".into());
        let chosen = BTreeMap::from([
            (1, Id::from("Nimitz")),
            (2, Id::from("rommel")),
            (6, Id::from("nimitz")),
            (7, Id::from("patton_tutorial")),
        ]);
        let factions = factions(&players, &nations, &chosen, 7);
        let id = |p| factions[&p].id.as_str();
        assert_eq!(id(1), "nimitz");
        // A faction of another nation's leads none of this one's sides, and
        // the lobby offers none of those only scenarios use.
        for p in [2, 3, 7] {
            assert!(["patton", "nimitz"].contains(&id(p)));
        }
        assert_eq!(id(4), "rommel");
        assert!(!factions.contains_key(&5));
        // The map's own faction leads over the one chosen, offered or not.
        assert_eq!(id(6), "patton_tutorial");
    }
}
