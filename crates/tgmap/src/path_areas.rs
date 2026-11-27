//! The areas a map is cut into for planning moves: rectangles of one kind
//! of tile, cut down to a size the map's width sets, small ones joined to
//! a neighbour. Moves too far to plan over tiles at once go from area to
//! area along a chain of joined ones.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use tgdata::AreaRules;

use crate::tile::Tile;

/// The most areas a chain takes in, the one it starts in included.
pub const MOST_AREAS: usize = 254;

/// One area: tiles of one kind.
#[derive(Debug, Clone, PartialEq)]
pub struct PathArea {
    /// Its tiles' bits, which say who can cross them.
    pub kind: u8,
    /// The middle of its tiles.
    pub centre: [f32; 2],
    pub tiles: u32,
    /// The areas sharing a tile edge with it, of any kind.
    pub borders: Vec<u32>,
}

/// A map's areas, and which tile is in which.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PathAreas {
    pub areas: Vec<PathArea>,
    side: usize,
    /// Row-major, each tile's area.
    tiles: Vec<u32>,
}

/// A chain of joined areas, the one it starts in first.
#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub areas: Vec<u32>,
    /// Whether it was cut short of where it was going, at the area reached
    /// nearest there.
    pub cut: bool,
}

/// A rectangle of tiles: its low corner and its size.
#[derive(Debug, Clone, Copy)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl PathAreas {
    /// The areas of a map `side` tiles across whose row-major tiles have
    /// the kinds `bits`.
    pub fn draw(side: usize, bits: &[u8], rules: &AreaRules) -> PathAreas {
        let pieces = rectangles(side, bits, rules)
            .into_iter()
            .flat_map(|r| cut(r, widest(side, rules)))
            .collect::<Vec<_>>();
        let mut tiles = vec![u32::MAX; side * side];
        let mut sizes: Vec<u32> = Vec::new();
        let mut kinds: Vec<u8> = Vec::new();
        let fill = |tiles: &mut Vec<u32>, r: Rect, area: u32| {
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    tiles[y * side + x] = area;
                }
            }
        };
        let big = |r: &Rect| {
            (r.w * r.h) as u32 >= rules.least_tiles
                && r.w as u32 >= rules.least_width
                && r.h as u32 >= rules.least_width
        };
        for r in pieces.iter().filter(|r| big(r)) {
            fill(&mut tiles, *r, sizes.len() as u32);
            sizes.push((r.w * r.h) as u32);
            kinds.push(bits[r.y * side + r.x]);
        }
        for r in pieces.iter().filter(|r| !big(r)) {
            let kind = bits[r.y * side + r.x];
            let mut shared: Vec<(u32, u32)> = Vec::new();
            for (x, y) in edge(*r, side) {
                let a = tiles[y * side + x];
                if a == u32::MAX
                    || kinds[a as usize] != kind
                    || sizes[a as usize] > rules.most_joined
                {
                    continue;
                }
                match shared.iter_mut().find(|(b, _)| *b == a) {
                    Some((_, n)) => *n += 1,
                    None => shared.push((a, 1)),
                }
            }
            // The most edge shared, then the smaller.
            let best = shared
                .iter()
                .max_by(|(a, n), (b, m)| n.cmp(m).then(sizes[*b as usize].cmp(&sizes[*a as usize])))
                .map(|&(a, _)| a);
            let area = best.unwrap_or_else(|| {
                sizes.push(0);
                kinds.push(kind);
                sizes.len() as u32 - 1
            });
            fill(&mut tiles, *r, area);
            sizes[area as usize] += (r.w * r.h) as u32;
        }
        let mut sums = vec![[0.0f32; 2]; sizes.len()];
        let mut borders = vec![Vec::new(); sizes.len()];
        for i in 0..tiles.len() {
            let a = tiles[i] as usize;
            let [cx, cy] = Tile::of_index(i, side).centre();
            sums[a] = [sums[a][0] + cx, sums[a][1] + cy];
            let (x, y) = (i % side, i / side);
            let next = [
                (x + 1 < side).then(|| i + 1),
                (y + 1 < side).then(|| i + side),
            ];
            for j in next.into_iter().flatten() {
                let b = tiles[j] as usize;
                if a != b && !borders[a].contains(&(b as u32)) {
                    borders[a].push(b as u32);
                    borders[b].push(a as u32);
                }
            }
        }
        let areas = (0..sizes.len())
            .map(|a| PathArea {
                kind: kinds[a],
                centre: sums[a].map(|s| s / sizes[a] as f32),
                tiles: sizes[a],
                borders: std::mem::take(&mut borders[a]),
            })
            .collect();
        PathAreas { areas, side, tiles }
    }

    /// The area the tile under `at` is in, if it's on the map.
    pub fn at(&self, at: [f32; 2]) -> Option<u32> {
        Tile::at(at).index(self.side).map(|i| self.tiles[i])
    }

    /// Whether `at` is in area `a` or one sharing a tile edge with it.
    pub fn in_or_beside(&self, a: u32, at: [f32; 2]) -> bool {
        let Some(here) = self.at(at) else {
            return false;
        };
        here == a || self.areas[a as usize].borders.contains(&here)
    }

    /// The chain of areas, each joined to the next and all with tiles
    /// `bit` lets a mover cross, from the one `from` is in to the one `to`
    /// is in, nearest first by the way between their centres. One longer
    /// than [`MOST_AREAS`] is cut at the area reached nearest `to`. None
    /// where no chain joins them.
    pub fn chain(&self, bit: u8, from: [f32; 2], to: [f32; 2]) -> Option<Chain> {
        let (start, goal) = (self.at(from)?, self.at(to)?);
        let crosses = |a: u32| self.areas[a as usize].kind & bit != 0;
        if !crosses(start) || !crosses(goal) {
            return None;
        }
        let n = self.areas.len();
        let left = |a: u32| distance(self.areas[a as usize].centre, to);
        let mut cost = vec![f32::INFINITY; n];
        let mut came = vec![u32::MAX; n];
        let mut depth = vec![0usize; n];
        let mut open = BinaryHeap::new();
        cost[start as usize] = 0.0;
        depth[start as usize] = 1;
        open.push(Open(left(start), start));
        let (mut nearest, mut cut) = (start, false);
        let mut found = false;
        while let Some(Open(_, a)) = open.pop() {
            if a == goal {
                found = true;
                break;
            }
            if left(a) < left(nearest) {
                nearest = a;
            }
            let d = depth[a as usize];
            for &b in &self.areas[a as usize].borders {
                if !crosses(b) {
                    continue;
                }
                let step = distance(self.areas[a as usize].centre, self.areas[b as usize].centre);
                let c = cost[a as usize] + step;
                if c >= cost[b as usize] {
                    continue;
                }
                if d >= MOST_AREAS {
                    cut = true;
                    continue;
                }
                (cost[b as usize], came[b as usize], depth[b as usize]) = (c, a, d + 1);
                open.push(Open(c + left(b), b));
            }
        }
        let end = match (found, cut) {
            (true, _) => goal,
            (false, true) => nearest,
            (false, false) => return None,
        };
        let mut areas = vec![end];
        while let Some(&a) = areas.last()
            && came[a as usize] != u32::MAX
        {
            areas.push(came[a as usize]);
        }
        areas.reverse();
        Some(Chain { areas, cut: !found })
    }
}

#[derive(PartialEq)]
struct Open(f32, u32);

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0)
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    libm::hypotf(a[0] - b[0], a[1] - b[1])
}

/// The longest side an area is cut to on a map `side` tiles across.
fn widest(side: usize, rules: &AreaRules) -> usize {
    let w = rules.widest * libm::powf(side as f32 / rules.base_width, rules.width_power);
    (w.round() as usize).max(1)
}

/// The rectangles of one kind of tile covering the map: squares from the
/// largest down, each grown right and down as far as its kind goes.
fn rectangles(side: usize, bits: &[u8], rules: &AreaRules) -> Vec<Rect> {
    let mut covered = vec![false; side * side];
    let mut out = Vec::new();
    let [mult, div] = rules.shrink;
    // Down to single tiles, so every tile is in one.
    let mut size = rules.first_square.max(1) as usize;
    while size >= 1 {
        for x in 0..side {
            for y in 0..side {
                let kind = bits[y * side + x];
                let fits = |r: Rect, covered: &[bool]| {
                    r.x + r.w <= side
                        && r.y + r.h <= side
                        && (r.y..r.y + r.h).all(|ty| {
                            (r.x..r.x + r.w).all(|tx| {
                                let i = ty * side + tx;
                                !covered[i] && bits[i] == kind
                            })
                        })
                };
                let mut r = Rect {
                    x,
                    y,
                    w: size,
                    h: size,
                };
                if !fits(r, &covered) {
                    continue;
                }
                loop {
                    // Only the column or row it would take in.
                    let column = Rect {
                        x: r.x + r.w,
                        w: 1,
                        ..r
                    };
                    let grew_w = fits(column, &covered);
                    if grew_w {
                        r.w += 1;
                    }
                    let row = Rect {
                        y: r.y + r.h,
                        h: 1,
                        ..r
                    };
                    let grew_h = fits(row, &covered);
                    if grew_h {
                        r.h += 1;
                    }
                    if !grew_w && !grew_h {
                        break;
                    }
                }
                for ty in r.y..r.y + r.h {
                    for tx in r.x..r.x + r.w {
                        covered[ty * side + tx] = true;
                    }
                }
                out.push(r);
            }
        }
        let next = size * mult as usize / div.max(1) as usize;
        size = if next < size { next } else { size - 1 };
    }
    out
}

/// `r` cut into equal pieces, as near as whole tiles allow, none longer
/// than `widest` along a side.
fn cut(r: Rect, widest: usize) -> Vec<Rect> {
    let spans = |start: usize, length: usize| {
        let n = length.div_ceil(widest);
        let (each, extra) = (length / n, length % n);
        let mut at = start;
        (0..n)
            .map(|i| {
                let l = each + usize::from(i < extra);
                at += l;
                (at - l, l)
            })
            .collect::<Vec<_>>()
    };
    let (xs, ys) = (spans(r.x, r.w), spans(r.y, r.h));
    xs.iter()
        .flat_map(|&(x, w)| ys.iter().map(move |&(y, h)| Rect { x, y, w, h }))
        .collect()
}

/// The tiles just outside `r`'s edges, on the map.
fn edge(r: Rect, side: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for x in r.x..r.x + r.w {
        if r.y > 0 {
            out.push((x, r.y - 1));
        }
        if r.y + r.h < side {
            out.push((x, r.y + r.h));
        }
    }
    for y in r.y..r.y + r.h {
        if r.x > 0 {
            out.push((r.x - 1, y));
        }
        if r.x + r.w < side {
            out.push((r.x + r.w, y));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_land_is_cut_into_squares_as_wide_as_the_map_allows() {
        let rules = AreaRules::default();
        assert_eq!(widest(64, &rules), 12);
        assert_eq!(widest(128, &rules), 15);
        assert_eq!(widest(256, &rules), 19);
        // 64 tiles of one kind: one rectangle, cut into 6 × 6 pieces of
        // 10 or 11.
        let areas = PathAreas::draw(64, &vec![1; 64 * 64], &rules);
        assert_eq!(areas.areas.len(), 36);
        assert!(areas.areas.iter().all(|a| (100..=121).contains(&a.tiles)));
        let corner = &areas.areas[areas.at([1.0, 1.0]).unwrap() as usize];
        assert_eq!(corner.centre, [11.0, 11.0]);
        assert_eq!(corner.borders.len(), 2);
    }

    #[test]
    fn a_strip_too_small_for_an_area_joins_the_one_it_shares_most_with() {
        // A 2-tile-wide strip of water down the middle, and a 2 × 2 patch
        // of land in it at the top.
        let side = 20;
        let mut bits = vec![1u8; side * side];
        for y in 0..side {
            bits[y * side + 9] = 2;
            bits[y * side + 10] = 2;
        }
        let areas = PathAreas::draw(side, &bits, &AreaRules::default());
        let water = areas.at([19.0, 5.0]).unwrap();
        assert_eq!(areas.areas[water as usize].kind, 2);
        // The strip is 2 wide, too thin for one of its own, and has no
        // water area to join: it's one of its own anyway.
        assert_eq!(areas.at([19.0, 35.0]), Some(water));
        assert!(areas.in_or_beside(water, [17.0, 5.0]));
    }

    #[test]
    fn chains_go_round_what_they_cant_cross() {
        // Water across the middle but for a gap at the right.
        let side = 40;
        let mut bits = vec![1u8; side * side];
        for x in 0..35 {
            for y in 18..22 {
                bits[y * side + x] = 2;
            }
        }
        let areas = PathAreas::draw(side, &bits, &AreaRules::default());
        let chain = areas.chain(1, [5.0, 5.0], [5.0, 75.0]).unwrap();
        assert!(!chain.cut);
        assert_eq!(chain.areas.first(), areas.at([5.0, 5.0]).as_ref());
        assert_eq!(chain.areas.last(), areas.at([5.0, 75.0]).as_ref());
        // It goes by the gap.
        let passes = |a: &u32| areas.areas[*a as usize].centre[0] > 70.0;
        assert!(chain.areas.iter().any(passes));
        // Ships can't get from the water to land at all.
        assert_eq!(areas.chain(2, [5.0, 40.0], [5.0, 5.0]), None);
    }

    #[test]
    fn a_chain_longer_than_254_areas_is_cut_at_the_nearest_reached() {
        // Rows of land 3 tiles deep between rows of water, joined at
        // alternate ends: a corridor winding through hundreds of areas.
        let side = 200;
        let mut bits = vec![1u8; side * side];
        for y in (3..side).step_by(4) {
            let gap = if y % 8 == 3 { side - 1 } else { 0 };
            for x in (0..side).filter(|&x| x != gap) {
                bits[y * side + x] = 2;
            }
        }
        let areas = PathAreas::draw(side, &bits, &AreaRules::default());
        let (from, to) = ([1.0, 1.0], [1.0, 2.0 * 197.0 + 1.0]);
        let chain = areas.chain(1, from, to).unwrap();
        // It stops at the nearest of the areas it reached.
        assert!(chain.cut && chain.areas.len() <= MOST_AREAS);
        let left = |a: u32| distance(areas.areas[a as usize].centre, to);
        let end = left(*chain.areas.last().unwrap());
        assert!(chain.areas.iter().all(|&a| left(a) >= end));
        // Not so a short way along it.
        let chain = areas.chain(1, from, [1.0, 2.0 * 9.0 + 1.0]).unwrap();
        assert!(!chain.cut);
    }
}
