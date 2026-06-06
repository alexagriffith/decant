use crate::Cli;
use clap::{Args, Subcommand};

#[derive(Subcommand, Debug)]
pub enum SessionCmd {
    /// List sessions.
    Ls(LsArgs),
    /// Render a full transcript.
    Show(ShowArgs),
}

#[derive(Args, Debug)]
pub struct LsArgs {
    /// Only this tool: claude_code | codex.
    #[arg(long)]
    pub tool: Option<String>,
    /// Max rows.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Session id (integer from `session ls`).
    pub id: i64,
}

pub fn run(cli: &Cli, cmd: &SessionCmd) -> anyhow::Result<i32> {
    match cmd {
        SessionCmd::Ls(a) => run_ls(cli, a),
        SessionCmd::Show(a) => run_show(cli, a),
    }
}

pub fn run_ls(_cli: &Cli, _args: &LsArgs) -> anyhow::Result<i32> {
    anyhow::bail!("session ls not implemented yet")
}

pub fn run_show(_cli: &Cli, _args: &ShowArgs) -> anyhow::Result<i32> {
    anyhow::bail!("session show not implemented yet")
}
