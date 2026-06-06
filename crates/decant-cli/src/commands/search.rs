use crate::Cli;
use clap::Args;

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Query string (FTS5 syntax supported).
    pub query: String,
    /// Max hits.
    #[arg(long, default_value_t = 30)]
    pub limit: i64,
}

pub fn run(_cli: &Cli, _args: &SearchArgs) -> anyhow::Result<i32> {
    anyhow::bail!("search not implemented yet")
}
