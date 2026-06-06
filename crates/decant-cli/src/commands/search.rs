use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, query};

#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Query string (FTS5 syntax supported).
    pub query: String,
    /// Max hits.
    #[arg(long, default_value_t = 30)]
    pub limit: i64,
}

pub fn run(cli: &Cli, args: &SearchArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    let hits = query::search(&conn, &args.query, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&hits)?,
        _ => {
            if hits.is_empty() {
                eprintln!("no matches for {:?}", args.query);
            }
            for h in &hits {
                println!(
                    "[{}] {}  —  {}",
                    h.session_id,
                    h.session_title.clone().unwrap_or_default(),
                    h.snippet
                );
            }
        }
    }
    Ok(0)
}
