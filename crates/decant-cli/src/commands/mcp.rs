use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, schema, stats};

#[derive(Subcommand, Debug)]
pub enum McpCmd {
    /// List MCP servers by call count.
    Ls(McpArgs),
    /// MCP server stats (tools, calls, errors), most-used first.
    Stats(McpArgs),
}

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Max rows.
    #[arg(long, default_value_t = 50)]
    pub limit: i64,
}

pub fn run(cli: &Cli, cmd: &McpCmd) -> anyhow::Result<i32> {
    let args = match cmd {
        McpCmd::Ls(a) | McpCmd::Stats(a) => a,
    };
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let rows = stats::mcp_usage(&conn, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            if rows.is_empty() {
                eprintln!("no MCP tool calls recorded");
            }
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(["SERVER", "TOOLS", "CALLS", "ERRORS"]);
            for r in &rows {
                table.add_row([
                    r.mcp_server.clone(),
                    r.tools.to_string(),
                    r.calls.to_string(),
                    r.errors.to_string(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
