//! Installing the daemon: files under /Library/Application Support/apple-sharpenerer (root-owned,
//! since they end up inside every app), a LaunchDaemon, and `sharpenerer` on the PATH.

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{notify, procs, settings};

pub const DIR: &str = "/Library/Application Support/apple-sharpenerer";
pub const LABEL: &str = "io.github.eriskii.sharpenerer";
pub const LOG: &str = "/Library/Logs/sharpenerer.log";
const PLIST: &str = "/Library/LaunchDaemons/io.github.eriskii.sharpenerer.plist";
const LINK: &str = "/usr/local/bin/sharpenerer";
const FILES: &[(&str, u32)] = &[("sharpenerer", 0o755), ("sharpenerer-thread", 0o755), ("libsharpenerer.dylib", 0o644)];

pub fn payload() -> PathBuf {
    Path::new(DIR).join("libsharpenerer.dylib")
}

fn require_root() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 { Ok(()) } else { Err("this needs root; run it with sudo".into()) }
}

fn launchctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("/bin/launchctl").args(args).output().map_err(|e| format!("launchctl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("launchctl {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn daemon_plist() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{DIR}/sharpenerer</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{LOG}</string>
    <key>StandardErrorPath</key>
    <string>{LOG}</string>
</dict>
</plist>
"#
    )
}

/// Copies this build (the `sharpenerer` being run and its two siblings) into place and
/// (re)starts the daemon.
pub fn install() -> Result<(), String> {
    require_root()?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let from = exe.parent().ok_or("can't tell where this build lives")?;
    let to = Path::new(DIR);
    std::fs::create_dir_all(to).map_err(|e| format!("{DIR}: {e}"))?;

    // Stop the old daemon first so nothing is mid-injection with files we replace.
    let _ = launchctl(&["bootout", &format!("system/{LABEL}")]);
    if from.canonicalize().ok() != to.canonicalize().ok() {
        for &(name, mode) in FILES {
            let (src, dst) = (from.join(name), to.join(name));
            // Copy then rename, so apps that already mapped the old payload keep a valid file.
            let tmp = to.join(format!(".{name}.new"));
            std::fs::copy(&src, &tmp).map_err(|e| format!("{}: {e}", src.display()))?;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
    }
    let _ = Command::new("/usr/sbin/chown").args(["-R", "root:wheel", DIR]).status();

    std::fs::create_dir_all("/usr/local/bin").map_err(|e| format!("/usr/local/bin: {e}"))?;
    let _ = std::fs::remove_file(LINK);
    symlink(to.join("sharpenerer"), LINK).map_err(|e| format!("{LINK}: {e}"))?;

    std::fs::write(PLIST, daemon_plist()).map_err(|e| format!("{PLIST}: {e}"))?;
    launchctl(&["bootstrap", "system", PLIST])?;

    // Give the person installing a settings file to edit, owned by them.
    if let Ok(user) = std::env::var("SUDO_USER")
        && let Some(uid) = procs::uid_of(&user)
        && let Some(home) = procs::home_of(uid)
    {
        let path = settings::path_in(&home);
        if !path.exists() {
            settings::Settings::defaults().save(&path)?;
            let _ = Command::new("/usr/sbin/chown").args(["-R", &user, &home.join(".config/sharpenerer").to_string_lossy()]).status();
        }
        notify::Channel::open()?.publish(settings::Settings::load(&path)?.config().pack())?;
    }
    Ok(())
}

pub fn uninstall() -> Result<(), String> {
    require_root()?;
    let _ = launchctl(&["bootout", &format!("system/{LABEL}")]);
    for path in [PLIST, LINK] {
        let _ = std::fs::remove_file(path);
    }
    std::fs::remove_dir_all(DIR).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
        .map_err(|e| format!("{DIR}: {e}"))
}

/// The daemon's pid if launchd has it running.
pub fn daemon_pid() -> Option<u32> {
    let out = Command::new("/bin/launchctl").args(["print", &format!("system/{LABEL}")]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find_map(|l| l.trim().strip_prefix("pid = ").and_then(|p| p.trim().parse().ok()))
}

pub fn installed() -> bool {
    Path::new(PLIST).exists()
}
