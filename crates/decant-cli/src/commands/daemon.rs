//! `decant daemon …` — run and manage the background service.
//!
//! `serve` runs the daemon in-process (foreground). The lifecycle subcommands
//! (`install`/`uninstall`/`start`/`stop`/`status`/`logs`) manage it as a macOS
//! LaunchAgent and via the lock/PID file. They are macOS-only for now; on other
//! platforms each prints a clear message and exits non-fatally (code 0).
//!
//! The pure, testable pieces — plist generation, PID-alive detection, lock-file
//! parsing, and status formatting — live in the [`lifecycle`] submodule. The
//! `launchctl` side effects are intentionally not unit-tested.

use crate::Cli;
use clap::Subcommand;
use decant_daemon::config::Config;

pub mod lifecycle;

#[derive(Subcommand, Debug)]
pub enum DaemonCmd {
    /// Run the daemon (HTTP service on loopback). Currently always foreground.
    Serve {
        /// Run in the foreground (the default; `install` runs it detached).
        #[arg(long)]
        foreground: bool,
        /// Override the listen port (also settable via $DECANT_DAEMON_PORT).
        #[arg(long)]
        port: Option<u16>,
    },
    /// Install a macOS LaunchAgent so the daemon runs in the background and at login.
    Install,
    /// Remove the macOS LaunchAgent (reverses `install`).
    Uninstall,
    /// Start the daemon (LaunchAgent if installed, else a detached `serve`).
    Start,
    /// Stop the running daemon (LaunchAgent if installed, else signal the lock PID).
    Stop,
    /// Report whether the daemon is running, plus health and last-sync info.
    Status,
    /// Print the daemon log (`-f` to follow, like `tail -f`).
    Logs {
        /// Follow the log file and stream new lines as they are written.
        #[arg(short, long)]
        follow: bool,
    },
}

pub fn run(cli: &Cli, cmd: &DaemonCmd) -> anyhow::Result<i32> {
    match cmd {
        DaemonCmd::Serve { foreground, port } => serve(*foreground, *port),
        DaemonCmd::Install => lifecycle::install(cli),
        DaemonCmd::Uninstall => lifecycle::uninstall(cli),
        DaemonCmd::Start => lifecycle::start(cli),
        DaemonCmd::Stop => lifecycle::stop(cli),
        DaemonCmd::Status => lifecycle::status(cli),
        DaemonCmd::Logs { follow } => lifecycle::logs(cli, *follow),
    }
}

fn serve(_foreground: bool, port: Option<u16>) -> anyhow::Result<i32> {
    if let Some(p) = port {
        std::env::set_var("DECANT_DAEMON_PORT", p.to_string());
    }
    let cfg = Config::from_env();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(decant_daemon::run(cfg))?;
    Ok(0)
}
