//! `decant recommendations mark <key>` — close the agent loop.
//!
//! After an agent finishes the work a recommendation seeded (codifying a Skill,
//! writing AGENTS.md, …), it runs this command so decant records the
//! recommendation as implemented. The command POSTs to the daemon's
//! `recommendations/mark-implemented` endpoint over loopback, authenticated with
//! the same bearer token the web client uses (`DECANT_DAEMON_TOKEN` env, else the
//! file at `~/.decant/daemon.token`). The base URL defaults to
//! `http://127.0.0.1:4577` and is overridable with `DECANT_DAEMON_URL`.

use crate::output::Format;
use crate::Cli;
use anyhow::Context;
use clap::Subcommand;
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:4577";

#[derive(Subcommand, Debug)]
pub enum RecommendationsCmd {
    /// Mark a recommendation implemented (records the agent's completed work).
    Mark(MarkArgs),
}

#[derive(clap::Args, Debug)]
pub struct MarkArgs {
    /// The recommendation key, e.g. `catalog:agents-md` or `signal:error:Bash`.
    pub key: String,

    /// Who marked it implemented (recorded as `status_source`).
    #[arg(long, default_value = "agent")]
    pub source: String,

    /// Optional free-text note stored with the record.
    #[arg(long)]
    pub note: Option<String>,
}

pub fn run(cli: &Cli, cmd: &RecommendationsCmd) -> anyhow::Result<i32> {
    match cmd {
        RecommendationsCmd::Mark(args) => mark(cli, args),
    }
}

fn mark(cli: &Cli, args: &MarkArgs) -> anyhow::Result<i32> {
    let out = cli.output();
    let base = base_url();
    let url = format!("{base}/api/v1/recommendations/mark-implemented");

    let mut body = serde_json::json!({ "key": args.key, "source": args.source });
    if let Some(note) = &args.note {
        body["note"] = serde_json::Value::String(note.clone());
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .context("failed to build HTTP client")?;

    let mut req = client
        .post(&url)
        .header("x-requested-api-version", "1")
        // The mark endpoint sits behind the daemon's Origin check; send a
        // loopback Origin so the guard allows this state-mutating request.
        .header("origin", base.clone())
        .json(&body);

    if let Some(token) = token() {
        req = req.bearer_auth(token);
    }

    let resp = match req.send() {
        Ok(resp) => resp,
        Err(err) => {
            if matches!(out.format, Format::Json) {
                crate::output::print_json(&serde_json::json!({
                    "ok": false,
                    "key": args.key,
                    "error": format!("could not reach the daemon at {base}: {err}"),
                }))?;
            } else if !out.quiet {
                eprintln!("could not reach the decant daemon at {base}: {err}");
            }
            return Ok(1);
        }
    };

    let status = resp.status();
    if status.is_success() {
        if matches!(out.format, Format::Json) {
            crate::output::print_json(&serde_json::json!({
                "ok": true,
                "key": args.key,
                "status": "implemented",
            }))?;
        } else if !out.quiet {
            println!("Marked {} as implemented.", args.key);
        }
        Ok(0)
    } else {
        let detail = error_detail(resp);
        if matches!(out.format, Format::Json) {
            crate::output::print_json(&serde_json::json!({
                "ok": false,
                "key": args.key,
                "http_status": status.as_u16(),
                "error": detail.trim_start_matches(": "),
            }))?;
        } else if !out.quiet {
            eprintln!("failed to mark {} ({status}){detail}", args.key);
        }
        Ok(1)
    }
}

/// Base URL of the daemon: `DECANT_DAEMON_URL` env, else the loopback default.
fn base_url() -> String {
    match std::env::var("DECANT_DAEMON_URL") {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => DEFAULT_BASE_URL.to_string(),
    }
}

/// Bearer token: `DECANT_DAEMON_TOKEN` env, else the file at
/// `~/.decant/daemon.token`. `None` when neither is available (the daemon then
/// answers 401, which we surface as a failure line).
fn token() -> Option<String> {
    if let Ok(v) = std::env::var("DECANT_DAEMON_TOKEN") {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }

    let home = std::env::var("HOME").ok()?;
    let path = std::path::Path::new(&home)
        .join(".decant")
        .join("daemon.token");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Pull a human-readable hint out of the daemon's error envelope, if present.
fn error_detail(resp: reqwest::blocking::Response) -> String {
    match resp.json::<serde_json::Value>() {
        Ok(json) => json
            .pointer("/errors/0/message")
            .and_then(|v| v.as_str())
            .map(|m| format!(": {m}"))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}
