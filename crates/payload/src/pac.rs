//! Pointer authentication for the arm64e slice. On the arm64 slice both functions are no-ops.

use core::arch::asm;
use core::ffi::c_void;

/// Signs a function pointer we hand to system code (IMPs, callbacks) the way arm64e C code would:
/// IA key, zero discriminator.
#[inline(always)]
pub fn sign(f: *const c_void) -> *const c_void {
    if !cfg!(arm64e) {
        return f;
    }
    let mut v = f as usize;
    unsafe { asm!("paciza {0}", inout(reg) v, options(pure, nomem, nostack)) };
    v as *const c_void
}

/// Strips the signature from a function pointer we got from system code, so Rust can call it
/// with a plain branch.
#[inline(always)]
pub fn strip(f: *const c_void) -> *const c_void {
    if !cfg!(arm64e) {
        return f;
    }
    let mut v = f as usize;
    unsafe { asm!("xpaci {0}", inout(reg) v, options(pure, nomem, nostack)) };
    v as *const c_void
}
