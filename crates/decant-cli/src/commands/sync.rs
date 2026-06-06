use crate::Cli;
use clap::Args;

#[derive(Args, Debug)]
pub struct SyncArgs {}

pub fn run(_cli: &Cli, _args: &SyncArgs) -> anyhow::Result<i32> {
    anyhow::bail!("sync not implemented yet")
}
