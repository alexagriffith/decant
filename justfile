# decant developer tasks. `just` lists recipes; `just check` is the full
# definition-of-done (same commands CI and pre-commit run — see AGENTS.md).

default:
    @just --list --unsorted

# ── Quality gates ────────────────────────────────────────────────────

# All gates: Rust + web (the Definition of done)
[group('gates')]
check: rust-check web-check

# Rust gates: tests, formatting, lints
[group('gates')]
rust-check:
    cargo test --workspace
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings

# Web gates: tests, formatting, warnings-as-errors compile
[group('gates')]
web-check:
    cd web && mix test
    cd web && mix format --check-formatted
    cd web && mix compile --warnings-as-errors

# ── Rust ─────────────────────────────────────────────────────────────

[group('rust')]
build:
    cargo build --workspace

[group('rust')]
build-release:
    cargo build --workspace --release

# Run Rust tests (e.g. `just test -p decant-core worktree`)
[group('rust')]
test *ARGS:
    cargo test --workspace {{ARGS}}

# Fix formatting in place
[group('rust')]
fmt:
    cargo fmt --all

[group('rust')]
clippy:
    cargo clippy --all-targets -- -D warnings

# ── Daemon (owns ~/.decant/decant.db; loopback HTTP on :4577) ────────

# Run the daemon in the foreground (dev)
[group('daemon')]
daemon:
    cargo run -p decant-cli -- daemon serve

# Running? + health + last sync
[group('daemon')]
daemon-status:
    cargo run -p decant-cli -- daemon status

# Tail the daemon log (`just daemon-logs -f` to follow)
[group('daemon')]
daemon-logs *ARGS:
    cargo run -p decant-cli -- daemon logs {{ARGS}}

# Install + load the macOS LaunchAgent (background + at login)
[group('daemon')]
daemon-install:
    cargo run -p decant-cli -- daemon install

[group('daemon')]
daemon-uninstall:
    cargo run -p decant-cli -- daemon uninstall

# Start (LaunchAgent if installed, else detached serve)
[group('daemon')]
daemon-start:
    cargo run -p decant-cli -- daemon start

[group('daemon')]
daemon-stop:
    cargo run -p decant-cli -- daemon stop

# ── Web (Phoenix; pure HTTP client of the daemon) ────────────────────

# Dev server at http://localhost:4000 (needs a running daemon)
[group('web')]
web:
    cd web && mix phx.server

[group('web')]
web-deps:
    cd web && mix deps.get

# Run web tests (e.g. `just web-test test/decant_web/live/analytics_live_test.exs`)
[group('web')]
web-test *ARGS:
    cd web && mix test {{ARGS}}

# Fix web formatting in place
[group('web')]
web-fmt:
    cd web && mix format

# ── Stack ────────────────────────────────────────────────────────────

# Bring up the stack: daemon (detached, skipped if already healthy), then
# the web dev server in the foreground (Ctrl-C leaves the daemon running).
# If the web is already up on :4000, report status and exit 0 (no duplicate).
[group('stack')]
up:
    #!/usr/bin/env sh
    if curl -sf -m 2 -o /dev/null http://localhost:4000/; then
        echo "stack already up — web on :4000"
        curl -sf -m 2 http://127.0.0.1:4577/api/v1/health >/dev/null \
            && echo "daemon :4577 healthy" \
            || echo "daemon :4577 not responding — run: just daemon-start"
        exit 0
    fi
    for i in $(seq 1 15); do
        curl -sf -m 2 http://127.0.0.1:4577/api/v1/health >/dev/null && break
        cargo run -q -p decant-cli -- daemon start >/dev/null 2>&1 || true
        sleep 2
    done
    if curl -sf -m 2 http://127.0.0.1:4577/api/v1/health >/dev/null; then
        echo "daemon :4577 healthy"
    else
        echo "daemon failed to start — see: just daemon-logs"
        exit 1
    fi
    cd web && mix phx.server

# Stop the detached daemon (the web server is foreground — Ctrl-C it)
[group('stack')]
down:
    cargo run -p decant-cli -- daemon stop

# Curl the daemon health endpoint and the web app
[group('stack')]
health:
    @curl -sf -m 3 http://127.0.0.1:4577/api/v1/health >/dev/null && echo "daemon :4577 OK" || echo "daemon :4577 NOT responding"
    @curl -sf -m 3 -o /dev/null http://localhost:4000/ && echo "web :4000 OK" || echo "web :4000 NOT responding"

# ── CLI reads (binary `decant`; respects --db / $DECANT_DB) ──────────

# List sessions (e.g. `just ls --limit 10`)
[group('cli')]
ls *ARGS:
    cargo run -p decant-cli -- ls {{ARGS}}

# Full-text search (e.g. `just search "worktree rollup"`)
[group('cli')]
search QUERY *ARGS:
    cargo run -p decant-cli -- search "{{QUERY}}" {{ARGS}}

# Usage & cost rollups (e.g. `just stats --by model`)
[group('cli')]
stats *ARGS:
    cargo run -p decant-cli -- stats {{ARGS}}

# One-shot headless ingest (daemon owns ingest normally; use with --db)
[group('cli')]
sync *ARGS:
    cargo run -p decant-cli -- sync {{ARGS}}

# ── Data / maintenance ───────────────────────────────────────────────

# Stop daemon, delete the derived DB, re-ingest (recomputes costs/rollups)
[group('data')]
[confirm("Delete ~/.decant/decant.db and re-ingest from ~/.claude + ~/.codex?")]
db-rebuild:
    cargo run -p decant-cli -- daemon stop || true
    rm -f ~/.decant/decant.db ~/.decant/decant.db-wal ~/.decant/decant.db-shm
    cargo run -p decant-cli -- daemon start
    @echo "Re-ingest started; watch with: just daemon-status"

# DB info (schema version, counts)
[group('data')]
db-info *ARGS:
    cargo run -p decant-cli -- db info {{ARGS}}

# Install the repo's git hooks
[group('data')]
hooks:
    pre-commit install

# ── macOS menu bar ───────────────────────────────────────────────────

# Build Decant.app and open it (menu bar drop icon)
[group('macos')]
menubar:
    bash macos/bundle.sh
    open macos/build/Decant.app

# Build the menu bar app bundle without launching
[group('macos')]
menubar-build:
    bash macos/bundle.sh

# DecantKit tests (swift-testing)
[group('macos')]
menubar-test:
    cd macos && swift test
