use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, export, query, schema};
use std::path::PathBuf;

#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Session id to export. Omit with --all to export everything.
    pub id: Option<i64>,
    /// Export every session.
    #[arg(long)]
    pub all: bool,
    /// Output directory (required for --all). For a single session, omit to write to stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

pub fn run(cli: &Cli, args: &ExportArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    let json = matches!(cli.output().format, crate::output::Format::Json);
    let ext = if json { "json" } else { "md" };

    let render = |detail: &query::SessionDetail| -> anyhow::Result<String> {
        Ok(if json {
            serde_json::to_string_pretty(detail)?
        } else {
            export::to_markdown(detail)
        })
    };

    if args.all {
        let dir = match &args.out {
            Some(d) => d.clone(),
            None => {
                eprintln!("error: --all requires --out <dir>");
                return Ok(2);
            }
        };
        std::fs::create_dir_all(&dir)?;
        let summaries = query::list_sessions(&conn, &query::ListFilter { tool: None, limit: i64::MAX })?;
        let mut n = 0;
        for s in &summaries {
            if let Some(detail) = query::get_session(&conn, s.id)? {
                let path = dir.join(format!("{}.{}", s.id, ext));
                std::fs::write(&path, render(&detail)?)?;
                n += 1;
            }
        }
        eprintln!("exported {n} sessions to {}", dir.display());
        return Ok(0);
    }

    let id = match args.id {
        Some(id) => id,
        None => {
            eprintln!("error: provide a session id, or --all --out <dir>");
            return Ok(2);
        }
    };
    let detail = match query::get_session(&conn, id)? {
        Some(d) => d,
        None => {
            eprintln!("error: no session with id {id}");
            return Ok(1);
        }
    };
    let content = render(&detail)?;
    match &args.out {
        Some(dir) => {
            std::fs::create_dir_all(dir)?;
            let path = dir.join(format!("{id}.{ext}"));
            std::fs::write(&path, content)?;
            eprintln!("wrote {}", path.display());
        }
        None => println!("{content}"),
    }
    Ok(0)
}
