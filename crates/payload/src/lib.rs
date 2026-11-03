//! Loaded into GUI apps: reshapes window corners and strips the window sheen.
//!
//! It runs on eris-inject-payload's `no_std` runtime, which is what makes it safe inside arm64e
//! apps. The daemon maps this image into an app itself (no dyld, so no library validation),
//! patches [`config::SHARPENERER_CONFIG`] and starts it at `sharpenerer_bootstrap`. During
//! development it can also just be `dlopen`ed, which runs the constructor instead.
#![no_std]

extern crate alloc;

pub(crate) use eris_inject_payload::{log, msg, objc, rt};

mod config;
mod hooks;
mod sys;
mod windows;

eris_inject_payload::payload!(name: "sharpenerer", log: "sharpenerer", start: start);

fn start(injected: bool) {
    let wait_for_appkit_ms = if injected { 10_000 } else { 0 };
    let pid = unsafe { rt::getpid() };
    if !objc::class(windows::OBSERVER_CLASS).is_null() {
        log!("already running in pid {pid}");
        return;
    }
    // The daemon loads us as soon as an app starts, which can be before AppKit is up.
    let mut waited = 0;
    while objc::class(c"NSCGSWindow").is_null() {
        if waited >= wait_for_appkit_ms {
            log!("no AppKit window internals in pid {pid}; nothing to do");
            return;
        }
        unsafe { rt::usleep(50_000) };
        waited += 50;
    }
    config::listen();
    hooks::install();
    windows::observe();
    sys::on_main(windows::refresh_all);
    log!("running in pid {pid}: {:?}", config::current());
}
