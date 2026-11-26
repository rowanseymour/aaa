//! Setting a match up from the game's data, and playing one with no window:
//! what a server of matches is built from.

mod client;
mod clock;
mod computers;
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
mod slots;
#[cfg(test)]
mod testing;

use std::collections::hash_map;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};

use log::{debug, warn};
use serde::{Deserialize, Serialize};
use tgdata::{Faction, GameData, Handicap, Id, Nation, RuleValues, Sai};
use tgmap::{FogSetting, Map, Player, Recruits};
use tgsim::World;

pub use client::{Client, NotJoined, enter, join, welcome};
pub use clock::Clock;
pub use computers::Profiles;
pub use film::{Film, NotFilm};
pub use host::{Event, Host, Refusal, allows, refuse, sayable};
use link::ping;
pub use link::{Link, Local, Lost, pair};
pub use lobby::{COUNTDOWN, Choice, Entry, Guest, Lobby, Staging, Told, enter_lobby};
pub use names::{NAME_LONGEST, NUMBERED, Naming, TAKEN_OVER, default_name, name, unique};
pub use slots::Slots;

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
    /// Every side a computer player plays, by player id, with the profile
    /// chosen for it; none for one at random.
    pub computers: BTreeMap<u32, Option<Id>>,
    /// The sides nobody holds, by player id, which leave the match unless
    /// the map requires them.
    pub open: BTreeSet<u32>,
    /// The world rules the match is played with, such as the fog of war,
    /// the time of day and the money each side starts with, in place of
    /// those the map stores.
    pub rules: RuleValues,
}

/// A match set up and yet to be played.
pub struct Started {
    pub map: Map,
    /// What raises the organizations and buildings the match adds.
    pub raising: Recruits,
    pub world: World,
    /// The faction that leads each side with one, by player id.
    pub factions: HashMap<u32, Faction>,
    /// The profile each computer player plays by, by player id.
    pub computers: BTreeMap<u32, Id>,
    /// Those profiles, read, and those for sides taken over later.
    pub profiles: Profiles,
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
    let (leaving, computer_sides) = staying(&map.players, &setup.open, &setup.computers);
    map.leave(&leaving);
    let mut world_rules = map.rules.values.clone();
    world_rules.extend(setup.rules.clone());
    let rule = |id: &str| world_rules.get(&Id::from(id)).map(String::as_str);
    if let Some(sun) = game.sun_for(rule(tgdata::SUN)) {
        map.light(&sun);
    }
    if let Some(money) = rule(tgdata::STOCKPILE).and_then(tgdata::number) {
        let most = game.economy_rules().stockpile_max;
        map.set_default_stockpile((money as f32).min(most));
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
        tgmap::Operation::Enchant(_)
        | tgmap::Operation::Reveal { .. }
        | tgmap::Operation::Strike(_)
        | tgmap::Operation::Summon(_) => None,
    });
    let drops = (dropped.filter_map(|t| Some((t.clone(), raising.building(t)?)))).collect();
    // What anything raised lists to build.
    let builds = (raisable.values())
        .flat_map(|r| &r.members)
        .filter_map(|m| m.fighter.as_ref())
        .flat_map(|f| &f.builds);
    let mut buildable = HashMap::new();
    for t in builds {
        if let hash_map::Entry::Vacant(entry) = buildable.entry(t.clone()) {
            match raising.building(t) {
                Some(b) => _ = entry.insert(b),
                None => debug!("can't build {t}"),
            }
        }
    }
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
    let mut experience = game.experience_rules();
    if let Some(starting) = rule(tgdata::STARTING_EXPERIENCE).and_then(tgdata::number) {
        experience.starting = starting as f32;
    }
    let heroes = game.heroes();
    let hero_templates = heroes.iter().map(|h| h.id.clone()).collect();
    let combat_profiles = game.combat_profiles();
    let combat_value_rules = game.combat_value_rules();
    let combat_means =
        tgsim::CombatMeans::of(&combat_profiles, &combat_value_rules, &hero_templates);
    let defaults = tgsim::Rules::default();
    let [climbing, falling] = game.efficiency_rates();
    let banners = game.banners();
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
        explore_rules: game.explore_rules(),
        guard_zones: game.guard_zones(),
        remains: game.remains(),
        stances: game.stances(),
        animation_sets: game.animation_sets(),
        damage_filters: game.damage_filters(),
        morale_rules: game.morale_rules(),
        morale_values: game.morale_values(),
        morale_breakpoints: game.morale_breakpoints(),
        morale_effects: game.morale_effects(),
        entrenchment: game.entrenchment(),
        combat_value_rules,
        combat_profiles,
        combat_means,
        combat_effects: game.combat_effects(),
        property_types: game.property_types(),
        formation_effects: game.formation_effects(),
        group_formations: game.group_formations(),
        broken_effects: game.broken_effects(),
        formation_types: game.formation_types(),
        broken_types: game.broken_types(),
        type_slots: tgsim::TypeSlots::new(&game.target_types()),
        experience_gains: game.experience_gains(),
        experience_levels: game.experience_levels(),
        supply_effects: game.supply_effects(),
        economy_effects: game.economy_effects(),
        enchantment_types: game.enchantment_types(),
        property_requires: game.property_requirements(),
        kept_from_members: game.kept_from_members(),
        kinds: game.kinds(),
        sort_objectives: game.sort_objectives(),
        heroes,
        faction_groups: game.faction_groups(),
        operations,
        drops,
        buildable,
        axis_locked: game.axis_locked(),
        building_padding: raising.padding(),
        decays: game.decays(),
        logistics: game.logistics(),
        nations: banners.of_template,
        badges: (banners.badges.into_iter())
            .map(|(n, b)| (n, b.len()))
            .collect(),
        resupply_rates: game.resupply_rates(),
        resupplying: game.resupplying(),
        busy_states: game.busy_states(),
        recruit_lists,
        raisable,
        airlifts,
        airfields: game.airfields(),
        aircraft: game.aircraft(),
        denizen_siege_rate: (game.denizen_siege_rate()).unwrap_or(defaults.denizen_siege_rate),
        falls: game.falls(),
        recruit_times: game.recruit_times(),
        research_lists: game.research_lists(),
        granted: game.granted_technologies(),
        experience_per_second: experience.per_second,
        sights: game.sights(),
        props: raising.prop_templates(),
        organization_shapes: raising.organization_shapes(),
        numbered: game.numbered(),
        sustaining: game.sustaining(),
        team_victory: game.team_victory(),
        plane_fall_tweak: (game.plane_fall_gravity_tweak()).unwrap_or(defaults.plane_fall_tweak),
        gravity: game.gravity().unwrap_or(defaults.gravity),
        efficiency_rates: [
            climbing.unwrap_or(defaults.efficiency_rates[0]),
            falling.unwrap_or(defaults.efficiency_rates[1]),
        ],
        catchup_speed: game.catchup_speed().unwrap_or(defaults.catchup_speed),
        banner_catchup_floor: (game.banner_catchup_floor())
            .unwrap_or(defaults.banner_catchup_floor),
        recruit_search_step: (game.recruit_search_step()).unwrap_or(defaults.recruit_search_step),
        siege_seconds: game.siege_seconds().unwrap_or(defaults.siege_seconds),
        kill_credit_seconds: (game.kill_credit_seconds()).unwrap_or(defaults.kill_credit_seconds),
        attacker_bonus: game.attacker_bonus().unwrap_or(defaults.attacker_bonus),
        attack_target_first: (game.attack_target_first()).unwrap_or(defaults.attack_target_first),
        engage_reaction: game.engage_reaction().unwrap_or(defaults.engage_reaction),
        ..defaults
    };
    if let Some(ratio) = rule(tgdata::CITY_VICTORY_RATIO).and_then(tgdata::number) {
        rules.city_victory_ratio = ratio.min(100);
    }
    let difficulty =
        (game.lobby_rules().into_iter()).find(|r| r.id == Id::from(tgdata::DIFFICULTY_LEVEL));
    if let Some(level) = difficulty {
        if let tgdata::RuleKind::Choice(choices) = &level.kind {
            rules.difficulties = choices.iter().map(|c| c.id.clone()).collect();
        }
        rules.difficulty = Some(Id::from(level.value(&world_rules)));
    }
    let mut world = World::new(&map, rules);
    let fog = rule(tgdata::FOG_OF_WAR).and_then(FogSetting::named);
    world.set_fog(fog.unwrap_or_else(|| game.fog_setting()));
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
    let handicaps = side_handicaps(&map.players, &setup.handicaps, &game.handicaps());
    for (player, handicap) in handicaps {
        if let Some(technology) = handicap.property {
            world.gain(player, technology);
        }
        let stockpiled = world.rules().economy.stockpiled().next().cloned();
        let extra = stockpiled.map_or(0.0, |s| handicap.extra.get(s.as_str()));
        world.set_stockpile(player, world.stockpile(player) + extra);
    }
    // Not from the match's seed: only the machine a computer player
    // thinks on needs its profile.
    let sais = game.sais();
    let picks = (&chosen, &factions, sais.as_slice());
    // A side taken over mid-match is given one as though left to chance.
    let every: BTreeMap<u32, Option<Id>> =
        tgmap::sides(&map.players).map(|p| (p.id, None)).collect();
    let taking_over = computers(&map.players, &every, picks, fresh_seed());
    let computers = computers(&map.players, &computer_sides, picks, fresh_seed());
    let profiles = Profiles::new(game, &computers, &taking_over, &sais);
    Ok(Started {
        map,
        raising,
        world,
        factions,
        computers,
        profiles,
    })
}

/// The sides of `open` that leave the match, and every side a computer
/// player plays: those of `computers`, and the open ones `players` require.
fn staying(
    players: &[Player],
    open: &BTreeSet<u32>,
    computers: &BTreeMap<u32, Option<Id>>,
) -> (HashSet<u32>, BTreeMap<u32, Option<Id>>) {
    let required = |p: &u32| players.iter().any(|q| q.id == *p && q.required);
    let (stay, leave): (Vec<u32>, Vec<u32>) = open.iter().partition(|p| required(p));
    let mut computers = computers.clone();
    computers.extend(stay.into_iter().map(|p| (p, None)));
    (leave.into_iter().collect(), computers)
}

/// What a side left to chance is given a profile from: each side's nation
/// and faction, by player id, and every profile.
type Picks<'a> = (
    &'a HashMap<u32, Nation>,
    &'a HashMap<u32, Faction>,
    &'a [Sai],
);

/// The profile each computer player of the match's `players` plays by, by
/// player id: the one `chosen` for it, else the one the map names for its
/// side, else one at random by `draw` that fits its side, else any.
fn computers(
    players: &[Player],
    chosen: &BTreeMap<u32, Option<Id>>,
    (nations, factions, sais): Picks,
    draw: u64,
) -> BTreeMap<u32, Id> {
    (chosen.iter())
        .filter_map(|(&p, sai)| Some((players.iter().find(|player| player.id == p)?, sai)))
        .filter_map(|(player, sai)| {
            let p = player.id;
            if let Some(sai) = sai {
                return Some((p, sai.clone()));
            }
            if let Some(named) = &player.ai_profile {
                if sais.iter().any(|s| s.id == *named) {
                    return Some((p, named.clone()));
                }
                // Such as `example_sai`, which no profile has.
                warn!("{}: no AI profile \"{named}\"", player.name);
            }
            let nation = nations.get(&p).map(|n| &n.id);
            let faction = factions.get(&p).map(|g| &g.id);
            let fitting: Vec<&Sai> = (sais.iter())
                .filter(|s| s.public && !s.easiest && s.fits(nation, faction))
                .collect();
            let any: Vec<&Sai> = sais.iter().collect();
            let from = if fitting.is_empty() { &any } else { &fitting };
            let sai = pick_at_random(draw, Pick::Computer, p, from)?;
            Some((p, sai.id.clone()))
        })
        .collect()
}

/// Each side's handicap, by player id: the one `chosen` for it, else the
/// one the map gives it.
pub fn side_handicaps(
    players: &[Player],
    chosen: &BTreeMap<u32, Id>,
    all: &[Handicap],
) -> HashMap<u32, Handicap> {
    tgmap::sides(players)
        .filter_map(|p| {
            let id = chosen.get(&p.id).or(p.handicap.as_ref())?;
            Some((p.id, all.iter().find(|h| h.id == *id)?.clone()))
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

/// How many of `voters` it takes to carry a vote: more than half.
pub(crate) fn majority(voters: usize) -> u32 {
    voters as u32 / 2 + 1
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
    Computer = 3,
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
    fn open_sides_leave_unless_required_when_a_computer_player_takes_them() {
        let mut players = players();
        players[2].required = true;
        let open = BTreeSet::from([12, 13]);
        let chosen = BTreeMap::from([(10, Some(Id::from("konev_sai")))]);
        let (leaving, computers) = staying(&players, &open, &chosen);
        assert_eq!(leaving, HashSet::from([13]));
        let expected = BTreeMap::from([(10, Some(Id::from("konev_sai"))), (12, None)]);
        assert_eq!(computers, expected);
    }

    #[test]
    fn computer_players_play_their_chosen_profile_else_the_maps_else_one_fitting_their_side() {
        let sai = |id: &str, nation: &str, faction: &str, public: bool, easiest: bool| Sai {
            id: Id::from(id),
            name: id.into(),
            nations: vec![Id::from(nation)],
            factions: vec![Id::from(faction)],
            public,
            easiest,
        };
        let sais = [
            sai("chuikov_sai", "nation_rus", "chuikov", true, false),
            sai("rus_sai", "nation_rus", "chuikov", false, false),
            sai("easy_chuikov_sai", "nation_rus", "chuikov", true, true),
        ];
        let nation = |id: &str| Nation {
            id: Id::from(id),
            name: id.into(),
            placeholders: HashMap::new(),
            factions: Vec::new(),
            flag: None,
            music: None,
            skin: Vec::new(),
        };
        let faction = |id: &str| Faction {
            id: Id::from(id),
            name: id.into(),
            short_name: id.into(),
            operations: Vec::new(),
            experience_max: None,
            public: true,
            portrait: None,
            computer_name: None,
            banner: None,
        };
        let nations = HashMap::from([(12, nation("nation_rus")), (13, nation("nation_usa"))]);
        let factions = HashMap::from([(12, faction("chuikov")), (13, faction("modded"))]);
        let chosen = BTreeMap::from([
            (10, Some(Id::from("konev_sai"))),
            (11, None),
            (12, None),
            (13, None),
            // Gone with its team.
            (14, Some(Id::from("patton_sai"))),
        ]);
        let mut players = players();
        players[0].ai_profile = Some(Id::from("chuikov_sai"));
        players[1].ai_profile = Some(Id::from("rus_sai"));
        // A profile no file has is passed over.
        players[2].ai_profile = Some(Id::from("example_sai"));
        for draw in 0..20 {
            let given = computers(&players, &chosen, (&nations, &factions, &sais), draw);
            assert_eq!(given.get(&10), Some(&Id::from("konev_sai")));
            // The map's, though not public.
            assert_eq!(given.get(&11), Some(&Id::from("rus_sai")));
            // The only public one fitting, not too easy.
            assert_eq!(given.get(&12), Some(&Id::from("chuikov_sai")));
            // None fits, so any.
            assert!(
                given
                    .get(&13)
                    .is_some_and(|s| sais.iter().any(|a| a.id == *s))
            );
            assert_eq!(given.len(), 4);
        }
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
