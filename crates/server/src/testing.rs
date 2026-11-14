//! What the crate's tests build their matches from.

use aaa_map::{Player, Relation};

pub(crate) fn player(id: u32, independent: bool, playable: bool) -> Player {
    Player {
        id,
        name: format!("kingdom_{id}"),
        color: "blue".into(),
        rgb: [0.0, 0.0, 1.0],
        team: None,
        independent,
        playable,
        default_defeat: true,
        outcome: Default::default(),
        won_by_cities: false,
        nation: None,
        general: None,
        relations: vec![Relation::Neutral; 4],
        team_relations: Vec::new(),
        stockpile: 0.0,
        technologies: Vec::new(),
        handicap: None,
        barred: Vec::new(),
    }
}
