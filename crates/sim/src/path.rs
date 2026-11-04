//! Finding a way across the tile grid: A* over tiles, then pulled straight
//! wherever the ground between two points is clear.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use aaa_map::pathing::{CELL_SIZE, tile_at, tile_centre};
use aaa_map::{Domain, Pathing};

/// Waypoints from `from` towards `to`, not including `from`. Where `to`
/// can't be reached the way ends at the reachable tile nearest it.
pub fn find(pathing: &Pathing, domain: Domain, from: [f32; 2], to: [f32; 2]) -> Vec<[f32; 2]> {
    let start = tile_at(from);
    let goal = tile_at(to);
    let tiles = search(pathing, domain, start, goal);
    let Some(&end) = tiles.last() else {
        return Vec::new();
    };
    let mut points: Vec<[f32; 2]> = tiles[1..].iter().map(|&(x, y)| tile_centre(x, y)).collect();
    match points.last_mut() {
        Some(last) if end == goal => *last = to,
        None if end == goal => points.push(to),
        _ => {}
    }
    smooth(pathing, domain, from, points)
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
/// if there's no way there. The start tile is left even if it's blocked.
fn search(
    pathing: &Pathing,
    domain: Domain,
    start: (i32, i32),
    goal: (i32, i32),
) -> Vec<(i32, i32)> {
    let side = pathing.side as i32;
    let inside = |(x, y): (i32, i32)| (0..side).contains(&x) && (0..side).contains(&y);
    if !inside(start) {
        return Vec::new();
    }
    let index = |(x, y): (i32, i32)| (y * side + x) as usize;
    let tile = |i: usize| ((i as i32) % side, (i as i32) / side);
    let estimate = |(x, y): (i32, i32)| {
        let (dx, dy) = ((x - goal.0).abs() as f32, (y - goal.1).abs() as f32);
        dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
    };
    let n = pathing.side * pathing.side;
    let mut cost = vec![f32::INFINITY; n];
    let mut came_from = vec![usize::MAX; n];
    let mut open = BinaryHeap::new();
    let s = index(start);
    cost[s] = 0.0;
    open.push(Open {
        estimate: estimate(start),
        tile: s,
    });
    let mut nearest = (estimate(start), s);
    while let Some(Open { tile: i, .. }) = open.pop() {
        let (x, y) = tile(i);
        let left = estimate((x, y));
        if left < nearest.0 {
            nearest = (left, i);
        }
        if (x, y) == goal {
            break;
        }
        for (dx, dy) in NEIGHBOURS {
            let next = (x + dx, y + dy);
            if !pathing.passable(next.0, next.1, domain) {
                continue;
            }
            // No squeezing diagonally between two blocked tiles.
            let diagonal = dx != 0 && dy != 0;
            if diagonal
                && !(pathing.passable(x + dx, y, domain) && pathing.passable(x, y + dy, domain))
            {
                continue;
            }
            let step = if diagonal {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let j = index(next);
            let c = cost[i] + step;
            if c < cost[j] {
                cost[j] = c;
                came_from[j] = i;
                open.push(Open {
                    estimate: c + estimate(next),
                    tile: j,
                });
            }
        }
    }
    let mut out = vec![tile(nearest.1)];
    let mut i = nearest.1;
    while came_from[i] != usize::MAX {
        i = came_from[i];
        out.push(tile(i));
    }
    out.reverse();
    out
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
fn clear(pathing: &Pathing, domain: Domain, a: [f32; 2], b: [f32; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    // A tenth of a tile at a time, so corners aren't cut by much.
    let steps = ((dx.hypot(dy) / CELL_SIZE) * 10.0).ceil().max(1.0) as usize;
    let first = tile_at(a);
    (1..=steps).all(|s| {
        let t = s as f32 / steps as f32;
        let (x, y) = tile_at([a[0] + dx * t, a[1] + dy * t]);
        (x, y) == first || pathing.passable(x, y, domain)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10-tile map with a wall down column 5 from row 0 to row 7.
    fn walled() -> Pathing {
        let mut p = Pathing::uniform(10, Domain::Land);
        for y in 0..8 {
            p.set_blocked(5, y);
        }
        p
    }

    #[test]
    fn straight_across_open_ground() {
        let p = Pathing::uniform(10, Domain::Land);
        assert_eq!(
            find(&p, Domain::Land, [1.0, 1.0], [15.0, 9.0]),
            [[15.0, 9.0]]
        );
    }

    #[test]
    fn around_a_wall() {
        let p = walled();
        let way = find(&p, Domain::Land, [3.0, 3.0], [17.0, 3.0]);
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
            p.set_blocked(5, y);
        }
        let way = find(&p, Domain::Land, [3.0, 3.0], [17.0, 3.0]);
        assert_eq!(way, [[9.0, 3.0]]);
    }

    #[test]
    fn nowhere_off_the_map() {
        let p = Pathing::uniform(10, Domain::Land);
        assert!(find(&p, Domain::Land, [-5.0, 3.0], [3.0, 3.0]).is_empty());
    }
}
