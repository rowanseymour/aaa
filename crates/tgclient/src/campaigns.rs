//! The campaigns: the screen that picks a campaign, a mission and a
//! difficulty, playing missions one after another with no lobby, and the
//! progress kept between runs. `spec/campaigns.md` is what it's built to.
//!
//! Progress is kept in a TOML file of this project's own, beside the
//! options file, rather than the original game's.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::image::CompressedImageFormatSupport;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use serde::{Deserialize, Serialize};
use tgdata::{Campaign, Id, MissionKind};

use crate::loading::Pending;
use crate::ui::{Font, Ink, Surface, button, called, coloured, label, panel, toggle};
use crate::{Data, Screen, Seed, confirm, menu};

/// The progress file's name, beside the options file.
const FILE: &str = "campaigns.toml";

/// The world rule a campaign's difficulty sets.
pub(crate) const DIFFICULTY: &str = tgdata::DIFFICULTY_LEVEL;

pub struct CampaignsPlugin;

impl Plugin for CampaignsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picked>()
            .add_systems(OnEnter(Screen::MainMenu), |mut commands: Commands| {
                commands.remove_resource::<Playing>()
            })
            .add_systems(OnEnter(Screen::Campaign), arrive)
            .add_systems(
                Update,
                spawn
                    .run_if(in_state(Screen::Campaign))
                    .run_if(resource_changed::<Picked>.or_else(resource_changed::<Progress>)),
            );
    }
}

/// The campaigns, in the order the screen lists them, and the
/// difficulties they can be played at.
#[derive(Resource)]
pub(crate) struct Campaigns {
    pub(crate) list: Vec<Campaign>,
    /// Each difficulty's id and name, easiest first.
    pub(crate) difficulties: Vec<(Id, String)>,
    /// What a campaign starts at.
    pub(crate) default: Id,
}

impl Campaigns {
    pub(crate) fn of(game: &tgdata::GameData) -> Self {
        let rule = game
            .lobby_rules()
            .into_iter()
            .find(|r| r.id == Id::from(DIFFICULTY));
        let difficulties = match rule.as_ref().map(|r| &r.kind) {
            Some(tgdata::RuleKind::Choice(choices)) => (choices.iter())
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect(),
            _ => Vec::new(),
        };
        // Spelt as the choice is.
        let default = Id::from(rule.map_or(String::new(), |r| r.default));
        let default = (difficulties.iter())
            .find(|(id, _)| *id == default)
            .map_or(default, |(id, _)| id.clone());
        Campaigns {
            list: game.campaigns(),
            difficulties,
            default,
        }
    }
}

/// How far each campaign has got and what it's played at, kept between
/// runs.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub(crate) struct Kept {
    /// The campaign last chosen, by id.
    last: Option<String>,
    /// Whether the screen has stopped suggesting a campaign be finished
    /// before the one after it.
    dependencies_waived: bool,
    campaigns: BTreeMap<String, Reached>,
}

/// How far one campaign has got.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
struct Reached {
    /// How many of its missions are complete: they're completed in order,
    /// so the one after them is the one open.
    complete: usize,
    difficulty: Option<String>,
}

/// The progress file, and what it holds.
#[derive(Resource, Default, Debug)]
pub(crate) struct Progress {
    /// None where it isn't kept, or couldn't be read, so as not to write
    /// over what the player may want to mend.
    path: Option<PathBuf>,
    kept: Kept,
    /// The difficulty each campaign was last played at, by id, kept only
    /// once it's played to a change of progress.
    chosen: BTreeMap<String, Id>,
}

impl Progress {
    /// What's kept beside the options file at `options`.
    pub(crate) fn beside(options: Option<&std::path::Path>) -> Progress {
        let Some(path) = options.and_then(|o| o.parent()).map(|d| d.join(FILE)) else {
            return Progress::default();
        };
        let read = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| e.to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Kept::default()),
            Err(e) => Err(e.to_string()),
        };
        match read {
            Ok(kept) => Progress {
                path: Some(path),
                kept,
                chosen: BTreeMap::new(),
            },
            Err(e) => {
                warn!("{}: {e}; campaign progress won't be kept", path.display());
                Progress::default()
            }
        }
    }

    fn write(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let written = toml::to_string(&self.kept)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(path, text).map_err(|e| e.to_string())
            });
        if let Err(e) = written {
            warn!("{}: {e}", path.display());
        }
    }

    /// How many of `campaign`'s missions are complete.
    pub(crate) fn complete(&self, campaign: &Id) -> usize {
        self.kept
            .campaigns
            .get(campaign.as_str())
            .map_or(0, |r| r.complete)
    }

    /// The difficulty `campaign` is played at, if one's been chosen.
    fn difficulty(&self, campaign: &Id) -> Option<Id> {
        if let Some(chosen) = self.chosen.get(campaign.as_str()) {
            return Some(chosen.clone());
        }
        let reached = self.kept.campaigns.get(campaign.as_str())?;
        reached.difficulty.as_deref().map(Id::from)
    }

    /// Marks mission `mission` of `campaign` complete, which opens the
    /// one after it if it was the one open; one complete already changes
    /// nothing.
    pub(crate) fn completed(&mut self, campaign: &Id, mission: usize) {
        let chosen = self.chosen.get(campaign.as_str()).map(Id::to_string);
        let reached = self.kept.campaigns.entry(campaign.to_string()).or_default();
        if mission == reached.complete {
            reached.complete += 1;
            reached.difficulty = chosen.or(reached.difficulty.take());
            self.write();
        }
    }

    /// Notes what's about to be played: the campaign, kept as the one last
    /// chosen, and the difficulty it's played at.
    fn playing(&mut self, campaign: &Id, difficulty: &Id) {
        self.kept.last = Some(campaign.to_string());
        self.chosen.insert(campaign.to_string(), difficulty.clone());
        self.write();
    }
}

/// What the campaign screen has picked.
#[derive(Resource, Default, Clone, Debug)]
pub(crate) struct Picked {
    campaign: usize,
    /// None where every mission is complete and none's been picked.
    mission: Option<usize>,
    difficulty: Option<Id>,
}

/// The campaign mission being played, from launching it until the menus.
#[derive(Resource, Clone, Debug)]
pub(crate) struct Playing {
    pub(crate) campaign: usize,
    pub(crate) mission: usize,
}

/// The missions of a campaign `complete` of which are complete that the
/// screen lists: those complete and the one open after them.
fn listed(missions: usize, complete: usize) -> usize {
    missions.min(complete + 1)
}

/// The mission picked on arriving at a campaign: the one open, if any.
fn open(missions: usize, complete: usize) -> Option<usize> {
    (complete < missions).then_some(complete)
}

/// Picks the campaign last chosen, its open mission and its difficulty.
fn arrive(
    mut commands: Commands,
    mut picked: ResMut<Picked>,
    campaigns: Res<Campaigns>,
    progress: Res<Progress>,
) {
    // Kept while what's on the screen is laid out again.
    commands.spawn((Camera2d, DespawnOnExit(Screen::Campaign)));
    let last = progress.kept.last.as_deref().map(Id::from);
    let at = |id: &Id| campaigns.list.iter().position(|c| &c.id == id);
    let campaign = last.as_ref().and_then(at).unwrap_or(0);
    *picked = pick(&campaigns, &progress, campaign);
}

fn pick(campaigns: &Campaigns, progress: &Progress, campaign: usize) -> Picked {
    let Some(c) = campaigns.list.get(campaign) else {
        return Picked::default();
    };
    Picked {
        campaign,
        mission: open(c.missions.len(), progress.complete(&c.id)),
        difficulty: Some(
            progress
                .difficulty(&c.id)
                .unwrap_or(campaigns.default.clone()),
        ),
    }
}

/// Everything on the screen, which is laid out again as what's picked
/// changes.
#[derive(Component)]
struct CampaignScreen;

#[derive(Component)]
struct CampaignChoice(usize);

#[derive(Component)]
struct MissionChoice(usize);

#[derive(Component)]
struct DifficultyChoice(Id);

#[allow(clippy::too_many_arguments)]
fn spawn(
    mut commands: Commands,
    campaigns: Res<Campaigns>,
    progress: Res<Progress>,
    picked: Res<Picked>,
    data: Res<Data>,
    shown: Query<Entity, With<CampaignScreen>>,
    mut images: ResMut<Assets<Image>>,
    formats: Res<CompressedImageFormatSupport>,
) {
    shown.iter().for_each(|e| commands.entity(e).despawn());
    let game = &data.0;
    let strings = game.strings();
    let said = crate::ui::said(&strings);
    let root = commands
        .spawn((
            DespawnOnExit(Screen::Campaign),
            CampaignScreen,
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(16)),
                column_gap: px(16),
                ..default()
            },
        ))
        .id();
    let column = |commands: &mut Commands, width: Option<f32>| {
        let node = Node {
            width: width.map_or(auto(), px),
            flex_grow: if width.is_some() { 0.0 } else { 1.0 },
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(12)),
            row_gap: px(8),
            ..default()
        };
        commands
            .spawn((ChildOf(root), panel(Surface::Panel, node)))
            .id()
    };

    let left = column(&mut commands, Some(320.0));
    for (i, c) in campaigns.list.iter().enumerate() {
        let done = progress.complete(&c.id) >= c.missions.len();
        let picture = if done {
            &c.complete_picture
        } else {
            &c.incomplete_picture
        };
        let image = picture
            .as_deref()
            .and_then(|p| crate::ui::read_image(game, p, formats.0));
        let entry = commands
            .spawn((
                toggle(c.name.clone(), Font::Large),
                called(format!("campaign.{}", c.id)),
                CampaignChoice(i),
                ChildOf(left),
            ))
            .insert(crate::ui::Chosen(i == picked.campaign))
            .observe(choose_campaign)
            .id();
        if let Some(image) = image {
            let picture = commands
                .spawn((
                    ImageNode::new(images.add(image)),
                    Node {
                        width: px(48),
                        height: px(48),
                        margin: UiRect::right(px(8)),
                        ..default()
                    },
                ))
                .id();
            commands
                .entity(entry)
                .insert_children(0, &[picture])
                .insert(Node {
                    align_items: AlignItems::Center,
                    ..crate::ui::card(crate::ui::padded())
                });
        }
    }

    let middle = column(&mut commands, None);
    if let Some(c) = campaigns.list.get(picked.campaign) {
        let complete = progress.complete(&c.id);
        let [done, undone] =
            [c.complete_colour, c.incomplete_colour].map(|[r, g, b]| Color::srgb(r, g, b));
        for (i, m) in c
            .missions
            .iter()
            .take(listed(c.missions.len(), complete))
            .enumerate()
        {
            let colour = if i < complete { done } else { undone };
            commands
                .spawn((
                    crate::ui::switch(crate::ui::card(crate::ui::padded())),
                    crate::ui::Card,
                    called(format!("mission.{i}")),
                    MissionChoice(i),
                    ChildOf(middle),
                    children![coloured(m.name.clone(), Font::Body, colour)],
                ))
                .insert(crate::ui::Chosen(picked.mission == Some(i)))
                .observe(choose_mission);
        }
    }
    commands.spawn((ChildOf(middle), menu::spacer()));
    commands.spawn((
        label(
            said("choose_campaign_Child_text", "Difficulty"),
            Font::Heading,
            Ink::Body,
        ),
        ChildOf(middle),
    ));
    let row = commands
        .spawn((
            ChildOf(middle),
            Node {
                column_gap: px(8),
                ..default()
            },
        ))
        .id();
    for (id, name) in &campaigns.difficulties {
        commands
            .spawn((
                toggle(name.clone(), Font::Body),
                called(format!("difficulty.{id}")),
                DifficultyChoice(id.clone()),
                ChildOf(row),
            ))
            .insert(crate::ui::Chosen(picked.difficulty.as_ref() == Some(id)))
            .observe(choose_difficulty);
    }

    let right = commands
        .spawn((
            ChildOf(root),
            Node {
                width: px(240),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::End,
                row_gap: px(12),
                ..default()
            },
        ))
        .id();
    if picked.mission.is_some() {
        commands
            .spawn((
                button(
                    said("choose_campaign_Client_View_text", "Play"),
                    Font::Large,
                ),
                called("campaign.play"),
                ChildOf(right),
            ))
            .observe(ask_play);
    }
    commands
        .spawn((
            button(
                said("choose_campaign_Client_View_text_4", "Back"),
                Font::Large,
            ),
            called("campaign.back"),
            ChildOf(right),
        ))
        .observe(menu::open(menu::MenuPage::NewGame));
}

fn choose_campaign(
    activate: On<Activate>,
    choices: Query<&CampaignChoice>,
    campaigns: Res<Campaigns>,
    progress: Res<Progress>,
    mut picked: ResMut<Picked>,
) {
    if let Ok(CampaignChoice(i)) = choices.get(activate.entity) {
        *picked = pick(&campaigns, &progress, *i);
    }
}

fn choose_mission(
    activate: On<Activate>,
    choices: Query<&MissionChoice>,
    mut picked: ResMut<Picked>,
) {
    if let Ok(MissionChoice(i)) = choices.get(activate.entity) {
        picked.mission = Some(*i);
    }
}

fn choose_difficulty(
    activate: On<Activate>,
    choices: Query<&DifficultyChoice>,
    mut picked: ResMut<Picked>,
) {
    if let Ok(DifficultyChoice(id)) = choices.get(activate.entity) {
        picked.difficulty = Some(id.clone());
    }
}

/// Plays the picked mission, first suggesting the campaign before it be
/// finished, unless that's been waved.
fn ask_play(
    _: On<Activate>,
    mut commands: Commands,
    campaigns: Res<Campaigns>,
    progress: Res<Progress>,
    picked: Res<Picked>,
    data: Res<Data>,
    words: Res<confirm::Words>,
) {
    let Some(c) = campaigns.list.get(picked.campaign) else {
        return;
    };
    let before = c
        .depends_on
        .as_ref()
        .and_then(|d| campaigns.list.iter().find(|b| &b.id == d));
    let unfinished = before.filter(|b| progress.complete(&b.id) < b.missions.len());
    match unfinished {
        Some(before) if !progress.kept.dependencies_waived => {
            let strings = data.0.strings();
            let question = crate::ui::said(&strings)(
                "campaign_dependent_campaign_confirm|ss",
                "Playing the %s is recommended before playing %s. Do you wish to continue?",
            )
            .replacen("%s", &before.name, 1)
            .replacen("%s", &c.name, 1);
            let yes = |_: On<Activate>, mut commands: Commands, mut progress: ResMut<Progress>| {
                progress.kept.dependencies_waived = true;
                progress.write();
                commands.queue(play_picked);
            };
            confirm::confirm(&mut commands, &words, Screen::Campaign, question, yes);
        }
        _ => commands.queue(play_picked),
    }
}

fn play_picked(world: &mut World) {
    let picked = world.resource::<Picked>().clone();
    let Some(mission) = picked.mission else {
        return;
    };
    let campaigns = world.resource::<Campaigns>();
    let Some(c) = campaigns.list.get(picked.campaign) else {
        return;
    };
    let id = c.id.clone();
    let difficulty = picked.difficulty.unwrap_or(campaigns.default.clone());
    let replay = mission < world.resource::<Progress>().complete(&id);
    world.resource_mut::<Progress>().playing(&id, &difficulty);
    play(world, picked.campaign, mission, replay);
}

/// Plays mission `mission` of campaign `campaign`. A video can't be shown,
/// so it counts as watched at once; then, unless `replay`ed, the mission
/// after it is played, or else it's back to the campaign screen.
pub(crate) fn play(world: &mut World, campaign: usize, mission: usize, replay: bool) {
    let campaigns = world.resource::<Campaigns>();
    let Some(c) = campaigns.list.get(campaign).cloned() else {
        return;
    };
    let difficulty =
        (world.resource::<Progress>().difficulty(&c.id)).unwrap_or(campaigns.default.clone());
    let mut at = mission;
    loop {
        let Some(m) = c.missions.get(at) else {
            world
                .resource_mut::<NextState<Screen>>()
                .set_if_different(Screen::Campaign);
            return;
        };
        let map = match &m.kind {
            MissionKind::Cinematic(video) => {
                info!("{video}: videos aren't shown, so it counts as watched");
                world.resource_mut::<Progress>().completed(&c.id, at);
                if replay {
                    world
                        .resource_mut::<NextState<Screen>>()
                        .set_if_different(Screen::Campaign);
                    return;
                }
                at += 1;
                continue;
            }
            MissionKind::Playable(map) | MissionKind::Cutscene(map) => map,
        };
        let game = world.resource::<Data>().0.clone();
        let Some(found) = game.find_map(map) else {
            error!("{}: no such map", map);
            world
                .resource_mut::<NextState<Screen>>()
                .set_if_different(Screen::Campaign);
            return;
        };
        let brief = match tgmap::Brief::load(&game, &found) {
            Ok(brief) => brief,
            Err(e) => {
                error!("{e}");
                world
                    .resource_mut::<NextState<Screen>>()
                    .set_if_different(Screen::Campaign);
                return;
            }
        };
        let person = brief
            .local
            .or_else(|| crate::lobby::default_person(&brief.players, crate::Side(None)));
        let seed = world.resource::<Seed>().get();
        let setup = tgserver::Setup {
            map: found,
            seed,
            computers: crate::lobby::computer_sides(&brief.players, person),
            rules: [(Id::from(DIFFICULTY), difficulty.to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        world.insert_resource(Pending {
            title: brief.name,
            description: brief.description,
            person,
            from_menu: true,
            back: Some(Screen::Campaign),
            joined: None,
            setup,
        });
        world.insert_resource(Playing {
            campaign,
            mission: at,
        });
        world
            .resource_mut::<NextState<Screen>>()
            .set(Screen::Loading);
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_missions_listed_are_those_complete_and_the_one_open() {
        assert_eq!(listed(5, 0), 1);
        assert_eq!(listed(5, 2), 3);
        assert_eq!(listed(5, 5), 5);
        assert_eq!(open(5, 2), Some(2));
        assert_eq!(open(5, 5), None);
    }

    #[test]
    fn only_completing_the_open_mission_opens_the_next() {
        let mut progress = Progress::default();
        let allies = Id::from("allies");
        progress.completed(&allies, 0);
        progress.completed(&allies, 1);
        assert_eq!(progress.complete(&allies), 2);
        // Replaying one complete already, or one not yet reached.
        progress.completed(&allies, 0);
        progress.completed(&allies, 5);
        assert_eq!(progress.complete(&allies), 2);
    }

    #[test]
    fn progress_is_kept_between_runs() {
        let dir = std::env::temp_dir().join(format!("campaigns-{}", std::process::id()));
        let options = dir.join("options.toml");
        let allies = Id::from("allies");
        let mut progress = Progress::beside(Some(&options));
        progress.playing(&allies, &Id::from("hard"));
        progress.completed(&allies, 0);
        // Played at another, with no progress made.
        progress.playing(&allies, &Id::from("easy"));
        assert_eq!(progress.difficulty(&allies), Some(Id::from("easy")));
        let again = Progress::beside(Some(&options));
        assert_eq!(again.complete(&allies), 1);
        assert_eq!(again.difficulty(&allies), Some(Id::from("hard")));
        assert_eq!(again.kept.last.as_deref(), Some("allies"));
        _ = std::fs::remove_dir_all(dir);
    }
}
