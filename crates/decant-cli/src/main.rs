mod commands;
mod output;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// decant — extract, browse, and search your Claude Code & Codex sessions.
#[derive(Parser, Debug)]
#[command(name = "decant", version, about, long_about = None)]
pub struct Cli {
    /// Path to the decant SQLite database (overrides $DECANT_DB).
    #[arg(long, global = true)]
    pub db: Option<PathBuf>,

    /// Emit machine-readable JSON.
    #[arg(long, global = true)]
    pub json: bool,

    /// Output format.
    #[arg(long, global = true, value_enum)]
    pub format: Option<output::Format>,

    /// Suppress non-essential output (and print bare IDs where applicable).
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable ANSI color (also honors the NO_COLOR env var).
    #[arg(long = "no-color", global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Scan session directories and upsert new/changed sessions into the DB.
    Sync(commands::sync::SyncArgs),
    /// Inspect sessions (ls, show).
    #[command(subcommand)]
    Session(commands::session::SessionCmd),
    /// List sessions (alias for `session ls`).
    Ls(commands::session::LsArgs),
    /// Render a full transcript (alias for `session show`).
    Show(commands::session::ShowArgs),
    /// Full-text search across all sessions.
    Search(commands::search::SearchArgs),
    /// Usage & cost rollups (overall, or --by tool|model|project|day).
    Stats(commands::stats::StatsArgs),
    /// File hotspots: which files agents read/edit most (--group path|ext).
    Files(commands::files::FilesArgs),
    /// Tool usage (built-in vs MCP): `tool ls` / `tool stats`.
    #[command(subcommand)]
    Tool(commands::tool::ToolCmd),
    /// MCP server usage: `mcp ls` / `mcp stats`.
    #[command(subcommand)]
    Mcp(commands::mcp::McpCmd),
    /// Export a session (or --all) to Markdown or JSON.
    Export(commands::export::ExportArgs),
    /// Generate a shell completion script (bash|zsh|fish|...).
    Completion(commands::completion::CompletionArgs),
    /// Database maintenance: `db info` / `db vacuum` / `db migrate`.
    #[command(subcommand)]
    Db(commands::db::DbCmd),
    /// Projects (workspaces): `project ls`.
    #[command(subcommand)]
    Project(commands::project::ProjectCmd),
    /// Run the background daemon: `daemon serve`.
    #[command(subcommand)]
    Daemon(commands::daemon::DaemonCmd),
    /// Recommendations: `recommendations mark <key>`.
    #[command(subcommand)]
    Recommendations(commands::recommendations::RecommendationsCmd),
}

impl Cli {
    pub fn output(&self) -> output::OutputCtx {
        output::OutputCtx::new(self.json, self.format, self.no_color, self.quiet)
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match run(&cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e:#}");
            1
        }
    };
    std::process::exit(code);
}

/// Returns the process exit code (0 ok, 3 = completed with ingest issues).
fn run(cli: &Cli) -> anyhow::Result<i32> {
    match &cli.command {
        Commands::Sync(args) => commands::sync::run(cli, args),
        Commands::Session(cmd) => commands::session::run(cli, cmd),
        Commands::Ls(args) => commands::session::run_ls(cli, args),
        Commands::Show(args) => commands::session::run_show(cli, args),
        Commands::Search(args) => commands::search::run(cli, args),
        Commands::Stats(args) => commands::stats::run(cli, args),
        Commands::Files(args) => commands::files::run(cli, args),
        Commands::Tool(cmd) => commands::tool::run(cli, cmd),
        Commands::Mcp(cmd) => commands::mcp::run(cli, cmd),
        Commands::Export(args) => commands::export::run(cli, args),
        Commands::Completion(args) => commands::completion::run(cli, args),
        Commands::Db(cmd) => commands::db::run(cli, cmd),
        Commands::Project(cmd) => commands::project::run(cli, cmd),
        Commands::Daemon(cmd) => commands::daemon::run(cli, cmd),
        Commands::Recommendations(cmd) => commands::recommendations::run(cli, cmd),
    }
}

#[cfg(test)]
mod conformance {
    use super::*;
    use clap::CommandFactory;

    /// Every (sub)command must carry help text (`about`).
    fn assert_has_help(cmd: &clap::Command) {
        for sub in cmd.get_subcommands() {
            assert!(
                sub.get_about().is_some(),
                "command `{}` is missing help/about text",
                sub.get_name()
            );
            assert_has_help(sub);
        }
    }

    #[test]
    fn every_command_has_help() {
        assert_has_help(&Cli::command());
    }

    #[test]
    fn global_flags_present() {
        let cmd = Cli::command();
        let ids: Vec<String> = cmd
            .get_arguments()
            .map(|a| a.get_id().as_str().to_string())
            .collect();
        for flag in ["db", "json", "format", "quiet", "no_color"] {
            assert!(
                ids.contains(&flag.to_string()),
                "missing global flag --{flag}"
            );
        }
    }
}
