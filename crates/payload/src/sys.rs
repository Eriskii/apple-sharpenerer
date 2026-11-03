//! CoreGraphics paths, plus eris-inject-payload's main-thread, notification and lock helpers.

use core::ffi::c_void;

use geometry::{Corners, Curve, PathOp, Rect};

pub use eris_inject_payload::sys::{SpinLock, notify_get_state, notify_register_check, observe_darwin, on_main};

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

impl From<CGRect> for Rect {
    fn from(r: CGRect) -> Self {
        Rect::new(r.origin.x, r.origin.y, r.size.width, r.size.height)
    }
}

pub type CGPathRef = *mut c_void;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPathCreateMutable() -> CGPathRef;
    fn CGPathMoveToPoint(path: CGPathRef, m: *const c_void, x: f64, y: f64);
    fn CGPathAddLineToPoint(path: CGPathRef, m: *const c_void, x: f64, y: f64);
    fn CGPathAddCurveToPoint(path: CGPathRef, m: *const c_void, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64);
    fn CGPathCloseSubpath(path: CGPathRef);
    pub fn CGPathRelease(path: CGPathRef);
}

/// A +1 CGPath of `rect` with the given corners.
pub fn path(rect: Rect, radii: Corners<f64>, curve: Curve) -> CGPathRef {
    let p = unsafe { CGPathCreateMutable() };
    let null = core::ptr::null();
    geometry::outline(rect, radii, curve, |op| unsafe {
        match op {
            PathOp::MoveTo(x, y) => CGPathMoveToPoint(p, null, x, y),
            PathOp::LineTo(x, y) => CGPathAddLineToPoint(p, null, x, y),
            PathOp::CurveTo { c1, c2, to } => CGPathAddCurveToPoint(p, null, c1.0, c1.1, c2.0, c2.1, to.0, to.1),
            PathOp::Close => CGPathCloseSubpath(p),
        }
    });
    p
}
