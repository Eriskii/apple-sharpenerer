//! The AppKit methods we replace (all private, checked against macOS 26.4).

use core::ffi::CStr;
use core::mem::transmute;

use geometry::Corners;

use crate::objc::{Id, Imp, Original, Sel, class, nsstring, swizzle};
use crate::{config, log, msg, windows};

/// `-[NSCGSWindow setCornerMask:]` is where AppKit hands the window shape to the window server;
/// we hand over our per-corner path instead.
static SET_CORNER_MASK: Original = Original::new();

extern "C" fn set_corner_mask(this: Id, cmd: Sel, mask: Id) {
    let original: extern "C" fn(Id, Sel, Id) = unsafe { transmute(SET_CORNER_MASK.get()) };
    let window = window_for(this);
    if windows::managed(window) {
        let ours = windows::corner_mask(window, windows::radii(window));
        if !ours.is_null() {
            original(this, cmd, ours);
            msg!((); ours, c"release");
            return;
        }
    }
    original(this, cmd, mask)
}

fn window_for(cgs_window: Id) -> Id {
    let id = msg!(u32; cgs_window, c"windowID");
    let app = msg!(Id; class(c"NSApplication"), c"sharedApplication");
    msg!(Id; app, c"windowWithWindowNumber:", id as isize => isize)
}

// AppKit also uses the window's corner radius for its own drawing (titlebar, sidebars). Reporting
// the smallest of the corners we apply keeps that drawing inside our shape.
fn smallest(r: Corners<f64>) -> f64 {
    r.top_left.min(r.top_right).min(r.bottom_left).min(r.bottom_right)
}

fn smallest_top(r: Corners<f64>) -> f64 {
    r.top_left.min(r.top_right)
}

fn smallest_bottom(r: Corners<f64>) -> f64 {
    r.bottom_left.min(r.bottom_right)
}

fn radius(original: &Original, this: Id, cmd: Sel, window: Id, pick: fn(Corners<f64>) -> f64) -> f64 {
    if windows::managed(window) {
        return pick(windows::radii(window));
    }
    let f: extern "C" fn(Id, Sel) -> f64 = unsafe { transmute(original.get()) };
    f(this, cmd)
}

macro_rules! radius_hooks {
    ($($hook:ident, $original:ident, $class:literal, $method:literal, $pick:ident, $window:expr;)*) => {
        $(
            static $original: Original = Original::new();
            extern "C" fn $hook(this: Id, cmd: Sel) -> f64 {
                radius(&$original, this, cmd, $window(this), $pick)
            }
        )*
        fn radius_hooks() -> [(&'static CStr, &'static CStr, Imp, &'static Original); 6] {
            [$(($class, $method, $hook as Imp, &$original)),*]
        }
    };
}

fn itself(w: Id) -> Id {
    w
}

fn its_window(view: Id) -> Id {
    msg!(Id; view, c"window")
}

radius_hooks! {
    window_radius, WINDOW_RADIUS, c"NSWindow", c"_cornerRadius", smallest, itself;
    window_effective_radius, WINDOW_EFFECTIVE_RADIUS, c"NSWindow", c"_effectiveCornerRadius", smallest, itself;
    window_top_radius, WINDOW_TOP_RADIUS, c"NSWindow", c"_topCornerRadius", smallest_top, itself;
    window_bottom_radius, WINDOW_BOTTOM_RADIUS, c"NSWindow", c"_bottomCornerRadius", smallest_bottom, itself;
    frame_radius, FRAME_RADIUS, c"NSThemeFrame", c"_cornerRadius", smallest, its_window;
    frame_cached_radius, FRAME_CACHED_RADIUS, c"NSThemeFrame", c"_getCachedWindowCornerRadius", smallest, its_window;
}

/// `-[NSWindow shadowParameters]` feeds the window server the shadow, including the two rims.
static SHADOW_PARAMETERS: Original = Original::new();

extern "C" fn shadow_parameters(this: Id, cmd: Sel) -> Id {
    let original: extern "C" fn(Id, Sel) -> Id = unsafe { transmute(SHADOW_PARAMETERS.get()) };
    let params = original(this, cmd);
    let config = config::current();
    if params.is_null() || (config.inner_rim && config.outer_rim) || !windows::managed(this) {
        return params;
    }
    let ours = msg!(Id; params, c"mutableCopy");
    let zero = msg!(Id; class(c"NSNumber"), c"numberWithDouble:", 0.0 => f64);
    let clear = |keys: &[&CStr]| {
        for key in keys {
            msg!((); ours, c"setObject:forKey:", zero => Id, nsstring(key) => Id);
        }
    };
    if !config.inner_rim {
        clear(&[c"com.apple.WindowShadowInnerRimDensityActive", c"com.apple.WindowShadowInnerRimDensityInactive"]);
    }
    if !config.outer_rim {
        clear(&[c"com.apple.WindowShadowRimDensityActive", c"com.apple.WindowShadowRimDensityInactive"]);
    }
    msg!(Id; ours, c"autorelease")
}

/// `-[_NSTitlebarDecorationView _updateTopHighlightLayer]` (re)builds the top-edge highlight.
static UPDATE_TOP_HIGHLIGHT: Original = Original::new();

extern "C" fn update_top_highlight(this: Id, cmd: Sel) {
    let original: extern "C" fn(Id, Sel) = unsafe { transmute(UPDATE_TOP_HIGHLIGHT.get()) };
    original(this, cmd);
    windows::apply_highlight(its_window(this));
}

pub fn install() {
    let mut hooks: alloc::vec::Vec<(&CStr, &CStr, Imp, &Original)> = alloc::vec![
        (c"NSCGSWindow", c"setCornerMask:", set_corner_mask as Imp, &SET_CORNER_MASK),
        (c"NSWindow", c"shadowParameters", shadow_parameters as Imp, &SHADOW_PARAMETERS),
        (c"_NSTitlebarDecorationView", c"_updateTopHighlightLayer", update_top_highlight as Imp, &UPDATE_TOP_HIGHLIGHT),
    ];
    hooks.extend(radius_hooks());
    for (cls, method, imp, original) in hooks {
        if !swizzle(cls, method, imp, original) {
            log!("missing {}.{}; that part is skipped", cls.to_str().unwrap_or("?"), method.to_str().unwrap_or("?"));
        }
    }
}
