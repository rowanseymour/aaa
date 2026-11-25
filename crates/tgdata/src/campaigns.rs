//! The campaigns: missions played one after another from the campaign
//! screen, as the data under `Campaign/` lists them.

use crate::strings::localized;
use crate::{GameData, Id, Strings, defs};

/// Where a campaign's pictures are, relative to.
const FOLDER: &str = "Campaign/";

/// A campaign, as the campaign screen offers it.
#[derive(Debug, Clone, PartialEq)]
pub struct Campaign {
    pub id: Id,
    pub name: String,
    /// The campaign the screen suggests finishing first.
    pub depends_on: Option<Id>,
    /// Its button's picture once every mission is complete, and until then,
    /// by archive path.
    pub complete_picture: Option<String>,
    pub incomplete_picture: Option<String>,
    /// What a complete mission's name is drawn in, and any other's.
    pub complete_colour: [f32; 3],
    pub incomplete_colour: [f32; 3],
    /// In the order they're played.
    pub missions: Vec<Mission>,
}

/// One of a [`Campaign`]'s missions.
#[derive(Debug, Clone, PartialEq)]
pub struct Mission {
    pub name: String,
    pub kind: MissionKind,
    /// The pictures either side of its loading screen, by archive path.
    pub loading_pictures: Option<[String; 2]>,
    /// What its debriefing says when it's won, and when it isn't.
    pub success: String,
    pub failure: String,
    /// Whether winning it goes to the credits rather than the debriefing.
    pub last: bool,
}

/// What a [`Mission`] is played as.
#[derive(Debug, Clone, PartialEq)]
pub enum MissionKind {
    /// A map played to a win or a loss, by the path the data gives.
    Playable(String),
    /// A map that plays itself.
    Cutscene(String),
    /// A video.
    Cinematic(String),
}

impl GameData {
    /// The campaigns, in the order the campaign screen lists them.
    pub fn campaigns(&self) -> Vec<Campaign> {
        let strings = self.strings();
        let read: Vec<defs::Campaign> = self.read_all(
            |n| n.starts_with("campaign/") || n.starts_with("templates/"),
            "Campaign",
        );
        let exists = |path: &str| self.exists(path);
        (read.into_iter())
            .filter_map(|c| campaign(c, &strings, &exists))
            .collect()
    }
}

fn campaign(
    c: defs::Campaign,
    strings: &Strings,
    exists: &dyn Fn(&str) -> bool,
) -> Option<Campaign> {
    let given = |path: Option<String>| {
        let path = path?.trim().to_string();
        let none = path.is_empty()
            || ["none", "null"]
                .iter()
                .any(|n| path.eq_ignore_ascii_case(n));
        (!none).then(|| format!("{FOLDER}{path}"))
    };
    let missions = c
        .mission
        .into_iter()
        .filter_map(|m| mission(m, strings, exists));
    let id = c.ids?;
    Some(Campaign {
        name: localized(strings, c.name.as_deref()).unwrap_or_else(|| id.to_string()),
        id,
        depends_on: (c.dependent_campaign_ids.as_deref())
            .and_then(crate::named)
            .map(Id::from),
        complete_picture: given(c.complete_texture),
        incomplete_picture: given(c.incomplete_texture),
        complete_colour: c.complete_color.unwrap_or([0.0, 0.75, 0.0]),
        incomplete_colour: c.incomplete_color.unwrap_or([1.0; 3]),
        missions: missions.collect(),
    })
}

fn mission(
    m: defs::CampaignMission,
    strings: &Strings,
    exists: &dyn Fn(&str) -> bool,
) -> Option<Mission> {
    let path =
        |p: Option<&String>| crate::named(p?.trim().trim_start_matches('/')).map(str::to_string);
    let kind = match m.mission_type.as_deref().map(str::trim) {
        Some(t) if t.eq_ignore_ascii_case("CINEMATIC") => {
            MissionKind::Cinematic(path(m.cinematic.as_ref())?)
        }
        Some(t) if t.eq_ignore_ascii_case("CUTSCENE") => {
            MissionKind::Cutscene(path(m.map.as_ref())?)
        }
        _ => MissionKind::Playable(path(m.map.as_ref())?),
    };
    let said =
        |key: Option<&String>| localized(strings, key.map(String::as_str)).unwrap_or_default();
    let pictures = [&m.loading_texture_left, &m.loading_texture_right]
        .map(|p| path(p.as_ref()).and_then(|p| loading_picture(&p, exists)));
    let loading_pictures = match pictures {
        [Some(left), Some(right)] => Some([left, right]),
        _ => None,
    };
    Some(Mission {
        name: said(m.name.as_ref()),
        kind,
        loading_pictures,
        success: said(m.debriefing_success.as_ref()),
        failure: said(m.debriefing_failure.as_ref()),
        last: m.is_final_mission.unwrap_or(false),
    })
}

/// A loading picture's archive path. The `%s` folder, named after a
/// screen width, is left out, as the data has one size of each.
fn loading_picture(path: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    let path = format!("{FOLDER}{}", path.replace("%s/", ""));
    exists(&path).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str, exists: &dyn Fn(&str) -> bool) -> Vec<Campaign> {
        let template = "[template Campaign]\n{\n\tstring ids\n\tlocalized name\n\
                        \tstring dependent_campaign_ids = null\n\
                        \tPoint3 complete_color = 0,0.75,0\n\
                        \t[list Mission]\n\t{\n\t\tstring map = NULL\n\
                        \t\tstring cinematic = NULL\n\t\tlocalized name\n\
                        \t\tstring mission_type\n\
                        \t\tflag is_final_mission = false\n\t}\n}\n";
        let files = [
            ("t.tgi".to_string(), tgi::parse(template)),
            ("c.tgi".to_string(), tgi::parse(text)),
        ];
        let strings: Strings = [("allies".to_string(), "Allies".to_string())]
            .into_iter()
            .collect();
        let scoped = crate::scoped(&files);
        let read = scoped
            .iter()
            .flat_map(|f| f.read::<defs::Campaign>("Campaign"));
        read.filter_map(|c| campaign(c, &strings, exists)).collect()
    }

    #[test]
    fn campaigns_are_read_with_their_missions_in_order() {
        let text = "[Campaign template=Campaign]\n{\n\tids = allies\n\tname = \"#allies\"\n\
                    \tdependent_campaign_ids = tutorial\n\
                    \tcomplete_texture = allies/Done.png\n\
                    \t[Mission]\n\t{\n\t\tcinematic = /Video/Intro.bik\n\
                    \t\tname = Intro\n\t\tmission_type = CINEMATIC\n\t}\n\
                    \t[Mission]\n\t{\n\t\tmap = /Maps/Campaign/one.rmp\n\
                    \t\tname = One\n\t\tmission_type = PLAYABLE\n\
                    \t\tloading_texture_left = allies/%s/OneLeft.png\n\
                    \t\tloading_texture_right = allies/%s/OneRight.png\n\
                    \t\tdebriefing_success = Won\n\t\tis_final_mission = true\n\t}\n}\n";
        let exists = |p: &str| p.starts_with("Campaign/allies/One");
        let campaigns = read(text, &exists);
        let [allies] = &campaigns[..] else {
            panic!("{campaigns:?}");
        };
        assert_eq!(allies.id, Id::from("allies"));
        assert_eq!(allies.name, "Allies");
        assert_eq!(allies.depends_on, Some(Id::from("tutorial")));
        assert_eq!(
            allies.complete_picture.as_deref(),
            Some("Campaign/allies/Done.png")
        );
        assert_eq!(allies.complete_colour, [0.0, 0.75, 0.0]);
        let kinds: Vec<_> = allies.missions.iter().map(|m| m.kind.clone()).collect();
        assert_eq!(
            kinds,
            [
                MissionKind::Cinematic("Video/Intro.bik".into()),
                MissionKind::Playable("Maps/Campaign/one.rmp".into()),
            ]
        );
        let one = &allies.missions[1];
        assert_eq!(
            one.loading_pictures,
            Some([
                "Campaign/allies/OneLeft.png".to_string(),
                "Campaign/allies/OneRight.png".to_string()
            ])
        );
        assert_eq!(one.success, "Won");
        assert!(one.last);
        assert!(!allies.missions[0].last);
    }

    #[test]
    fn a_campaign_with_no_dependency_says_null() {
        let text = "[Campaign template=Campaign]\n{\n\tids = tutorial\n\tname = Tutorial\n}\n";
        let campaigns = read(text, &|_| false);
        assert_eq!(campaigns[0].depends_on, None);
        assert!(campaigns[0].missions.is_empty());
    }
}
