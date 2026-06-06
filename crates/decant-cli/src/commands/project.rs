use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, query, schema};

#[derive(Subcommand, Debug)]
pub enum ProjectCmd {
    /// List projects with session counts and cost.
    Ls(ProjectArgs),
}

#[derive(Args, Debug)]
pub struct ProjectArgs {}

pub fn run(cli: &Cli, cmd: &ProjectCmd) -> anyhow::Result<i32> {
    let ProjectCmd::Ls(_) = cmd; // exhaustive today; a new variant will force a compile error here
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let rows = query::list_projects(&conn)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header(["ID", "PROJECT", "SESSIONS", "COST$", "LAST"]);
            for r in &rows {
                table.add_row([
                    r.id.to_string(),
                    r.path.clone(),
                    r.sessions.to_string(),
                    format!("{:.2}", r.estimated_cost_usd),
                    r.last_seen_at.clone().unwrap_or_default(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
