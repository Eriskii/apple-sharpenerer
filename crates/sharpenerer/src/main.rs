mod daemon;
mod inject;
mod install;
mod macho;
mod notify;
mod procs;
mod settings;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use geometry::{Anchor, Config, Curve};

use settings::Settings;

#[derive(Parser)]
#[command(version, about = "Per-corner, screen-aware window corners and no sheen, for macOS 26")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install the daemon that loads into every app (run with sudo from the build directory).
    Install,
    /// Remove the daemon and its files; running apps keep their changes until they quit.
    Uninstall,
    /// Show whether the daemon is running and the settings in effect.
    Status,
    /// Print the settings file.
    Show,
    /// Change a setting and apply it live, e.g. `set corners 0` or `set top-highlight on`.
    Set {
        /// enabled, corners, top-left, top-right, bottom-left, bottom-right, curve,
        /// screen-aware (on, off, real or a radius), anchor, top-highlight, inner-rim, outer-rim
        key: String,
        #[arg(required = true, num_args = 1..)]
        values: Vec<String>,
    },
    /// Put every setting back to its default.
    Reset,
    /// Apply the settings file after editing it by hand.
    Reload,
    /// Leave an app alone from its next launch on.
    Exclude { bundle_id: String },
    /// Stop leaving an app alone.
    Include { bundle_id: String },
    /// Load into one running app right now (needs root).
    Inject {
        pid: i32,
        #[arg(long)]
        payload: Option<PathBuf>,
    },
    #[command(hide = true)]
    Daemon,
    #[command(hide = true)]
    SelfTest { payload: PathBuf },
}

/// The settings file of whoever is running this (the real user under sudo).
fn settings_path() -> Result<PathBuf, String> {
    let uid = std::env::var("SUDO_USER").ok().and_then(|u| procs::uid_of(&u)).unwrap_or(unsafe { libc::getuid() });
    procs::home_of(uid).map(|home| settings::path_in(&home)).ok_or_else(|| "can't find your home directory".into())
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn describe(c: &Config) -> String {
    let b = c.shape.base;
    let curve = match c.shape.curve {
        Curve::Continuous => "continuous",
        Curve::Circular => "circular",
    };
    let aware = match c.shape.screen_aware {
        None => "off".to_string(),
        Some(aware) => {
            let from = if aware.anchor == Anchor::Usable { "usable area" } else { "display edges" };
            match aware.radius {
                None => format!("real display curve, measured from the {from}"),
                Some(r) => format!("{r} near rounded display corners, measured from the {from}"),
            }
        }
    };
    format!(
        "enabled:        {}\ncorners:        {} {} {} {} (top-left, top-right, bottom-left, bottom-right), {curve}\nscreen-aware:   {aware}\ntop highlight:  {}\ninner rim:      {}\nouter rim:      {}",
        on_off(c.enabled),
        b.top_left,
        b.top_right,
        b.bottom_left,
        b.bottom_right,
        on_off(c.top_highlight),
        on_off(c.inner_rim),
        on_off(c.outer_rim),
    )
}

/// Saves `settings` and pushes them to every app that has the payload.
fn apply(settings: &Settings) -> Result<(), String> {
    let path = settings_path()?;
    settings.save(&path)?;
    notify::Channel::open()?.publish(settings.config().pack())?;
    println!("{}", describe(&settings.config()));
    Ok(())
}

fn status() -> Result<(), String> {
    match (install::installed(), install::daemon_pid()) {
        (_, Some(pid)) => println!("daemon:         running (pid {pid})"),
        (true, None) => println!("daemon:         installed but not running; see {}", install::LOG),
        (false, None) => println!("daemon:         not installed (sudo sharpenerer install)"),
    }
    println!("settings file:  {}", settings_path()?.display());
    let live = notify::Channel::open()?.current().and_then(Config::unpack);
    match live {
        Some(config) => println!("{}", describe(&config)),
        None => println!("live config:    none published yet"),
    }
    Ok(())
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Install => {
            install::install()?;
            println!("installed; the daemon is loading into your apps now");
            println!("settings: {} (change with `sharpenerer set`)", settings_path()?.display());
            Ok(())
        }
        Command::Uninstall => {
            install::uninstall()?;
            println!("uninstalled; apps go back to normal as they're relaunched");
            Ok(())
        }
        Command::Status => status(),
        Command::Show => {
            print!("{}", Settings::load(&settings_path()?)?);
            Ok(())
        }
        Command::Set { key, values } => {
            let mut settings = Settings::load(&settings_path()?)?;
            settings.set(&key, &values)?;
            apply(&settings)
        }
        Command::Reset => apply(&Settings::defaults()),
        Command::Reload => apply(&Settings::load(&settings_path()?)?),
        Command::Exclude { bundle_id } => {
            let mut settings = Settings::load(&settings_path()?)?;
            settings.set_excluded(&bundle_id, true);
            settings.save(&settings_path()?)?;
            println!("{bundle_id} is left alone from its next launch on");
            Ok(())
        }
        Command::Include { bundle_id } => {
            let mut settings = Settings::load(&settings_path()?)?;
            settings.set_excluded(&bundle_id, false);
            settings.save(&settings_path()?)?;
            println!("{bundle_id} gets reshaped from its next launch on");
            Ok(())
        }
        Command::Inject { pid, payload } => {
            let packed = notify::Channel::open()?
                .current()
                .unwrap_or_else(|| Settings::load(&settings_path().unwrap_or_default()).map(|s| s.config()).unwrap_or_default().pack());
            inject::inject(pid, &payload.unwrap_or_else(install::payload), packed).map_err(|e| format!("loading into {pid} failed: {e}"))
        }
        Command::Daemon => daemon::run(std::path::Path::new(install::DIR)),
        Command::SelfTest { payload } => {
            inject::self_test(&payload, Config::default().pack()).map_err(|e| format!("self-test failed: {e}"))
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sharpenerer: {e}");
            ExitCode::FAILURE
        }
    }
}
