mod inject;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Load a payload dylib into a running app (needs root).
    Inject {
        pid: i32,
        /// Absolute path to the dylib; it must be readable by the target app.
        payload: PathBuf,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Inject { pid, payload } => match inject::inject(pid, &payload) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("sharpenerer: injecting into {pid} failed: {e}");
                ExitCode::FAILURE
            }
        },
    }
}
