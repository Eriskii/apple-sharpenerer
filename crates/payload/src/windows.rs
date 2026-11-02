//! Which windows we manage, what their corners should be, and re-applying everything when a
//! window moves or the config changes.

use alloc::vec::Vec;
use core::ffi::{CStr, c_void};

use geometry::{Corners, Rect, Screen};

use crate::objc::{self, Id, Imp, NIL, Sel, class, nsstring, responds};
use crate::sys::{self, CGPoint, CGRect, SpinLock};
use crate::{config, msg};

const TITLED: usize = 1;
const UTILITY: usize = 1 << 4;
const HUD: usize = 1 << 13;
const FULL_SCREEN: usize = 1 << 14;

/// Ordinary app windows only: titled, normal level, not panels, not full screen.
pub fn managed(w: Id) -> bool {
    if w.is_null() || !config::current().enabled {
        return false;
    }
    let style = msg!(usize; w, c"styleMask");
    if style & TITLED == 0 || style & (UTILITY | HUD | FULL_SCREEN) != 0 {
        return false;
    }
    if msg!(isize; w, c"level") != 0 || msg!(bool; w, c"isKindOfClass:", class(c"NSPanel") => Id) {
        return false;
    }
    let frame = msg!(CGRect; w, c"frame");
    frame.size.width >= 100.0 && frame.size.height >= 50.0
}

pub fn radii(w: Id) -> Corners<f64> {
    let config = config::current();
    let frame = Rect::from(msg!(CGRect; w, c"frame"));
    let mut screen = msg!(Id; w, c"screen");
    if screen.is_null() {
        screen = msg!(Id; class(c"NSScreen"), c"mainScreen");
    }
    if screen.is_null() {
        return geometry::window_radii(frame, &Screen { frame, visible_frame: frame, corner_extents: Corners::all(0.0) }, &config.shape);
    }
    let screen_frame = msg!(CGRect; screen, c"frame");
    let mut corner_extents = display_corners(screen, screen_frame);
    if config.display_radius > 0.0 {
        let extent = config.display_radius * config.shape.curve.extent_factor();
        corner_extents = corner_extents.map(|e| if e > 0.0 { extent } else { 0.0 });
    }
    let screen = Screen { frame: screen_frame.into(), visible_frame: msg!(CGRect; screen, c"visibleFrame").into(), corner_extents };
    geometry::window_radii(frame, &screen, &config.shape)
}

/// Measured display corners, keyed by display frame. Cleared when displays or the config change.
static DISPLAYS: SpinLock<Vec<(CGRect, Corners<f64>)>> = SpinLock::new(Vec::new());

pub fn forget_displays() {
    DISPLAYS.with(|d| d.clear());
}

fn display_corners(screen: Id, frame: CGRect) -> Corners<f64> {
    if let Some(known) = DISPLAYS.with(|d| d.iter().find(|(f, _)| *f == frame).map(|(_, c)| *c)) {
        return known;
    }
    let corners = measure_bezel(screen);
    DISPLAYS.with(|d| d.push((frame, corners)));
    corners
}

/// Reads the panel outline from the private `NSScreen.bezelPath`; external displays have none.
fn measure_bezel(screen: Id) -> Corners<f64> {
    if !responds(screen, c"bezelPath") {
        return Corners::all(0.0);
    }
    let path = msg!(Id; screen, c"bezelPath");
    if path.is_null() {
        return Corners::all(0.0);
    }
    let bounds = msg!(CGRect; path, c"bounds");
    let count = msg!(isize; path, c"elementCount");
    let mut points = Vec::new();
    let mut p = [CGPoint::default(); 3];
    for i in 0..count {
        let kind = msg!(usize; path, c"elementAtIndex:associatedPoints:", i => isize, p.as_mut_ptr() => *mut CGPoint);
        // moveTo, lineTo, cubic curveTo, closePath, quadratic curveTo
        let end = match kind {
            0 | 1 => p[0],
            2 => p[2],
            4 => p[1],
            _ => continue,
        };
        points.push((end.x, end.y));
    }
    geometry::corner_extents_from_outline(points, bounds.into())
}

/// A +1 `NSCGSWindowCornerPathMask` for `w`, or nil if this macOS doesn't have the class.
pub fn corner_mask(w: Id, radii: Corners<f64>) -> Id {
    let cls = class(c"NSCGSWindowCornerPathMask");
    if cls.is_null() {
        return NIL;
    }
    let frame = msg!(CGRect; w, c"frame");
    let rect = Rect::new(0.0, 0.0, frame.size.width, frame.size.height);
    let path = sys::path(rect, radii, config::current().shape.curve);
    let mask = msg!(Id; msg!(Id; cls, c"alloc"), c"initWithCornerPath:clipsWindowContents:definesShadowShape:",
        path => sys::CGPathRef, true => bool, true => bool);
    unsafe { sys::CGPathRelease(path) };
    mask
}

/// Shows or hides the bright line AppKit draws along the top edge of `w`.
pub fn apply_highlight(w: Id) {
    let hide = managed(w) && !config::current().top_highlight;
    let content = msg!(Id; w, c"contentView");
    if content.is_null() {
        return;
    }
    let frame_view = msg!(Id; content, c"superview");
    if frame_view.is_null() {
        return;
    }
    let layer = find_layer(msg!(Id; frame_view, c"layer"), nsstring(c"CUIWindowBorderLayer"), 0);
    if !layer.is_null() {
        msg!((); layer, c"setHidden:", hide => bool);
    }
}

fn find_layer(layer: Id, name: Id, depth: u32) -> Id {
    if layer.is_null() || depth > 8 {
        return NIL;
    }
    let own = msg!(Id; layer, c"name");
    if !own.is_null() && msg!(bool; own, c"isEqualToString:", name => Id) {
        return layer;
    }
    let sublayers = msg!(Id; layer, c"sublayers");
    let count = if sublayers.is_null() { 0 } else { msg!(usize; sublayers, c"count") };
    for i in 0..count {
        let found = find_layer(msg!(Id; sublayers, c"objectAtIndex:", i => usize), name, depth + 1);
        if !found.is_null() {
            return found;
        }
    }
    NIL
}

fn is_titled(w: Id) -> bool {
    !w.is_null() && msg!(usize; w, c"styleMask") & TITLED != 0
}

/// Recomputes the window's shape (after a move or resize).
pub fn refresh_shape(w: Id) {
    if is_titled(w) && responds(w, c"_cornerMaskChanged") {
        msg!((); w, c"_cornerMaskChanged");
    }
}

/// Re-applies everything to one window.
pub fn refresh(w: Id) {
    if !is_titled(w) {
        return;
    }
    refresh_shape(w);
    apply_highlight(w);
    if responds(w, c"_setShadowParameters") {
        msg!((); w, c"_setShadowParameters");
    }
    msg!((); w, c"invalidateShadow");
}

pub extern "C" fn refresh_all(_: *mut c_void) {
    let app = msg!(Id; class(c"NSApplication"), c"sharedApplication");
    let windows = msg!(Id; app, c"windows");
    let count = if windows.is_null() { 0 } else { msg!(usize; windows, c"count") };
    for i in 0..count {
        refresh(msg!(Id; windows, c"objectAtIndex:", i => usize));
    }
}

extern "C" fn window_changed(_: Id, _: Sel, notification: Id) {
    refresh_shape(msg!(Id; notification, c"object"));
}

extern "C" fn screens_changed(_: Id, _: Sel, _: Id) {
    forget_displays();
    refresh_all(core::ptr::null_mut());
}

/// Name of the class we register; its existence also marks a process as already loaded.
pub const OBSERVER_CLASS: &CStr = c"SharpenererObserver";

/// Re-shapes windows when they move, resize, change screen or enter/leave full screen, and all
/// of them when the display layout changes.
pub fn observe() {
    let cls = objc::define_class(OBSERVER_CLASS, c"NSObject", &[
        (c"sharpenererWindowChanged:", window_changed as Imp, c"v@:@"),
        (c"sharpenererScreensChanged:", screens_changed as Imp, c"v@:@"),
    ]);
    if cls.is_null() {
        return;
    }
    // Lives as long as the process.
    let observer = msg!(Id; msg!(Id; cls, c"alloc"), c"init");
    let center = msg!(Id; class(c"NSNotificationCenter"), c"defaultCenter");
    let add = |method: &CStr, name: &CStr| {
        msg!((); center, c"addObserver:selector:name:object:",
            observer => Id, objc::sel(method) => Sel, nsstring(name) => Id, NIL => Id);
    };
    for name in [
        c"NSWindowDidMoveNotification",
        c"NSWindowDidResizeNotification",
        c"NSWindowDidChangeScreenNotification",
        c"NSWindowDidEnterFullScreenNotification",
        c"NSWindowDidExitFullScreenNotification",
    ] {
        add(c"sharpenererWindowChanged:", name);
    }
    add(c"sharpenererScreensChanged:", c"NSApplicationDidChangeScreenParametersNotification");
}
