//! Leaving a match for its debriefing, and putting the match away behind
//! it so the menus, and another match, can follow. `spec/outcome.toml` and
//! `spec/objectives.toml` say what the debriefing shows.

use std::collections::HashSet;

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use tgsim::{Objective, Outcome, Standing};

use crate::campaigns::{Campaigns, Playing, Progress};
use crate::ui::{Font, Ink, Surface, button, called, label, panel};
use crate::{Loaded, Local, Screen, Sim, confirm};

pub struct LeavingPlugin;

impl Plugin for LeavingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnExit(Screen::Match), put_away)
            .add_systems(OnEnter(Screen::Debriefing), spawn);
        // A match can also be left, or fail to load, before it's begun.
        for entered in [
            Screen::MainMenu,
            Screen::Lobby,
            Screen::Lan,
            Screen::Campaign,
        ] {
            let left = OnTransition {
                exited: Screen::Loading,
                entered,
            };
            app.add_systems(left, put_away);
        }
    }
}

/// What's being played, as the menus launched it.
#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct Mission {
    /// What the loading screen called it.
    pub(crate) title: String,
}

/// How the match came out for the player's side, as it stood when they
/// left, for the debriefing.
#[derive(Resource, Clone, Debug)]
pub(crate) struct Debrief {
    title: String,
    /// Lost, or still playing when left, is a defeat.
    won: bool,
    objectives: Vec<Objective>,
    /// The boxes a completed objective and any other are drawn behind.
    icons: [Option<String>; 2],
    /// The campaign mission it was, if one.
    campaign: Option<Debriefed>,
}

/// What the debriefing of a campaign mission adds.
#[derive(Clone, Debug)]
struct Debriefed {
    playing: Playing,
    /// What the campaign says of it, won or not.
    said: String,
    /// Whether there's a mission after it to go on to: there is once it's
    /// complete, unless it's the campaign's last.
    next: bool,
}

/// The entities that were there before the match began loading, which
/// putting it away leaves.
#[derive(Resource)]
pub(crate) struct Kept(HashSet<Entity>);

/// What's drawn or laid out, the entities a match can leave behind. Not
/// the app's own, such as its observers and systems.
type Shown = Or<(With<Transform>, With<Node>)>;

/// Notes what's there as a match begins loading. Many of the things a
/// match spawns aren't tied to its screen, so this is how they're found
/// once it's left.
pub(crate) fn note_kept(world: &mut World) {
    let mut shown = world.query_filtered::<Entity, Shown>();
    let kept = shown.iter(world).collect();
    world.insert_resource(Kept(kept));
}

/// The campaign mission being played, if one, and the campaigns' state.
type InCampaign<'w> = (
    Option<Res<'w, Playing>>,
    Option<Res<'w, Campaigns>>,
    Option<ResMut<'w, Progress>>,
);

/// Leaves the match for its debriefing, or for the menus with none if
/// it hasn't begun. A campaign mission won is complete from here.
#[allow(clippy::too_many_arguments)]
pub(crate) fn end_mission(
    _: On<Activate>,
    mut commands: Commands,
    sim: Option<Res<Sim>>,
    local: Res<Local>,
    loaded: Option<Res<Loaded>>,
    mission: Option<Res<Mission>>,
    campaign: InCampaign<'_>,
    mut next: ResMut<NextState<Screen>>,
) {
    let (Some(sim), Some(loaded)) = (sim, loaded) else {
        next.set(Screen::MainMenu);
        return;
    };
    let side = local.0;
    let won = side.is_some_and(|s| sim.0.outcome(s) == Outcome::Won);
    let held = side.and_then(|s| sim.0.objectives.get(&s));
    let looks = loaded.game.objective_looks();
    let debriefed = match campaign {
        (Some(playing), Some(campaigns), Some(mut progress)) => {
            debrief(&playing, &campaigns, &mut progress, won)
        }
        _ => None,
    };
    commands.insert_resource(Debrief {
        title: mission.map_or_else(|| loaded.map.title.clone(), |m| m.title.clone()),
        won,
        objectives: held.map(|h| h.0.clone()).unwrap_or_default(),
        icons: [looks.completed.icon, looks.failed.icon],
        campaign: debriefed,
    });
    next.set(Screen::Debriefing);
}

/// What the debriefing of a campaign mission says, marking it complete
/// if it was `won`.
fn debrief(
    playing: &Playing,
    campaigns: &Campaigns,
    progress: &mut Progress,
    won: bool,
) -> Option<Debriefed> {
    let c = campaigns.list.get(playing.campaign)?;
    let m = c.missions.get(playing.mission)?;
    if won {
        progress.completed(&c.id, playing.mission);
    }
    let complete = playing.mission < progress.complete(&c.id);
    Some(Debriefed {
        said: if won { &m.success } else { &m.failure }.clone(),
        next: complete && playing.mission + 1 < c.missions.len(),
        playing: playing.clone(),
    })
}

/// Asks whether to end the mission, and leaves for the debriefing if so.
pub(crate) fn ask_end(_: On<Activate>, mut commands: Commands, words: Res<confirm::Words>) {
    let question = words.sure.clone();
    confirm::confirm(&mut commands, &words, Screen::Match, question, end_mission);
}

/// Takes down everything the match spawned and the state it was played
/// from, leaving its host if there's one here, and lets the clock run
/// again for the menus.
fn put_away(world: &mut World) {
    let kept = world
        .remove_resource::<Kept>()
        .map(|k| k.0)
        .unwrap_or_default();
    let mut shown = world.query_filtered::<Entity, (Shown, Without<ChildOf>)>();
    let spawned: Vec<Entity> = shown.iter(world).filter(|e| !kept.contains(e)).collect();
    for entity in spawned {
        world.despawn(entity);
    }
    // Dropping the orders leaves the match.
    world.remove_resource::<crate::Orders>();
    world.remove_resource::<Sim>();
    world.remove_resource::<Loaded>();
    world.remove_resource::<crate::Drawn>();
    world.remove_resource::<crate::views::Viewed>();
    world.remove_resource::<crate::Leaders>();
    world.remove_resource::<crate::lobby::Lineup>();
    world.remove_resource::<crate::lobby::Computers>();
    world.remove_resource::<crate::Raising>();
    world.remove_resource::<crate::effects::Effects>();
    world.remove_resource::<crate::sunlight::Sunlight>();
    world.remove_resource::<crate::fog::Fog>();
    world.remove_resource::<Mission>();
    // Its thread is left to finish on its own.
    world.remove_resource::<crate::loading::Preparing>();
    world.insert_resource(Local(None));
    let mut time = world.resource_mut::<Time<Virtual>>();
    time.unpause();
    time.set_relative_speed(1.0);
}

/// The objectives the debriefing lists: not those taken away, which the
/// side no longer holds, nor the line a side with none is shown.
fn debriefed(objectives: &[Objective], primary: bool) -> impl Iterator<Item = &Objective> {
    objectives.iter().filter(move |o| o.primary == primary)
}

fn spawn(
    mut commands: Commands,
    debrief: Res<Debrief>,
    data: Res<crate::Data>,
    outcome: Res<crate::outcome::Words>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    let game = &data.0;
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    let [completed, failed] = debrief.icons.clone().map(|path| {
        let image = path.and_then(|p| crate::ui::read_image(game, &p, formats.0));
        image.map(|i| images.add(i)).unwrap_or_default()
    });
    commands.spawn((Camera2d, DespawnOnExit(Screen::Debriefing)));
    let root = commands
        .spawn((
            DespawnOnExit(Screen::Debriefing),
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                column_gap: px(16),
                ..default()
            },
        ))
        .id();
    let left = commands
        .spawn((
            ChildOf(root),
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                ..default()
            },
        ))
        .id();
    let block = |commands: &mut Commands, heading: String| {
        commands.spawn((label(heading, Font::Heading, Ink::Body), ChildOf(left)));
        commands
            .spawn((
                ChildOf(left),
                panel(
                    Surface::Panel,
                    Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(12)),
                        row_gap: px(4),
                        ..default()
                    },
                ),
            ))
            .id()
    };
    let name = block(&mut commands, said("debriefing_Label_text", "Mission Name"));
    commands.spawn((
        label(debrief.title.clone(), Font::Body, Ink::Body),
        ChildOf(name),
    ));
    let status = block(
        &mut commands,
        said(
            "debriefing_MissionStatusViewport_ItemPanel_MissionStatusLabel_Label_text",
            "Mission Status",
        ),
    );
    let result = if debrief.won {
        &outcome.won
    } else {
        &outcome.lost
    };
    commands.spawn((
        label(result.clone(), Font::Body, Ink::Body),
        ChildOf(status),
    ));
    if let Some(debriefed) = &debrief.campaign
        && !debriefed.said.is_empty()
    {
        commands.spawn((
            label(debriefed.said.clone(), Font::Body, Ink::Body),
            ChildOf(status),
        ));
    }
    let headings = [
        (said("debriefing_Label_text_2", "Primary Objectives"), true),
        (
            said("debriefing_Label_text_3", "Secondary Objectives"),
            false,
        ),
    ];
    for (heading, primary) in headings {
        let list = block(&mut commands, heading);
        for objective in debriefed(&debrief.objectives, primary) {
            // One still active at the end counts as failed.
            let done = objective.standing == Standing::Completed;
            let icon = if done { &completed } else { &failed };
            commands.spawn((
                ChildOf(list),
                Node {
                    column_gap: px(4),
                    align_items: AlignItems::Center,
                    ..default()
                },
                children![
                    (
                        ImageNode::new(icon.clone()),
                        Node {
                            width: px(20),
                            height: px(20),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ),
                    label(objective.text.clone(), Font::Body, Ink::Body),
                ],
            ));
        }
    }
    let right = commands
        .spawn((
            ChildOf(root),
            Node {
                width: px(320),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::End,
                row_gap: px(12),
                ..default()
            },
        ))
        .id();
    let mut exit = Screen::MainMenu;
    if let Some(Debriefed { playing, next, .. }) = &debrief.campaign {
        exit = Screen::Campaign;
        let Playing { campaign, mission } = *playing;
        if *next {
            commands
                .spawn((
                    button(
                        said("debriefing_Client_View_text_4", "Next Mission"),
                        Font::Large,
                    ),
                    called("debriefing.next"),
                    ChildOf(right),
                ))
                .observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |world: &mut World| {
                        crate::campaigns::play(world, campaign, mission + 1, false)
                    });
                });
        }
        commands
            .spawn((
                button(
                    said("debriefing_Client_View_text_3", "Replay Mission"),
                    Font::Large,
                ),
                called("debriefing.replay"),
                ChildOf(right),
            ))
            .observe(move |_: On<Activate>, mut commands: Commands| {
                commands.queue(move |world: &mut World| {
                    crate::campaigns::play(world, campaign, mission, true)
                });
            });
    }
    commands
        .spawn((
            button(said("debriefing_Client_View_text_5", "Exit"), Font::Large),
            called("debriefing.exit"),
            ChildOf(right),
        ))
        .observe(crate::menu::go(exit));
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;

    use super::*;

    fn objective(text: &str, primary: bool, standing: Standing) -> Objective {
        Objective {
            name: text.into(),
            text: text.into(),
            primary,
            standing,
        }
    }

    #[test]
    fn the_debriefing_splits_objectives_into_primary_and_secondary() {
        let all = [
            objective("Take the bridge", true, Standing::Completed),
            objective("Save the tanks", false, Standing::Active),
            objective("Hold the town", true, Standing::Failed),
        ];
        let texts =
            |primary| -> Vec<_> { debriefed(&all, primary).map(|o| o.text.as_str()).collect() };
        assert_eq!(texts(true), ["Take the bridge", "Hold the town"]);
        assert_eq!(texts(false), ["Save the tanks"]);
    }

    #[derive(Component)]
    struct FromMatch;

    #[test]
    fn leaving_before_the_match_has_begun_goes_to_the_menus() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(Screen::Loading)
            .insert_resource(Local(None))
            .add_plugins(LeavingPlugin);
        app.world_mut().run_system_cached(note_kept).unwrap();
        let world = app.world_mut();
        let screen = world.spawn(Node::default()).id();
        world.insert_resource(Mission::default());
        let entity = world.spawn_empty().observe(end_mission).id();
        world.trigger(Activate { entity });
        app.update();
        let world = app.world();
        assert_eq!(*world.resource::<State<Screen>>().get(), Screen::MainMenu);
        assert!(world.get_entity(screen).is_err());
        assert!(!world.contains_resource::<Mission>());
        assert!(!world.contains_resource::<Kept>());
    }

    #[test]
    fn putting_a_match_away_takes_what_it_spawned_and_leaves_the_rest() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, StatesPlugin))
            .insert_state(Screen::Match)
            .add_systems(OnExit(Screen::Match), put_away);
        let before = app.world_mut().spawn(Transform::default()).id();
        app.world_mut().run_system_cached(note_kept).unwrap();
        let world = app.world_mut();
        let spawned = world.spawn((FromMatch, Transform::default())).id();
        let child = world.spawn((Node::default(), ChildOf(spawned))).id();
        world.insert_resource(Sim(crate::orders::testing::fresh()));
        world.insert_resource(Local(Some(1)));
        world.insert_resource(Mission::default());
        world.resource_mut::<Time<Virtual>>().pause();
        app.update();
        let world = app.world_mut();
        world
            .resource_mut::<NextState<Screen>>()
            .set(Screen::MainMenu);
        app.update();
        let world = app.world();
        assert!(world.get_entity(before).is_ok());
        assert!(world.get_entity(spawned).is_err());
        assert!(world.get_entity(child).is_err());
        assert!(!world.contains_resource::<Sim>());
        assert!(!world.contains_resource::<Mission>());
        assert_eq!(world.resource::<Local>().0, None);
        assert!(!world.resource::<Time<Virtual>>().is_paused());
    }
}
