use crate::Cli;
use clap::Args;
use decant_core::{config::Config, db, enrich, schema, stats};

#[derive(Args, Debug)]
pub struct FilesArgs {
    /// Group by: path (hotspot files) | ext (language lens).
    #[arg(long, default_value = "path")]
    pub group: String,
    /// Keep only one operation: read | edit | write | delete.
    #[arg(long)]
    pub op: Option<String>,
    /// Max rows.
    #[arg(long, default_value_t = 25)]
    pub limit: i64,
}

pub fn run(cli: &Cli, args: &FilesArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;

    let Some(group) = stats::FileGroup::parse(&args.group) else {
        eprintln!(
            "error: unknown --group value {:?} (expected: path | ext)",
            args.group
        );
        return Ok(2);
    };
    let op = match &args.op {
        None => None,
        Some(raw) => match enrich::Operation::parse(raw) {
            Some(o) => Some(o),
            None => {
                eprintln!(
                    "error: unknown --op value {raw:?} (expected: read | edit | write | delete)"
                );
                return Ok(2);
            }
        },
    };

    let rows = stats::file_hotspots(&conn, group, op, args.limit)?;
    let out = cli.output();
    match out.format {
        crate::output::Format::Json => crate::output::print_json(&rows)?,
        _ => {
            use comfy_table::{presets::UTF8_FULL, Table};
            let mut table = Table::new();
            table.load_preset(UTF8_FULL);
            table.set_header([
                if matches!(group, stats::FileGroup::Ext) {
                    "EXT"
                } else {
                    "FILE"
                },
                "READS",
                "EDITS",
                "WRITES",
                "DELETES",
                "SESSIONS",
                "LAST TOUCHED",
            ]);
            for r in &rows {
                table.add_row([
                    r.key.clone(),
                    r.reads.to_string(),
                    r.edits.to_string(),
                    r.writes.to_string(),
                    r.deletes.to_string(),
                    r.sessions.to_string(),
                    r.last_touched_at
                        .as_deref()
                        .map(|t| t.chars().take(10).collect())
                        .unwrap_or_default(),
                ]);
            }
            println!("{table}");
        }
    }
    Ok(0)
}
