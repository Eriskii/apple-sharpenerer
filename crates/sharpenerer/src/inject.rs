//! Puts the payload into a running app without going through dyld, so library validation never
//! looks at it:
//!
//! 1. map the payload's segments into the app and resolve its imports against the shared cache,
//!    which sits at the same address in every process;
//! 2. patch the packed config into it;
//! 3. have `sharpenerer-thread` start a thread at the payload's bootstrap code, which hands over
//!    to a proper pthread running `sharpenerer_main`.
//!
//! Needs root, SIP's debugging restrictions off and, for arm64e apps, the `-arm64e_preview_abi`
//! boot-arg. Based on the technique yabai uses for its scripting addition (MIT, Åsmund Vikane;
//! arm64e path after Jeremy Legendre); the code here is our own.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::macho::Payload;

type KernReturn = c_int;
type MachPort = u32;

unsafe extern "C" {
    static mach_task_self_: MachPort;
    fn task_for_pid(task: MachPort, pid: c_int, out: *mut MachPort) -> KernReturn;
    fn mach_port_deallocate(task: MachPort, port: MachPort) -> KernReturn;
    fn mach_vm_allocate(task: MachPort, addr: *mut u64, size: u64, flags: c_int) -> KernReturn;
    fn mach_vm_write(task: MachPort, addr: u64, data: *const c_void, count: u32) -> KernReturn;
    fn mach_vm_protect(task: MachPort, addr: u64, size: u64, set_max: c_int, prot: c_int) -> KernReturn;
    fn mach_vm_read_overwrite(task: MachPort, addr: u64, size: u64, data: u64, out_size: *mut u64) -> KernReturn;
    fn task_info(task: MachPort, flavor: c_int, info: *mut i32, count: *mut u32) -> KernReturn;
    fn mach_error_string(kr: KernReturn) -> *const c_char;
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const VM_FLAGS_ANYWHERE: c_int = 1;
const RTLD_LAZY: c_int = 1;
const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
const STACK_SIZE: u64 = 64 * 1024;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Payload(String),
    Mach(&'static str, KernReturn),
    Helper(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Payload(e) => write!(f, "{e}"),
            Error::Mach(step, kr) => {
                let msg = unsafe { CStr::from_ptr(mach_error_string(*kr)) }.to_string_lossy();
                write!(f, "{step}: {msg} ({kr})")
            }
            Error::Helper(e) => write!(f, "starting the thread failed: {e}"),
        }
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Error::Payload(e)
    }
}

fn check(step: &'static str, kr: KernReturn) -> Result<(), Error> {
    if kr == 0 { Ok(()) } else { Err(Error::Mach(step, kr)) }
}

/// A task port; ours is borrowed, others are released on drop.
struct Task {
    port: MachPort,
    own: bool,
}

impl Task {
    fn of(pid: i32) -> Result<Self, Error> {
        if pid == std::process::id() as i32 {
            return Ok(Task { port: unsafe { mach_task_self_ }, own: true });
        }
        let mut port = 0;
        check("task_for_pid", unsafe { task_for_pid(mach_task_self_, pid, &mut port) })?;
        Ok(Task { port, own: false })
    }

    fn allocate(&self, size: u64) -> Result<u64, Error> {
        let mut addr = 0;
        check("allocate", unsafe { mach_vm_allocate(self.port, &mut addr, size, VM_FLAGS_ANYWHERE) })?;
        Ok(addr)
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        if !self.own {
            unsafe { mach_port_deallocate(mach_task_self_, self.port) };
        }
    }
}

/// Looks symbols up in our own process. System libraries come from the shared cache, mapped at
/// the same address everywhere, so the answer holds in the target too.
fn resolve(name: &str) -> Option<u64> {
    let name = CString::new(name).ok()?;
    let mut address = unsafe { dlsym(RTLD_DEFAULT, name.as_ptr()) } as u64;
    if address == 0 {
        return None;
    }
    // Drop any pointer signature; the payload calls imports with plain branches.
    unsafe { std::arch::asm!("xpaci {0}", inout(reg) address, options(pure, nomem, nostack)) };
    Some(address)
}

#[repr(C, packed(4))]
#[derive(Default)]
struct TaskDyldInfo {
    all_image_info_addr: u64,
    all_image_info_size: u64,
    all_image_info_format: i32,
}

const TASK_DYLD_INFO: c_int = 17;
/// Offset of `libSystemInitialized` in `struct dyld_all_image_infos`.
const LIBSYSTEM_INITIALIZED: usize = 25;

/// Whether `pid` is far enough along to take the payload: libSystem (and with it pthreads and
/// the Objective-C runtime) is initialised. The payload itself waits for AppKit.
pub fn ready(pid: i32) -> Result<bool, Error> {
    let task = Task::of(pid)?;
    let mut info = TaskDyldInfo::default();
    let mut count = (size_of::<TaskDyldInfo>() / 4) as u32;
    check("task_info", unsafe { task_info(task.port, TASK_DYLD_INFO, (&raw mut info).cast(), &mut count) })?;
    let address = info.all_image_info_addr;
    if address == 0 {
        return Ok(false);
    }
    let mut header = [0u8; 32];
    let mut read = 0;
    check("read dyld info", unsafe {
        mach_vm_read_overwrite(task.port, address, header.len() as u64, header.as_mut_ptr() as u64, &mut read)
    })?;
    Ok(header[LIBSYSTEM_INITIALIZED] != 0)
}

/// Where the payload's code ended up and where its first thread should run.
pub struct Loaded {
    pub bootstrap: u64,
    pub main: u64,
    pub stack_top: u64,
}

/// Maps `payload` into `pid` with `config` patched in, without starting it.
pub fn load(pid: i32, payload_path: &Path, config: u64) -> Result<Loaded, Error> {
    let bytes = std::fs::read(payload_path).map_err(Error::Io)?;
    let payload = Payload::parse(&bytes)?;
    // Make sure every library the payload links is loaded here, so dlsym can find its symbols.
    for dylib in &payload.dylibs {
        let path = CString::new(dylib.as_str()).map_err(|_| format!("bad library path {dylib}"))?;
        if unsafe { dlopen(path.as_ptr(), RTLD_LAZY) }.is_null() {
            return Err(format!("can't load {dylib}").into());
        }
    }

    let task = Task::of(pid)?;
    let address = task.allocate(payload.size())?;
    let mut image = payload.image(address, resolve)?;
    let config_at = payload.symbol("_SHARPENERER_CONFIG")? as usize;
    image[config_at..config_at + 8].copy_from_slice(&config.to_le_bytes());

    check("write payload", unsafe { mach_vm_write(task.port, address, image.as_ptr().cast(), image.len() as u32) })?;
    for (segment, offset) in payload.layout() {
        check("protect payload", unsafe {
            mach_vm_protect(task.port, address + offset, segment.vmsize, 0, segment.initprot)
        })?;
    }
    let stack = task.allocate(STACK_SIZE)?;
    Ok(Loaded {
        bootstrap: address + payload.symbol("_sharpenerer_bootstrap")?,
        main: address + payload.symbol("_sharpenerer_main")?,
        stack_top: stack + STACK_SIZE - 0x100,
    })
}

fn thread_helper() -> Result<PathBuf, Error> {
    let exe = std::env::current_exe().map_err(Error::Io)?;
    Ok(exe.with_file_name("sharpenerer-thread"))
}

pub fn inject(pid: i32, payload_path: &Path, config: u64) -> Result<(), Error> {
    let loaded = load(pid, payload_path, config)?;
    let output = Command::new(thread_helper()?)
        .arg(pid.to_string())
        .arg(format!("{:#x}", loaded.bootstrap))
        .arg(format!("{:#x}", loaded.stack_top))
        .output()
        .map_err(Error::Io)?;
    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Helper(String::from_utf8_lossy(&output.stderr).trim().to_string()))
    }
}

/// Maps the payload into this process and runs its entry point directly: exercises parsing,
/// mapping and fixups without root. There's no AppKit here, so the payload just reports in.
pub fn self_test(payload_path: &Path, config: u64) -> Result<(), Error> {
    let loaded = load(std::process::id() as i32, payload_path, config)?;
    let main: extern "C" fn(*mut c_void) -> *mut c_void = unsafe { std::mem::transmute(loaded.main as *const ()) };
    main(std::ptr::null_mut());
    Ok(())
}
