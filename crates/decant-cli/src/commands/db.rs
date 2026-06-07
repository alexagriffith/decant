use crate::Cli;
use clap::Subcommand;
use decant_core::{config::Config, db, schema};

#[derive(Subcommand, Debug)]
pub enum DbCmd {
    /// Show DB path, size, schema version, and row counts.
    Info,
    /// Reclaim free space (VACUUM).
    Vacuum,
    /// Apply schema migrations explicitly (sync also does this automatically).
    Migrate,
}

pub fn run(cli: &Cli, cmd: &DbCmd) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;
    match cmd {
        DbCmd::Migrate => eprintln!("schema up to date at {}", config.db_path.display()),
        DbCmd::Vacuum => {
            conn.execute_batch("VACUUM;")?;
            eprintln!("vacuumed {}", config.db_path.display());
        }
        DbCmd::Info => {
            let size = std::fs::metadata(&config.db_path)
                .map(|m| m.len())
                .unwrap_or(0);
            let version: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            let (sessions, messages, tool_calls): (i64, i64, i64) = conn.query_row(
                "SELECT (SELECT COUNT(*) FROM session),
                        (SELECT COUNT(*) FROM message),
                        (SELECT COUNT(*) FROM tool_call)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            println!("path:       {}", config.db_path.display());
            println!("size_bytes: {size}");
            println!("schema:     v{version}");
            println!("sessions:   {sessions}");
            println!("messages:   {messages}");
            println!("tool_calls: {tool_calls}");
        }
    }
    Ok(0)
}
