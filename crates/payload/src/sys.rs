//! CoreGraphics, CoreFoundation and libSystem bits, plus a spin lock (no std Mutex here).

use core::cell::UnsafeCell;
use core::ffi::{CStr, c_char, c_int, c_void};
use core::sync::atomic::{AtomicBool, Ordering};

use geometry::{Corners, Curve, PathOp, Rect};

use crate::objc::Imp;
use crate::pac;

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

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFNotificationCenterGetDarwinNotifyCenter() -> *mut c_void;
    fn CFNotificationCenterAddObserver(
        center: *mut c_void,
        observer: *const c_void,
        callback: Imp,
        name: *const c_void,
        object: *const c_void,
        behavior: isize,
    );
    fn CFStringCreateWithCString(allocator: *const c_void, s: *const c_char, encoding: u32) -> *const c_void;
}

#[link(name = "System")]
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: Imp);
    pub fn notify_register_check(name: *const c_char, token: *mut c_int) -> u32;
    pub fn notify_get_state(token: c_int, state: *mut u64) -> u32;
}

/// Runs `work` on the main thread.
pub fn on_main(work: extern "C" fn(*mut c_void)) {
    unsafe { dispatch_async_f(&raw const _dispatch_main_q, core::ptr::null_mut(), pac::sign(work as Imp)) };
}

pub type DarwinCallback = extern "C" fn(*mut c_void, *const c_void, *const c_void, *const c_void, *const c_void);

/// Calls `callback` (on the main thread) whenever the Darwin notification `name` is posted.
pub fn observe_darwin(name: &CStr, callback: DarwinCallback) {
    const UTF8: u32 = 0x0800_0100;
    const DELIVER_IMMEDIATELY: isize = 4;
    unsafe {
        let cf_name = CFStringCreateWithCString(core::ptr::null(), name.as_ptr(), UTF8);
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetDarwinNotifyCenter(),
            core::ptr::null(),
            pac::sign(callback as Imp),
            cf_name,
            core::ptr::null(),
            DELIVER_IMMEDIATELY,
        );
    }
}

/// Guards tiny bits of shared state. Never call into AppKit while holding it.
pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), value: UnsafeCell::new(value) }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        let r = f(unsafe { &mut *self.value.get() });
        self.locked.store(false, Ordering::Release);
        r
    }
}
