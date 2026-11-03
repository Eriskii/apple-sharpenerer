//! Installing the daemon (an eris-inject service: files under /Library/Application
//! Support/apple-sharpenerer, a LaunchDaemon, and `sharpenerer` on the PATH), plus the settings
//! file of whoever installs it.

use std::path::{Path, PathBuf};
use std::process::Command;

use eris_inject::inject::{self, Payload};
use eris_inject::launchd::Service;
use eris_inject::procs;

use crate::{notify, settings};

pub const PAYLOAD: &str = "libsharpenerer.dylib";

pub const SERVICE: Service = Service {
    label: "io.github.eriskii.sharpenerer",
    dir: "/Library/Application Support/apple-sharpenerer",
    log: "/Library/Logs/sharpenerer.log",
    files: &[("sharpenerer", 0o755), (inject::THREAD_HELPER, 0o755), (PAYLOAD, 0o644)],
    arguments: &["daemon"],
    link: Some("/usr/local/bin/sharpenerer"),
};

/// The installed payload.
pub fn installed_payload() -> PathBuf {
    SERVICE.file(PAYLOAD)
}

/// Loads the payload at `path` into `pid` (this process for a self-test), starting with the packed
/// config `config`.
pub fn load_into(pid: i32, path: &Path, config: u64) -> Result<(), inject::Error> {
    let payload = Payload { path, name: "sharpenerer", patches: &[("_SHARPENERER_CONFIG", config)] };
    if pid == std::process::id() as i32 { inject::self_test(&payload) } else { inject::inject(pid, &payload) }
}

/// Copies this build into place and (re)starts the daemon, then gives the person installing a
/// settings file to edit, owned by them.
pub fn install() -> Result<(), String> {
    SERVICE.install()?;
    if let Ok(user) = std::env::var("SUDO_USER")
        && let Some(uid) = procs::uid_of(&user)
        && let Some(home) = procs::home_of(uid)
    {
        let path = settings::path_in(&home);
        if !path.exists() {
            settings::Settings::defaults().save(&path)?;
            let _ = Command::new("/usr/sbin/chown").args(["-R", &user, &home.join(".config/sharpenerer").to_string_lossy()]).status();
        }
        notify::channel()?.publish(settings::Settings::load(&path)?.config().pack())?;
    }
    Ok(())
}
