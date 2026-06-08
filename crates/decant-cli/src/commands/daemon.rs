use crate::Cli;
use clap::Subcommand;
use decant_daemon::config::Config;

#[derive(Subcommand, Debug)]
pub enum DaemonCmd {
    /// Run the daemon (HTTP service on loopback). Currently always foreground.
    Serve {
        /// Run in the foreground (the default; detached mode arrives with `daemon install`).
        #[arg(long)]
        foreground: bool,
        /// Override the listen port (also settable via $DECANT_DAEMON_PORT).
        #[arg(long)]
        port: Option<u16>,
    },
}

pub fn run(_cli: &Cli, cmd: &DaemonCmd) -> anyhow::Result<i32> {
    match cmd {
        DaemonCmd::Serve { foreground, port } => serve(*foreground, *port),
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
