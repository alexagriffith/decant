# How Decant analytics work

Decant derives analytics from the normalized session records already stored in
the local archive. It does not call a model or upload transcripts. This page
defines the units behind the CLI, API, UI, and reports so their numbers can be
interpreted consistently.

## Scope and dates

The `from` and `to` filters are inclusive UTC calendar dates. A session belongs
to a window according to the date prefix of its `started_at` timestamp. Invalid
date strings are ignored by the API, so callers that need a strict contract
should validate dates before sending them.

Archived and deleted sessions are excluded by default. Statistics endpoints
that accept `include_archived=true` can include user-archived sessions; deleted
sessions remain excluded. See [Archive and data lifecycle](data-lifecycle.md).

## Sessions, runs, and subagents

A top-level session is a run that is not marked as a subagent. A subagent is a
nested run linked to a parent session.

- `sessions` in aggregate statistics counts top-level sessions only.
- Message, tool-call, token, and estimated-cost totals include all visible
  sessions in scope, including subagents.
- `GET /api/sessions` omits subagents as list rows by default.
  `include_subagents=true` includes them; `with_subagents=true` attaches nested
  summaries to the returned rows.
- `subagent_count` and `subagent_estimated_cost_usd` on one summary describe its
  direct children. For a complete tree, request subagents as rows and join them
  by `parent_session_id`; attached nested summaries are capped at five levels.

This distinction matters when comparing throughput with cost: a single
top-level session can coordinate many separately metered runs.

## Work type and completion labels

Work type and outcome are lightweight transcript-shape heuristics, not evidence
that a change shipped or achieved its goal. Work type starts with keywords in
the first user prompt and can fall back to the mix of file and web activity.
Outcome looks at how the main transcript ended: a normal assistant completion,
an interruption, a trailing user/tool turn, or an error result.

Use these labels to organize follow-up analysis. Do not treat `completed` as a
merged change, a passing test suite, or a successful business outcome without
joining Decant data to evidence from the system where the work landed.

## Token and cost totals

Decant preserves provider-reported input, output, cache-read, cache-creation,
and reasoning usage when the source exposes it. Claude reasoning can be
estimated by subtraction when the source does not report it directly; the API
keeps reported and estimated reasoning separate.

Costs use the pricing table that existed when the session was ingested. They
are estimates of standard API token rates, not a reconstruction of a ChatGPT
subscription, Codex credits, discounts, or provider invoices. Updating
[pricing](pricing.md) does not rewrite historical rows; rebuild the archive to
re-estimate them.

## Activity buckets

Token economics assigns generation, context-window volume, tool calls,
estimated cost, and active time to four buckets:

| Bucket | What it represents |
| --- | --- |
| `context` | Reading, searching, listing, web/MCP retrieval, and read-only shell or Git commands. Unknown tools default here rather than overstating implementation. Every MCP tool lands here, which is why an MCP retrieval server's own cost is reported separately under [Retrieval attribution](#retrieval-attribution). |
| `planning` | Thinking/reasoning blocks and explicit plan-management tools. |
| `code` | Structured edits and shell commands that clearly build, test, write, or otherwise mutate work. |
| `communicating` | Visible text and other non-tool, non-thinking output. |

Shell classification is deliberately conservative. Read-only commands such as
`rg`, `cat`, and `git diff` are context; mutating or unrecognized shell commands
are code. A bucket is an analytical attribution, not a provider billing field
or a quality judgment.

Generation is allocated from per-message usage when available, then by block
size when it is not. Tool-result bytes contribute to context-window volume.
Bucket costs are proportional allocations of the session's estimated input and
output cost, so they reconcile to the total but should not be read as separate
provider charges. The retrieval slice below is priced from the same two
denominators, so it is a decomposition of these figures, not a separately
metered one.

## Orientation and implementation

Phases are orthogonal to activity buckets:

- **Orientation** is everything before the first detected file edit.
- **Implementation** begins with that edit and includes everything after it.
- A session that never edits a file is entirely orientation.

Orientation therefore includes any MCP retrieval that ran before the first
edit. [Retrieval attribution](#retrieval-attribution) reports that slice
separately without changing this definition.

Structured edit tools establish the boundary directly. Shell edits use narrow,
high-confidence patterns such as `git apply`, `sed -i`, and explicit file-write
APIs. The classifier prefers missing a weak signal over moving the boundary
forward on a false positive.

Each phase carries a `cost_share`: its fraction of the enclosing object's
`estimated_cost_usd`, on a 0-1 scale like `TokenEconomicsBucket.cost_share`.
Inside a bucket it is that phase's share of that bucket; inside `totals` it is
that phase's share of the archive or session total. An edit-free session
reports an orientation share of 1. Where there is no cost to divide -- an empty
archive, or a bucket that recorded no spend -- both shares are 0 rather than
summing to 1, so read the pair as a split of a total, not as a partition that
always adds up.

The split is reported by `decant economics`, which appends `orientation` and
`implementation` rows after the bucket rows, by the generated report's
"Orientation vs implementation" table, and by the Activity breakdown panel in
the local UI.

### Retrieval attribution

Every MCP tool is bucketed `context`, so an MCP retrieval server that runs at
the start of a session lands inside orientation. Read naively, the orientation
share then charges a retrieval tool with part of the cost that tool exists to
remove. `totals.retrieval` decomposes that number rather than redefining it:

- `by_server` gives the phase split for every MCP server seen, keyed by the raw
  slug. It is always present and does not depend on which servers a request
  named, so any allowlist -- including one Decant did not pick -- can be
  re-derived from a stored response. A slug is not a display name: `dosu` and
  `claude_ai_Dosu` are two registrations of one product and stay two keys.
- `attributed` is the phase split of the servers a request named through
  `--exclude-mcp-server` or `?exclude_mcp_server=`, echoed back in
  `excluded_servers`.
- `remainder` is `totals.phases` minus `attributed`, clamped at zero.
- The default exclusion set is **empty**. Nothing is excluded unless a caller
  names it, so `totals.phases` is exactly what it was before this block
  existed, and `attributed` is zero.

#### Which denominator each `cost_share` uses

Each of the three blocks normalizes `cost_share` against its **own** total, the
way a bucket's `phases.*.cost_share` is normalized within that bucket rather
than against the archive. That makes `remainder.orientation.cost_share`
directly comparable to `totals.phases.orientation.cost_share` -- the same
orientation share with the named servers removed from both halves, which is the
corrected form of the headline number.

It also means **`attributed.orientation.cost_share` is not the number a study
wants**. It answers "what fraction of the retrieval slice was orientation", not
"what fraction of the archive was retrieval", and on an archive where all
retrieval happens pre-edit it is exactly 1.0. Compute the archive-wide
retrieval percentage explicitly:

```
retrieval share = totals.retrieval.attributed.orientation.estimated_cost_usd
                / totals.estimated_cost_usd
```

`decant tokens --exclude-mcp-server <slug>` already does this: the percentages
in its `orientation_all_in` / `orientation_retrieval` / `orientation_remainder`
rows are all taken against `totals.estimated_cost_usd`, so they add up in the
column. The same command's `--json` output reports the within-block
`cost_share` instead. The two are different quantities under one field name, so
take the percentage from the row that names its denominator, or divide the
`estimated_cost_usd` values yourself. Worked example from a fixture archive
with `--exclude-mcp-server dosu --exclude-mcp-server github`:

| Quantity | Within-block `cost_share` | Share of archive cost |
| --- | --- | --- |
| orientation all-in | 70.28% | 70.28% |
| orientation retrieval | 100.00% | **2.12%** |
| orientation remainder | 69.63% | 68.15% |

A study should pre-register and publish **all three**: orientation all-in
(`totals.phases.orientation`), the retrieval slice
(`totals.retrieval.attributed.orientation`), and the remainder. `remainder`
alone flatters the retrieval tool and `phases` alone penalizes it.

Two limits are worth stating alongside the figures:

- A `tool_call` with no linked call block has no message sequence, and the
  phase classifier assigns those to implementation so orientation is never
  overstated. The same rule applies to the retrieval slice, so **orientation
  retrieval is a lower bound**, not a point estimate.
- Codex allocates generation in aggregate rather than per message, so Codex MCP
  *generation* is an approximation. Tool-result *window bytes* -- the larger
  part of a retrieval call -- are measured directly for both sources.

## Active time and user wait

Active time is an attribution from message timestamps, not stopwatch time. The
gap between two messages is charged to the later message, split across that
message's blocks, and capped at five minutes. Gaps closed by user-authored text
are reported separately as `waiting_on_user_ms`.

Consequences:

- long idle periods do not dominate the result;
- blockless or missing-timestamp messages cannot contribute;
- wall-clock session duration can be much larger than active time;
- time estimates are best for relative comparisons, not billing or timesheets.

## Context-window occupancy and compaction

For one model call, occupancy is:

`input_tokens + cache_read_tokens + cache_creation_tokens`

It is the prompt resident in the window for that call, not cumulative token
consumption. Peak occupancy is the largest observed call. Codex logs can carry
an explicit model window; Claude window size is inferred from the model family
when the source does not record it, and the API marks inferred values.

Compactions come from provider boundary records. Pre- and post-compaction token
counts are preserved when the source supplies enough information; missing
values remain unavailable rather than being invented.

## Data quality signals

`unparsed_line` means Decant could not normalize a source line and is treated
as a substantive ingest issue. Other diagnostics, such as an unknown record
type or imperfect tool linkage, are informational format-drift sensors. Both
are preserved, but informational notices should not be described as data loss
without inspecting the session's issue details.

For the machine-readable field definitions, see the
[OpenAPI contract](api/openapi.yaml). For practical queries, see
[Local API recipes](api/recipes.md).
