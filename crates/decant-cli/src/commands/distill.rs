use crate::Cli;
use clap::{Args, Subcommand};
use decant_core::{config::Config, db, distill, schema};
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum DistillCmd {
    /// Workflow script from your real command history (frequency-ranked).
    Script(ScriptArgs),
    /// Reproduce one session's commands + file writes as a script.
    Replay(ReplayArgs),
    /// Generate a SKILL.md / AGENTS.md section / slash-command from a project.
    Skill(SkillArgs),
}

#[derive(Args, Debug)]
pub struct ScriptArgs {
    /// Limit to a project (name or path).
    #[arg(long)]
    pub project: Option<String>,
    /// Limit to a work type (debugging|feature|refactor|research|ops).
    #[arg(long)]
    pub work_type: Option<String>,
    /// Distill one session faithfully instead of the cross-session frequency recipe.
    #[arg(long)]
    pub from_session: Option<i64>,
    /// Output format: sh | just | make. (`--format` is reserved for global output.)
    #[arg(long = "as", default_value = "sh")]
    pub as_format: String,
    /// Keep commands seen in at least this fraction of sessions (0.0–1.0).
    #[arg(long)]
    pub min_frequency: Option<f32>,
    /// Write to a file instead of stdout.
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Overwrite the output file if it exists.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct ReplayArgs {
    /// Session id (see `decant ls`).
    pub session_id: i64,
    /// Keep commands that errored in the original run (commented).
    #[arg(long)]
    pub include_errors: bool,
    /// Write to a file instead of stdout.
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Overwrite the output file if it exists.
    #[arg(long)]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct SkillArgs {
    /// Limit to a project (name or path).
    #[arg(long)]
    pub project: Option<String>,
    /// Limit to a work type.
    #[arg(long)]
    pub work_type: Option<String>,
    /// Artifact kind: skill | agents | command.
    #[arg(long, default_value = "skill")]
    pub kind: String,
    /// Write to a file instead of stdout.
    #[arg(short, long)]
    pub out: Option<PathBuf>,
    /// Overwrite the output file if it exists.
    #[arg(long)]
    pub force: bool,
}

pub fn run(cli: &Cli, cmd: &DistillCmd) -> anyhow::Result<i32> {
    match cmd {
        DistillCmd::Script(a) => run_script(cli, a),
        DistillCmd::Replay(a) => run_replay(cli, a),
        DistillCmd::Skill(a) => run_skill(cli, a),
    }
}

fn is_json(cli: &Cli) -> bool {
    matches!(cli.output().format, crate::output::Format::Json)
}

/// Write the artifact to `out` (refusing to clobber without --force) or stdout.
fn emit(cli: &Cli, artifact: &str, out: &Option<PathBuf>, force: bool) -> anyhow::Result<i32> {
    match out {
        Some(path) => {
            if path.exists() && !force {
                eprintln!(
                    "error: {} exists (use --force to overwrite)",
                    path.display()
                );
                return Ok(2);
            }
            std::fs::write(path, artifact)?;
            if !cli.quiet {
                eprintln!("wrote {}", path.display());
            }
            Ok(0)
        }
        None => {
            print!("{artifact}");
            Ok(0)
        }
    }
}

fn run_script(cli: &Cli, args: &ScriptArgs) -> anyhow::Result<i32> {
    let Some(format) = distill::ScriptFormat::parse(&args.as_format) else {
        eprintln!(
            "error: unknown --as {:?} (expected: sh | just | make)",
            args.as_format
        );
        return Ok(2);
    };
    let min_frequency = args.min_frequency.unwrap_or(0.25);
    if !min_frequency.is_finite() || !(0.0..=1.0).contains(&min_frequency) {
        eprintln!("error: --min-frequency must be a number between 0.0 and 1.0");
        return Ok(2);
    }
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;

    let scope = distill::Scope {
        project: args.project.clone(),
        work_type: args.work_type.clone(),
        from_session: args.from_session,
    };
    let d = distill::timeline(&conn, &scope)?;
    if d.ops.is_empty() {
        eprintln!(
            "no commands found for {} — try a different scope",
            d.scope_label
        );
        return Ok(1);
    }
    let opts = distill::ScriptOpts {
        format,
        min_frequency,
        exemplar: args.from_session.is_some(),
    };
    let artifact = distill::render_script(&d, &opts);
    if is_json(cli) {
        let v = serde_json::json!({
            "scope": d.scope_label,
            "session_count": d.session_count,
            "date_from": d.date_from,
            "date_to": d.date_to,
            "generated_with": d.generated_with,
            "ops": d.ops,
            "artifact": artifact,
        });
        crate::output::print_json(&v)?;
        return Ok(0);
    }
    emit(cli, &artifact, &args.out, args.force)
}

fn run_replay(cli: &Cli, args: &ReplayArgs) -> anyhow::Result<i32> {
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;

    let Some(artifact) = distill::render_replay(&conn, args.session_id, args.include_errors)?
    else {
        eprintln!("error: no session with id {}", args.session_id);
        return Ok(1);
    };
    if is_json(cli) {
        // Match the artifact: errored commands are dropped unless --include-errors.
        let ops: Vec<_> = distill::replay_ops(&conn, args.session_id)?
            .into_iter()
            .filter(|o| args.include_errors || !(o.kind == distill::OpKind::Command && o.is_error))
            .collect();
        let v = serde_json::json!({
            "session_id": args.session_id,
            "ops": ops,
            "artifact": artifact,
        });
        crate::output::print_json(&v)?;
        return Ok(0);
    }
    emit(cli, &artifact, &args.out, args.force)
}

fn run_skill(cli: &Cli, args: &SkillArgs) -> anyhow::Result<i32> {
    let Some(kind) = distill::SkillKind::parse(&args.kind) else {
        eprintln!(
            "error: unknown --kind {:?} (expected: skill | agents | command)",
            args.kind
        );
        return Ok(2);
    };
    let config = Config::resolve(cli.db.clone(), None, None);
    let conn = db::open(&config.db_path)?;
    schema::migrate(&conn)?;

    let scope = distill::Scope {
        project: args.project.clone(),
        work_type: args.work_type.clone(),
        from_session: None,
    };
    let d = distill::timeline(&conn, &scope)?;
    let hot = distill::hot_context(&conn, &scope, 15)?;
    if d.ops.is_empty() && hot.is_empty() {
        eprintln!(
            "no data found for {} — try a different scope",
            d.scope_label
        );
        return Ok(1);
    }
    let project = args
        .project
        .clone()
        .unwrap_or_else(|| "your project".to_string());
    let artifact = distill::render_skill(&d, &hot, kind, &project);
    if is_json(cli) {
        let v = serde_json::json!({
            "project": project,
            "session_count": d.session_count,
            "hot_context": hot,
            "ops": d.ops,
            "artifact": artifact,
        });
        crate::output::print_json(&v)?;
        return Ok(0);
    }
    emit(cli, &artifact, &args.out, args.force)
}
