//! Finding a way across the tile grid: A* over tiles, then pulled straight
//! wherever the ground between two points is clear.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use tgmap::pathing::CELL_SIZE;
use tgmap::{Domain, Pathing, Tile};

/// The most tile steps a way takes, a diagonal one counting as one.
pub const MOST_STEPS: u32 = 254;

/// A way found across the tiles.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Way {
    /// Waypoints from where it starts, not including that.
    pub points: Vec<[f32; 2]>,
    /// Whether it's only the first stage of a way longer than
    /// [`MOST_STEPS`], ending at the tile reached nearest where it's going.
    pub stage: bool,
}

/// The way from `from` towards `to`. Where `to` can't be reached the way
/// ends at the reachable tile nearest it, as it does where the way would be
/// longer than [`MOST_STEPS`]; `None` where that cuts it short at the tile
/// `from` is in.
pub fn find(pathing: &Pathing, domain: Domain, from: [f32; 2], to: [f32; 2]) -> Option<Way> {
    let start = Tile::at(from);
    let goal = Tile::at(to);
    let (tiles, stage) = search(pathing, domain, start, goal);
    let Some(&end) = tiles.last() else {
        return Some(Way::default());
    };
    if stage && end == start {
        return None;
    }
    let mut points: Vec<[f32; 2]> = tiles[1..].iter().map(|t| t.centre()).collect();
    match points.last_mut() {
        Some(last) if end == goal => *last = to,
        None if end == goal => points.push(to),
        _ => {}
    }
    let points = smooth(pathing, domain, from, points);
    Some(Way { points, stage })
}

/// Whether a mover on `domain` can get from `from` to `to`.
pub fn reaches(pathing: &Pathing, domain: Domain, from: [f32; 2], to: [f32; 2]) -> bool {
    let goal = Tile::at(to);
    pathing.passable(goal, domain)
        && search(pathing, domain, Tile::at(from), goal).0.last() == Some(&goal)
}

#[derive(PartialEq)]
struct Open {
    estimate: f32,
    tile: usize,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.estimate.total_cmp(&self.estimate)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The tiles from `start` to `goal`, or to the explored tile nearest `goal`
/// if there's no way there or the search got [`MOST_STEPS`] out first, and
/// whether it did. The start tile is left even if it's blocked.
fn search(pathing: &Pathing, domain: Domain, start: Tile, goal: Tile) -> (Vec<Tile>, bool) {
    let side = pathing.side;
    let Some(s) = start.index(side) else {
        return (Vec::new(), false);
    };
    let estimate = |t: Tile| {
        let (dx, dy) = ((t.x - goal.x).abs() as f32, (t.y - goal.y).abs() as f32);
        dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
    };
    let n = side * side;
    let mut cost = vec![f32::INFINITY; n];
    let mut came_from = vec![usize::MAX; n];
    let mut steps = vec![0u32; n];
    let mut cut = false;
    let mut open = BinaryHeap::new();
    cost[s] = 0.0;
    open.push(Open {
        estimate: estimate(start),
        tile: s,
    });
    let mut nearest = (estimate(start), s);
    while let Some(Open { tile: i, .. }) = open.pop() {
        let at = Tile::of_index(i, side);
        let left = estimate(at);
        if left < nearest.0 {
            nearest = (left, i);
        }
        if at == goal {
            break;
        }
        if cut {
            break;
        }
        for (dx, dy) in NEIGHBOURS {
            let next = at.offset(dx, dy);
            let Some(j) = next.index(side).filter(|_| pathing.passable(next, domain)) else {
                continue;
            };
            // No squeezing diagonally between two blocked tiles.
            let diagonal = dx != 0 && dy != 0;
            if diagonal
                && !(pathing.passable(at.offset(dx, 0), domain)
                    && pathing.passable(at.offset(0, dy), domain))
            {
                continue;
            }
            let step = if diagonal {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let c = cost[i] + step;
            if c < cost[j] && steps[i] >= MOST_STEPS {
                // One step further than a way goes: it settles for what
                // it's reached.
                cut = true;
                continue;
            }
            if c < cost[j] {
                steps[j] = steps[i] + 1;
                cost[j] = c;
                came_from[j] = i;
                open.push(Open {
                    estimate: c + estimate(next),
                    tile: j,
                });
            }
        }
    }
    let mut out = vec![Tile::of_index(nearest.1, side)];
    let mut i = nearest.1;
    while came_from[i] != usize::MAX {
        i = came_from[i];
        out.push(Tile::of_index(i, side));
    }
    out.reverse();
    (out, cut)
}

const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Drops each waypoint that the one before it can see past.
fn smooth(
    pathing: &Pathing,
    domain: Domain,
    from: [f32; 2],
    points: Vec<[f32; 2]>,
) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let mut at = from;
    let mut i = 0;
    while i < points.len() {
        let mut far = i;
        while far + 1 < points.len() && clear(pathing, domain, at, points[far + 1]) {
            far += 1;
        }
        out.push(points[far]);
        at = points[far];
        i = far + 1;
    }
    out
}

/// Whether every tile the straight line from `a` to `b` crosses is
/// passable, apart from the one `a` is in.
pub(crate) fn clear(pathing: &Pathing, domain: Domain, a: [f32; 2], b: [f32; 2]) -> bool {
    along(a, b, |tile| pathing.passable(tile, domain))
}

/// Whether the straight line from `a` to `b` crosses no tile whose ground
/// is closed to a mover on `domain`, whatever stands on it, apart from the
/// one `a` is in.
pub(crate) fn open(pathing: &Pathing, domain: Domain, a: [f32; 2], b: [f32; 2]) -> bool {
    along(a, b, |tile| pathing.crossable(tile, domain))
}

/// Whether every tile the straight line from `a` to `b` crosses is `fit`,
/// apart from the one `a` is in.
fn along(a: [f32; 2], b: [f32; 2], fit: impl Fn(Tile) -> bool) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    // A tenth of a tile at a time, so corners aren't cut by much.
    let steps = ((libm::hypotf(dx, dy) / CELL_SIZE) * 10.0).ceil().max(1.0) as usize;
    let first = Tile::at(a);
    (1..=steps).all(|s| {
        let t = s as f32 / steps as f32;
        let tile = Tile::at([a[0] + dx * t, a[1] + dy * t]);
        tile == first || fit(tile)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10-tile map with a wall down column 5 from row 0 to row 7.
    fn walled() -> Pathing {
        let mut p = Pathing::uniform(10, Domain::Land);
        for y in 0..8 {
            p.set_blocked(Tile::new(5, y));
        }
        p
    }

    #[test]
    fn straight_across_open_ground() {
        let p = Pathing::uniform(10, Domain::Land);
        assert_eq!(
            find(&p, Domain::Land, [1.0, 1.0], [15.0, 9.0])
                .unwrap()
                .points,
            [[15.0, 9.0]]
        );
    }

    #[test]
    fn around_a_wall() {
        let p = walled();
        let way = find(&p, Domain::Land, [3.0, 3.0], [17.0, 3.0])
            .unwrap()
            .points;
        assert_eq!(way.last(), Some(&[17.0, 3.0]));
        // It has to pass below the wall's end at row 7.
        assert!(way.len() >= 2);
        assert!(way.iter().any(|&[_, y]| y >= 16.0));
        let mut at = [3.0, 3.0];
        for &w in &way {
            assert!(clear(&p, Domain::Land, at, w), "{at:?} to {w:?}");
            at = w;
        }
    }

    #[test]
    fn to_the_nearest_reachable_tile() {
        let mut p = Pathing::uniform(10, Domain::Land);
        for y in 0..10 {
            p.set_blocked(Tile::new(5, y));
        }
        let way = find(&p, Domain::Land, [3.0, 3.0], [17.0, 3.0]).unwrap();
        assert_eq!(way.points, [[9.0, 3.0]]);
        assert!(!way.stage);
    }

    #[test]
    fn nowhere_off_the_map() {
        let p = Pathing::uniform(10, Domain::Land);
        assert!(
            find(&p, Domain::Land, [-5.0, 3.0], [3.0, 3.0])
                .unwrap()
                .points
                .is_empty()
        );
    }

    #[test]
    fn a_way_stops_at_254_tile_steps_for_the_tile_reached_nearest() {
        let p = Pathing::uniform(300, Domain::Land);
        // 280 tiles east: cut 254 steps out, the nearest reached.
        let way = find(&p, Domain::Land, [1.0, 1.0], [561.0, 1.0]).unwrap();
        assert!(way.stage);
        assert_eq!(way.points, [Tile::new(254, 0).centre()]);
        // 254 steps diagonally is just within it.
        let way = find(&p, Domain::Land, [1.0, 1.0], [509.0, 509.0]).unwrap();
        assert!(!way.stage);
        assert_eq!(way.points, [[509.0, 509.0]]);
    }

    #[test]
    fn a_way_cut_short_where_it_starts_is_none() {
        // A winding corridor: east along row 0 then back west along row 2,
        // so 254 steps out it's no nearer the end of row 2 than the start.
        let mut p = Pathing::uniform(300, Domain::Land);
        for x in 0..299 {
            p.set_blocked(Tile::new(x, 1));
        }
        for x in 0..300 {
            p.set_blocked(Tile::new(x, 3));
        }
        assert_eq!(find(&p, Domain::Land, [1.0, 1.0], [1.0, 5.0]), None);
    }
}
