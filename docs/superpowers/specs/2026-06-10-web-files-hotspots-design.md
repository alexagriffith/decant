# Web "Files" Hotspots View — Design

**Goal:** Surface the daemon's new `/analytics/files` evidence in the web app:
which files agents read/edit/write most, per project and per language, with the
framing that heavy re-reads are distillation candidates (AGENTS.md / skills) and
heavy edits are churn hotspots.

**Approach:** A dedicated `FilesLive` page at `/files` mirroring `ToolsLive`'s
shape (the simplest existing list page): shared `date_range` + `filter_chips`
controls, two URL-persisted view params (`group=path|ext`, `op=…`), and one
sortable table backed by a new `Archive.file_hotspots/2` →
`Daemon.file_hotspots/1` client pair. No new daemon work — the endpoint shipped
in PR #4.

**Tech stack:** Elixir/Phoenix LiveView in `web/` only. Hand-built Tailwind v4
tokens via the existing `panel`/`sort_header`/`badge`/`empty_state` components
(no daisyUI). Tests with Mimic + `DaemonStubs` per the established pattern.

---

## 1. Decisions locked

1. **Dedicated page, not an Analytics section.** Analytics is already dense and
   hotspots needs its own controls (group/op). `ToolsLive` is the precedent for
   a focused list page. Nav: key `:files`, label "Files", icon `hero-document-text`,
   path `/files`, inserted after Tools.
2. **`group` and `op` live in the URL** (`/files?group=ext&op=edit&from=…`),
   composing with the standard filter params — shareable links, back-button
   friendly, same convention as every other filter. Rendered as two small
   link-chip rows (segmented-control style), not selects.
3. **A computed TOTAL column anchors the table.** The API orders by total ops;
   the client maps rows and adds `total = reads+edits+writes+deletes` so
   `TableSort` has a real column to sort. Default sort `{:total, :desc}`.
4. **No row-level `phx-click`.** Per the LiveView nested-clickable pitfall, rows
   are hover-highlighted only; the single per-row action is a plain link to
   `/search?q="<key>"` (pre-filled FTS) — uses an existing page, no daemon
   changes. Project shows as a muted basename with the full path in `title`.
5. **No signal badges here.** Hot-context/churn *signals* are the Insights
   page's job (they already render there via the recommendation engine, with
   30-day windowing the client shouldn't re-derive). Files stays a clean
   evidence table; the subtitle carries the determinism framing in one line.
6. **Empty state explains backfill**: enrichment rows appear after the daemon's
   first post-upgrade sync re-ingests the archive.

## 2. Components and data flow

- `web/lib/decant/daemon.ex` — `file_hotspots(opts)` →
  `request(:get, "/analytics/files", params: opts)` (mirrors `tools_usage`).
- `web/lib/decant/archive.ex` — `file_hotspots(filters \\ %{}, opts \\ [])`:
  `to_params(filters)` + `group`/`op`/`limit: 100`; unwraps the envelope; maps
  string keys → atoms; adds `total`; returns `[]` on any daemon error (house
  style).
- `web/lib/decant_web/live/files_live.ex` — mount (bounds, `page_title:
  "Files"`, `files_sort: {:total, :desc}`); `handle_params` parses `Filters` +
  `group` (default `:path`, unknown → `:path`) + `op` (default `nil`, unknown →
  `nil`) and fetches; `handle_event("sort", …)` via `TableSort`; re-fetch on the
  same archive-updated notification AnalyticsLive uses.
- Router + `Layouts` nav entry.
- Table columns: FILE (or EXT when `group=ext`) · PROJECT (path mode only) ·
  READS · EDITS · WRITES · DELETES · SESSIONS · TOTAL · LAST TOUCHED
  (`relative_time`). Numbers via `Format.int/1`.

## 3. Testing

`files_live_test.exs` with `DaemonStubs.install()` + a `file_hotspots` stub
(fixture rows incl. a zero-edit hot reader and an ext-mode variant):

- renders nav-active page with table rows and formatted numbers
- `?group=ext` switches the key column and drops PROJECT; chips mark active state
- `?op=edit` threads the param to the client (assert via stub expectation)
- sort event toggles direction and reorders rows
- empty stub → `empty_state` copy mentions the backfill
- archive-updated notification triggers a re-fetch (same mechanism as the
  existing pages' tests, if covered there)

Browser verification (not CI): run this branch's daemon against the enriched
scratch DB on a side port, `mix phx.server` pointed at it, and click through
group/op/sort once — LiveViewTest cannot catch browser-level event quirks.

## 4. Out of scope

- Per-file drill-down page, file→sessions cross-filtering (needs a daemon-side
  `file=` session filter — separate proposal if wanted).
- Signal badges duplicated from Insights (§1.5).
- `/analytics/now` surfacing (menu-bar oriented; separate plan).
