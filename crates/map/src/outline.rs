//! The edges of the ground zones cover, traced a tile's side at a time.

use std::collections::{BTreeMap, HashSet};

use crate::pathing::{CELL_SIZE, Zone};

/// The edges of the ground zones cover between them, as closed loops of
/// tile corners with that ground on their left: one around each patch and
/// one around each hole in it, patches that touch only at a corner apart.
/// It's kept from one change of the zones to the next, as where a loop
/// starts follows from what changed.
#[derive(Debug, Clone, Default)]
pub struct Outline {
    covered: HashSet<(i32, i32)>,
    /// Each loop's corners, in tiles, from its start.
    loops: Vec<Vec<(i32, i32)>>,
}

impl Outline {
    /// Traces the outline of the ground `zones` cover now. `block` is how
    /// many tiles a side the squares the game cuts the map into are: a
    /// loop through a square where the cover changed starts where it
    /// comes into the first such square, and any other where it did
    /// before.
    pub fn retrace<'a>(&mut self, zones: impl IntoIterator<Item = &'a Zone>, block: i32) {
        let block = block.max(1);
        let covered: HashSet<(i32, i32)> = zones.into_iter().flat_map(Zone::tiles).collect();
        // A square answers for the tiles just beyond its far sides too.
        let changed: HashSet<(i32, i32)> = (covered.symmetric_difference(&self.covered))
            .flat_map(|&(x, y)| [(x, y), (x - 1, y), (x, y - 1), (x - 1, y - 1)])
            .map(|(x, y)| (x.div_euclid(block), y.div_euclid(block)))
            .collect();
        // By its first side, as loops can share a corner.
        let first = |l: &Vec<(i32, i32)>| Some([*l.first()?, *l.get(1)?]);
        let starts: HashSet<[(i32, i32); 2]> = self.loops.iter().filter_map(first).collect();
        self.loops = trace(&covered)
            .into_iter()
            .map(|mut corners| {
                let start = start(&corners, block, &changed, &starts);
                corners.rotate_left(start);
                corners
            })
            .collect();
        self.covered = covered;
    }

    /// The loops, as world points a tile's side apart.
    pub fn loops(&self) -> Vec<Vec<[f32; 2]>> {
        let point = |&(x, y): &(i32, i32)| [x as f32 * CELL_SIZE, y as f32 * CELL_SIZE];
        (self.loops.iter())
            .map(|l| l.iter().map(point).collect())
            .collect()
    }
}

/// The loops round `covered`, each from wherever it was come upon.
fn trace(covered: &HashSet<(i32, i32)>) -> Vec<Vec<(i32, i32)>> {
    // Each side with open ground beyond it, from one corner to the next
    // going round the tile.
    let mut sides: BTreeMap<(i32, i32), Vec<(i32, i32)>> = BTreeMap::new();
    for &(x, y) in covered {
        let corners = [(x, y), (x + 1, y), (x + 1, y + 1), (x, y + 1)];
        let beyond = [(x, y - 1), (x + 1, y), (x, y + 1), (x - 1, y)];
        for (i, tile) in beyond.iter().enumerate() {
            if !covered.contains(tile) {
                let to = corners[(i + 1) % 4];
                sides.entry(corners[i]).or_default().push(to);
            }
        }
    }
    let mut loops = Vec::new();
    while let Some((&start, _)) = sides.iter().next() {
        let mut corners = Vec::new();
        let (mut at, mut from) = (start, None);
        // As many sides come to a corner as leave it, so this ends up
        // back where it began.
        while let Some(leaving) = sides.get_mut(&at) {
            // Where two leave, tiles touch at the corner alone, and the
            // way round the one it's on turns left.
            let left = from.map(|(fx, fy): (i32, i32)| (at.0 - (at.1 - fy), at.1 + (at.0 - fx)));
            let turn = leaving.iter().position(|&to| Some(to) == left);
            if at == start && from.is_some() && turn.is_none() {
                break;
            }
            let to = leaving.swap_remove(turn.unwrap_or(leaving.len() - 1));
            if leaving.is_empty() {
                sides.remove(&at);
            }
            corners.push(at);
            (from, at) = (Some(at), to);
        }
        loops.push(corners);
    }
    loops
}

/// Where in a loop's corners it starts.
fn start(
    corners: &[(i32, i32)],
    block: i32,
    changed: &HashSet<(i32, i32)>,
    starts: &HashSet<[(i32, i32); 2]>,
) -> usize {
    let n = corners.len();
    // Each side's tile, the lower of the two it's between, with that
    // tile's far side along y before its far side along x; and the
    // square that tile is in, a row of squares at a time.
    let side = |i: usize| {
        let ((ax, ay), (bx, by)) = (corners[i], corners[(i + 1) % n]);
        match ay == by {
            true => (ax.min(bx), ay - 1, 0),
            false => (ax - 1, ay.min(by), 1),
        }
    };
    let square = |i: usize| {
        let (x, y, _) = side(i);
        (y.div_euclid(block), x.div_euclid(block))
    };
    let redrawn = |i: &usize| {
        let (y, x) = square(*i);
        changed.contains(&(x, y))
    };
    let first = match (0..n).filter(redrawn).map(square).min() {
        Some(first) => first,
        // Untouched, it keeps its start, if it had one.
        None => match (0..n).find(|&i| starts.contains(&[corners[i], corners[(i + 1) % n]])) {
            Some(kept) => return kept,
            None => (0..n).map(square).min().unwrap_or_default(),
        },
    };
    let within = |i: usize| square(i) == first;
    let earliest = |sides: &mut dyn Iterator<Item = usize>| sides.min_by_key(|&i| side(i));
    // The stretches of it in that square, by where each comes in.
    let entries: Vec<usize> = (0..n)
        .filter(|&i| within(i) && !within((i + n - 1) % n))
        .collect();
    if entries.is_empty() {
        // All of it is: from the far end of its earliest side.
        return earliest(&mut (0..n)).map_or(0, |i| (i + 1) % n);
    }
    let stretch = |&entry: &usize| {
        let mut sides = (0..n).map(|k| (entry + k) % n).take_while(|&i| within(i));
        earliest(&mut sides).map(side)
    };
    entries.into_iter().min_by_key(stretch).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pathing::{Domain, Pathing};

    fn outlined(zones: &[&Zone]) -> Vec<Vec<[f32; 2]>> {
        let mut outline = Outline::default();
        outline.retrace(zones.iter().copied(), 4);
        outline.loops()
    }

    #[test]
    fn zones_that_touch_share_one_outline() {
        let p = Pathing::uniform(40, Domain::Land);
        let (a, b) = (p.zone([21.0, 41.0], 6.0), p.zone([31.0, 41.0], 6.0));
        let far = p.zone([61.0, 61.0], 6.0);
        let alone = outlined(&[&a]);
        assert_eq!(alone.len(), 1);
        // Each point is a tile's corner on the zone's edge: some of the
        // tiles round it are in the zone and some aren't.
        for &[x, y] in &alone[0] {
            let near = |dx: f32, dy: f32| a.holds([x + dx, y + dy]);
            let round = [
                near(0.5, 0.5),
                near(-0.5, 0.5),
                near(0.5, -0.5),
                near(-0.5, -0.5),
            ];
            assert!(round.contains(&true) && round.contains(&false), "{x}, {y}");
        }
        // Each step is a tile's side, with what it bounds on its left.
        let path = &alone[0];
        for i in 0..path.len() {
            let [x, y] = path[i];
            let [nx, ny] = path[(i + 1) % path.len()];
            let (dx, dy) = (nx - x, ny - y);
            assert_eq!(dx.abs() + dy.abs(), CELL_SIZE, "{x}, {y}");
            let (mx, my) = ((x + nx) / 2.0, (y + ny) / 2.0);
            let across = 0.6 / CELL_SIZE;
            assert!(a.holds([mx - dy * across, my + dx * across]), "{x}, {y}");
            assert!(!a.holds([mx + dy * across, my - dx * across]), "{x}, {y}");
        }
        let joined = outlined(&[&a, &b]);
        assert_eq!(joined.len(), 1);
        assert!(joined[0].len() > alone[0].len());
        assert!(joined[0].len() < 2 * alone[0].len());
        assert_eq!(outlined(&[&a, &b, &far]).len(), 2);
        assert!(outlined(&[]).is_empty());
    }

    fn patch(tiles: &[(i32, i32)]) -> Zone {
        Zone::of(tiles)
    }

    fn block(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
        (from.1..to.1)
            .flat_map(|y| (from.0..to.0).map(move |x| (x, y)))
            .collect()
    }

    #[test]
    fn tiles_touching_at_a_corner_are_outlined_apart() {
        let mut outline = Outline::default();
        outline.retrace([&patch(&[(1, 1), (2, 2)])], 4);
        let mut loops = outline.loops.clone();
        loops.sort();
        assert_eq!(loops.len(), 2);
        assert!(loops.iter().all(|l| l.len() == 4));
    }

    #[test]
    fn loops_start_where_they_come_into_the_first_square_that_changed() {
        let mut outline = Outline::default();
        // Wholly inside one square: from the far end of its earliest
        // side, the far side along x of the tile beside its first.
        outline.retrace([&patch(&[(1, 1), (2, 1)])], 4);
        assert_eq!(outline.loops[0][..2], [(1, 1), (2, 1)]);
        // Across squares, appearing on its own: where it comes into the
        // lowest square of the lowest row that holds any of its sides,
        // here along its far edge.
        let wide = patch(&block((2, 5), (10, 7)));
        outline.retrace([&wide], 4);
        assert_eq!(outline.loops.len(), 1);
        assert_eq!(outline.loops[0][..2], [(4, 7), (3, 7)]);
        // Growing at its far end moves the start to the first square
        // touched there; one that hasn't changed keeps its start.
        let before = outline.loops.clone();
        outline.retrace([&wide], 4);
        assert_eq!(outline.loops, before);
        let longer = patch(&block((2, 5), (13, 7)));
        outline.retrace([&longer], 4);
        assert_eq!(outline.loops[0][0], (8, 5));
    }
}
