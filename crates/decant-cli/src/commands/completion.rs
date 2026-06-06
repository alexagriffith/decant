use crate::Cli;
use clap::{Args, CommandFactory};
use clap_complete::Shell;

#[derive(Args, Debug)]
pub struct CompletionArgs {
    /// Shell to generate a completion script for.
    #[arg(value_enum)]
    pub shell: Shell,
}

pub fn run(_cli: &Cli, args: &CompletionArgs) -> anyhow::Result<i32> {
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(0)
}
