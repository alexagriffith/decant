# Security Policy

## Supported versions

decant is pre-1.0 and under active development. Security fixes are applied to the
latest `main`. Until a stable release line exists, please verify issues against
the current `main` before reporting.

## Reporting a vulnerability

**Please do not open a public issue for security vulnerabilities.**

Report privately through GitHub's [private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability):
go to the repository's **Security** tab and click **Report a vulnerability**.
This opens a private draft advisory visible only to the maintainers.

Please include:

- a description of the issue and its impact,
- steps to reproduce (or a proof of concept),
- affected version/commit and your environment (OS, toolchain),
- any suggested remediation if you have one.

We aim to acknowledge reports within a few days and will keep you updated as we
investigate and prepare a fix. Please give us a reasonable window to address the
issue before any public disclosure; we're happy to credit you in the advisory.

## Scope and threat model

decant is a **local-first, offline** tool. It reads CLI session logs that already
exist on your disk (`~/.claude`, `~/.codex`), writes a local SQLite archive, and
makes no outbound network requests. The Phoenix web UI is intended for **local
use** (binds to localhost by default) and reads the archive **read-only**.

Things we particularly care about, and that are in scope for reports:

- Parsing untrusted/malformed session files in a way that could cause unsafe
  behavior (decant treats inputs as untrusted and records parse errors rather
  than failing hard).
- The web app exposing the archive beyond the local machine, or any write path
  to the read-only database.
- The "Sync now" action (which shells out to the `decant` binary) being
  exploitable for command/argument injection.
- Accidental inclusion of secrets or private transcript data in the repository.

Out of scope: deploying the web app on an untrusted/public network without your
own authentication and hardening (it ships for local use), and vulnerabilities in
upstream dependencies (please report those upstream, though we welcome a heads-up).

## Handling your data

Your session transcripts and the archive contain potentially sensitive content
and stay on your machine. Never commit a real archive or session data to the
repository — the only database in version control is the small synthetic test
fixture. The pre-commit hooks include a private-key check, but treat it as a
backstop, not a guarantee.
