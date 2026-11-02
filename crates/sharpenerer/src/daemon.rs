//! The root daemon: loads the payload into every app of the logged-in user as it starts, and
//! keeps the live config published.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::{inject, notify, procs, settings};

const POLL: Duration = Duration::from_millis(100);
/// An app that hasn't finished starting by then isn't going to.
const GIVE_UP: Duration = Duration::from_secs(15);
/// An app that dies this soon after we loaded into it counts as a crash.
const CRASH_WINDOW: Duration = Duration::from_secs(10);
const CRASHES_BEFORE_BLOCKING: usize = 2;

/// System UI that isn't ordinary windows, and apps holding secrets.
const ALWAYS_SKIPPED: &[&str] = &[
    "com.apple.dock",
    "com.apple.loginwindow",
    "com.apple.WindowManager",
    "com.apple.controlcenter",
    "com.apple.notificationcenterui",
    "com.apple.Spotlight",
    "com.apple.systemuiserver",
    "com.apple.SecurityAgent",
    "com.apple.Passwords",
    "com.apple.keychainaccess",
    "com.1password.1password",
    "com.agilebits.onepassword7",
    "com.bitwarden.desktop",
];

macro_rules! log {
    ($($arg:tt)*) => { eprintln!("{} {}", timestamp(), format!($($arg)*)) };
}

fn timestamp() -> String {
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let mut buf = [0u8; 32];
    let n = unsafe {
        libc::localtime_r(&now, &mut tm);
        libc::strftime(buf.as_mut_ptr().cast(), buf.len(), c"%Y-%m-%d %H:%M:%S".as_ptr(), &tm)
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

enum Seen {
    Ignored,
    Starting { since: Instant, bundle: String },
    Loaded { at: Instant, bundle: String },
}

pub fn run(dir: &Path) -> Result<(), String> {
    let payload = dir.join("libsharpenerer.dylib");
    // Held for the daemon's lifetime so the published config survives apps coming and going.
    let channel = notify::Channel::open()?;
    let mut published_for = None;
    let mut seen: HashMap<i32, Seen> = HashMap::new();
    let mut crashes: HashMap<String, Vec<Instant>> = HashMap::new();
    let mut blocked: HashSet<String> = HashSet::new();
    let me = std::process::id() as i32;
    log!("started; payload {}", payload.display());

    loop {
        std::thread::sleep(POLL);
        let Some((uid, home)) = procs::console_user() else {
            continue;
        };
        let settings_path = settings::path_in(&home);
        let load_settings = || settings::Settings::load(&settings_path).unwrap_or_else(|e| {
            log!("{e}; using defaults");
            settings::Settings::defaults()
        });
        if published_for != Some(uid) {
            // A fresh login (or boot): the notification has no state yet.
            if channel.current().is_none()
                && let Err(e) = channel.publish(load_settings().config().pack())
            {
                log!("{e}");
            }
            published_for = Some(uid);
        }

        let pids: HashSet<i32> = procs::all_pids().into_iter().collect();
        seen.retain(|pid, state| {
            if pids.contains(pid) {
                return true;
            }
            if let Seen::Loaded { at, bundle } = state
                && at.elapsed() < CRASH_WINDOW
            {
                let recent = crashes.entry(bundle.clone()).or_default();
                recent.push(Instant::now());
                recent.retain(|t| t.elapsed() < Duration::from_secs(300));
                log!("{bundle} (pid {pid}) exited {:.1}s after loading", at.elapsed().as_secs_f32());
                if recent.len() >= CRASHES_BEFORE_BLOCKING && blocked.insert(bundle.clone()) {
                    log!("{bundle} keeps dying after loading; leaving it alone until the daemon restarts");
                }
            }
            false
        });

        let mut settings = None;
        for &pid in &pids {
            if pid == me {
                continue;
            }
            if let Entry::Vacant(entry) = seen.entry(pid) {
                entry.insert(classify(pid, uid, settings.get_or_insert_with(load_settings), &blocked));
            }
            let Some(Seen::Starting { since, bundle }) = seen.get(&pid) else {
                continue;
            };
            let next = match inject::ready(pid) {
                Ok(false) if since.elapsed() < GIVE_UP => continue,
                Ok(false) => {
                    log!("{bundle} (pid {pid}) never finished starting");
                    Seen::Ignored
                }
                Ok(true) => {
                    let packed = channel.current().unwrap_or_else(|| settings.get_or_insert_with(load_settings).config().pack());
                    match inject::inject(pid, &payload, packed) {
                        Ok(()) => {
                            log!("loaded into {bundle} (pid {pid})");
                            Seen::Loaded { at: Instant::now(), bundle: bundle.clone() }
                        }
                        Err(e) => {
                            log!("loading into {bundle} (pid {pid}) failed: {e}");
                            Seen::Ignored
                        }
                    }
                }
                Err(e) => {
                    log!("checking {bundle} (pid {pid}) failed: {e}");
                    Seen::Ignored
                }
            };
            seen.insert(pid, next);
        }
    }
}

fn classify(pid: i32, uid: u32, settings: &settings::Settings, blocked: &HashSet<String>) -> Seen {
    if procs::owner(pid) != Some(uid) {
        return Seen::Ignored;
    }
    let Some(exe) = procs::executable(pid) else {
        return Seen::Ignored;
    };
    let Some(id) = procs::app_bundle(&exe).and_then(|bundle| procs::regular_app(&bundle, &exe)) else {
        return Seen::Ignored;
    };
    if ALWAYS_SKIPPED.contains(&id.as_str()) || settings.excluded().contains(&id) || blocked.contains(&id) {
        return Seen::Ignored;
    }
    Seen::Starting { since: Instant::now(), bundle: id }
}
