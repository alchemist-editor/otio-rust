//! pyaaf2's keyframe interpolation, for baking a varying value per frame.
//!
//! A port of `VaryingValue.value_at` from pyaaf2's `misc.py` and the curve
//! code it calls in `interpolation.py`. Upstream's adapter calls it once per
//! frame of an effect when asked to bake keyframed properties, and the
//! numbers land in the timeline's metadata, so they are computed here with
//! the same operations in the same order, to get the same doubles.
//!
//! pyaaf2 fails in two places this does not:
//!
//! - A Bézier segment whose second point is not after its first reaches a
//!   line returning `p[1]`, a name that does not exist, and raises. This
//!   returns the first point's value, which is what the clamping around it
//!   does for times outside a segment.
//! - A Bézier segment whose curve has no root in range trips an
//!   `assert False`, beside a comment asking whether to fall back to the
//!   older method. This falls back to it.

/// How close to the unit interval a root may land and still count.
const EPSILON: f64 = 1e-10;

/// How a varying value interpolates between its control points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interpolation {
    /// Each point's value holds until the next.
    Constant,
    /// Straight lines between points.
    Linear,
    /// A Bézier curve through each pair, shaped by the points' tangents.
    Bezier,
    /// Media Composer's spline, tangents worked out from the neighbours.
    Cubic,
}

/// One control point, as `value_at` reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    pub(crate) time: f64,
    pub(crate) value: f64,
    /// The in and out tangents, each as a time and value offset.
    pub(crate) tangents: [(f64, f64); 2],
}

/// pyaaf2's `VaryingValue.value_at`: the value at time `t`.
///
/// `points` is the point list in order, and must not be empty.
pub(crate) fn value_at(points: &[Point], interpolation: Interpolation, t: f64) -> f64 {
    let index = nearest_index(points, t);
    let p1 = points[index];

    // Clamp if t is outside the range.
    if t < p1.time || index + 1 >= points.len() {
        return p1.value;
    }
    let p2 = points[index + 1];

    match interpolation {
        Interpolation::Constant => p1.value,
        Interpolation::Linear => {
            let t_len = p2.time - p1.time;
            let t_diff = t - p1.time;
            let t_mix = t_diff / t_len;
            lerp(p1.value, p2.value, t_mix)
        }
        Interpolation::Bezier => {
            let (t0, v0) = (p1.time, p1.value);
            let (t3, v3) = (p2.time, p2.value);
            let tangent = p1.tangents[1];
            let (t1, v1) = (t0 + tangent.0, v0 + tangent.1);
            let tangent = p2.tangents[0];
            let (t2, v2) = (t3 + tangent.0, v3 + tangent.1);
            bezier_interpolate((t0, v0), (t1, v1), (t2, v2), (t3, v3), t)
        }
        Interpolation::Cubic => {
            let (t1, v1) = (p1.time, p1.value);
            let (t2, v2) = (p2.time, p2.value);
            let (t0, v0) = if index >= 1 {
                let p0 = points[index - 1];
                (p0.time, p0.value)
            } else {
                (t1 - ((t2 - t1) * 0.5), v1)
            };
            let (t3, v3) = if index + 2 < points.len() {
                let p3 = points[index + 2];
                (p3.time, p3.value)
            } else {
                (t2 + ((t2 - t1) * 0.5), v2)
            };
            cubic_interpolate((t0, v0), (t1, v1), (t2, v2), (t3, v3), t)
        }
    }
}

/// A binary search for the last point at or before `t`, or the first point.
fn nearest_index(points: &[Point], t: f64) -> usize {
    let mut start: isize = 0;
    let mut end = isize::try_from(points.len()).unwrap_or(isize::MAX) - 1;
    loop {
        if end < start {
            return usize::try_from(end.max(0)).unwrap_or_default();
        }
        let m = (start + end).div_euclid(2);
        let p = points[usize::try_from(m).unwrap_or_default()];
        if p.time < t {
            start = m + 1;
        } else if p.time > t {
            end = m - 1;
        } else {
            return usize::try_from(m).unwrap_or_default();
        }
    }
}

type P = (f64, f64);

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn cubic_bezier(p0: f64, p1: f64, p2: f64, p3: f64, t: f64) -> f64 {
    let u = 1.0 - t;
    let w1 = u * u * u;
    let w2 = 3.0 * u * u * t;
    let w3 = 3.0 * u * t * t;
    let w4 = t * t * t;
    w1 * p0 + w2 * p1 + w3 * p2 + w4 * p3
}

fn valid_root(v: f64) -> bool {
    // There can be floating-point error.
    (-EPSILON..=1.0 + EPSILON).contains(&v)
}

fn cube_root(x: f64) -> f64 {
    if x < 0.0 {
        -x.abs().powf(1.0 / 3.0)
    } else {
        x.powf(1.0 / 3.0)
    }
}

/// Cardano's method for where a cubic Bézier crosses zero, as pyaaf2 does it.
#[expect(
    clippy::many_single_char_names,
    reason = "the names are pyaaf2's, which are the formula's"
)]
fn bezier_cubic_roots(pa: f64, pb: f64, pc: f64, pd: f64) -> Vec<f64> {
    let mut a = 3.0 * pa - 6.0 * pb + 3.0 * pc;
    let mut b = -3.0 * pa + 3.0 * pb;
    let mut c = pa;
    let d = -pa + 3.0 * pb - 3.0 * pc + pd;

    let mut result = Vec::new();
    if d.abs() < EPSILON {
        // Not a cubic curve.
        if a.abs() < EPSILON {
            // Not a quadratic either.
            if b.abs() < EPSILON {
                return result;
            }
            let root = -c / b;
            if valid_root(root) {
                result.push(root);
            }
            return result;
        }
        let q = (b * b - 4.0 * a * c).sqrt();
        let a2 = 2.0 * a;
        let root = (q - b) / a2;
        if valid_root(root) {
            result.push(root);
        }
        let root = (-b - q) / a2;
        if valid_root(root) {
            result.push(root);
        }
        return result;
    }

    a /= d;
    b /= d;
    c /= d;

    let p = (3.0 * b - a * a) / 3.0;
    let p3 = p / 3.0;
    let q = (2.0 * a * a * a - 9.0 * a * b + 27.0 * c) / 27.0;
    let q2 = q / 2.0;
    let discriminant = q2 * q2 + p3 * p3 * p3;

    if discriminant < 0.0 {
        let mp3 = -p / 3.0;
        let mp33 = mp3 * mp3 * mp3;
        let r = mp33.sqrt();
        let t = -q / (2.0 * r);
        let cosphi = t.clamp(-1.0, 1.0);
        let phi = cosphi.acos();
        let crtr = cube_root(r);
        let t1 = 2.0 * crtr;
        for turn in [0.0, 2.0, 4.0] {
            let root = t1 * ((phi + turn * std::f64::consts::PI) / 3.0).cos() - a / 3.0;
            if valid_root(root) {
                result.push(root);
            }
        }
        return result;
    }

    // Three real roots, two of them equal.
    if discriminant == 0.0 {
        let u1 = if q2 < 0.0 {
            cube_root(-q2)
        } else {
            -cube_root(q2)
        };
        let root = 2.0 * u1 - a / 3.0;
        if valid_root(root) {
            result.push(root);
        }
        let root = -u1 - a / 3.0;
        if valid_root(root) {
            result.push(root);
        }
        return result;
    }

    // One real root and two complex ones.
    let sd = discriminant.sqrt();
    let u1 = cube_root(sd - q2);
    let v1 = cube_root(sd + q2);
    let root = u1 - v1 - a / 3.0;
    if valid_root(root) {
        result.push(root);
    }
    result
}

/// A handle moved along its line to `p2`'s time, by similar triangles.
fn scale_handle(p0: P, p1: P, p2: P) -> P {
    let y = (p1.1 - p0.1) * (p2.0 - p0.0) / (p1.0 - p0.0);
    (p2.0, p0.1 + y)
}

fn bezier_interpolate(p0: P, p1: P, p2: P, p3: P, x: f64) -> f64 {
    // pyaaf2 returns `p[1]` here, which raises; see the module's notes.
    if p0.0 >= p3.0 {
        return p0.1;
    }

    let p1 = if p1.0 > p3.0 {
        scale_handle(p0, p1, p3)
    } else if p1.0 < p0.0 {
        (p0.0, p1.1)
    } else {
        p1
    };
    let p2 = if p2.0 < p0.0 {
        scale_handle(p3, p2, p0)
    } else if p2.0 > p3.0 {
        (p3.0, p2.1)
    } else {
        p2
    };

    // Offset the points so that x is the axis, and solve for zero.
    let roots = bezier_cubic_roots(p0.0 - x, p1.0 - x, p2.0 - x, p3.0 - x);
    let Some(&root) = roots.first() else {
        // pyaaf2 asserts here; see the module's notes.
        return bezier_interpolate_old(p0, p1, p2, p3, x);
    };
    cubic_bezier(p0.1, p1.1, p2.1, p3.1, root.clamp(0.0, 1.0))
}

/// pyaaf2's earlier Bézier method, which searches for the curve parameter.
#[expect(clippy::float_cmp, reason = "pyaaf2 compares exactly")]
fn bezier_interpolate_old(p0: P, p1: P, p2: P, p3: P, t: f64) -> f64 {
    let t_len = p3.0 - p0.0;
    let t_diff = t - p0.0;
    let mut guess_t = t_diff / t_len;
    for _ in 0..20 {
        let x = cubic_bezier(p0.0, p1.0, p2.0, p3.0, guess_t);
        if x == t {
            break;
        }
        let offset = x - t;
        guess_t -= offset / t_len;
        guess_t = guess_t.clamp(0.0, 1.0);
    }
    cubic_bezier(p0.1, p1.1, p2.1, p3.1, guess_t)
}

fn sign_no_zero(v: f64) -> i8 {
    if v >= 0.0 { 1 } else { -1 }
}

/// Media Composer's tangent at `p1`, as pyaaf2 worked it out.
fn calculate_tangent(p0: P, p1: P, p2: P, in_tangent: bool) -> P {
    let (x, y) = p1;
    let (px, py) = p0;
    let (nx, ny) = p2;

    let mut tan_x = if in_tangent {
        0.4 * (x - px)
    } else {
        0.4 * (nx - x)
    };

    let slope = (ny - py) / (nx - px);
    let prev_slope = (y - py) / (x - px);
    let next_slope = (ny - y) / (nx - x);

    #[expect(clippy::float_cmp, reason = "pyaaf2 compares exactly")]
    let flat = sign_no_zero(prev_slope) != sign_no_zero(next_slope)
        || sign_no_zero(slope) != sign_no_zero(next_slope)
        || ny == py;
    let mut tan_y = if flat {
        0.0
    } else {
        let height = (ny - py).abs();
        let h1 = (ny - y).abs();
        let h2 = (y - py).abs();
        let scale = h1.min(h2) / height * 2.0;
        scale * slope * tan_x
    };

    if in_tangent {
        tan_x *= -1.0;
        tan_y *= -1.0;
    }
    (tan_x, tan_y)
}

fn cubic_interpolate(p0: P, p1: P, p2: P, p3: P, t: f64) -> f64 {
    let (tan_x0, tan_y0) = calculate_tangent(p0, p1, p2, false);
    let (tan_x1, tan_y1) = calculate_tangent(p1, p2, p3, true);
    let start = p1;
    let end = p2;
    let handle0 = (start.0 + tan_x0, start.1 + tan_y0);
    let handle1 = (end.0 + tan_x1, end.1 + tan_y1);
    bezier_interpolate(start, handle0, handle1, end, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(time: f64, value: f64) -> Point {
        Point {
            time,
            value,
            tangents: [(0.0, 0.0); 2],
        }
    }

    #[test]
    fn times_outside_the_points_take_the_nearest_value() {
        let points = [point(10.0, 1.0), point(20.0, 3.0)];
        assert_eq!(value_at(&points, Interpolation::Linear, 0.0), 1.0);
        assert_eq!(value_at(&points, Interpolation::Linear, 30.0), 3.0);
    }

    #[test]
    fn linear_and_constant_go_as_their_names_say() {
        let points = [point(0.0, 0.0), point(10.0, 5.0)];
        assert_eq!(value_at(&points, Interpolation::Linear, 4.0), 2.0);
        assert_eq!(value_at(&points, Interpolation::Constant, 4.0), 0.0);
    }

    #[test]
    fn a_straight_bezier_is_a_line() {
        let mut a = point(0.0, 0.0);
        a.tangents[1] = (2.0, 2.0);
        let mut b = point(6.0, 6.0);
        b.tangents[0] = (-2.0, -2.0);
        let found = value_at(&[a, b], Interpolation::Bezier, 3.0);
        assert!((found - 3.0).abs() < 1e-9, "{found}");
    }

    #[test]
    fn a_degenerate_bezier_segment_does_not_fail() {
        // Two points at one time: pyaaf2 raises here.
        let points = [point(5.0, 1.0), point(5.0, 2.0), point(5.0, 3.0)];
        assert!(value_at(&points, Interpolation::Bezier, 5.0).is_finite());
    }
}
