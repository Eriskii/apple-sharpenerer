mod inject;
mod macho;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use geometry::Config;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load the payload into a running app (needs root).
    Inject {
        pid: i32,
        /// Path to libsharpenerer.dylib.
        payload: PathBuf,
        /// Packed config to start with, in hex (defaults to the built-in defaults).
        #[arg(long, value_parser = parse_hex)]
        config: Option<u64>,
    },
    /// Map the payload into this process and run it, to check the loader without root.
    SelfTest { payload: PathBuf },
}

fn parse_hex(s: &str) -> Result<u64, String> {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Inject { pid, payload, config } => {
            inject::inject(pid, &payload, config.unwrap_or_else(|| Config::default().pack()))
                .map_err(|e| format!("injecting into {pid} failed: {e}"))
        }
        Command::SelfTest { payload } => {
            inject::self_test(&payload, Config::default().pack()).map_err(|e| format!("self-test failed: {e}"))
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("sharpenerer: {e}");
            ExitCode::FAILURE
        }
    }
}
