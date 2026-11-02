//! The live config: patched in by the injector, then kept current from a Darwin notification's
//! state.

use core::ffi::{CStr, c_int, c_void};
use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use geometry::Config;

use crate::{sys, windows};

/// The injector writes the packed config here before the payload starts. 0 means defaults.
#[unsafe(no_mangle)]
pub static SHARPENERER_CONFIG: AtomicU64 = AtomicU64::new(0);

/// Posted (with the packed config as its state) whenever the config changes.
pub const NOTIFICATION: &CStr = c"io.github.eriskii.sharpenerer.config";

static TOKEN: AtomicI32 = AtomicI32::new(-1);

pub fn current() -> Config {
    Config::unpack(SHARPENERER_CONFIG.load(Ordering::Relaxed)).unwrap_or_default()
}

pub fn listen() {
    let mut token: c_int = -1;
    if unsafe { sys::notify_register_check(NOTIFICATION.as_ptr(), &mut token) } == 0 {
        TOKEN.store(token, Ordering::Relaxed);
        adopt_state();
    }
    sys::observe_darwin(NOTIFICATION, changed);
}

fn adopt_state() {
    let mut state = 0;
    let token = TOKEN.load(Ordering::Relaxed);
    if token >= 0 && unsafe { sys::notify_get_state(token, &mut state) } == 0 && Config::unpack(state).is_some() {
        SHARPENERER_CONFIG.store(state, Ordering::Relaxed);
    }
}

extern "C" fn changed(_: *mut c_void, _: *const c_void, _: *const c_void, _: *const c_void, _: *const c_void) {
    adopt_state();
    windows::forget_displays();
    sys::on_main(windows::refresh_all);
}
