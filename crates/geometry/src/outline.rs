//! Window outlines with independent corner radii, built from lines and cubic Béziers so any
//! path API (CGPath, NSBezierPath) can replay them.

use crate::{CONTINUOUS_EXTENT, Corners, Curve, Rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathOp {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    CurveTo { c1: (f64, f64), c2: (f64, f64), to: (f64, f64) },
    Close,
}

/// One corner's curve for a unit radius, in corner-local coordinates: `u` runs back along the edge
/// the path arrives on, `v` forward along the edge it leaves on, both from the corner point. The
/// curve starts at `(extent, 0)`; each segment is `[c1, c2, to]`.
struct CornerCurve {
    extent: f64,
    segments: &'static [[(f64, f64); 3]],
}

/// A corner of the outline: where it is, the directions the path travels into and out of it, and
/// how much it's rounded.
#[derive(Clone, Copy)]
struct Corner {
    point: (f64, f64),
    arrive: (f64, f64),
    leave: (f64, f64),
    radius: f64,
}

impl Corner {
    /// A point in this corner's local coordinates, scaled by its radius.
    fn at(&self, (u, v): (f64, f64)) -> (f64, f64) {
        (
            self.point.0 + self.radius * (v * self.leave.0 - u * self.arrive.0),
            self.point.1 + self.radius * (v * self.leave.1 - u * self.arrive.1),
        )
    }
}

/// Control point distance of a cubic approximating a unit quarter circle.
const KAPPA: f64 = 0.552_284_749_8;

const CIRCULAR: CornerCurve = CornerCurve {
    extent: 1.0,
    segments: &[[(1.0 - KAPPA, 0.0), (0.0, 1.0 - KAPPA), (0.0, 1.0)]],
};

/// The widely used three-cubic approximation of Apple's continuous corner.
const CONTINUOUS: CornerCurve = CornerCurve {
    extent: CONTINUOUS_EXTENT,
    segments: &[
        [(1.088_493_23, 0.0), (0.868_406_89, 0.0), (0.631_493_99, 0.074_911_00)],
        [(0.372_823_92, 0.169_058_99), (0.169_058_99, 0.372_823_92), (0.074_911_00, 0.631_493_99)],
        [(0.0, 0.868_406_89), (0.0, 1.088_493_23), (0.0, CONTINUOUS_EXTENT)],
    ],
};

/// Emits the outline of `rect` counter-clockwise (y up), starting on the bottom edge. Radii are
/// clamped so no corner takes more than half of the shorter side.
pub fn outline(rect: Rect, radii: Corners<f64>, curve: Curve, mut emit: impl FnMut(PathOp)) {
    let shape = match curve {
        Curve::Circular => &CIRCULAR,
        Curve::Continuous => &CONTINUOUS,
    };
    let limit = rect.width.min(rect.height).max(0.0) / 2.0 / shape.extent;
    let r = radii.map(|r| r.max(0.0).min(limit));
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.max_x(), rect.max_y());
    let corners = [
        Corner { point: (x1, y0), arrive: (1.0, 0.0), leave: (0.0, 1.0), radius: r.bottom_right },
        Corner { point: (x1, y1), arrive: (0.0, 1.0), leave: (-1.0, 0.0), radius: r.top_right },
        Corner { point: (x0, y1), arrive: (-1.0, 0.0), leave: (0.0, -1.0), radius: r.top_left },
        Corner { point: (x0, y0), arrive: (0.0, -1.0), leave: (1.0, 0.0), radius: r.bottom_left },
    ];

    // Begin where the bottom-left corner's curve ends, so the loop closes on it.
    let (x, y) = corners[3].at((0.0, shape.extent));
    emit(PathOp::MoveTo(x, y));
    for corner in corners {
        if corner.radius == 0.0 {
            emit(PathOp::LineTo(corner.point.0, corner.point.1));
            continue;
        }
        let (x, y) = corner.at((shape.extent, 0.0));
        emit(PathOp::LineTo(x, y));
        for &[c1, c2, to] in shape.segments {
            emit(PathOp::CurveTo { c1: corner.at(c1), c2: corner.at(c2), to: corner.at(to) });
        }
    }
    emit(PathOp::Close);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(rect: Rect, radii: Corners<f64>, curve: Curve) -> Vec<PathOp> {
        let mut ops = Vec::new();
        outline(rect, radii, curve, |op| ops.push(op));
        ops
    }

    fn points(ops: &[PathOp]) -> Vec<(f64, f64)> {
        ops.iter()
            .flat_map(|op| match *op {
                PathOp::MoveTo(x, y) | PathOp::LineTo(x, y) => vec![(x, y)],
                PathOp::CurveTo { c1, c2, to } => vec![c1, c2, to],
                PathOp::Close => vec![],
            })
            .collect()
    }

    #[test]
    fn square_corners_are_plain_lines() {
        let ops = collect(Rect::new(10.0, 20.0, 100.0, 50.0), Corners::all(0.0), Curve::Continuous);
        assert_eq!(
            ops,
            [
                PathOp::MoveTo(10.0, 20.0),
                PathOp::LineTo(110.0, 20.0),
                PathOp::LineTo(110.0, 70.0),
                PathOp::LineTo(10.0, 70.0),
                PathOp::LineTo(10.0, 20.0),
                PathOp::Close,
            ]
        );
    }

    #[test]
    fn rounded_outline_stays_inside_and_closes() {
        let rect = Rect::new(0.0, 0.0, 300.0, 200.0);
        for curve in [Curve::Circular, Curve::Continuous] {
            let radii = Corners { top_left: 30.0, top_right: 0.0, bottom_left: 12.0, bottom_right: 50.0 };
            let ops = collect(rect, radii, curve);
            for (x, y) in points(&ops) {
                assert!((-1e-9..=300.0 + 1e-9).contains(&x) && (-1e-9..=200.0 + 1e-9).contains(&y), "{curve:?} ({x}, {y})");
            }
            let PathOp::MoveTo(sx, sy) = ops[0] else { panic!() };
            let PathOp::CurveTo { to, .. } = ops[ops.len() - 2] else { panic!("{:?}", ops[ops.len() - 2]) };
            assert!((to.0 - sx).abs() < 1e-9 && (to.1 - sy).abs() < 1e-9);
        }
    }

    #[test]
    fn continuous_corner_starts_further_out() {
        let ops = collect(Rect::new(0.0, 0.0, 400.0, 400.0), Corners { bottom_right: 10.0, ..Corners::all(0.0) }, Curve::Continuous);
        assert_eq!(ops[1], PathOp::LineTo(400.0 - 10.0 * CONTINUOUS_EXTENT, 0.0));
    }

    #[test]
    fn oversized_radii_are_clamped() {
        let ops = collect(Rect::new(0.0, 0.0, 40.0, 40.0), Corners::all(100.0), Curve::Circular);
        assert_eq!(ops[0], PathOp::MoveTo(20.0, 0.0));
        assert_eq!(ops[1], PathOp::LineTo(20.0, 0.0));
    }
}
