//! The bits of a runtime a no_std dylib needs: allocator, panic handler, logging.

use core::alloc::{GlobalAlloc, Layout};
use core::ffi::{c_char, c_int, c_void};

#[link(name = "System")]
unsafe extern "C" {
    pub fn getpid() -> c_int;
    fn abort() -> !;
    fn malloc(size: usize) -> *mut c_void;
    fn free(p: *mut c_void);
    fn posix_memalign(out: *mut *mut c_void, align: usize, size: usize) -> c_int;
    fn syslog(priority: c_int, format: *const c_char, ...);
}

struct Malloc;

unsafe impl GlobalAlloc for Malloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // malloc already guarantees 16-byte alignment on Darwin.
        if layout.align() <= 16 {
            return unsafe { malloc(layout.size()) as *mut u8 };
        }
        let mut p = core::ptr::null_mut();
        match unsafe { posix_memalign(&mut p, layout.align(), layout.size()) } {
            0 => p as *mut u8,
            _ => core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, p: *mut u8, _: Layout) {
        unsafe { free(p as *mut c_void) }
    }
}

#[global_allocator]
static ALLOCATOR: Malloc = Malloc;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    crate::log!("panic: {info}");
    unsafe { abort() }
}

/// Writes one line to the unified log (`log stream --predicate 'eventMessage BEGINSWITH "[sharpenerer]"'`).
pub fn log_line(message: &str) {
    let mut line = alloc::vec::Vec::with_capacity(message.len() + 16);
    line.extend_from_slice(b"[sharpenerer] ");
    line.extend(message.bytes().filter(|&b| b != 0));
    line.push(0);
    const LOG_NOTICE: c_int = 5;
    unsafe { syslog(LOG_NOTICE, c"%s".as_ptr(), line.as_ptr()) };
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::rt::log_line(&alloc::format!($($arg)*)) };
}
