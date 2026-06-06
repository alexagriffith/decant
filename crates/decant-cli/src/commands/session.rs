use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::query::ListFilter;
use decant_core::{config::Config, db, query};

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

pub fn run_ls(cli: &Cli, args: &LsArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    let filter = ListFilter { tool: args.tool.clone(), limit: args.limit };
    let sessions = query::list_sessions(&conn, &filter)?;
    let out = cli.output();

    match out.format {
        crate::output::Format::Json => crate::output::print_json(&sessions)?,
        _ => {
            if out.quiet {
                for s in &sessions {
                    println!("{}", s.id);
                }
            } else {
                use comfy_table::{presets::UTF8_FULL, Table};
                let mut table = Table::new();
                table.load_preset(UTF8_FULL);
                table.set_header(["ID", "TOOL", "TITLE", "MODEL", "MSGS", "COST$", "STARTED"]);
                for s in &sessions {
                    table.add_row([
                        s.id.to_string(),
                        s.tool.clone(),
                        s.title.clone().unwrap_or_default(),
                        s.model.clone().unwrap_or_default(),
                        s.message_count.to_string(),
                        format!("{:.2}", s.estimated_cost_usd),
                        s.started_at.clone().unwrap_or_default(),
                    ]);
                }
                println!("{table}");
            }
        }
    }
    Ok(0)
}

pub fn run_show(_cli: &Cli, _args: &ShowArgs) -> anyhow::Result<i32> {
    anyhow::bail!("session show not implemented yet")
}
