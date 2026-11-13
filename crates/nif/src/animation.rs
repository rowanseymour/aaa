//! Playing animation data: sampling key groups and applying controllers'
//! data to transforms.

use crate::{Controller, Interpolation, Key, Keyframes, Keys, PathSettings, Rotation, Transform};

impl Controller {
    /// The time in the data's keys at `t` seconds of playing.
    pub fn key_time(&self, t: f32) -> f32 {
        let span = self.stop - self.start;
        let t = t * self.frequency + self.phase;
        if span <= 0.0 {
            return self.start;
        }
        // Flag bits 1-2: 0 loops, 1 plays back and forth, 2 holds the end.
        match (self.flags >> 1) & 3 {
            1 => {
                let u = (t - self.start).rem_euclid(2.0 * span);
                self.start + if u > span { 2.0 * span - u } else { u }
            }
            2 => t.clamp(self.start, self.stop),
            _ => self.start + (t - self.start).rem_euclid(span),
        }
    }
}

impl Keyframes {
    /// `base` with its animated parts taken from the keys at `time`.
    pub fn apply(&self, base: &Transform, time: f32) -> Transform {
        let mut out = *base;
        if let Some(t) = self.translation.sample(time) {
            out.translation = t;
        }
        if let Some(s) = self.scale.sample(time) {
            out.scale = s;
        }
        match &self.rotation {
            Rotation::Quaternion(keys) => {
                if let Some(q) = keys.sample_rotation(time) {
                    out.rotation = quaternion_matrix(q);
                }
            }
            Rotation::Euler(axes) if axes.iter().any(|k| !k.keys.is_empty()) => {
                let [x, y, z] = std::array::from_fn(|i| axes[i].sample(time).unwrap_or(0.0));
                out.rotation = euler_matrix(x, y, z);
            }
            Rotation::Euler(_) => {}
        }
        out
    }
}

/// A path controller's curve and how far along it its object is, measured
/// so the object can be placed by distance along it.
#[derive(Debug, Clone)]
pub struct Path {
    pub settings: PathSettings,
    pub positions: Keys<[f32; 3]>,
    /// The fraction of the way along, by the controller's key time.
    pub percent: Keys<f32>,
    /// Times through the curve's keys, each with the curve's length up to
    /// it.
    lengths: Vec<(f32, f32)>,
}

/// Steps measured along each stretch of curve between two keys.
const PATH_STEPS: usize = 32;

impl Path {
    pub fn new(settings: PathSettings, positions: Keys<[f32; 3]>, percent: Keys<f32>) -> Path {
        let mut lengths = Vec::new();
        for pair in positions.keys.windows(2) {
            let (t0, t1) = (pair[0].time, pair[1].time);
            if t1 <= t0 {
                continue;
            }
            if lengths.is_empty() {
                lengths.push((t0, 0.0));
            }
            let mut at = positions.sample(t0).unwrap_or(pair[0].value);
            for i in 1..=PATH_STEPS {
                let t = t0 + (t1 - t0) * i as f32 / PATH_STEPS as f32;
                let next = positions.sample(t).unwrap_or(pair[1].value);
                let total = lengths.last().map_or(0.0, |&(_, l)| l) + distance(at, next);
                lengths.push((t, total));
                at = next;
            }
        }
        Path {
            settings,
            positions,
            percent,
            lengths,
        }
    }

    pub fn constant_speed(&self) -> bool {
        self.settings.flags & 0x10 != 0
    }

    pub fn follows(&self) -> bool {
        self.settings.flags & 0x20 != 0
    }

    pub fn flips(&self) -> bool {
        self.settings.flags & 0x40 != 0
    }

    /// `base` moved to where the curve puts it at key time `time`, and, if
    /// it follows the curve, turned along it.
    pub fn apply(&self, base: &Transform, time: f32) -> Transform {
        let mut out = *base;
        let Some(p) = self.percent.sample(time) else {
            return out;
        };
        let t = match self.constant_speed() {
            true => self.time_at(p.rem_euclid(1.0)),
            false => p,
        };
        let Some(position) = self.positions.sample(t) else {
            return out;
        };
        out.translation = position;
        if self.follows()
            && let Some(rotation) = self.facing(t)
        {
            out.rotation = rotation;
        }
        out
    }

    /// The curve's key time a fraction `p` of its length along.
    fn time_at(&self, p: f32) -> f32 {
        let Some(&(_, total)) = self.lengths.last() else {
            return self.positions.keys.first().map_or(0.0, |k| k.time);
        };
        let d = p * total;
        let i = self.lengths.partition_point(|&(_, l)| l < d);
        match i {
            0 => self.lengths[0].0,
            i if i >= self.lengths.len() => self.lengths[i - 1].0,
            i => {
                let ((t0, l0), (t1, l1)) = (self.lengths[i - 1], self.lengths[i]);
                match l1 > l0 {
                    true => t0 + (t1 - t0) * (d - l0) / (l1 - l0),
                    false => t1,
                }
            }
        }
    }

    /// Rows of a rotation pointing the follow axis along the curve at key
    /// time `t` and laying another level beside it, turned half about the
    /// third for the flip flag (0x40); none where the curve doesn't move.
    /// Running straight up or down, only the follow axis is left, so the
    /// node is squashed onto the line of travel.
    fn facing(&self, t: f32) -> Option<[[f32; 3]; 3]> {
        let mut travel = normalized(self.tangent(t)?)?;
        let level = cross([0.0, 0.0, 1.0], travel);
        let mut level = normalized(level).unwrap_or([0.0; 3]);
        if self.flips() {
            travel = travel.map(|v| -v);
            level = level.map(|v| -v);
        }
        let [x, y, z] = match self.settings.follow_axis {
            0 => [travel, level, cross(travel, level)],
            1 => {
                let x = level.map(|v| -v);
                [x, travel, cross(x, travel)]
            }
            2 => [cross(level, travel), level, travel],
            _ => return None,
        };
        // The axes are the matrix's columns.
        Some(std::array::from_fn(|r| [x[r], y[r], z[r]]))
    }

    /// The direction of the curve at key time `t`, unscaled: at a key, the
    /// direction leaving it, at the last key the direction arriving.
    fn tangent(&self, t: f32) -> Option<[f32; 3]> {
        let keys = &self.positions.keys;
        if keys.len() < 2 {
            return None;
        }
        let i = keys
            .partition_point(|k| k.time <= t)
            .clamp(1, keys.len() - 1);
        let (a, b) = (&keys[i - 1], &keys[i]);
        let u = match b.time > a.time {
            true => ((t - a.time) / (b.time - a.time)).clamp(0.0, 1.0),
            false => 1.0,
        };
        Some(match (a.tangents, b.tangents) {
            (Some([_, out]), Some([inc, _]))
                if self.positions.interpolation == Interpolation::Quadratic =>
            {
                let u2 = u * u;
                a.value
                    .scaled(6.0 * u2 - 6.0 * u)
                    .plus(out.scaled(3.0 * u2 - 4.0 * u + 1.0))
                    .plus(b.value.scaled(6.0 * u - 6.0 * u2))
                    .plus(inc.scaled(3.0 * u2 - 2.0 * u))
            }
            _ => sub(b.value, a.value),
        })
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    sub(a, b).iter().map(|v| v * v).sum::<f32>().sqrt()
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalized(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = v.iter().map(|c| c * c).sum::<f32>().sqrt();
    (len > 1e-6).then(|| v.map(|c| c / len))
}

impl<T: Value> Keys<T> {
    /// The value at `time`, held before the first key and after the last.
    pub fn sample(&self, time: f32) -> Option<T> {
        let (a, b, u) = self.around(time)?;
        Some(match (a.tangents, b.tangents) {
            (Some([_, out]), Some([inc, _])) if self.interpolation == Interpolation::Quadratic => {
                let (u2, u3) = (u * u, u * u * u);
                a.value
                    .scaled(2.0 * u3 - 3.0 * u2 + 1.0)
                    .plus(out.scaled(u3 - 2.0 * u2 + u))
                    .plus(b.value.scaled(-2.0 * u3 + 3.0 * u2))
                    .plus(inc.scaled(u3 - u2))
            }
            _ => a.value.scaled(1.0 - u).plus(b.value.scaled(u)),
        })
    }

    /// The keys either side of `time` and how far it is between them.
    fn around(&self, time: f32) -> Option<(&Key<T>, &Key<T>, f32)> {
        let first = self.keys.first()?;
        let next = self.keys.iter().position(|k| k.time > time);
        Some(match next {
            Some(0) => (first, first, 0.0),
            None => {
                let last = self.keys.last()?;
                (last, last, 0.0)
            }
            Some(i) => {
                let (a, b) = (&self.keys[i - 1], &self.keys[i]);
                (a, b, (time - a.time) / (b.time - a.time))
            }
        })
    }
}

impl Keys<[f32; 4]> {
    /// The rotation at `time`, eased along the shorter arc.
    pub fn sample_rotation(&self, time: f32) -> Option<[f32; 4]> {
        let (a, b, u) = self.around(time)?;
        let dot: f32 = (0..4).map(|i| a.value[i] * b.value[i]).sum();
        let b = if dot < 0.0 {
            b.value.scaled(-1.0)
        } else {
            b.value
        };
        let q = a.value.scaled(1.0 - u).plus(b.scaled(u));
        let len = q.iter().map(|v| v * v).sum::<f32>().sqrt();
        (len > 0.0).then(|| q.map(|v| v / len))
    }
}

/// Values keys can hold.
pub trait Value: Copy {
    fn scaled(self, f: f32) -> Self;
    fn plus(self, other: Self) -> Self;
}

impl Value for f32 {
    fn scaled(self, f: f32) -> Self {
        self * f
    }
    fn plus(self, other: Self) -> Self {
        self + other
    }
}

impl<const N: usize> Value for [f32; N] {
    fn scaled(self, f: f32) -> Self {
        self.map(|v| v * f)
    }
    fn plus(self, other: Self) -> Self {
        std::array::from_fn(|i| self[i] + other[i])
    }
}

/// Rows of the matrix that rotates column vectors by `q` (w, x, y, z).
fn quaternion_matrix([w, x, y, z]: [f32; 4]) -> [[f32; 3]; 3] {
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

fn euler_matrix(x: f32, y: f32, z: f32) -> [[f32; 3]; 3] {
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    // Rz * Ry * Rx
    [
        [cy * cz, sx * sy * cz - cx * sz, cx * sy * cz + sx * sz],
        [cy * sz, sx * sy * sz + cx * cz, cx * sy * sz - sx * cz],
        [-sy, sx * cy, cx * cy],
    ]
}
