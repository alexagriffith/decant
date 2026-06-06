use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::query::ListFilter;
use decant_core::{config::Config, db, query, schema};

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
    schema::migrate(&conn)?;
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

pub fn run_show(cli: &Cli, args: &ShowArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let detail = match query::get_session(&conn, args.id)? {
        Some(d) => d,
        None => {
            eprintln!("error: no session with id {}", args.id);
            return Ok(1);
        }
    };
    let out = cli.output();
    if matches!(out.format, crate::output::Format::Json) {
        crate::output::print_json(&detail)?;
        return Ok(0);
    }

    let s = &detail.summary;
    println!("# {}", s.title.clone().unwrap_or_else(|| s.source_session_id.clone()));
    println!(
        "{} · {} · {} msgs · ${:.2}",
        s.tool,
        s.model.clone().unwrap_or_default(),
        s.message_count,
        s.estimated_cost_usd
    );
    println!();
    for m in &detail.messages {
        println!("## {}", m.role.to_uppercase());
        for b in &m.blocks {
            match b.block_type.as_str() {
                "text" | "thinking" => {
                    if let Some(t) = &b.text {
                        if b.block_type == "thinking" {
                            println!("_(thinking)_ {t}");
                        } else {
                            println!("{t}");
                        }
                    }
                }
                "tool_use" => {
                    println!(
                        "→ tool: {} {}",
                        b.tool_name.clone().unwrap_or_default(),
                        b.tool_input.clone().unwrap_or_default()
                    );
                }
                "tool_result" => {
                    println!("← result: {}", b.tool_result.clone().unwrap_or_default());
                }
                other => println!("[{other}]"),
            }
        }
        println!();
    }
    Ok(0)
}
