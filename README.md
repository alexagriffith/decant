# decant

[![CI](https://github.com/dosu-ai/decant/actions/workflows/ci.yml/badge.svg)](https://github.com/dosu-ai/decant/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

Extract Claude Code and Codex CLI sessions into a normalized,
full-text-searchable SQLite archive, then browse, search, analyze, and distill
that history from a fast local CLI or web UI.

decant reads the JSONL logs those tools already write
(`~/.claude/projects/*.jsonl`, `~/.codex/sessions/rollout-*.jsonl`), normalizes
the formats into one WAL + FTS5 SQLite archive, and keeps everything local. Your
transcripts never leave your machine.

## Features

- One archive for Claude Code and Codex sessions.
- Full-text search across messages and tool calls.
- Usage, cost, tool, MCP, file-hotspot, and activity analytics.
- Deterministic `distill` artifacts from real command history: scripts, replays,
  and skill/AGENTS snippets.
- Persisted recommendations with implemented-state tracking.
- Local React UI served by the same Bun process: `decant serve`.
- Watch mode with native filesystem events plus a periodic sweep.
- Scriptable JSON output, shell completions, and stable exit codes.

## Quick Start

Install from a published npm release without installing Bun:

```bash
npx @dosu/decant sync
npx @dosu/decant ls
npx @dosu/decant search "auth bug"
npx @dosu/decant serve
```

Run the published Docker image:

```bash
docker run --rm \
  -p 127.0.0.1:4577:4577 \
  -v decant-data:/var/lib/decant \
  -v "$HOME/.claude/projects:/sources/claude:ro" \
  -v "$HOME/.codex:/sources/codex:ro" \
  ghcr.io/dosu-ai/decant:latest
```

Keep the `127.0.0.1:` host prefix on the Docker port publish. Publishing as
`-p 4577:4577` exposes the archive port on every host interface.

Use source:

```bash
bun install --frozen-lockfile
bun src/cli.ts sync
bun src/cli.ts ls
bun src/cli.ts serve
```

The UI runs at `http://127.0.0.1:4577`.

## CLI

```bash
decant sync
decant ls
decant show 1
decant search "auth bug"
decant stats --by model
decant files --group ext
decant tool stats
decant mcp stats
decant distill script
decant distill replay 1
decant distill skill --kind agents
decant recommendations ls
decant export 1 > session.md
decant completion zsh
```

All read commands support `--json`. Use `--db /path/to/decant.db` or
`DECANT_DB` for an alternate archive.

## Configuration

- `DECANT_DB`: archive path, default `~/.decant/decant.db`.
- `DECANT_CLAUDE_DIR`: Claude projects directory, default
  `~/.claude/projects`.
- `DECANT_CODEX_DIR`: Codex home directory, default `~/.codex`.
- `DECANT_CONFIG_DIR`: settings directory, default `~/.config/decant`.

`decant serve` binds `127.0.0.1:4577` by default. Override with
`--host`/`--port`.

Archives older than schema v8 are rebuild-only. Delete the archive and re-run
`decant sync`; source logs remain the source of truth.

## How It Works

```
~/.claude + ~/.codex
        |
        v
 Bun + TypeScript decant process
 parse -> enrich -> ingest -> SQLite WAL + FTS5
        |
        +--> CLI reads / JSON
        +--> local React UI + JSON routes + SSE
```

There is no background daemon and no cross-process API contract. The old
Rust/Phoenix/Swift implementation is preserved in the signed `pre-typescript`
tag.

Route reference for the local UI lives in [docs/api/routes.md](docs/api/routes.md).
Distribution notes live in [docs/distribution.md](docs/distribution.md).
Release automation publishes npm packages and the GHCR image from the
`Release` workflow.

## Development

```bash
bun test
bunx tsc --noEmit
bunx biome check .
just check
```

Build distribution artifacts:

```bash
bun run scripts/build-binaries.ts --target native
bun run scripts/build-npm.ts --target native --no-build
docker build --platform linux/amd64 -t decant:local .
```

See [AGENTS.md](AGENTS.md) for the full command list, conventions, and project
invariants.

## Security and Privacy

decant is local-first and offline at runtime. It reads files already on disk and
makes no outbound runtime network calls. Do not commit real session data,
personal archives, tokens, keys, or `.env` files. To report a vulnerability, see
[SECURITY.md](SECURITY.md).

## License

Licensed under the [Apache License 2.0](LICENSE).
