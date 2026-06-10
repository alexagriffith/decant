# Web Files Hotspots View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `/files` LiveView surfacing `/analytics/files` hotspots with group (path|ext) and op filters, sortable columns, and the standard date/filter controls.

**Architecture:** Mirror `ToolsLive` exactly: `Daemon.file_hotspots/1` (thin HTTP wrapper) → `Archive.file_hotspots/2` (envelope unwrap + atom keys + computed `total`) → `FilesLive` (fetch in `handle_params` so the global `SyncHook` push-patch refreshes it on `archive_updated`; `TableSort` for column sorting; `group`/`op` as URL params composing with `Filters`).

**Tech Stack:** Phoenix 1.8 LiveView, Mimic + `DaemonStubs` tests, hand-built Tailwind v4 tokens via existing `panel`/`sort_header`/`empty_state`/`date_range`/`filter_chips` components.

**Spec:** `docs/superpowers/specs/2026-06-10-web-files-hotspots-design.md`

---

## File Structure

**Create:** `web/lib/decant_web/live/files_live.ex`, `web/test/decant_web/live/files_live_test.exs`
**Modify:** `web/lib/decant/daemon.ex` (after `mcp_usage`), `web/lib/decant/archive.ex` (after `by_dimension`), `web/lib/decant_web/router.ex` (after `/tools`), `web/lib/decant_web/components/layouts.ex` (`@nav` after Tools), `web/test/support/daemon_stubs.ex` (stub + fixtures).

## Task 1: client pair + stub

- [ ] **Step 1: failing test** (`files_live_test.exs` started with just the Archive-level assertions):

```elixir
defmodule DecantWeb.FilesLiveTest do
  use DecantWeb.ConnCase, async: false
  use Mimic
  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  describe "Archive.file_hotspots/2" do
    test "maps rows, adds total, defaults on missing counts" do
      rows = Decant.Archive.file_hotspots(%{}, group: :path)
      assert [%{key: "src/main.rs", project: "/Users/dev/proj", total: 9} | _] = rows
      assert Enum.all?(rows, &is_integer(&1.total))
    end

    test "threads group/op params through to the daemon" do
      Mimic.expect(Decant.Daemon, :file_hotspots, fn opts ->
        assert opts[:group] == "ext"
        assert opts[:op] == "edit"
        {:ok, [], %{}}
      end)

      assert Decant.Archive.file_hotspots(%{}, group: :ext, op: :edit) == []
    end

    test "returns [] on daemon error" do
      Mimic.stub(Decant.Daemon, :file_hotspots, fn _ -> {:error, :down} end)
      assert Decant.Archive.file_hotspots(%{}) == []
    end
  end
end
```

- [ ] **Step 2:** `cd web && mix test test/decant_web/live/files_live_test.exs` → FAIL (`file_hotspots` undefined).
- [ ] **Step 3: implement.** `daemon.ex` after `mcp_usage`:

```elixir
@doc "Per-file hotspot rows (or per-extension with group: \"ext\")."
def file_hotspots(opts \\ []) do
  request(:get, "/analytics/files", params: opts)
end
```

`archive.ex` after `by_dimension`:

```elixir
@doc """
File hotspots within `filters`. Options: `group: :path | :ext` (default :path),
`op: :read | :edit | :write | :delete | nil`, `limit:` (default 100). Adds a
computed `total` so tables have a rank column.
"""
def file_hotspots(filters \\ %{}, opts \\ []) do
  params =
    to_params(filters) ++
      ([group: opts[:group], op: opts[:op], limit: Keyword.get(opts, :limit, 100)]
       |> Enum.reject(fn {_k, v} -> is_nil(v) end)
       |> Enum.map(fn {k, v} -> {k, to_string(v)} end))

  case Daemon.file_hotspots(params) do
    {:ok, rows, _meta} when is_list(rows) ->
      Enum.map(rows, fn r ->
        reads = r["reads"] || 0
        edits = r["edits"] || 0
        writes = r["writes"] || 0
        deletes = r["deletes"] || 0

        %{
          key: r["key"] || "",
          project: r["project"],
          reads: reads,
          edits: edits,
          writes: writes,
          deletes: deletes,
          total: reads + edits + writes + deletes,
          sessions: r["sessions"] || 0,
          last_touched_at: r["last_touched_at"]
        }
      end)

    _ ->
      []
  end
end
```

`daemon_stubs.ex`: add to `install/0` `stub(Decant.Daemon, :file_hotspots, &file_hotspots/1)`; add fixtures + stub fn (group=ext variant keyed off `opts[:group]`):

```elixir
@file_rows [
  %{"key" => "src/main.rs", "project" => "/Users/dev/proj", "reads" => 5, "edits" => 3,
    "writes" => 1, "deletes" => 0, "sessions" => 4, "last_touched_at" => "2026-05-02T14:00:00Z"},
  %{"key" => "AGENTS.md", "project" => "/Users/dev/proj", "reads" => 8, "edits" => 0,
    "writes" => 0, "deletes" => 0, "sessions" => 8, "last_touched_at" => "2026-05-01T09:00:00Z"}
]
@ext_rows [
  %{"key" => "rs", "project" => nil, "reads" => 5, "edits" => 3, "writes" => 1,
    "deletes" => 0, "sessions" => 4, "last_touched_at" => "2026-05-02T14:00:00Z"}
]

defp file_hotspots(opts) do
  rows = if to_string(opts[:group] || "path") == "ext", do: @ext_rows, else: @file_rows
  rows = if op = opts[:op], do: Enum.filter(rows, &(&1[to_string(op) <> "s"] > 0)), else: rows
  {:ok, rows, %{}}
end
```

- [ ] **Step 4:** run → PASS. **Step 5:** commit `feat(web): Archive.file_hotspots client for /analytics/files`.

## Task 2: FilesLive + route + nav

- [ ] **Step 1: failing LiveView tests** (append to `files_live_test.exs`):

```elixir
describe "FilesLive" do
  test "renders hotspot table with totals and project basenames", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/files")
    assert html =~ "File hotspots"
    assert html =~ "src/main.rs"
    assert html =~ "AGENTS.md"
    # total = 5+3+1+0 for main.rs
    assert html =~ ">9<"
    # project renders as basename
    assert html =~ "proj"
  end

  test "group=ext switches the key column and drops project", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/files?group=ext")
    assert html =~ ">rs<"
    refute html =~ "src/main.rs"
    refute html =~ ">Project<"
  end

  test "op chip is active and threads to the client", %{conn: conn} do
    Mimic.expect(Decant.Daemon, :file_hotspots, fn opts ->
      assert opts[:op] == "edit"
      {:ok, [], %{}}
    end)

    {:ok, _view, html} = live(conn, ~p"/files?op=edit")
    assert html =~ "No file activity"
  end

  test "sort event reorders rows", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/files")
    html = render_click(view, "sort", %{"table" => "files", "col" => "reads"})
    # reads desc puts AGENTS.md (8 reads) first
    assert html =~ ~r/AGENTS\.md.*src\/main\.rs/s
  end

  test "unknown group/op fall back to defaults", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/files?group=bogus&op=bogus")
    assert html =~ "src/main.rs"
  end
end
```

- [ ] **Step 2:** run → FAIL (no route). **Step 3: implement.**
  Router: `live "/files", FilesLive, :index` after the `/tools` line.
  Layouts `@nav`: `%{key: :files, label: "Files", path: "/files", icon: "hero-document-text"}` after Tools.
  `files_live.ex`:

```elixir
defmodule DecantWeb.FilesLive do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters
  alias DecantWeb.TableSort

  @groups ~w(path ext)
  @ops ~w(read edit write delete)

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Files",
       files_sort: {:total, :desc}
     )}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)
    group = if params["group"] in @groups, do: String.to_existing_atom(params["group"]), else: :path
    op = if params["op"] in @ops, do: String.to_existing_atom(params["op"]), else: nil

    files =
      Archive.file_hotspots(filters, group: group, op: op)
      |> TableSort.sort(socket.assigns.files_sort)

    {:noreply, assign(socket, filters: filters, group: group, op: op, files: files)}
  end

  @impl true
  def handle_event("sort", %{"table" => "files", "col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.files_sort, col)
    {:noreply, assign(socket, files_sort: sort, files: TableSort.sort(socket.assigns.files, sort))}
  end

  # group/op chips are patch links: /files?group=…&op=… built with the current
  # filter query so the standard filters survive toggling.
  defp view_path(filters, group, op) do
    base = Filters.url(~p"/files", filters)
    extra =
      [group: group != :path && group, op: op]
      |> Enum.filter(fn {_k, v} -> v end)
      |> Enum.map_join("&", fn {k, v} -> "#{k}=#{v}" end)

    cond do
      extra == "" -> base
      String.contains?(base, "?") -> base <> "&" <> extra
      true -> base <> "?" <> extra
    end
  end

  defp basename(nil), do: nil
  defp basename(path), do: path |> String.trim_trailing("/") |> Path.basename()

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:files}
      page_title="Files"
      syncing={@syncing}
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
      <div class="space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold tracking-tight">File hotspots</h1>
            <p class="text-sm text-muted">
              What agents touch most. Heavy re-reads with few edits are AGENTS.md / skill candidates; heavy edits are churn.
            </p>
          </div>
          <.date_range filters={@filters} bounds={@bounds} path={~p"/files"} />
        </div>

        <.filter_chips filters={@filters} path={~p"/files"} />

        <div class="flex flex-wrap items-center gap-4 text-sm">
          <div class="inline-flex items-center gap-1">
            <.link patch={view_path(@filters, :path, @op)} class={seg_class(@group == :path)}>Files</.link>
            <.link patch={view_path(@filters, :ext, @op)} class={seg_class(@group == :ext)}>Languages</.link>
          </div>
          <div class="inline-flex items-center gap-1">
            <.link patch={view_path(@filters, @group, nil)} class={seg_class(is_nil(@op))}>All ops</.link>
            <.link :for={o <- [:read, :edit, :write, :delete]} patch={view_path(@filters, @group, o)} class={seg_class(@op == o)}>
              {Phoenix.Naming.humanize(o)}
            </.link>
          </div>
        </div>

        <.panel title={if @group == :ext, do: "Languages", else: "Hotspots"} body_class="p-0">
          <:subtitle>Per-operation counts from tool-call evidence, ordered by activity</:subtitle>
          <.empty_state
            :if={@files == []}
            icon="hero-document-text"
            title="No file activity"
            message="Hotspots appear once the daemon has re-ingested the archive with enrichment (automatic after upgrade)."
          />
          <div :if={@files != []} class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <.sort_header col="key" label={if @group == :ext, do: "Extension", else: "File"} sort={@files_sort} table="files" />
                  <.sort_header :if={@group == :path} col="project" label="Project" sort={@files_sort} table="files" />
                  <.sort_header col="reads" label="Reads" sort={@files_sort} table="files" align="right" />
                  <.sort_header col="edits" label="Edits" sort={@files_sort} table="files" align="right" />
                  <.sort_header col="writes" label="Writes" sort={@files_sort} table="files" align="right" />
                  <.sort_header col="deletes" label="Deletes" sort={@files_sort} table="files" align="right" />
                  <.sort_header col="sessions" label="Sessions" sort={@files_sort} table="files" align="right" />
                  <.sort_header col="total" label="Total" sort={@files_sort} table="files" align="right" />
                  <.sort_header col="last_touched_at" label="Last touched" sort={@files_sort} table="files" align="right" />
                </tr>
              </thead>
              <tbody>
                <tr :for={r <- @files} class="border-b border-line/60 transition-colors hover:bg-elevated">
                  <td class="px-4 py-2.5 font-mono text-fg">
                    <.link navigate={~p"/search?q=#{"\"" <> r.key <> "\""}"} class="hover:text-accent" title={"Search sessions touching #{r.key}"}>
                      {r.key}
                    </.link>
                  </td>
                  <td :if={@group == :path} class="px-4 py-2.5 text-muted" title={r.project}>
                    {basename(r.project) || "·"}
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.reads)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.edits)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.writes)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.deletes)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.sessions)}</td>
                  <td class="px-4 py-2.5 text-right font-medium tabular-nums">{int(r.total)}</td>
                  <td class="px-4 py-2.5 text-right text-muted">{relative_time(r.last_touched_at)}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end

  defp seg_class(active?) do
    base = "rounded-md px-2.5 py-1 text-sm transition-colors"

    if active? do
      [base, "bg-accent/10 font-medium text-accent"]
    else
      [base, "text-muted hover:bg-elevated hover:text-fg"]
    end
  end
end
```

  (Adjust helper imports to whatever `ToolsLive` actually has in scope: `int`/`relative_time` come from `DecantWeb.Format` via the html helpers; `Filters.url/2` exists per tools/analytics usage. If `/search` takes a different query param name, match it.)
- [ ] **Step 4:** run the file's tests → PASS; then the whole suite `mix test`. **Step 5:** commit `feat(web): Files hotspots page (/files) with group/op views and sortable table`.

## Task 3: gates + browser verification

- [ ] `cd web && mix format && mix test && mix format --check-formatted && mix compile --warnings-as-errors` → all green; commit any formatting as part of Task 2's commit (amend before push).
- [ ] Browser check (not CI): start this branch's daemon on a side port against the enriched scratch DB (`DECANT_DB=/tmp/decant-verify.db DECANT_CONFIG_DIR=/tmp/decant-verify-cfg DECANT_DAEMON_PORT=4599 ./target/release/decant daemon serve`), then `cd web && DECANT_DAEMON_URL=http://127.0.0.1:4599 DECANT_DAEMON_TOKEN=$(cat /tmp/decant-verify-cfg/daemon.token) mix phx.server`; with playwright-cli: load `/files`, click Languages, an op chip, and a sort header; screenshot for the PR.

## Self-review

Spec §1.1→T2 (route/nav), §1.2→T2 (URL chips), §1.3→T1 (total), §1.4→T2 (link-only rows), §1.5/§4 honored by omission, §1.6→T2 (empty state), §2→T1+T2, §3→T1+T2 tests + T3 browser pass. Names consistent: `file_hotspots`, `files_sort`, table id `"files"`, params `group`/`op`.
