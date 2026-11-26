//! The message log at the top left of the screen: a line, behind its kind's
//! icon, for each thing the player is told, fading a while after it's
//! posted. Clicking a line's icon, or the key for it, looks at where it
//! happened. `docs/spec/messages.toml` is what it's built to.

mod draw;
mod log;
mod told;

pub(crate) use draw::{LogBox, Shade, WholeLog, cycle, list, log_node, relist, show, spawn};
pub(crate) use log::{About, MessageLog, filled, start};
pub(crate) use told::{By, Failed, Refused, refuse, tell, tell_leaving};

use bevy::prelude::*;

/// The key that steps the camera back through the lines.
pub const CYCLE: &str = "CycleMessageLog";
/// The log's width, before it wraps a line.
pub(crate) const WIDTH: f32 = 564.0;
pub(crate) const ICON: Vec2 = Vec2::new(16.0, 17.0);
/// Between the icon and its text.
pub(crate) const ICON_GAP: f32 = 8.0;
/// Above the icon, to sit it level with its text.
pub(crate) const ICON_MARGIN: f32 = 1.0;
pub(crate) const LINE_GAP: f32 = 3.0;
/// What keeps the text legible over pale ground, as there's no box behind
/// it.
const SHADOW: TextShadow = TextShadow {
    offset: Vec2::splat(1.0),
    color: Color::srgba(0.0, 0.0, 0.0, 0.9),
};
/// How long a line takes to fade once it's been up its while.
const FADE_SECONDS: f32 = 1.0;

/// Stock settings, for data that lacks them.
const SHOWN: usize = 5;
const KEPT: usize = 50;
const UP_MS: f32 = 10000.0;
const CYCLE_TIMEOUT: f32 = 1.0;

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// A log of no kinds of line or events but those its tests know.
    pub(crate) fn log() -> MessageLog {
        MessageLog::new(super::tests::look(&[]))
    }

    /// The text of each line it holds, oldest first.
    pub(crate) fn texts(log: &MessageLog) -> Vec<&str> {
        log.lines.iter().map(|l| l.text.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use tgdata::{GameEvent, Id, InterfaceVars};
    use tgserver::protocol::Said;
    use tgsim::{Channel, Notice, Pausing, Votes, World};

    use super::draw::*;
    use super::log::*;
    use super::told::*;
    use super::*;
    use crate::Leaders;

    pub(super) fn look(vars: &[(&str, &str)]) -> Look {
        let kind = |filter: Option<&str>| Kind {
            icon: None,
            modern: None,
            colour: Color::WHITE,
            filter: filter.map(Into::into),
            sound: None,
        };
        let event = |id: &str, kind: &str, message: &str, filter: Option<&str>| {
            let e = GameEvent {
                id: Id::from(id),
                message: Some(message.into()),
                kind: Some(Id::from(kind)),
                filter: filter.map(Into::into),
                message_filter: None,
            };
            (Id::from(id), e)
        };
        Look {
            kinds: HashMap::from([
                (
                    Id::from("GENERIC_EVENT"),
                    kind(Some("UIMessageFilterGeneric")),
                ),
                (Id::from("GENERIC_ERROR"), kind(None)),
            ]),
            events: HashMap::from([
                event("hq_attach", "GENERIC_EVENT", "%s: attaching %s", None),
                event(
                    "engaged",
                    "GENERIC_EVENT",
                    "%s engaged",
                    Some("UIEventFilterEngagement"),
                ),
            ]),
            vars: vars_of(vars),
            strings: Arc::default(),
            names: HashMap::new(),
            map: Default::default(),
            to_location: String::new(),
            gem: Default::default(),
        }
    }

    fn vars_of(vars: &[(&str, &str)]) -> InterfaceVars {
        let text: String = vars.iter().map(|(k, v)| format!("{k} = {v}\n")).collect();
        InterfaceVars::parse(&format!("[Vars]\n{{\n{text}}}\n"))
    }

    fn place(at: [f32; 2]) -> Option<Place> {
        Some(Place { about: None, at })
    }

    /// A match of sides 1 and 2, allies, and 3, their enemy.
    fn allied_sides() -> World {
        let mut players: Vec<_> = [1, 2, 3].map(crate::tests::player).into();
        for p in &mut players {
            p.relations = vec![tgmap::Relation::Enemy; 3];
        }
        players[0].relations[1] = tgmap::Relation::Ally;
        players[1].relations[0] = tgmap::Relation::Ally;
        let sides = tgsim::Sides {
            names: Default::default(),
            computers: Default::default(),
            players,
            teams: Vec::new(),
        };
        World::from_organizations(sides, tgmap::Pathing::uniform(8, tgmap::Domain::Land), &[])
    }

    #[test]
    fn chat_reaches_those_its_channel_does() {
        let sim = allied_sides();
        let look = look(&[]);
        let leaders = Leaders::default();
        let heard = |local, from, to| {
            let said = chat(&sim, local, &leaders, &look, from, to, "hi".into());
            said.map(|(kind, _)| kind)
        };
        // Everyone hears all, those watching too, and an enemy's in its
        // own kind.
        assert_eq!(heard(Some(2), Some(1), Channel::All), Some("GENERIC_EVENT"));
        assert_eq!(heard(Some(3), Some(1), Channel::All), Some("CHAT_PUBLIC"));
        assert_eq!(heard(None, Some(1), Channel::All), Some("CHAT_PUBLIC"));
        // Team chat reaches the sender and allies, or those watching.
        assert_eq!(
            heard(Some(1), Some(1), Channel::Team),
            Some("GENERIC_EVENT")
        );
        assert_eq!(
            heard(Some(2), Some(1), Channel::Team),
            Some("GENERIC_EVENT")
        );
        assert_eq!(heard(Some(3), Some(1), Channel::Team), None);
        assert_eq!(heard(None, Some(1), Channel::Team), None);
        assert_eq!(heard(None, None, Channel::Team), Some("CHAT_PUBLIC"));
        assert_eq!(heard(Some(1), None, Channel::Team), None);
        // A whisper, only where it's for, and not back to its sender.
        assert_eq!(
            heard(Some(3), Some(1), Channel::To(3)),
            Some("CHAT_PRIVATE")
        );
        assert_eq!(heard(Some(2), Some(1), Channel::To(3)), None);
        assert_eq!(heard(Some(1), Some(1), Channel::To(3)), None);
        assert_eq!(heard(Some(3), None, Channel::To(3)), None);
    }

    #[test]
    fn chat_shows_its_sender_and_words_in_their_colours() {
        let sim = allied_sides();
        let look = look(&[("point3 ChatTeamColor", "0,0.5,0")]);
        let leaders = Leaders::default();
        let said = chat(
            &sim,
            Some(2),
            &leaders,
            &look,
            Some(1),
            Channel::Team,
            "go".into(),
        );
        let (_, words) = said.unwrap();
        assert_eq!(words.text, "kingdom_1: go");
        let ink = |at, rgb| Ink { at, rgb };
        assert_eq!(
            words.inks,
            [
                ink(0, Some([1.0; 3])),
                ink(9, None),
                ink(11, Some([0.0, 0.5, 0.0])),
                ink(13, None),
            ]
        );
        let said = chat(
            &sim,
            None,
            &leaders,
            &look,
            None,
            Channel::All,
            "hello".into(),
        );
        assert_eq!(said.unwrap().1.text, "Observer: hello");
    }

    #[test]
    fn a_person_is_named_as_themselves_marked_as_their_side_if_they_have_one() {
        let sim = allied_sides();
        let player = sim.sides.player(1).unwrap();
        let bob = person(Some(&sim), "Bob", &[1]);
        assert_eq!(bob, Fill::side_of("Bob".into(), player));
        assert_eq!(
            person(Some(&sim), "Ann", &[]),
            Fill::from("Ann".to_string())
        );
    }

    #[test]
    fn what_is_said_in_the_connection_dialog_is_plain() {
        let look = look(&[]);
        let said = |from| Said {
            from,
            name: "Bob".into(),
            words: "wait".into(),
        };
        let line = said_line(&look, said(Some(1)));
        assert_eq!(line.text, "Bob: wait");
        assert!(line.inks.is_empty() && line.marks.is_empty());
        // One who watches is named all the same.
        assert_eq!(said_line(&look, said(None)).text, "Bob: wait");
    }

    #[test]
    fn a_vote_to_resign_names_the_voter_and_their_team() {
        let mut sim = allied_sides();
        sim.sides.teams.push(tgsim::Team {
            id: 10,
            name: "Allies".into(),
            relations: Vec::new(),
            team_relations: Vec::new(),
        });
        sim.sides.names.insert(1, "Bob".into());
        let (look, leaders) = (look(&[]), Leaders::default());
        let told = |notice| told(&sim, Some(3), &leaders, &look, notice);
        let vote = told(Notice::ResignVote { player: 1, team: 0 });
        let Some(Told::Words(kind, words)) = vote else {
            panic!("a vote is a line of its own");
        };
        assert_eq!(kind, "GENERIC_EVENT");
        assert_eq!(words.text, "Bob voted for Allies to resign");
        let resigned = told(Notice::TeamResigned { team: 0 });
        assert!(matches!(resigned, Some(Told::Event("team_resigned", ..))));
        let resigned = told(Notice::Resigned { player: 1 });
        assert!(matches!(
            resigned,
            Some(Told::Event("kingdom_resigned", ..))
        ));
    }

    #[test]
    fn a_side_taken_over_names_the_computer_player_then_its_profile() {
        let mut sim = allied_sides();
        sim.sides.computers.insert(1, "Bob AI".into());
        let (look, leaders) = (look(&[]), Leaders::default());
        let taken_over = Notice::TakenOver {
            player: 1,
            computer: "Bob AI".into(),
            by: "Kesselring AI".into(),
        };
        let Some(Told::Words(kind, words)) = told(&sim, Some(3), &leaders, &look, taken_over)
        else {
            panic!("a takeover is a line of its own");
        };
        assert_eq!(kind, "GENERIC_EVENT");
        assert_eq!(words.text, "Bob AI taken over by Kesselring AI");
        // The side now goes by the computer player's name.
        let side = &sim.sides.players[0];
        assert_eq!(leaders.owner(side, &sim.sides, &look.map), "Bob AI");
    }

    #[test]
    fn a_number_with_its_places_given_fills_like_any_other() {
        let line = fill_all("Game speed changed to %.0f%%", &["141"]);
        assert_eq!(line.text, "Game speed changed to 141%");
        assert_eq!(fill_all("%f and %.f", &["1", "2"]).text, "1 and 2");
        // What's not one is left as it is.
        assert_eq!(fill_all("%.x %s", &["a"]).text, "%.x a");
    }

    #[test]
    fn pausing_names_each_person_and_counts_the_votes() {
        let look = look(&[]);
        let line = |pausing| {
            let side = |id: u32| Fill::from(format!("P{id}"));
            paused_line(&look, pausing, side).text
        };
        let votes = Votes {
            votes: 1,
            needed: 3,
        };
        assert_eq!(line(Pausing::Paused { by: 1 }), "P1 has paused the game");
        assert_eq!(
            line(Pausing::Needed(votes)),
            "Votes needed to unpause 3, current votes 1"
        );
        let voted = |cast| Pausing::Voted { by: 2, cast, votes };
        assert_eq!(line(voted(true)), "P2 votes to unpause the game (1/3)");
        assert_eq!(line(voted(false)), "P2 has retracted vote to unpause (1/3)");
        assert_eq!(
            line(Pausing::Unpaused { by: 1 }),
            "P1 has unpaused the game"
        );
        assert_eq!(line(Pausing::VotedOff), "Game unpaused");
        assert_eq!(
            line(Pausing::Refused { by: 1 }),
            "P1 attempted to pause but failed due to recent votes"
        );
    }

    #[test]
    fn a_line_takes_colours_its_text_marks() {
        let words = fill_all("Whispering to %s: <color=1,1,1>%s", &["Bob", "hi <there>"]);
        assert_eq!(words.text, "Whispering to Bob: hi <there>");
        let white = Ink {
            at: 19,
            rgb: Some([1.0; 3]),
        };
        assert_eq!(words.inks, [white]);
        let line = Line {
            serial: 0,
            kind: Id::from("CHAT_PRIVATE"),
            text: words.text,
            marks: Vec::new(),
            inks: words.inks,
            place: None,
            posted: 0.0,
        };
        let inks: Vec<_> = parts_of(&line)
            .into_iter()
            .map(|p| (p.word, p.ink))
            .collect();
        assert_eq!(inks[2], ("Bob:".to_string(), None));
        assert_eq!(inks[3], ("hi".to_string(), Some([1.0; 3])));
    }

    #[test]
    fn only_the_player_here_hears_buildings_change_hands() {
        let lost = |player, by| Notice::Lost {
            player,
            building: 0,
            by,
        };
        let captured = |player| Notice::Captured {
            player,
            building: 0,
        };
        assert_eq!(
            sound(&lost(1, 2), Some(1)),
            Some("actor_captured_by_enemy_sfx")
        );
        assert_eq!(sound(&captured(1), Some(1)), Some("actor_captured_sfx"));
        // One AI taking from another.
        assert_eq!(sound(&lost(2, 3), Some(1)), None);
        assert_eq!(sound(&captured(3), Some(1)), None);
        assert_eq!(sound(&captured(1), None), None);
        assert_eq!(sound(&Notice::Repaired { player: 1 }, Some(1)), None);
    }

    #[test]
    fn placeholders_fill_in_turn() {
        assert_eq!(
            fill_all("%s: attaching %s", &["1st HQ", "2nd Regiment"]).text,
            "1st HQ: attaching 2nd Regiment"
        );
        assert_eq!(
            fill_all("Your production is %d%% short", &["40"]).text,
            "Your production is 40% short"
        );
        assert_eq!(
            fill_all("%i%s given to %s", &["50", "$", "Bob"]).text,
            "50$ given to Bob"
        );
        assert_eq!(fill_all("100% %x", &[] as &[&str]).text, "100% %x");
    }

    #[test]
    fn a_side_named_in_a_line_is_marked_by_its_gem() {
        let red = [1.0, 0.0, 0.0];
        let with = [Fill::side("Germany".into(), red), "Tank".into()];
        let words = fill_all("%s's %s destroyed", &with);
        assert_eq!(words.text, "Germany's Tank destroyed");
        assert_eq!(words.marks, [Mark { at: 0, rgb: red }]);
        let line = Line {
            serial: 0,
            kind: Id::from("GENERIC_EVENT"),
            text: words.text,
            marks: words.marks,
            inks: Vec::new(),
            place: None,
            posted: 0.0,
        };
        let part = |gem, word: &str| Part {
            gem,
            word: word.into(),
            ink: None,
        };
        assert_eq!(
            parts_of(&line),
            [
                part(Some(red), "Germany's"),
                part(None, "Tank"),
                part(None, "destroyed"),
            ]
        );
        // And mid-line, only where the side is.
        let words = fill_all("Player %s quit the game", &[Fill::side("Bob".into(), red)]);
        assert_eq!(words.marks, [Mark { at: 7, rgb: red }]);
        let line = Line {
            text: words.text,
            marks: words.marks,
            ..line
        };
        let parts = parts_of(&line);
        assert_eq!(parts[..2], [part(None, "Player"), part(Some(red), "Bob")]);
        // A side with no name still shows its gem, at the end too.
        let words = fill_all("captured by %s", &[Fill::side(String::new(), red)]);
        let line = Line {
            text: words.text,
            marks: words.marks,
            ..line
        };
        assert_eq!(parts_of(&line).last(), Some(&part(Some(red), "")));
        // An independent side's name is left unmarked.
        let mut civilian = crate::tests::player(3);
        civilian.independent = true;
        assert_eq!(Fill::side_of("Civilian".into(), &civilian).side, None);
        let kingdom = crate::tests::player(1);
        assert_eq!(
            Fill::side_of("Bob".into(), &kingdom).side,
            Some(kingdom.rgb)
        );
    }

    #[test]
    fn events_post_their_lines_unless_filtered() {
        let mut log = MessageLog::new(look(&[]));
        assert!(log.event("hq_attach", &["HQ", "Regiment"], None, 0.0));
        // An event filter the files don't set is off.
        assert!(!log.event("engaged", &["Regiment"], None, 0.0));
        assert!(!log.event("unknown", &[] as &[&str], None, 0.0));
        let texts: Vec<&str> = log.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["HQ: attaching Regiment"]);

        let mut log = MessageLog::new(look(&[
            ("UIMessageFilterGeneric", "false"),
            ("UIEventFilterEngagement", "true"),
        ]));
        assert!(!log.event("engaged", &["Regiment"], None, 0.0));
        assert!(log.post("GENERIC_ERROR", "Cannot unpack".to_string(), None, 0.0));
    }

    #[test]
    fn lines_stay_up_then_fade_and_the_newest_show() {
        let mut log = MessageLog::new(look(&[
            ("UIMessageLogMaximumLines", "2"),
            ("UIMessageLogFadeTime", "10000"),
            ("MessageLogMaximumLines", "3"),
        ]));
        for (i, at) in [0.0, 1.0, 2.0, 3.0].into_iter().enumerate() {
            log.post("GENERIC_ERROR", format!("{i}"), None, at);
        }
        let shown = |log: &MessageLog, now| -> Vec<(String, f32)> {
            (log.on_screen(now).into_iter())
                .map(|(l, a)| (l.text.clone(), a))
                .collect()
        };
        assert_eq!(shown(&log, 3.0), [("2".into(), 1.0), ("3".into(), 1.0)]);
        assert_eq!(shown(&log, 12.5), [("2".into(), 0.5), ("3".into(), 1.0)]);
        assert_eq!(shown(&log, 13.0), [("3".into(), 1.0)]);
        assert_eq!(shown(&log, 14.0), []);
        // Only so many are kept.
        let kept: Vec<&str> = log.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(kept, ["1", "2", "3"]);
    }

    #[test]
    fn the_key_steps_back_through_placed_lines_and_round() {
        let mut log = MessageLog::new(look(&[("UIMessageLogCycleTimeout", "1.0")]));
        log.post("GENERIC_ERROR", "a".to_string(), place([1.0, 0.0]), 0.0);
        log.post("GENERIC_ERROR", "unplaced".to_string(), None, 0.0);
        log.post("GENERIC_ERROR", "b".to_string(), place([2.0, 0.0]), 0.0);
        let mut at = |now| log.cycle(now).map(|l| l.text.clone());
        assert_eq!(at(10.0).as_deref(), Some("b"));
        assert_eq!(at(10.5).as_deref(), Some("a"));
        // Round to the newest after the oldest.
        assert_eq!(at(11.0).as_deref(), Some("b"));
        assert_eq!(at(11.5).as_deref(), Some("a"));
        // A longer pause starts from the newest again.
        assert_eq!(at(20.0).as_deref(), Some("b"));
    }

    #[test]
    fn only_our_interface_draws_our_icons() {
        use bevy::ecs::world::CommandQueue;

        let original = Handle::<Image>::Uuid(bevy::asset::uuid::Uuid::from_u128(7), default());
        let mut log = MessageLog::new(look(&[]));
        let kind = log.look.kinds.get_mut(&Id::from("GENERIC_EVENT")).unwrap();
        kind.icon = Some(original.clone());
        kind.modern = Some("Modern/event.svg".into());
        assert!(log.event("hq_attach", &["HQ", "Regiment"], None, 0.0));
        let drawn = |hud| {
            let mut world = bevy::ecs::world::World::new();
            let root = world.spawn_empty().id();
            let mut queue = CommandQueue::default();
            let mut commands = Commands::new(&mut queue, &world);
            spawn_row(&mut commands, root, &log.look, &log.lines[0], hud);
            queue.apply(&mut world);
            let ours = world.query::<&crate::displays::Icon>().iter(&world).count();
            let theirs = (world.query::<&ImageNode>().iter(&world)).any(|i| i.image == original);
            (ours, theirs)
        };
        assert_eq!(drawn(crate::hud::Hud::Modern), (1, false));
        assert_eq!(drawn(crate::hud::Hud::Original), (0, true));
    }
}
