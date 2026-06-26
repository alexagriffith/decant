use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, schema, stats};

#[derive(Args, Debug)]
pub struct StatsArgs {
    /// Break down by: tool | model | project | day. Omit for the overall rollup.
    #[arg(long)]
    pub by: Option<String>,
}

pub fn run(cli: &Cli, args: &StatsArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let out = cli.output();

    if let Some(by) = &args.by {
        let dim = match stats::Dimension::parse(by) {
            Some(d) => d,
            None => {
                eprintln!(
                    "error: unknown --by value {by:?} (expected: tool | model | project | day)"
                );
                return Ok(2);
            }
        };
        let rows = stats::by_dimension(&conn, dim)?;
        match out.format {
            crate::output::Format::Json => crate::output::print_json(&rows)?,
            _ => {
                use comfy_table::{presets::UTF8_FULL, Table};
                let mut table = Table::new();
                table.load_preset(UTF8_FULL);
                table.set_header([
                    by.to_uppercase().as_str(),
                    "SESSIONS",
                    "IN_TOK",
                    "OUT_TOK",
                    "REASON_TOK",
                    "EST_REASON",
                    "COST$",
                ]);
                for r in &rows {
                    table.add_row([
                        r.key.clone(),
                        r.sessions.to_string(),
                        r.input_tokens.to_string(),
                        r.output_tokens.to_string(),
                        r.reasoning_tokens.to_string(),
                        r.est_reasoning_tokens.to_string(),
                        format!("{:.2}", r.estimated_cost_usd),
                    ]);
                }
                println!("{table}");
            }
        }
    } else {
        let t = stats::totals(&conn)?;
        match out.format {
            crate::output::Format::Json => crate::output::print_json(&t)?,
            _ => {
                println!("sessions:   {}", t.sessions);
                println!("messages:   {}", t.messages);
                println!("tool calls: {}", t.tool_calls);
                println!("input tok:  {}", t.input_tokens);
                println!("output tok: {}", t.output_tokens);
                println!(
                    "reason tok: {} (exact) / ~{} (est)",
                    t.reasoning_tokens, t.est_reasoning_tokens
                );
                println!("est. cost:  ${:.2}", t.estimated_cost_usd);
            }
        }
    }
    Ok(0)
}
