//! Lifecycle management for the decant daemon (macOS LaunchAgent + lock/PID file).
//!
//! Pure helpers (`plist_contents`, `pid_is_alive`, `parse_lock_pid`,
//! `format_status`) are unit-tested; the `launchctl` calls and process spawning
//! are side effects exercised by hand, not in tests.

#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::output::Format;
use crate::Cli;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::time::Duration;

/// The LaunchAgent label, also the plist filename stem.
pub const LAUNCH_AGENT_LABEL: &str = "com.decant.daemon";

/// Default daemon base URL for the status probe (matches the daemon default and
/// the web client). Overridable with `DECANT_DAEMON_URL`.
const DEFAULT_BASE_URL: &str = "http://127.0.0.1:4577";

/// Build the macOS LaunchAgent plist for the daemon.
///
/// `program_args` is the full `ProgramArguments` array (argv[0] is the absolute
/// path to the `decant` binary, followed by `daemon`, `serve`). `log_path` is
/// used for both `StandardOutPath` and `StandardErrorPath`.
pub fn plist_contents(program_args: &[String], log_path: &str) -> String {
    let mut args_xml = String::new();
    for arg in program_args {
        args_xml.push_str("    <string>");
        args_xml.push_str(&xml_escape(arg));
        args_xml.push_str("</string>\n");
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{log}</string>
  <key>StandardErrorPath</key>
  <string>{log}</string>
</dict>
</plist>
"#,
        label = LAUNCH_AGENT_LABEL,
        args = args_xml,
        log = xml_escape(log_path),
    )
}

/// Minimal XML entity escaping for the few characters that can appear in paths.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Is a process with this PID currently alive?
///
/// Uses `kill(pid, 0)`: success means the process exists and we may signal it;
/// `EPERM` means it exists but is owned by another user (still alive); `ESRCH`
/// means no such process. Non-positive PIDs are never "alive".
#[cfg(unix)]
pub fn pid_is_alive(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: `kill` with signal 0 performs error checking without sending a
    // signal; it has no memory effects.
    let rc = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if rc == 0 {
        return true;
    }
    // errno == EPERM => the process exists but we can't signal it.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
pub fn pid_is_alive(_pid: i32) -> bool {
    false
}

/// Parse the bare PID written into the lock file (best-effort, trims whitespace).
pub fn parse_lock_pid(contents: &str) -> Option<i32> {
    contents.trim().parse::<i32>().ok()
}

/// Read and parse the PID recorded in the lock file at `path`, if any.
pub fn read_lock_pid(path: &Path) -> Option<i32> {
    let contents = std::fs::read_to_string(path).ok()?;
    parse_lock_pid(&contents)
}

/// A snapshot of daemon status, assembled by [`status`] and rendered by
/// [`format_status`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct StatusReport {
    /// Whether the daemon appears to be running (lock PID is alive).
    pub running: bool,
    /// The PID from the lock file, if present.
    pub pid: Option<i32>,
    /// The base URL probed for health.
    pub base_url: String,
    /// Whether a macOS LaunchAgent plist is installed.
    pub installed: bool,
    /// `/api/v1/health` `data.status`, if reachable.
    pub health: Option<String>,
    /// API version reported by `/health`, if reachable.
    pub api_version: Option<u64>,
    /// `last_sync_at` from `/api/v1/metadata/sync-status`, if reachable.
    pub last_sync_at: Option<String>,
    /// Whether a sync is currently in progress, if reachable.
    pub sync_in_progress: Option<bool>,
}

/// Render a [`StatusReport`] as a human-readable block.
pub fn format_status(r: &StatusReport) -> String {
    let mut out = String::new();
    if r.running {
        match r.pid {
            Some(pid) => out.push_str(&format!("decant daemon: running (pid {pid})\n")),
            None => out.push_str("decant daemon: running\n"),
        }
    } else {
        out.push_str("decant daemon: not running\n");
    }
    out.push_str(&format!("  url:        {}\n", r.base_url));
    out.push_str(&format!(
        "  installed:  {}\n",
        if r.installed {
            "yes (LaunchAgent)"
        } else {
            "no"
        }
    ));
    if let Some(h) = &r.health {
        let ver = r
            .api_version
            .map(|v| format!(" (api v{v})"))
            .unwrap_or_default();
        out.push_str(&format!("  health:     {h}{ver}\n"));
    } else if r.running {
        out.push_str("  health:     unreachable\n");
    }
    if let Some(last) = &r.last_sync_at {
        out.push_str(&format!("  last sync:  {last}\n"));
    } else if r.running && r.health.is_some() {
        out.push_str("  last sync:  never\n");
    }
    if let Some(true) = r.sync_in_progress {
        out.push_str("  sync:       in progress\n");
    }
    if !r.running {
        out.push_str("\nStart it with `decant daemon start` (or `decant daemon install`).\n");
    }
    out
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

/// `~/Library/LaunchAgents/com.decant.daemon.plist`.
fn plist_path() -> PathBuf {
    home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// `~/Library/Logs/decant`.
fn logs_dir() -> PathBuf {
    home_dir().join("Library").join("Logs").join("decant")
}

/// `~/Library/Logs/decant/daemon.log`.
fn log_path() -> PathBuf {
    logs_dir().join("daemon.log")
}

/// `~/.decant/daemon.lock` (matches the daemon's default config).
fn lock_path() -> PathBuf {
    decant_daemon::config::Config::from_env().lock_path()
}

/// Emit the "macOS only" notice on unsupported platforms and exit non-fatally.
#[cfg(not(target_os = "macos"))]
fn not_supported(cli: &Cli, action: &str) -> anyhow::Result<i32> {
    let out = cli.output();
    if matches!(out.format, Format::Json) {
        crate::output::print_json(&serde_json::json!({
            "ok": false,
            "action": action,
            "error": "daemon lifecycle management is supported on macOS only for now",
        }))?;
    } else if !out.quiet {
        eprintln!(
            "`decant daemon {action}` is supported on macOS only for now. \
             Run `decant daemon serve` to run the daemon in the foreground."
        );
    }
    Ok(0)
}

#[cfg(target_os = "macos")]
pub fn install(cli: &Cli) -> anyhow::Result<i32> {
    let out = cli.output();
    let exe = std::env::current_exe()?;
    let log = log_path();
    let plist = plist_path();

    std::fs::create_dir_all(logs_dir())?;
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let program_args = vec![
        exe.to_string_lossy().to_string(),
        "daemon".to_string(),
        "serve".to_string(),
    ];
    let contents = plist_contents(&program_args, &log.to_string_lossy());
    std::fs::write(&plist, contents)?;

    // Load it (and enable, with -w) so launchd starts it now and at login.
    let loaded = run_launchctl(&["load", "-w", &plist.to_string_lossy()]);

    if matches!(out.format, Format::Json) {
        crate::output::print_json(&serde_json::json!({
            "ok": true,
            "plist": plist.to_string_lossy(),
            "log": log.to_string_lossy(),
            "loaded": loaded.is_ok(),
        }))?;
    } else if !out.quiet {
        println!("Installed LaunchAgent at {}", plist.display());
        println!("Logs: {}", log.display());
        match loaded {
            Ok(_) => println!("Loaded and started via launchctl."),
            Err(e) => eprintln!("warning: launchctl load failed ({e}); plist written anyway"),
        }
    }
    Ok(0)
}

#[cfg(target_os = "macos")]
pub fn uninstall(cli: &Cli) -> anyhow::Result<i32> {
    let out = cli.output();
    let plist = plist_path();

    // Unload first (ignore "not loaded"), then remove the plist.
    let _ = run_launchctl(&["unload", "-w", &plist.to_string_lossy()]);
    let removed = if plist.exists() {
        std::fs::remove_file(&plist).map(|_| true)
    } else {
        Ok(false)
    };

    if matches!(out.format, Format::Json) {
        crate::output::print_json(&serde_json::json!({
            "ok": removed.is_ok(),
            "plist": plist.to_string_lossy(),
            "removed": removed.as_ref().copied().unwrap_or(false),
        }))?;
    } else if !out.quiet {
        match removed {
            Ok(true) => println!("Removed LaunchAgent at {}", plist.display()),
            Ok(false) => println!("No LaunchAgent installed at {}", plist.display()),
            Err(ref e) => eprintln!("failed to remove {}: {e}", plist.display()),
        }
    }
    match removed {
        Ok(_) => Ok(0),
        Err(_) => Ok(1),
    }
}

#[cfg(target_os = "macos")]
pub fn start(cli: &Cli) -> anyhow::Result<i32> {
    let out = cli.output();
    let plist = plist_path();

    if plist.exists() {
        // Prefer the modern domain-target API; fall back to load -w on older macOS.
        let target = gui_target();
        let res = run_launchctl(&["kickstart", "-k", &target])
            .or_else(|_| run_launchctl(&["load", "-w", &plist.to_string_lossy()]));
        return report_simple(&out, "start", res, "Started via launchctl.");
    }

    let pid = spawn_detached_serve()?;
    if matches!(out.format, Format::Json) {
        crate::output::print_json(&serde_json::json!({
            "ok": true,
            "mode": "detached",
            "pid": pid,
        }))?;
    } else if !out.quiet {
        println!(
            "Started detached daemon (pid {pid}). Logs: {}",
            log_path().display()
        );
        println!("Tip: `decant daemon install` runs it as a LaunchAgent (starts at login).");
    }
    Ok(0)
}

#[cfg(target_os = "macos")]
pub fn stop(cli: &Cli) -> anyhow::Result<i32> {
    let out = cli.output();
    let plist = plist_path();

    if plist.exists() {
        let target = gui_target();
        let res = run_launchctl(&["bootout", &target])
            .or_else(|_| run_launchctl(&["unload", "-w", &plist.to_string_lossy()]));
        return report_simple(&out, "stop", res, "Stopped via launchctl.");
    }

    match read_lock_pid(&lock_path()) {
        Some(pid) if pid_is_alive(pid) => {
            signal_term(pid);
            let stopped = wait_for_exit(pid, std::time::Duration::from_secs(30));
            if matches!(out.format, Format::Json) {
                crate::output::print_json(
                    &serde_json::json!({ "ok": true, "signaled": pid, "stopped": stopped }),
                )?;
            } else if !out.quiet {
                if stopped {
                    println!("Stopped daemon (pid {pid}).");
                } else {
                    println!("Sent SIGTERM to daemon (pid {pid}); still shutting down.");
                }
            }
            Ok(if stopped { 0 } else { 1 })
        }
        _ => {
            if matches!(out.format, Format::Json) {
                crate::output::print_json(&serde_json::json!({
                    "ok": false,
                    "error": "daemon is not running",
                }))?;
            } else if !out.quiet {
                eprintln!("decant daemon is not running.");
            }
            Ok(0)
        }
    }
}

#[cfg(target_os = "macos")]
pub fn status(cli: &Cli) -> anyhow::Result<i32> {
    let out = cli.output();
    let report = build_status_report();

    if matches!(out.format, Format::Json) {
        crate::output::print_json(&report)?;
    } else {
        print!("{}", format_status(&report));
    }
    // Exit non-zero when not running so scripts can branch on it.
    Ok(if report.running { 0 } else { 1 })
}

#[cfg(target_os = "macos")]
pub fn logs(cli: &Cli, follow: bool) -> anyhow::Result<i32> {
    let out = cli.output();
    let log = log_path();
    if !log.exists() {
        if !out.quiet {
            eprintln!(
                "No log file yet at {} (the daemon writes here once installed/started).",
                log.display()
            );
        }
        return Ok(0);
    }

    if follow {
        // Delegate to `tail -f` for a faithful follow experience; this replaces
        // the current process semantics only when it exits (Ctrl-C).
        let status = std::process::Command::new("tail")
            .arg("-f")
            .arg(&log)
            .status()?;
        return Ok(status.code().unwrap_or(0));
    }

    let contents = std::fs::read_to_string(&log)?;
    print!("{contents}");
    Ok(0)
}

#[cfg(not(target_os = "macos"))]
pub fn install(cli: &Cli) -> anyhow::Result<i32> {
    not_supported(cli, "install")
}
#[cfg(not(target_os = "macos"))]
pub fn uninstall(cli: &Cli) -> anyhow::Result<i32> {
    not_supported(cli, "uninstall")
}
#[cfg(not(target_os = "macos"))]
pub fn start(cli: &Cli) -> anyhow::Result<i32> {
    not_supported(cli, "start")
}
#[cfg(not(target_os = "macos"))]
pub fn stop(cli: &Cli) -> anyhow::Result<i32> {
    not_supported(cli, "stop")
}
#[cfg(not(target_os = "macos"))]
pub fn status(cli: &Cli) -> anyhow::Result<i32> {
    not_supported(cli, "status")
}
#[cfg(not(target_os = "macos"))]
pub fn logs(cli: &Cli, _follow: bool) -> anyhow::Result<i32> {
    not_supported(cli, "logs")
}

#[cfg(target_os = "macos")]
fn gui_target() -> String {
    // SAFETY: getuid has no preconditions and no memory effects.
    let uid = unsafe { libc::getuid() };
    format!("gui/{uid}/{LAUNCH_AGENT_LABEL}")
}

/// Run `launchctl <args>`, mapping a non-zero exit to an error.
#[cfg(target_os = "macos")]
fn run_launchctl(args: &[&str]) -> anyhow::Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|e| anyhow::anyhow!("could not run launchctl: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!(
            "launchctl {} failed: {}",
            args.join(" "),
            stderr.trim()
        ))
    }
}

/// Print a one-line success/failure for a launchctl-backed command.
#[cfg(target_os = "macos")]
fn report_simple(
    out: &crate::output::OutputCtx,
    action: &str,
    res: anyhow::Result<()>,
    ok_msg: &str,
) -> anyhow::Result<i32> {
    match res {
        Ok(_) => {
            if matches!(out.format, Format::Json) {
                crate::output::print_json(&serde_json::json!({ "ok": true, "action": action }))?;
            } else if !out.quiet {
                println!("{ok_msg}");
            }
            Ok(0)
        }
        Err(e) => {
            if matches!(out.format, Format::Json) {
                crate::output::print_json(&serde_json::json!({
                    "ok": false,
                    "action": action,
                    "error": e.to_string(),
                }))?;
            } else if !out.quiet {
                eprintln!("failed to {action}: {e}");
            }
            Ok(1)
        }
    }
}

/// Spawn `decant daemon serve` as a detached child, redirecting stdout/stderr to
/// the daemon log file. Returns the child PID.
#[cfg(target_os = "macos")]
fn spawn_detached_serve() -> anyhow::Result<u32> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    std::fs::create_dir_all(logs_dir())?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())?;
    let log_err = log.try_clone()?;
    let exe = std::env::current_exe()?;
    let child = std::process::Command::new(exe)
        .args(["daemon", "serve"])
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        // Own process group: survives the spawner's Ctrl-C / group kill.
        .process_group(0)
        .spawn()?;
    Ok(child.id())
}

/// Poll until `pid` exits or the timeout elapses. Returns whether it exited.
#[cfg(target_os = "macos")]
fn wait_for_exit(pid: i32, timeout: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if !pid_is_alive(pid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    !pid_is_alive(pid)
}

#[cfg(all(unix, target_os = "macos"))]
fn signal_term(pid: i32) {
    // SAFETY: sending SIGTERM to a PID has no memory effects.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

/// Build the status report: lock PID + liveness, plist presence, and (if
/// reachable) the daemon's health and last-sync metadata.
#[cfg(target_os = "macos")]
fn build_status_report() -> StatusReport {
    let pid = read_lock_pid(&lock_path());
    let running = pid.map(pid_is_alive).unwrap_or(false);
    let base_url = base_url();
    let installed = plist_path().exists();

    let mut report = StatusReport {
        running,
        pid,
        base_url: base_url.clone(),
        installed,
        health: None,
        api_version: None,
        last_sync_at: None,
        sync_in_progress: None,
    };

    if running {
        if let Some(health) = http_get_json(&format!("{base_url}/api/v1/health"), None) {
            report.health = health
                .pointer("/data/status")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            report.api_version = health.pointer("/data/api_version").and_then(|v| v.as_u64());
        }
        if let Some(sync) = http_get_json(
            &format!("{base_url}/api/v1/metadata/sync-status"),
            token().as_deref(),
        ) {
            report.last_sync_at = sync
                .pointer("/data/last_sync_at")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            report.sync_in_progress = sync.pointer("/data/in_progress").and_then(|v| v.as_bool());
        }
    }
    report
}

/// Blocking GET returning parsed JSON, or `None` on any error (unreachable,
/// non-2xx, or undecodable). Sends a bearer token when provided.
#[cfg(target_os = "macos")]
fn http_get_json(url: &str, bearer: Option<&str>) -> Option<serde_json::Value> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let mut req = client.get(url).header("x-requested-api-version", "1");
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    let resp = req.send().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<serde_json::Value>().ok()
}

/// Base URL of the daemon: `DECANT_DAEMON_URL` env, else the loopback default.
#[cfg(target_os = "macos")]
fn base_url() -> String {
    match std::env::var("DECANT_DAEMON_URL") {
        Ok(v) if !v.trim().is_empty() => v.trim().trim_end_matches('/').to_string(),
        _ => DEFAULT_BASE_URL.to_string(),
    }
}

/// Bearer token: `DECANT_DAEMON_TOKEN` env, else `~/.decant/daemon.token`.
#[cfg(target_os = "macos")]
fn token() -> Option<String> {
    if let Ok(v) = std::env::var("DECANT_DAEMON_TOKEN") {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }
    let path = decant_daemon::config::Config::from_env().token_path();
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_has_label_program_args_and_paths() {
        let args = vec![
            "/usr/local/bin/decant".to_string(),
            "daemon".to_string(),
            "serve".to_string(),
        ];
        let log = "/Users/me/Library/Logs/decant/daemon.log";
        let p = plist_contents(&args, log);

        assert!(p.contains("<string>com.decant.daemon</string>"));
        assert!(p.contains("<string>/usr/local/bin/decant</string>"));
        assert!(p.contains("<string>daemon</string>"));
        assert!(p.contains("<string>serve</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n  <true/>"));
        assert!(p.contains(&format!(
            "<key>StandardOutPath</key>\n  <string>{log}</string>"
        )));
        assert!(p.contains(&format!(
            "<key>StandardErrorPath</key>\n  <string>{log}</string>"
        )));
        assert!(p.starts_with("<?xml version=\"1.0\""));
        assert!(p.trim_end().ends_with("</plist>"));
    }

    #[test]
    fn plist_escapes_xml_in_paths() {
        let args = vec!["/opt/a&b/decant".to_string()];
        let p = plist_contents(&args, "/tmp/<weird>.log");
        assert!(p.contains("<string>/opt/a&amp;b/decant</string>"));
        assert!(p.contains("<string>/tmp/&lt;weird&gt;.log</string>"));
        assert!(!p.contains("a&b"));
    }

    #[test]
    fn parse_lock_pid_reads_bare_integer() {
        assert_eq!(parse_lock_pid("12345"), Some(12345));
        assert_eq!(parse_lock_pid("  6789\n"), Some(6789));
        assert_eq!(parse_lock_pid(""), None);
        assert_eq!(parse_lock_pid("not-a-pid"), None);
    }

    #[cfg(unix)]
    #[test]
    fn current_process_is_alive() {
        let me = std::process::id() as i32;
        assert!(pid_is_alive(me), "the current process must report as alive");
    }

    #[cfg(unix)]
    #[test]
    fn nonpositive_and_reaped_pids_are_dead() {
        assert!(!pid_is_alive(0));
        assert!(!pid_is_alive(-1));

        // Spawn a trivial child, reap it, then assert its PID is no longer alive.
        let child = std::process::Command::new("true")
            .spawn()
            .expect("spawn `true`");
        let pid = child.id() as i32;
        let mut child = child;
        child.wait().expect("reap child");
        assert!(
            !pid_is_alive(pid),
            "a reaped child PID ({pid}) must report as dead"
        );
    }

    #[test]
    fn format_status_running_includes_pid_health_and_sync() {
        let r = StatusReport {
            running: true,
            pid: Some(4242),
            base_url: "http://127.0.0.1:4577".to_string(),
            installed: true,
            health: Some("ok".to_string()),
            api_version: Some(1),
            last_sync_at: Some("2026-06-09T12:00:00Z".to_string()),
            sync_in_progress: Some(false),
        };
        let s = format_status(&r);
        assert!(s.contains("running (pid 4242)"));
        assert!(s.contains("http://127.0.0.1:4577"));
        assert!(s.contains("installed:  yes (LaunchAgent)"));
        assert!(s.contains("health:     ok (api v1)"));
        assert!(s.contains("last sync:  2026-06-09T12:00:00Z"));
        assert!(!s.contains("sync:       in progress"));
        assert!(!s.contains("daemon start"));
    }

    #[test]
    fn format_status_not_running_suggests_start() {
        let r = StatusReport {
            running: false,
            pid: None,
            base_url: "http://127.0.0.1:4577".to_string(),
            installed: false,
            health: None,
            api_version: None,
            last_sync_at: None,
            sync_in_progress: None,
        };
        let s = format_status(&r);
        assert!(s.contains("not running"));
        assert!(s.contains("installed:  no"));
        assert!(s.contains("daemon start"));
        // No health line when not running and unreachable beyond the marker.
        assert!(!s.contains("api v"));
    }

    #[test]
    fn format_status_running_in_progress_shown() {
        let r = StatusReport {
            running: true,
            pid: Some(7),
            base_url: "http://127.0.0.1:4577".to_string(),
            installed: false,
            health: Some("ok".to_string()),
            api_version: Some(1),
            last_sync_at: None,
            sync_in_progress: Some(true),
        };
        let s = format_status(&r);
        assert!(s.contains("sync:       in progress"));
        // last_sync None but reachable => "never".
        assert!(s.contains("last sync:  never"));
    }
}
