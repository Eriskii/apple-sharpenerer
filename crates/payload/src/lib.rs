//! Loaded into GUI apps: reshapes window corners and strips the window sheen.
//!
//! This crate is `no_std` on purpose. Apple's own apps are arm64e, where every code pointer that
//! crosses into system code is PAC-signed, and rustc doesn't emit that signing. std's thread-locals
//! are reached through signed pointers, so std (stdio, panics, HashMap, thread handles) crashes
//! inside an arm64e process. Everything here goes through [`pac`] instead.
//!
//! The injector maps this image into an app itself (no dyld, so no library validation), patches
//! [`config::SHARPENERER_CONFIG`] and starts a thread at `sharpenerer_bootstrap`. During
//! development it can also just be `dlopen`ed, which runs the constructor instead.
#![no_std]

extern crate alloc;

mod config;
mod hooks;
mod objc;
mod pac;
mod rt;
mod sys;
mod windows;

use core::ffi::c_void;

// The injector starts a bare Mach thread here. Such a thread can't run normal code, so it asks for
// a real pthread to run `sharpenerer_main`, then parks with a marker in x0 for the injector to see.
core::arch::global_asm!(
    ".globl _sharpenerer_bootstrap",
    ".no_dead_strip _sharpenerer_bootstrap",
    ".p2align 2",
    "_sharpenerer_bootstrap:",
    "  sub sp, sp, #0x20",
    "  mov x0, sp",
    "  mov x1, #0",
    "  adrp x2, _sharpenerer_main@PAGE",
    "  add x2, x2, _sharpenerer_main@PAGEOFF",
    "  paciza x2",
    "  mov x3, #0",
    "  bl _pthread_create_from_mach_thread",
    "  movz x0, #0x7072",
    "  movk x0, #0x6873, lsl #16",
    "1: b 1b",
);

#[unsafe(no_mangle)]
pub extern "C" fn sharpenerer_main(_: *mut c_void) -> *mut c_void {
    start();
    core::ptr::null_mut()
}

#[used]
#[unsafe(link_section = "__DATA,__mod_init_func")]
static INIT: extern "C" fn() = loaded_by_dyld;

extern "C" fn loaded_by_dyld() {
    start();
}

fn start() {
    let pid = unsafe { rt::getpid() };
    if !objc::class(windows::OBSERVER_CLASS).is_null() {
        log!("already running in pid {pid}");
        return;
    }
    if objc::class(c"NSCGSWindow").is_null() {
        log!("no AppKit window internals in pid {pid}; nothing to do");
        return;
    }
    config::listen();
    hooks::install();
    windows::observe();
    sys::on_main(windows::refresh_all);
    log!("running in pid {pid}: {:?}", config::current());
}
