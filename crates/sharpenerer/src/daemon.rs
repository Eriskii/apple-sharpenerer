//! The root daemon: loads the payload into every app of the logged-in user as it starts (through
//! eris-inject's daemon loop), and keeps the live config published.

use std::path::{Path, PathBuf};

use eris_inject::daemon::Client;
use eris_inject::notify::Channel;
use eris_inject::{inject, log, procs};

use crate::{install, notify, settings};

struct Sharpenerer {
    payload: PathBuf,
    /// Held for the daemon's lifetime so the published config survives apps coming and going.
    channel: Channel,
    settings_path: PathBuf,
}

impl Sharpenerer {
    fn settings(&self) -> settings::Settings {
        settings::Settings::load(&self.settings_path).unwrap_or_else(|e| {
            log!("{e}; using defaults");
            settings::Settings::defaults()
        })
    }
}

/// Apps and agents (LSUIElement apps like About This Mac, menu bar apps' windows) that aren't part
/// of the system, nor helpers browsers and Electron keep in their Frameworks folders, nor in
/// eris-inject's [`ALWAYS_SKIPPED`](eris_inject::daemon::ALWAYS_SKIPPED). The payload only changes
/// ordinary titled windows, so loading into an agent without any is harmless.
fn has_windows(app: &procs::App) -> bool {
    matches!(app.kind, procs::Kind::Regular | procs::Kind::Agent)
        && !app.system
        && !(app.nested && app.in_frameworks)
        && !eris_inject::daemon::ALWAYS_SKIPPED.contains(&app.id.as_str())
}

impl Client for Sharpenerer {
    fn user_changed(&mut self, _uid: u32, home: &Path) {
        self.settings_path = settings::path_in(home);
        // A fresh login (or boot): the notification has no state yet.
        if self.channel.current().is_none()
            && let Err(e) = self.channel.publish(self.settings().config().pack())
        {
            log!("{e}");
        }
    }

    fn wants(&mut self, app: &procs::App, _home: &Path) -> bool {
        has_windows(app) && !self.settings().excluded().iter().any(|id| *id == app.id)
    }

    fn load(&mut self, pid: i32, _bundle_id: &str) -> Result<(), inject::Error> {
        let packed = self.channel.current().unwrap_or_else(|| self.settings().config().pack());
        install::load_into(pid, &self.payload, packed)
    }
}

pub fn run(dir: &Path) -> Result<(), String> {
    let payload = dir.join(install::PAYLOAD);
    log!("started; payload {}", payload.display());
    let channel = notify::channel()?;
    eris_inject::daemon::run(&mut Sharpenerer { payload, channel, settings_path: PathBuf::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(executable: &str) -> bool {
        procs::app(Path::new(executable)).is_some_and(|app| has_windows(&app))
    }

    #[test]
    fn apps_and_agents_but_not_system_ui() {
        assert!(wanted("/System/Applications/TextEdit.app/Contents/MacOS/TextEdit"));
        assert!(wanted("/System/Library/CoreServices/Applications/About This Mac.app/Contents/MacOS/About This Mac"));
        assert!(!wanted("/System/Library/CoreServices/ControlCenter.app/Contents/MacOS/ControlCenter"));
        assert!(!wanted("/System/Library/CoreServices/loginwindow.app/Contents/MacOS/loginwindow"));
    }
}
