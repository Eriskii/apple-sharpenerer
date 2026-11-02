//! Just enough of the Objective-C runtime: messaging, method swizzling, a runtime-defined class.

use core::ffi::{CStr, c_char, c_void};
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::pac;

pub type Id = *mut c_void;
pub type Sel = *const c_void;
pub type Imp = *const c_void;
pub const NIL: Id = core::ptr::null_mut();

#[link(name = "objc")]
unsafe extern "C" {
    pub fn objc_msgSend();
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn class_getInstanceMethod(cls: Id, sel: Sel) -> *mut c_void;
    fn method_getImplementation(method: *mut c_void) -> Imp;
    fn method_setImplementation(method: *mut c_void, imp: Imp) -> Imp;
    fn objc_allocateClassPair(superclass: Id, name: *const c_char, extra_bytes: usize) -> Id;
    fn objc_registerClassPair(cls: Id);
    fn class_addMethod(cls: Id, sel: Sel, imp: Imp, types: *const c_char) -> bool;
}

pub fn class(name: &CStr) -> Id {
    unsafe { objc_getClass(name.as_ptr()) }
}

pub fn sel(name: &CStr) -> Sel {
    unsafe { sel_registerName(name.as_ptr()) }
}

/// `msg!(ReturnType; receiver, c"selector:", arg => ArgType, ...)`
#[macro_export]
macro_rules! msg {
    ($ret:ty; $obj:expr, $sel:expr $(, $arg:expr => $ty:ty)* $(,)?) => {{
        #[allow(unused_unsafe)]
        let result = unsafe {
            let send: unsafe extern "C" fn($crate::objc::Id, $crate::objc::Sel $(, $ty)*) -> $ret =
                core::mem::transmute($crate::objc::objc_msgSend as *const ());
            send($obj, $crate::objc::sel($sel) $(, $arg)*)
        };
        result
    }};
}

pub fn responds(obj: Id, name: &CStr) -> bool {
    !obj.is_null() && msg!(bool; obj, c"respondsToSelector:", sel(name) => Sel)
}

pub fn nsstring(s: &CStr) -> Id {
    msg!(Id; class(c"NSString"), c"stringWithUTF8String:", s.as_ptr() => *const c_char)
}

/// A method's implementation from before we replaced it, stripped so Rust can call it.
pub struct Original(AtomicPtr<c_void>);

impl Original {
    pub const fn new() -> Self {
        Self(AtomicPtr::new(core::ptr::null_mut()))
    }

    pub fn get(&self) -> Imp {
        self.0.load(Ordering::Acquire)
    }
}

/// Replaces `class`'s implementation of `method`; false if either doesn't exist.
pub fn swizzle(class_name: &CStr, method: &CStr, imp: Imp, original: &Original) -> bool {
    let cls = class(class_name);
    if cls.is_null() {
        return false;
    }
    let m = unsafe { class_getInstanceMethod(cls, sel(method)) };
    if m.is_null() {
        return false;
    }
    // Publish the original before our implementation can be called.
    original.0.store(pac::strip(unsafe { method_getImplementation(m) }) as *mut c_void, Ordering::Release);
    unsafe { method_setImplementation(m, pac::sign(imp)) };
    true
}

/// Creates and registers an NSObject-style class; nil if a class with that name already exists.
pub fn define_class(name: &CStr, superclass: &CStr, methods: &[(&CStr, Imp, &CStr)]) -> Id {
    let cls = unsafe { objc_allocateClassPair(class(superclass), name.as_ptr(), 0) };
    if cls.is_null() {
        return NIL;
    }
    for &(method, imp, types) in methods {
        unsafe { class_addMethod(cls, sel(method), pac::sign(imp), types.as_ptr()) };
    }
    unsafe { objc_registerClassPair(cls) };
    cls
}
