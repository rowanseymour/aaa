//! What the log is told: refused orders, who joins, leaves and pauses,
//! what's said, and the sim's notices.

use bevy::prelude::*;
use tgserver::protocol::Said;
use tgsim::{Channel, Notice, Pause, Pausing, Piece, Relation, Seen, Votes, World};

use crate::actions::{self, Aim, fill};
use crate::sound::Sounds;
use crate::{Leaders, Local, Orders, Sim};

use super::log::{About, Fill, Look, MessageLog, Place, Words, fill_all};

/// A click that aimed an order at something it can't be given on: `by`
/// the order that refuses such a thing, or none where none of the selected
/// can attack it.
#[derive(Message)]
pub struct Refused {
    pub about: About,
    pub by: Option<Aim>,
}

/// An order the sim refused, as the player's told.
#[derive(Message)]
pub struct Failed {
    pub text: String,
    /// What it was given to, where the message points, if anything.
    pub about: Option<About>,
}

/// Tells the player of each order just [`Refused`] or [`Failed`].
pub fn refuse(
    mut refused: MessageReader<Refused>,
    mut failed: MessageReader<Failed>,
    sim: Res<Sim>,
    looks: Res<actions::Looks>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    for Refused { about, by } in refused.read() {
        let name = match *about {
            About::Organization(r) => log.look.organization(&sim.0, r),
            About::Building(b) => log.look.building(&sim.0, b),
        };
        let text = match by {
            Some(aim) => {
                let said = log.look.said(
                    "ui_error_command_target_actor|ss",
                    "%s is an invalid target for %s",
                );
                fill(&said, &[&name, looks.order(*aim)])
            }
            None => {
                let said = log
                    .look
                    .said("kill_error_invalid_target|s", "Invalid target: %s");
                fill(&said, &[&name])
            }
        };
        let place = Some(Place::of(&sim.0, *about));
        log.say("GENERIC_ERROR", text, place, &time, &mut sounds);
    }
    for Failed { text, about } in failed.read() {
        let place = about.map(|a| Place::of(&sim.0, a));
        log.say("GENERIC_ERROR", text.clone(), place, &time, &mut sounds);
    }
}

/// Posts a line once the host is leaving a match that's over, then one
/// for each machine that has left the match since last time, whether by
/// choice, by being lost or by being kicked out, the host's among them,
/// and one for each kick vote cast or taken back. While the match is
/// still being set up, nobody's side is marked.
pub fn tell_leaving(
    mut orders: ResMut<Orders>,
    sim: Option<Res<Sim>>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    if orders.take_ended() {
        let said = log.look.said(
            "ui_mission_message_host_has_ended_game",
            "The host has ended the game.",
        );
        log.say("GENERIC_EVENT", said, None, &time, &mut sounds);
    }
    let sim = sim.as_deref().map(|s| &s.0);
    let person = |name: &str, players: &[u32]| person(sim, name, players);
    let mut lines = Vec::new();
    // The votes came before any kick they brought.
    for kick in orders.take_kicking() {
        let named = |client| {
            orders
                .seen(client)
                .map(|(players, name)| person(name, players))
        };
        let (Some(voter), Some(target)) = (named(kick.voter), named(kick.target)) else {
            continue;
        };
        let said = match kick.vote {
            true => log.look.said(
                "networking_vote_to_kick_player_voted_to_kick_player|ssdd",
                "%s has voted to kick %s (%d/%d)",
            ),
            false => log.look.said(
                "networking_vote_to_kick_player_has_retracted_vote_to_kick_player|ssdd",
                "%s has retracted vote to kick %s (%d/%d)",
            ),
        };
        let counts = [kick.votes, kick.needed].map(|n| Fill::from(n.to_string()));
        lines.push(fill_all(&said, &[[voter, target], counts].concat()));
    }
    for gone in orders.take_left() {
        let (client, players) = (gone.client, &gone.players);
        info!("client {client} has left the match, giving the orders of {players:?}");
        let said = match gone.kicked {
            true => log.look.said(
                "networking_player_kicked|s",
                "Player %s was kicked from the game",
            ),
            false => log
                .look
                .said("networking_player_quit|s", "Player %s quit the game"),
        };
        lines.push(fill_all(&said, &[person(&gone.name, players)]));
    }
    for words in lines {
        log.say("GENERIC_EVENT", words, None, &time, &mut sounds);
    }
    // What's said in the connection dialog makes no sound.
    for said in orders.take_said() {
        let words = said_line(&log.look, said);
        log.post("GENERIC_EVENT", words, None, time.elapsed_secs());
    }
}

/// A person as lines name them: by their own name, marked as the first of
/// `players`, the sides they play, is; unmarked if they only watch.
pub(crate) fn person(sim: Option<&World>, name: &str, players: &[u32]) -> Fill {
    let side = players.first().and_then(|&p| sim?.sides.player(p));
    match side {
        Some(player) => Fill::side_of(name.to_string(), player),
        None => Fill::from(name.to_string()),
    }
}

/// The line for what's said in the connection dialog: plain, in none of
/// the chat bar's colours.
pub(super) fn said_line(look: &Look, said: Said) -> Words {
    fill_all(
        &look.said("public_chat|ss", "%s: %s"),
        &[said.name, said.words],
    )
}

/// The line for what came of someone asking to pause or unpause, with
/// each person named by `side`, as their side's name.
pub(super) fn paused_line(look: &Look, pausing: Pausing, side: impl Fn(u32) -> Fill) -> Words {
    let count = |n: u32| Fill::from(n.to_string());
    let (key, or, fills) = match pausing {
        Pausing::Paused { by } => (
            "networking_vote_to_pause_player_game_paused|s",
            "%s has paused the game",
            vec![side(by)],
        ),
        // The count needed comes first here.
        Pausing::Needed(Votes { votes, needed }) => (
            "networking_vote_to_pause_needed_votes|dd",
            "Votes needed to unpause %d, current votes %d",
            vec![count(needed), count(votes)],
        ),
        Pausing::Unpaused { by } => (
            "networking_vote_to_pause_player_game_unpaused|s",
            "%s has unpaused the game",
            vec![side(by)],
        ),
        Pausing::Voted { by, cast, votes } => {
            let (key, or) = match cast {
                true => (
                    "networking_vote_to_pause_player_votes_to_unpause|sdd",
                    "%s votes to unpause the game (%d/%d)",
                ),
                false => (
                    "networking_vote_to_pause_player_retracts_vote|sdd",
                    "%s has retracted vote to unpause (%d/%d)",
                ),
            };
            (
                key,
                or,
                vec![side(by), count(votes.votes), count(votes.needed)],
            )
        }
        Pausing::VotedOff => (
            "networking_vote_to_pause_game_unpaused",
            "Game unpaused",
            vec![],
        ),
        Pausing::Refused { by } => (
            "networking_vote_to_pause_pause_rejected|s",
            "%s attempted to pause but failed due to recent votes",
            vec![side(by)],
        ),
    };
    fill_all(&look.said(key, or), &fills)
}

/// What one who only watches is called.
pub(crate) fn observer(look: &Look) -> String {
    look.said("game_interface_KingdomDropDown_observer_text", "Observer")
}

/// Posts a line for each thing the sim has told the player since last
/// time.
pub fn tell(
    mut sim: ResMut<Sim>,
    local: Res<Local>,
    leaders: Res<Leaders>,
    time: Res<Time>,
    mut log: ResMut<MessageLog>,
    mut sounds: ResMut<Sounds>,
) {
    // Taking them marks the sim changed, which what redraws on a change
    // would then do every frame.
    if sim.0.notices().is_empty() {
        return;
    }
    let notices = sim.0.take_notices();
    let now = time.elapsed_secs();
    for notice in notices {
        if let Some(sound) = sound(&notice, local.0) {
            sounds.interface(&time, sound);
        }
        let posted = match told(&sim.0, local.0, &leaders, &log.look, notice) {
            Some(Told::Event(event, with, place)) => log.event(event, &with, place, now),
            Some(Told::Line(kind, text, place)) => log.post(kind, text, place, now),
            Some(Told::Words(kind, words)) => log.post(kind, words, None, now),
            // Chat sounds whether or not its line shows, and as itself
            // rather than as its line's kind.
            Some(Told::Chat(kind, words)) => {
                log.post(kind, words, None, now);
                if log.look.vars.flag("UIEventFilterChatSound", true) {
                    sounds.interface(&time, "chat_sfx");
                }
                false
            }
            None => false,
        };
        if posted {
            log.sound(&time, &mut sounds);
        }
    }
}

/// The sound `local`, the player here, hears for a notice: none for one
/// told to anyone else.
pub(crate) fn sound(notice: &Notice, local: Option<u32>) -> Option<&'static str> {
    match *notice {
        Notice::Lost { player, .. } if Some(player) == local => Some("actor_captured_by_enemy_sfx"),
        Notice::Captured { player, .. } if Some(player) == local => Some("actor_captured_sfx"),
        _ => None,
    }
}

/// What a notice tells the player.
pub(crate) enum Told {
    /// Chat, as a line of a kind of message.
    Chat(&'static str, Words),
    /// A game event, with what fills its line.
    Event(&'static str, Vec<Fill>, Option<Place>),
    /// A line of a kind of message.
    Line(&'static str, String, Option<Place>),
    /// A line, with what fills it, of a kind of message.
    Words(&'static str, Words),
}

/// The line chat from the person playing `from`, or watching for none,
/// is posted as for `local`, the side here, with its kind, if it reaches
/// them.
pub(crate) fn chat(
    sim: &World,
    local: Option<u32>,
    leaders: &Leaders,
    look: &Look,
    from: Option<u32>,
    to: Channel,
    words: String,
) -> Option<(&'static str, Words)> {
    let allied = |a: u32, b: u32| a == b || sim.sides.relation(a, b) == Relation::Ally;
    let hears = match to {
        Channel::All => true,
        // Those watching talk among themselves.
        Channel::Team => match (local, from) {
            (Some(local), Some(from)) => allied(local, from),
            (None, None) => true,
            _ => false,
        },
        // Whoever whispers was shown it as they sent it.
        Channel::To(side) => from.is_some_and(|from| local == Some(side) && from != side),
    };
    if !hears {
        return None;
    }
    let (key, channel_ink) = match to {
        Channel::All => ("public_chat|ss", ("ChatPublicColor", [1.0, 1.0, 0.0])),
        Channel::Team => ("team_chat|ss", ("ChatTeamColor", [0.0, 1.0, 0.0])),
        Channel::To(_) => ("private_chat|ss", ("ChatWhisperColor", [0.0, 0.9, 0.9])),
    };
    let ink = |(var, stock): (&str, [f32; 3])| look.vars.rgb(var).unwrap_or(stock);
    let name = match from.and_then(|f| sim.sides.player(f)) {
        Some(player) => leaders.owner(player, &sim.sides, &look.map),
        None => observer(look),
    };
    let with = [
        Fill::inked(name, ink(("UIChatPlayerColorNormal", [1.0; 3]))),
        Fill::inked(words, ink(channel_ink)),
    ];
    let friendly = from
        .zip(local)
        .is_some_and(|(from, local)| allied(from, local));
    let kind = match (friendly, to) {
        (true, _) => "GENERIC_EVENT",
        (false, Channel::To(_)) => "CHAT_PRIVATE",
        (false, _) => "CHAT_PUBLIC",
    };
    Some((kind, fill_all(&look.said(key, "%s: %s"), &with)))
}

/// What `notice` tells `local`, if it's for them.
pub(crate) fn told(
    sim: &World,
    local: Option<u32>,
    leaders: &Leaders,
    look: &Look,
    notice: Notice,
) -> Option<Told> {
    let ours = |player: u32| Some(player) == local;
    let at = |about| Some(Place::of(sim, about));
    // What happens to another side's thing is told only where it's seen.
    let sees = |owner: u32, place: &Place| {
        ours(owner) || local.is_some_and(|p| sim.seen(p, place.at) == Seen::Visible)
    };
    let side = |id: u32| match sim.sides.player(id) {
        Some(p) => Fill::side_of(leaders.owner(p, &sim.sides, &look.map), p),
        None => Fill::from(String::new()),
    };
    let team_name = |team: usize| {
        let name = &sim.sides.teams[team].name;
        look.map.get(name).unwrap_or(name).clone()
    };
    Some(match notice {
        Notice::Pausing(pausing) => Told::Words("GENERIC_EVENT", paused_line(look, pausing, side)),
        Notice::Speed(speed) => {
            let said = look.said(
                "networking_change_game_speed|f",
                "Game speed changed to %.0f%%",
            );
            let percent = format!("{:.0}", speed * 100.0);
            Told::Words("GENERIC_EVENT", fill_all(&said, &[percent]))
        }
        Notice::Chat { from, to, words } => {
            let (kind, words) = chat(sim, local, leaders, look, from, to, words)?;
            Told::Chat(kind, words)
        }
        Notice::Short { player, percent } if ours(player) => {
            let percent = (percent as i64).to_string();
            Told::Event("kingdom_upkeep_shortage", vec![percent.into()], None)
        }
        Notice::Repaired { player } if ours(player) => {
            Told::Event("kingdom_upkeep_repaired", vec![], None)
        }
        Notice::Paused {
            player,
            building,
            template,
            why,
        } if ours(player) => {
            let text = match why {
                Pause::Limit(_) => {
                    let said = look.said(
                        "recruit_error_unit_limit|s",
                        "Insufficient unit limit to recruit %s",
                    );
                    fill(&said, &[&look.name(&template)])
                }
                Pause::Disabled | Pause::Supply => {
                    let said = match why {
                        Pause::Supply => look.said(
                            "structure_error_queue_no_supply|s",
                            "%s paused production queue - no supply",
                        ),
                        _ => look.said(
                            "structure_error_queue_disabled|s",
                            "%s paused production queue - disabled",
                        ),
                    };
                    fill(&said, &[&look.building(sim, building)])
                }
            };
            Told::Line("GENERIC_ERROR", text, at(About::Building(building)))
        }
        Notice::NotCreated { player, template } if ours(player) => {
            let said = look.said("recruit_error_creating|s", "Could not create %s");
            let text = fill(&said, &[&look.name(template.as_str())]);
            Told::Line("GENERIC_ERROR", text, None)
        }
        Notice::CannotAfford {
            player,
            organization,
            template,
        } if ours(player) => {
            let said = look.said(
                "construct_error_affording|ss",
                "Insufficient funds for %s. Requires %s.",
            );
            let cost = sim.site_cost(&template).ceil().to_string();
            let text = fill(&said, &[&look.name(template.as_str()), &cost]);
            Told::Line("GENERIC_ERROR", text, at(About::Organization(organization)))
        }
        Notice::NotBuilt {
            player,
            organization,
            template,
        } if ours(player) => {
            let said = look.said("construct_error_creating|s", "Could not create %s");
            let text = fill(&said, &[&look.name(template.as_str())]);
            Told::Line("GENERIC_ERROR", text, at(About::Organization(organization)))
        }
        Notice::Constructed { player, building } if ours(player) => Told::Event(
            "building_construction_complete",
            vec![look.building(sim, building).into()],
            at(About::Building(building)),
        ),
        Notice::Mended { player, building } if ours(player) => Told::Event(
            "building_repair_complete",
            vec![look.building(sim, building).into()],
            at(About::Building(building)),
        ),
        Notice::HeroKilled {
            player,
            hero,
            organization,
        } if ours(player) => Told::Event(
            "hero_killed",
            vec![look.name(hero.as_str()).into()],
            at(About::Organization(organization)),
        ),
        Notice::SummonFailed { player, template } if ours(player) => {
            let said = look.said("ability_effect_summon_failed|s", "Failed to summon %s");
            let text = fill(&said, &[&look.name(template.as_str())]);
            Told::Line("GENERIC_ERROR", text, None)
        }
        Notice::CannotUnpack {
            player,
            organization,
        } if ours(player) => {
            let said = look.said("tai_error_unpack|s", "Cannot unpack %s");
            let text = fill(
                &said,
                &[&look.name(sim.organizations[organization].template.as_str())],
            );
            Told::Line("GENERIC_ERROR", text, at(About::Organization(organization)))
        }
        Notice::Attached {
            player,
            hq,
            organization,
        }
        | Notice::Detached {
            player,
            hq,
            organization,
        } if ours(player) => {
            let event = match notice {
                Notice::Attached { .. } => "hq_attach",
                _ => "hq_detach",
            };
            let hqs = sim.hqs();
            let hq = hqs.iter().find(|h| h.id == hq);
            let hq = hq.map(|h| look.hq(sim, h)).unwrap_or_default();
            let joining = look.organization(sim, organization);
            Told::Event(
                event,
                vec![hq.into(), joining.into()],
                at(About::Organization(organization)),
            )
        }
        Notice::PutOut {
            player,
            organization,
            ..
        } if ours(player) => {
            // An organization is commissioned, a unit on its own such as a
            // truck recruited.
            let event = match crate::object_display::lone(sim, organization) {
                true => "company_recruited",
                false => "company_commissioned",
            };
            let name = look.organization(sim, organization);
            Told::Event(
                event,
                vec![name.into()],
                at(About::Organization(organization)),
            )
        }
        // Named by its template: it has no number until it's dropped.
        Notice::Recruited {
            player, delivery, ..
        } if ours(player) => {
            let delivery = &sim.deliveries[delivery];
            let dropped = delivery
                .organization
                .map(|r| &sim.organizations[r].template);
            let name = delivery.cargo().or(dropped).map(|t| look.name(t.as_str()));
            let place = Place {
                about: None,
                at: delivery.position,
            };
            Told::Event(
                "company_recruited",
                name.into_iter().map(Fill::from).collect(),
                Some(place),
            )
        }
        Notice::Researched {
            player,
            building,
            technology,
        } if ours(player) => {
            let with = vec![look.name(technology.as_str()).into()];
            Told::Event("research_complete", with, at(About::Building(building)))
        }
        Notice::Gained { player, technology } if ours(player) => Told::Event(
            "technology_gained",
            vec![look.name(technology.as_str()).into()],
            None,
        ),
        // A side always sees its own, so is always told: the loser while
        // it's still theirs, the taker once it is.
        Notice::Lost {
            player,
            building,
            by,
        } if ours(player) => {
            let with = vec![look.building(sim, building).into(), side(by)];
            Told::Event(
                "actor_captured_by_enemy",
                with,
                at(About::Building(building)),
            )
        }
        Notice::Captured { player, building } if ours(player) => {
            let with = vec![look.building(sim, building).into()];
            Told::Event("actor_captured", with, at(About::Building(building)))
        }
        Notice::Sold {
            player,
            building,
            paid,
        } if ours(player) => {
            let with = vec![look.building(sim, building).into(), paid.to_string().into()];
            Told::Event("building_sold", with, at(About::Building(building)))
        }
        Notice::Levelled {
            player,
            organization,
            ..
        } if ours(player) => Told::Event(
            "company_experience_gained_level",
            vec![look.organization(sim, organization).into()],
            at(About::Organization(organization)),
        ),
        Notice::Routed {
            player,
            organization,
        } if ours(player) => Told::Event(
            "company_routed",
            vec![look.organization(sim, organization).into()],
            at(About::Organization(organization)),
        ),
        // Its owner is told it's lost, and its enemies who see it go that
        // it's destroyed, whoever did it. What's nobody's tells no one.
        Notice::Destroyed { owner, what } if sim.sides.player(owner).is_some() => {
            let (name, about, event) = match what {
                Piece::Organization(r) => (
                    look.organization(sim, r),
                    About::Organization(r),
                    "mobile_actor_lost",
                ),
                Piece::Building(b) => (
                    look.building(sim, b),
                    About::Building(b),
                    "static_actor_lost",
                ),
            };
            let place = Place::of(sim, about);
            let enemy = local.is_some_and(|p| sim.sides.relation(p, owner) == Relation::Enemy);
            match ours(owner) {
                true => Told::Event(event, vec![name.into()], Some(place)),
                false if enemy && sees(owner, &place) => Told::Event(
                    "actor_destroyed",
                    vec![side(owner), name.into()],
                    Some(place),
                ),
                false => return None,
            }
        }
        // How the match comes out is told to everyone.
        Notice::Defeated { player } => Told::Event("kingdom_defeated", vec![side(player)], None),
        Notice::Victorious { player } => Told::Event("kingdom_victory", vec![side(player)], None),
        Notice::Resigned { player } => Told::Event("kingdom_resigned", vec![side(player)], None),
        Notice::TeamResigned { team } => {
            Told::Event("team_resigned", vec![team_name(team).into()], None)
        }
        Notice::TakenOver {
            player,
            person,
            computer,
        } => {
            let said = look.said("sai_midgame_name_change|ss", "%s taken over by %s");
            let named = |name: String| match sim.sides.player(player) {
                Some(p) => Fill::side_of(name, p),
                None => Fill::from(name),
            };
            let with = [named(person), named(computer)];
            Told::Words("GENERIC_EVENT", fill_all(&said, &with))
        }
        Notice::ResignVote { player, team } => {
            let said = look.said(
                "networking_player_voted_to_resign_team|ss",
                "%s voted for %s to resign",
            );
            let with = [side(player), team_name(team).into()];
            Told::Words("GENERIC_EVENT", fill_all(&said, &with))
        }
        Notice::TeamDefeated { team } => {
            Told::Event("team_defeated", vec![team_name(team).into()], None)
        }
        Notice::TeamVictorious { team } => {
            Told::Event("team_victory", vec![team_name(team).into()], None)
        }
        Notice::CitiesHeld { player, cities } => {
            let team = sim.sides.player(player).and_then(|p| p.team);
            let name = team.map_or_else(|| side(player), |t| team_name(t).into());
            Told::Event(
                "city_ratio_victory",
                vec![name, cities.to_string().into()],
                None,
            )
        }
        _ => return None,
    })
}
