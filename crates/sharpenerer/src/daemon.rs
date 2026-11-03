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
        eris_inject::daemon::regular(app) && !self.settings().excluded().iter().any(|id| *id == app.id)
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
