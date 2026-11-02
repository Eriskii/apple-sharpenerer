//! Corner geometry shared by the payload and the CLI.
//!
//! Coordinates are AppKit screen coordinates: points, origin at the bottom-left, y grows upward.
//! Corner sizes come in two units: a *radius* (what the user configures) and an *extent* (how far
//! the rounding runs along each edge). They only differ for [`Curve::Continuous`], whose curve
//! starts further from the corner than a circular arc of the same radius.
#![cfg_attr(not(test), no_std)]

mod outline;

pub use outline::{PathOp, outline};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Corners<T> {
    pub top_left: T,
    pub top_right: T,
    pub bottom_left: T,
    pub bottom_right: T,
}

impl<T: Copy> Corners<T> {
    pub const fn all(v: T) -> Self {
        Self { top_left: v, top_right: v, bottom_left: v, bottom_right: v }
    }

    pub fn map<U>(self, f: impl Fn(T) -> U) -> Corners<U> {
        Corners {
            top_left: f(self.top_left),
            top_right: f(self.top_right),
            bottom_left: f(self.bottom_left),
            bottom_right: f(self.bottom_right),
        }
    }

    pub fn zip<U: Copy, V>(self, other: Corners<U>, f: impl Fn(T, U) -> V) -> Corners<V> {
        Corners {
            top_left: f(self.top_left, other.top_left),
            top_right: f(self.top_right, other.top_right),
            bottom_left: f(self.bottom_left, other.bottom_left),
            bottom_right: f(self.bottom_right, other.bottom_right),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Curve {
    Circular,
    /// Apple's "squircle" corner, as used by `CALayerCornerCurve.continuous`.
    #[default]
    Continuous,
}

/// How far a continuous corner of radius 1 extends along each edge.
pub const CONTINUOUS_EXTENT: f64 = 1.528_664_83;

impl Curve {
    pub const fn extent_factor(self) -> f64 {
        match self {
            Curve::Circular => 1.0,
            Curve::Continuous => CONTINUOUS_EXTENT,
        }
    }
}

/// Which rectangle a window's corners are measured against in screen-aware mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Anchor {
    /// The usable area: below the menu bar and beside the Dock (`NSScreen.visibleFrame`).
    #[default]
    Usable,
    /// The physical panel edges (`NSScreen.frame`).
    Display,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub frame: Rect,
    pub visible_frame: Rect,
    /// How far the physical rounding of each display corner extends; 0 for a square corner.
    pub corner_extents: Corners<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Radius of each corner when it isn't near a rounded screen corner.
    pub base: Corners<f64>,
    pub curve: Curve,
    /// `None` turns screen-aware rounding off.
    pub screen_aware: Option<Anchor>,
}

/// The radius for each corner of `window` on `screen`.
///
/// With screen-aware rounding, a corner follows the matching screen corner concentrically: flush
/// against it the window gets the screen's rounding, and every point of distance takes a point off,
/// until the corner falls back to its base radius. Square screen corners never round a window.
pub fn window_radii(window: Rect, screen: &Screen, settings: &Settings) -> Corners<f64> {
    let factor = settings.curve.extent_factor();
    let mut extents = settings.base.map(|r| r.max(0.0) * factor);
    if let Some(anchor) = settings.screen_aware {
        let anchor = match anchor {
            Anchor::Usable => screen.visible_frame,
            Anchor::Display => screen.frame,
        };
        let follow = screen.corner_extents.zip(corner_gaps(window, anchor), |e, gap| e - gap);
        extents = extents.zip(follow, f64::max);
    }
    let limit = window.width.min(window.height).max(0.0) / 2.0;
    extents.map(|e| e.max(0.0).min(limit) / factor)
}

/// Distance from each window corner to the matching corner of `anchor`: the larger of the
/// horizontal and vertical gaps. A window poking past the anchor counts as flush.
pub fn corner_gaps(window: Rect, anchor: Rect) -> Corners<f64> {
    let left = (window.x - anchor.x).max(0.0);
    let right = (anchor.max_x() - window.max_x()).max(0.0);
    let top = (anchor.max_y() - window.max_y()).max(0.0);
    let bottom = (window.y - anchor.y).max(0.0);
    Corners {
        top_left: left.max(top),
        top_right: right.max(top),
        bottom_left: left.max(bottom),
        bottom_right: right.max(bottom),
    }
}

/// Measures each corner's rounding from a display outline (`NSScreen.bezelPath`).
///
/// `points` are the outline's element end points, not a flattened path: a corner curve then
/// leaves each edge exactly at its first end point, so the gap to the edge's last on-edge point is
/// the corner's extent. `bounds` is the outline's bounding box.
pub fn corner_extents_from_outline(points: impl IntoIterator<Item = (f64, f64)>, bounds: Rect) -> Corners<f64> {
    const ON_EDGE: f64 = 0.5;
    let (min_x, max_x, min_y, max_y) = (bounds.x, bounds.max_x(), bounds.y, bounds.max_y());
    // For each edge, the on-edge points closest to either end of it.
    let mut left = (f64::INFINITY, f64::NEG_INFINITY);
    let mut right = (f64::INFINITY, f64::NEG_INFINITY);
    let mut top = (f64::INFINITY, f64::NEG_INFINITY);
    let mut bottom = (f64::INFINITY, f64::NEG_INFINITY);
    let widen = |span: &mut (f64, f64), v: f64| *span = (span.0.min(v), span.1.max(v));
    for (x, y) in points {
        if x - min_x <= ON_EDGE {
            widen(&mut left, y);
        }
        if max_x - x <= ON_EDGE {
            widen(&mut right, y);
        }
        if max_y - y <= ON_EDGE {
            widen(&mut top, x);
        }
        if y - min_y <= ON_EDGE {
            widen(&mut bottom, x);
        }
    }
    // An edge with no points at all means the outline is unusable there; call it square.
    let gap = |d: f64| if d.is_finite() { d.max(0.0) } else { 0.0 };
    Corners {
        top_left: gap(max_y - left.1).max(gap(top.0 - min_x)),
        top_right: gap(max_y - right.1).max(gap(max_x - top.1)),
        bottom_left: gap(left.0 - min_y).max(gap(bottom.0 - min_x)),
        bottom_right: gap(right.0 - min_y).max(gap(max_x - bottom.1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISPLAY: Rect = Rect::new(0.0, 0.0, 1800.0, 1169.0);
    // Menu bar is 38pt tall on this display.
    const USABLE: Rect = Rect::new(0.0, 0.0, 1800.0, 1131.0);

    fn macbook() -> Screen {
        Screen {
            frame: DISPLAY,
            visible_frame: USABLE,
            corner_extents: Corners { top_left: 44.0, top_right: 44.0, bottom_left: 0.0, bottom_right: 0.0 },
        }
    }

    fn settings(anchor: Option<Anchor>) -> Settings {
        Settings { base: Corners::all(0.0), curve: Curve::Circular, screen_aware: anchor }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn base_radii_without_screen_awareness() {
        let mut s = settings(None);
        s.base = Corners { top_left: 10.0, top_right: 0.0, bottom_left: 4.0, bottom_right: 0.0 };
        let r = window_radii(Rect::new(0.0, 0.0, 500.0, 500.0), &macbook(), &s);
        assert_eq!(r, s.base);
    }

    #[test]
    fn flush_window_takes_screen_rounding_and_bottoms_stay_square() {
        let window = Rect::new(0.0, 0.0, 900.0, 1169.0);
        let r = window_radii(window, &macbook(), &settings(Some(Anchor::Display)));
        assert_eq!(r, Corners { top_left: 44.0, top_right: 0.0, bottom_left: 0.0, bottom_right: 0.0 });
    }

    #[test]
    fn rounding_shrinks_concentrically_with_distance() {
        // Tiled with 8pt padding inside the usable area.
        let window = Rect::new(900.0, 8.0, 892.0, 1115.0);
        let r = window_radii(window, &macbook(), &settings(Some(Anchor::Usable)));
        assert!(close(r.top_right, 36.0));
        assert_eq!((r.top_left, r.bottom_left, r.bottom_right), (0.0, 0.0, 0.0));
    }

    #[test]
    fn display_anchor_ignores_corners_hidden_by_the_menu_bar() {
        let window = Rect::new(8.0, 8.0, 800.0, 1115.0);
        let r = window_radii(window, &macbook(), &settings(Some(Anchor::Display)));
        assert_eq!(r, Corners::all(0.0));
    }

    #[test]
    fn base_radius_wins_when_larger() {
        let mut s = settings(Some(Anchor::Usable));
        s.base = Corners::all(12.0);
        let r = window_radii(Rect::new(100.0, 100.0, 400.0, 300.0), &macbook(), &s);
        assert_eq!(r, Corners::all(12.0));
    }

    #[test]
    fn continuous_curve_converts_extent_back_to_radius() {
        let mut s = settings(Some(Anchor::Usable));
        s.curve = Curve::Continuous;
        let r = window_radii(Rect::new(0.0, 0.0, 900.0, 1131.0), &macbook(), &s);
        assert!(close(r.top_left * CONTINUOUS_EXTENT, 44.0));
    }

    #[test]
    fn radii_are_clamped_to_half_the_short_side() {
        let mut s = settings(None);
        s.base = Corners::all(100.0);
        let r = window_radii(Rect::new(0.0, 0.0, 60.0, 40.0), &macbook(), &s);
        assert_eq!(r, Corners::all(20.0));
    }

    #[test]
    fn window_past_the_anchor_counts_as_flush() {
        let gaps = corner_gaps(Rect::new(-10.0, -10.0, 100.0, 100.0), Rect::new(0.0, 0.0, 500.0, 500.0));
        assert_eq!(gaps.bottom_left, 0.0);
        assert_eq!(gaps.top_left, 410.0);
    }

    #[test]
    fn measures_macbook_bezel_outline() {
        // End points of NSScreen.bezelPath on a notched 1800x1169pt MacBook panel (macOS 26.4).
        let points = [
            (0.1, 1124.9), (0.8, 1148.0), (6.8, 1162.2), (21.0, 1168.2), (44.1, 1168.9),
            (781.7, 1168.9), (789.9, 1160.8), (789.9, 1148.0), (793.5, 1134.4), (807.0, 1130.8),
            (992.9, 1130.8), (1006.5, 1134.4), (1010.1, 1148.0), (1010.1, 1160.8), (1018.2, 1168.9),
            (1755.8, 1168.9), (1778.9, 1168.2), (1793.1, 1162.2), (1799.2, 1148.0), (1799.8, 1124.9),
            (1799.8, 0.1), (0.1, 0.1),
        ];
        let bounds = Rect::new(0.087, 0.077, 1799.748, 1168.836);
        let e = corner_extents_from_outline(points, bounds);
        assert!((e.top_left - 44.0).abs() < 0.2, "{e:?}");
        assert!((e.top_right - 44.0).abs() < 0.2, "{e:?}");
        assert!(e.bottom_left < 0.1 && e.bottom_right < 0.1, "{e:?}");
    }
}
