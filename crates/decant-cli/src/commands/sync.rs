use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, ingest, schema};
use serde::Serialize;

#[derive(Args, Debug)]
pub struct SyncArgs {
    /// Override the Claude projects directory.
    #[arg(long)]
    pub claude_dir: Option<std::path::PathBuf>,
    /// Override the Codex home directory.
    #[arg(long)]
    pub codex_dir: Option<std::path::PathBuf>,
}

#[derive(Serialize)]
struct ReportJson {
    scanned: usize,
    ingested: usize,
    skipped: usize,
    issues: usize,
    failed: usize,
}

pub fn run(cli: &Cli, args: &SyncArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(
        cli.db.clone(),
        args.claude_dir.clone(),
        args.codex_dir.clone(),
    );
    if let Some(parent) = config.db_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;

    let report = ingest::sync(&mut conn, &config)?;
    let out = cli.output();

    if matches!(out.format, crate::output::Format::Json) {
        crate::output::print_json(&ReportJson {
            scanned: report.scanned,
            ingested: report.ingested,
            skipped: report.skipped,
            issues: report.issues,
            failed: report.failed,
        })?;
    } else if !out.quiet {
        // Human summary goes to stderr (this command produces no stdout data).
        eprintln!(
            "synced: {} scanned, {} ingested, {} skipped, {} issues, {} failed",
            report.scanned, report.ingested, report.skipped, report.issues, report.failed
        );
    }

    // Exit 3 if completed but with parse issues (CI can branch on this).
    Ok(if report.issues > 0 { 3 } else { 0 })
}
