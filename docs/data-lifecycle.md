# Archive and data lifecycle

Decant reads coding-agent logs into a local SQLite archive. The source logs and
the archive have different jobs: source JSONL is the durable record produced by
Claude Code or Codex; `~/.decant/decant.db` is a searchable, rebuildable index
plus Decant-owned user state.

Nothing in this lifecycle uploads transcripts or calls a hosted service. It does
copy them onto your disk. See
[What the archive stores](#what-the-archive-stores) for what lands in the
archive, how to inspect it, and how to remove it.

## Source logs and the archive

By default Decant discovers:

- Claude Code sessions under `~/.claude/projects`;
- Codex sessions under `~/.codex/sessions` and source-archived sessions under
  `~/.codex/archived_sessions`.

`decant sync` inserts new sessions and replaces changed ones transactionally.
Unchanged files are skipped by metadata and content checks. Watch mode combines
native filesystem events with a periodic sweep so missed notifications do not
leave the archive stale.

The archive stores normalized messages and blocks, canonical raw records,
tools, files, costs, context rollups, diagnostics, recommendations, and local
session state. Deleting or rebuilding the archive does not delete the source
JSONL files.

## What the archive stores

The archive is a searchable copy of your session content, not a summary of it.
Everything Decant ingests is written into it verbatim.

| What it holds | Columns |
| --- | --- |
| The full source record for every message, exactly as the tool wrote it | `message.raw` |
| Prompt, response, and reasoning text | `block.text` |
| Tool arguments, including shell commands, file paths, and patch bodies | `block.tool_input`, `tool_call.input` |
| Tool output, including the contents of files an agent read | `block.tool_result`, `tool_call.output_preview` |
| Absolute local paths for the working directory, the source log, and every file an agent touched | `session.cwd`, `session.source_path`, `file_ref.path`, `ingest_source.path` |
| Lines a parser could not read, kept verbatim so they can be diagnosed | `ingest_issue.raw_line` |

Only `block.text`, `block.tool_name`, and `block.tool_input` are full-text
indexed. `block.tool_result` and `message.raw` are stored but not searchable,
which makes them harder to find, not absent.

There is no redaction step. Decant does not detect, mask, or strip secrets,
tokens, keys, or personal data. Whatever your agents read, and whatever was
pasted into a session, is in the archive in the clear.

### Permissions

Decant creates `~/.decant/decant.db` and its `-wal` and `-shm` sidecars at mode
`0600`, and creates `~/.decant` at `0700`. It sets the directory mode only when
it creates the directory; a directory that already existed keeps whatever mode
its owner gave it. Check what yours actually are:

```sh
ls -ld ~/.decant
ls -l ~/.decant/decant.db
```

These are filesystem permissions, not encryption. The archive is a plain SQLite
file, so anything that can read it can read every transcript in it, including a
backup, a synced folder, or another process running as you.

### Inspecting and removing it

`decant db info` reports where the archive is, how large it is, and how much it
holds. It prints no transcript content.

```sh
decant db info
decant db info --full   # adds fts_rows and text_bytes: a full scan, slow on a large archive
```

Delete one session tree from the CLI, or use **Delete session** in the web UI:

```sh
decant ls                # find the id
decant session rm 42     # deletes that session and its descendants
decant db vacuum
```

Both paths are a hard delete: the rows are removed and their full-text index
entries with them. The bytes are not. SQLite returns freed pages to its own free
list without zeroing them, so deleted transcript text remains readable inside the
archive file, recoverable with `grep`, until `decant db vacuum` rewrites it.
`decant db info` reports `freelist_bytes`, and a non-zero value means a vacuum is
owed.

To remove everything Decant holds:

```sh
rm -rf ~/.decant
```

Under Docker the archive lives in the **named** volume mounted at
`/var/lib/decant`, and a named volume outlives its containers. Neither `--rm`
nor `docker rm` removes it; only removing the volume does:

```sh
docker volume rm decant-data
```

The source mounts in the documented `docker run` are read-only (`:ro`), so
Decant cannot modify your Claude Code or Codex logs from inside the container.

Removing the archive never removes the source logs. Those stay where Claude Code
and Codex wrote them.

## Automatic and explicit sync

Read commands normally sync first. `decant serve` performs a startup sync and
then watches the configured source directories.

Set `DECANT_NO_SYNC` or pass `--no-sync` to suppress syncs Decant starts on its
own. With `serve`, this also disables the source watcher. It does not disable an
operator-requested `POST /api/sync` or the UI's **Sync now** action.

Always use `--no-sync` with a scratch database unless you want it populated
from the real default source directories:

```sh
decant --db /tmp/decant-review.db --no-sync serve --no-open
```

## Visible, archived, and deleted

Session state is keyed by stable provider identity rather than the transient
SQLite row id.

| Action | Archive rows | Default reads and statistics | Later sync | Source JSONL |
| --- | --- | --- | --- | --- |
| Archive | Retained | Hidden, including effective descendants | Remains archived | Unchanged |
| Restore visibility | Retained | Visible unless an ancestor or source state still hides it | Remains visible | Unchanged |
| Delete | Selected session tree is physically removed; freed bytes persist until `db vacuum` | Excluded | Tombstones prevent resurrection | Unchanged |

Archiving records a direct override only on the selected session. Descendants
inherit effective visibility from their current ancestry. This lets a child
that was archived independently stay archived after its parent is restored.

Deleting applies to the existing descendant tree and keeps identity tombstones.
Provider lineage metadata lets a late-arriving descendant inherit the deletion
instead of reappearing after a later sync. Deleted sessions are not returned by
`include_archived=true` and cannot be restored through the normal session-state
API after their rows are removed.

A provider can also mark a session archived in its own source layout. Clearing
a Decant user override does not move or rewrite provider files.

## Sensitive sessions

Deleting a session tree removes its transcript-derived rows from Decant and
prevents the configured source from re-ingesting that identity. The original
Claude Code or Codex log remains on disk. If the source record must also be
removed, manage it through the owning tool or delete that source file only
after deciding that losing the original transcript is intended.

Removing the rows does not remove their bytes. Until `decant db vacuum` runs,
the deleted transcript is still readable in the archive file. For a session that
was sensitive enough to delete, the sequence is both commands:

```sh
decant session rm <id>
decant db vacuum
```

See [Inspecting and removing it](#inspecting-and-removing-it) for the rest of
the removal paths.

Never commit a real archive, transcript, exported session, source path dump, or
fixture derived from private content. Synthetic fixtures are the only session
data allowed in this repository.

## Rebuilds and migrations

Source logs are sufficient to rebuild transcript-derived state. Supported
archives migrate to the current baseline on open; older archives are
rebuild-only. The next sync backfills persisted economics, parser enrichments,
and context rollups when required.

Costs are materialized at ingest. A rebuild uses the pricing table in the new
Decant version and can therefore change historical estimates even when the
source logs did not change.

Local state that does not come from provider logs belongs to the archive. That
includes recommendation status, session archive overrides, and deletion
tombstones.
Preserve the database before a rebuild if that state matters, and do not assume
it can be reconstructed from transcripts.

## API visibility

Default lists, full-text search, command-palette search, statistics, tools,
files, and economics exclude user-hidden sessions. Statistics operations that
accept `include_archived=true` add archived sessions back; deleted sessions and
their content remain absent.

See the Response and archive semantics section of docs/api/routes.md for the
wire behavior and [How Decant analytics work](analytics-methodology.md) for the
effect on analytical denominators.
