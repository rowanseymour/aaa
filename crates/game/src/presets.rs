//! The ways a lobby can arrange a map's sides into teams: as the map has
//! them, so many equal teams, or everyone for themselves.

use aaa_map::Player;

/// The most sides a match has.
const MOST: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    /// The teams the map was made with, every side playing.
    Custom,
    /// So many teams of so many sides.
    Teams { teams: usize, size: usize },
    /// So many sides, each on its own.
    FreeForAll(usize),
}

impl Preset {
    /// Every preset, in the order the lobby lists them.
    pub fn all() -> impl Iterator<Item = Preset> {
        let teams = [(2, 2), (2, 3), (2, 4), (3, 2), (4, 2)]
            .map(|(teams, size)| Preset::Teams { teams, size });
        std::iter::once(Preset::Custom)
            .chain(teams)
            .chain((2..=MOST).map(Preset::FreeForAll))
    }

    /// E.g. `2v2v2` or `3 Free For All`.
    pub fn label(self) -> String {
        match self {
            Preset::Custom => "Custom".into(),
            Preset::Teams { teams, size } => vec![size.to_string(); teams].join("v"),
            Preset::FreeForAll(n) => format!("{n} Free For All"),
        }
    }

    /// The sides it takes; none for [`Preset::Custom`], which takes them all.
    fn sides(self) -> Option<usize> {
        match self {
            Preset::Custom => None,
            Preset::Teams { teams, size } => Some(teams * size),
            Preset::FreeForAll(n) => Some(n),
        }
    }

    /// The presets a map's `players` can be played as: any that has enough
    /// sides, unless some side is the computer's alone, as in a mission,
    /// which is played only as made.
    pub fn offered(players: &[Player]) -> Vec<Preset> {
        let sides: Vec<_> = aaa_map::sides(players).collect();
        let fixed = sides.iter().any(|p| !p.playable);
        Preset::all()
            .filter(|p| match p.sides() {
                None => true,
                Some(n) => !fixed && n <= sides.len(),
            })
            .collect()
    }

    /// The preset a map is made for: equal teams holding all its sides, else
    /// [`Preset::Custom`].
    pub fn made_for(players: &[Player]) -> Preset {
        let groups = groups(players);
        let size = groups.first().map_or(0, Vec::len);
        let made = Preset::Teams {
            teams: groups.len(),
            size,
        };
        let even = groups.iter().all(|g| g.len() == size);
        let teamed = aaa_map::sides(players).all(|p| p.team.is_some());
        match teamed && even && Preset::offered(players).contains(&made) {
            true => made,
            false => Preset::Custom,
        }
    }

    /// The teams it puts the map's sides in, by player id; `None` for
    /// [`Preset::Custom`], or where there aren't enough sides. It keeps
    /// to the map's own teams where they're big enough, as the map's start
    /// locations are laid out for them.
    pub fn arrange(self, players: &[Player]) -> Option<Vec<Vec<u32>>> {
        let groups = groups(players);
        let loners = aaa_map::sides(players).filter(|p| p.team.is_none());
        let loners: Vec<u32> = loners.map(|p| p.id).collect();
        let arranged: Vec<Vec<u32>> = match self {
            Preset::Custom => return None,
            Preset::Teams { teams, size } => {
                let big: Vec<&Vec<u32>> = groups.iter().filter(|g| g.len() >= size).collect();
                match big.len() >= teams {
                    true => big[..teams].iter().map(|g| g[..size].to_vec()).collect(),
                    false => {
                        let all: Vec<u32> = groups.concat().into_iter().chain(loners).collect();
                        all.chunks(size).take(teams).map(<[u32]>::to_vec).collect()
                    }
                }
            }
            // A side from each of the map's teams in turn, so they start
            // apart.
            Preset::FreeForAll(n) => {
                let mut groups = groups;
                groups.extend(loners.into_iter().map(|id| vec![id]));
                let deepest = groups.iter().map(Vec::len).max().unwrap_or(0);
                (0..deepest)
                    .flat_map(|i| groups.iter().filter_map(move |g| g.get(i)))
                    .take(n)
                    .map(|&id| vec![id])
                    .collect()
            }
        };
        let full = arranged.iter().map(Vec::len).sum::<usize>() == self.sides()?;
        full.then_some(arranged)
    }
}

/// The sides on each of the map's teams that has any, by player id.
fn groups(players: &[Player]) -> Vec<Vec<u32>> {
    let teams = players
        .iter()
        .filter_map(|p| p.team)
        .max()
        .map_or(0, |t| t + 1);
    (0..teams)
        .map(|t| {
            let members = aaa_map::sides(players).filter(|p| p.team == Some(t));
            members.map(|p| p.id).collect::<Vec<_>>()
        })
        .filter(|g| !g.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_map::Relation;

    fn side(id: u32, team: Option<usize>) -> Player {
        Player {
            id,
            name: format!("kingdom_{id}"),
            color: "blue".into(),
            rgb: [1.0; 3],
            team,
            independent: false,
            playable: true,
            nation: None,
            general: None,
            relations: vec![Relation::Neutral; 6],
            team_relations: Vec::new(),
            stockpile: 0.0,
        }
    }

    /// As (2v2)Berlin has them: sides 1 and 2 on its second team, 3 and 4 on
    /// its first, and an independent.
    fn berlin() -> Vec<Player> {
        let mut civilians = side(9, None);
        civilians.independent = true;
        vec![
            side(1, Some(1)),
            side(2, Some(1)),
            civilians,
            side(3, Some(0)),
            side(4, Some(0)),
        ]
    }

    #[test]
    fn labels_presets_as_the_game_does() {
        let labels: Vec<String> = Preset::all().map(Preset::label).collect();
        assert_eq!(
            labels[..8],
            [
                "Custom",
                "2v2",
                "3v3",
                "4v4",
                "2v2v2",
                "2v2v2v2",
                "2 Free For All",
                "3 Free For All"
            ]
        );
    }

    #[test]
    fn offers_presets_with_enough_sides() {
        let offered: Vec<String> = Preset::offered(&berlin())
            .into_iter()
            .map(Preset::label)
            .collect();
        assert_eq!(
            offered,
            [
                "Custom",
                "2v2",
                "2 Free For All",
                "3 Free For All",
                "4 Free For All"
            ]
        );
        let mut mission = berlin();
        mission[0].playable = false;
        assert_eq!(Preset::offered(&mission), [Preset::Custom]);
    }

    #[test]
    fn a_map_is_made_for_its_own_teams() {
        assert_eq!(
            Preset::made_for(&berlin()),
            Preset::Teams { teams: 2, size: 2 }
        );
        let loners = vec![side(1, None), side(2, None)];
        assert_eq!(Preset::made_for(&loners), Preset::Custom);
    }

    #[test]
    fn arranges_sides_on_the_maps_own_teams() {
        let two_v_two = Preset::Teams { teams: 2, size: 2 };
        assert_eq!(
            two_v_two.arrange(&berlin()),
            Some(vec![vec![3, 4], vec![1, 2]])
        );
        // One from each team in turn.
        assert_eq!(
            Preset::FreeForAll(3).arrange(&berlin()),
            Some(vec![vec![3], vec![1], vec![4]])
        );
        assert_eq!(Preset::Custom.arrange(&berlin()), None);
        assert_eq!(Preset::FreeForAll(5).arrange(&berlin()), None);
    }

    #[test]
    fn deals_sides_out_where_the_maps_teams_are_too_small() {
        let loners: Vec<Player> = (1..=4).map(|id| side(id, None)).collect();
        let two_v_two = Preset::Teams { teams: 2, size: 2 };
        assert_eq!(
            two_v_two.arrange(&loners),
            Some(vec![vec![1, 2], vec![3, 4]])
        );
        let three = vec![
            side(1, Some(0)),
            side(2, Some(0)),
            side(3, Some(0)),
            side(4, Some(1)),
        ];
        let pairs = Preset::Teams { teams: 2, size: 2 };
        assert_eq!(pairs.arrange(&three), Some(vec![vec![1, 2], vec![3, 4]]));
    }
}
