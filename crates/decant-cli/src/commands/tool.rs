use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, schema, stats};

#[derive(Subcommand, Debug)]
pub enum ToolCmd {
    /// List tools by call count (alias of `stats`).
    Ls(ToolArgs),
    /// Tool usage stats (calls, errors), most-used first.
    Stats(ToolArgs),
}

#[derive(Args, Debug)]
pub struct ToolArgs {
    /// Only tools that have at least one error.
    #[arg(long)]
    pub errors_only: bool,
    /// Max rows.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

pub fn run(cli: &Cli, cmd: &ToolCmd) -> anyhow::Result<i32> {
    let args = match cmd {
        ToolCmd::Ls(a) | ToolCmd::Stats(a) => a,
    };
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let rows = stats::tool_usage(&conn, args.errors_only, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(["TOOL", "KIND", "SERVER", "CALLS", "ERRORS"]);
            for r in &rows {
                table.add_row([
                    r.tool_name.clone(),
                    r.tool_kind.clone(),
                    r.mcp_server.clone().unwrap_or_default(),
                    r.calls.to_string(),
                    r.errors.to_string(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
