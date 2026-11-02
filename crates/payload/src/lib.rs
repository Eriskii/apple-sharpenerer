//! Loaded into GUI apps: reshapes window corners and strips the window sheen.
//!
//! This crate is `no_std` on purpose. Apple's own apps are arm64e, where every code pointer that
//! crosses into system code is PAC-signed, and rustc's arm64e target doesn't emit that signing.
//! std's thread-locals are reached through signed pointers, so std (stdio, panics, HashMap, thread
//! handles) crashes inside an arm64e process. Everything here goes through [`pac`] instead.
#![no_std]

extern crate alloc;

mod pac;
mod rt;

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static INIT: extern "C" fn() = init;

extern "C" fn init() {
    log!("loaded into pid {} (arm64e: {})", unsafe { rt::getpid() }, cfg!(arm64e));
}
