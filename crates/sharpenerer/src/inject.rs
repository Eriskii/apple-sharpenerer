//! Loads a dylib into a running process: a thread is started in the target that spawns a real
//! pthread, which calls `dlopen`. Same technique as yabai's scripting-addition loader (MIT, by
//! Åsmund Vikane, arm64e path based on work by Jeremy Legendre); the shellcode here is our own.
//!
//! Needs root, plus SIP's debugging restrictions off and the `-arm64e_preview_abi` boot-arg for
//! arm64e targets.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Duration;

type KernReturn = c_int;
type MachPort = u32;

// Entry point runs on a bare mach thread, which can't call most of libSystem. It asks
// pthread_create_from_mach_thread for a real thread to run `2:` (dlopen), then parks with a
// marker in x0 so the injector knows it can kill it.
std::arch::global_asm!(
    ".p2align 3",
    ".globl _sharpenerer_shellcode", "_sharpenerer_shellcode:",
    "  sub sp, sp, #0x20",
    "  mov x0, sp",
    "  mov x1, #0",
    "  adr x2, 2f",
    "  paciza x2",
    "  mov x3, #0",
    "  ldr x9, 3f",
    "  blr x9",
    "  movz x0, #0x7072",
    "  movk x0, #0x6873, lsl #16",
    "1: b 1b",
    "2: pacibsp",
    "  stp x29, x30, [sp, #-16]!",
    "  mov x29, sp",
    "  adr x0, 5f",
    "  mov x1, #2",
    "  ldr x9, 4f",
    "  blr x9",
    "  mov x0, #0",
    "  ldp x29, x30, [sp], #16",
    "  retab",
    "  .p2align 3",
    ".globl _sharpenerer_shellcode_pcfmt", "_sharpenerer_shellcode_pcfmt:", "3: .quad 0",
    ".globl _sharpenerer_shellcode_dlopen", "_sharpenerer_shellcode_dlopen:", "4: .quad 0",
    ".globl _sharpenerer_shellcode_path", "_sharpenerer_shellcode_path:", "5: .space 1024",
    ".globl _sharpenerer_shellcode_end", "_sharpenerer_shellcode_end:",
);

/// What the parked entry thread leaves in x0 ("shrp").
const DONE_MARKER: u64 = 0x6873_7072;
const PATH_CAPACITY: usize = 1024;

unsafe extern "C" {
    static sharpenerer_shellcode: u8;
    static sharpenerer_shellcode_pcfmt: u8;
    static sharpenerer_shellcode_dlopen: u8;
    static sharpenerer_shellcode_path: u8;
    static sharpenerer_shellcode_end: u8;

    static mach_task_self_: MachPort;
    fn task_for_pid(task: MachPort, pid: c_int, out: *mut MachPort) -> KernReturn;
    fn mach_port_deallocate(task: MachPort, port: MachPort) -> KernReturn;
    fn mach_vm_allocate(task: MachPort, addr: *mut u64, size: u64, flags: c_int) -> KernReturn;
    fn mach_vm_write(task: MachPort, addr: u64, data: *const c_void, count: u32) -> KernReturn;
    fn mach_vm_protect(task: MachPort, addr: u64, size: u64, set_max: c_int, prot: c_int) -> KernReturn;
    fn thread_create(task: MachPort, out: *mut MachPort) -> KernReturn;
    fn thread_create_running(task: MachPort, flavor: c_int, state: *const u32, count: u32, out: *mut MachPort) -> KernReturn;
    fn thread_terminate(thread: MachPort) -> KernReturn;
    fn thread_get_state(thread: MachPort, flavor: c_int, state: *mut u32, count: *mut u32) -> KernReturn;
    fn thread_convert_thread_state(
        thread: MachPort,
        direction: c_int,
        flavor: c_int,
        in_state: *const u32,
        in_count: u32,
        out_state: *mut u32,
        out_count: *mut u32,
    ) -> KernReturn;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn mach_error_string(kr: KernReturn) -> *const c_char;
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ArmThreadState64 {
    x: [u64; 29],
    fp: u64,
    lr: u64,
    sp: u64,
    pc: u64,
    cpsr: u32,
    flags: u32,
}

const ARM_THREAD_STATE64: c_int = 6;
const STATE_COUNT: u32 = (size_of::<ArmThreadState64>() / 4) as u32;
const THREAD_CONVERT_THREAD_STATE_FROM_SELF: c_int = 2;
const VM_FLAGS_ANYWHERE: c_int = 1;
const VM_PROT_READ: c_int = 1;
const VM_PROT_EXECUTE: c_int = 4;
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
const STACK_SIZE: u64 = 16 * 1024;

#[derive(Debug)]
pub enum Error {
    Mach(&'static str, KernReturn),
    PathTooLong,
    Timeout,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Mach(step, kr) => {
                let msg = unsafe { CStr::from_ptr(mach_error_string(*kr)) }.to_string_lossy();
                write!(f, "{step}: {msg} ({kr})")
            }
            Error::PathTooLong => write!(f, "payload path is longer than {} bytes", PATH_CAPACITY - 1),
            Error::Timeout => write!(f, "the loader thread never finished"),
        }
    }
}

fn check(step: &'static str, kr: KernReturn) -> Result<(), Error> {
    if kr == 0 { Ok(()) } else { Err(Error::Mach(step, kr)) }
}

/// Owns a send right and gives it back on drop.
struct Port(MachPort);

impl Drop for Port {
    fn drop(&mut self) {
        unsafe { mach_port_deallocate(mach_task_self_, self.0) };
    }
}

/// Address of a libSystem function, without its pointer signature. The shared cache sits at the
/// same address in every process, so it's valid in the target too.
fn system_fn(name: &CStr) -> u64 {
    let mut v = unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) } as u64;
    unsafe { std::arch::asm!("xpaci {0}", inout(reg) v, options(pure, nomem, nostack)) };
    v
}

fn shellcode(payload: &Path) -> Result<Vec<u8>, Error> {
    let path = CString::new(payload.as_os_str().as_bytes()).map_err(|_| Error::PathTooLong)?;
    let path = path.as_bytes_with_nul();
    if path.len() > PATH_CAPACITY {
        return Err(Error::PathTooLong);
    }
    let start = &raw const sharpenerer_shellcode as usize;
    let len = &raw const sharpenerer_shellcode_end as usize - start;
    let mut code = unsafe { std::slice::from_raw_parts(start as *const u8, len) }.to_vec();
    let mut patch = |at: *const u8, bytes: &[u8]| {
        let offset = at as usize - start;
        code[offset..offset + bytes.len()].copy_from_slice(bytes);
    };
    patch(&raw const sharpenerer_shellcode_pcfmt, &system_fn(c"pthread_create_from_mach_thread").to_le_bytes());
    patch(&raw const sharpenerer_shellcode_dlopen, &system_fn(c"dlopen").to_le_bytes());
    patch(&raw const sharpenerer_shellcode_path, path);
    Ok(code)
}

pub fn inject(pid: i32, payload: &Path) -> Result<(), Error> {
    let code = shellcode(payload)?;
    let mut task = 0;
    check("task_for_pid", unsafe { task_for_pid(mach_task_self_, pid, &mut task) })?;
    let task = Port(task);

    let (mut stack, mut remote_code) = (0u64, 0u64);
    unsafe {
        check("allocate stack", mach_vm_allocate(task.0, &mut stack, STACK_SIZE, VM_FLAGS_ANYWHERE))?;
        check("allocate code", mach_vm_allocate(task.0, &mut remote_code, code.len() as u64, VM_FLAGS_ANYWHERE))?;
        check("write code", mach_vm_write(task.0, remote_code, code.as_ptr().cast(), code.len() as u32))?;
        check(
            "protect code",
            mach_vm_protect(task.0, remote_code, code.len() as u64, 0, VM_PROT_READ | VM_PROT_EXECUTE),
        )?;
    }

    // Build the thread state as if for ourselves, then have the kernel re-sign it for the target
    // (needed when the target is arm64e).
    let state = ArmThreadState64 { pc: remote_code, sp: stack + STACK_SIZE / 2, ..Default::default() };
    let mut converted = ArmThreadState64::default();
    let mut count = STATE_COUNT;
    let mut scratch = 0;
    unsafe {
        check("thread_create", thread_create(task.0, &mut scratch))?;
        let kr = thread_convert_thread_state(
            scratch,
            THREAD_CONVERT_THREAD_STATE_FROM_SELF,
            ARM_THREAD_STATE64,
            (&raw const state).cast(),
            STATE_COUNT,
            (&raw mut converted).cast(),
            &mut count,
        );
        thread_terminate(scratch);
        mach_port_deallocate(mach_task_self_, scratch);
        check("convert thread state", kr)?;
    }
    let mut thread = 0;
    check("start thread", unsafe {
        thread_create_running(task.0, ARM_THREAD_STATE64, (&raw const converted).cast(), count, &mut thread)
    })?;
    let thread = Port(thread);

    // dlopen runs on the pthread; the entry thread only waits to be reaped.
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(10));
        let mut s = ArmThreadState64::default();
        let mut c = STATE_COUNT;
        check("read thread state", unsafe { thread_get_state(thread.0, ARM_THREAD_STATE64, (&raw mut s).cast(), &mut c) })?;
        if s.x[0] == DONE_MARKER {
            unsafe { thread_terminate(thread.0) };
            return Ok(());
        }
    }
    unsafe { thread_terminate(thread.0) };
    Err(Error::Timeout)
}
