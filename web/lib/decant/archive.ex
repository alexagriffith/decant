defmodule Decant.Archive do
  @moduledoc """
  Read access to the decant archive, served by the local `decant` daemon's HTTP
  read API (`Decant.Daemon`). This module is the dashboard's data context: it
  calls the daemon, maps the JSON envelope into the plain atom-keyed maps the
  LiveViews already expect, and degrades gracefully when the daemon is down.

  Every public function accepts the same `filters` map the dashboard builds, so
  pages can scope by date range and drill down by dimension:

      %{from: ~D[2026-06-01], to: ~D[2026-06-07], tool: "codex", model: "gpt-5.5", project: "/path"}

  Any key may be omitted or nil. `from`/`to` accept a `Date` or an ISO date
  string. The daemon treats both bounds as inclusive whole days.

  When the daemon is unreachable (or returns an error) every function returns a
  safe empty default of the right shape — an empty list, zeroed totals,
  `%{min: nil, max: nil}`, and so on — so pages still render instead of crashing.
  """
  alias Decant.Daemon

  @doc "List sessions matching `filters`, newest first. `limit` caps the page size."
  def list_sessions(filters \\ %{}, limit \\ 200) do
    list_sessions_page(filters, limit).rows
  end

  @doc """
  Paginated session summaries plus daemon pagination metadata.

  Options:
    * `:cursor` - daemon cursor for the next page
    * `:sort` - daemon sort key (`"started_at_desc"`, `"started_at_asc"`, `"cost_desc"`)
  """
  def list_sessions_page(filters \\ %{}, limit \\ 200, opts \\ []) do
    params = to_params(filters) ++ page_params(limit, opts)

    case Daemon.list_sessions(params) do
      {:ok, rows, meta} when is_list(rows) ->
        page(Enum.map(rows, &to_summary/1), meta)

      _ ->
        empty_page()
    end
  end

  defp to_summary(s) do
    %{
      id: s["id"],
      tool: s["tool"],
      title: s["title"],
      model: s["model"],
      message_count: s["message_count"] || 0,
      cost: s["estimated_cost_usd"] || 0.0,
      started_at: s["started_at"],
      project: s["project"],
      source_session_id: s["source_session_id"]
    }
  end

  @doc """
  Full session detail: summary + computed stats + ordered messages, each with
  its blocks. Returns `nil` when the session does not exist or the daemon is
  unreachable.
  """
  def get_session(id) do
    case Daemon.get_session(id) do
      {:ok, %{"summary" => summary} = detail} ->
        %{
          summary: to_summary(summary),
          messages: detail |> Map.get("messages", []) |> Enum.map(&to_message/1),
          stats: to_stats(Map.get(detail, "stats", %{}))
        }

      _ ->
        nil
    end
  end

  # The transcript header stats. Cache tokens are summed (read + creation) to
  # preserve the historical `cache_tokens` field; the UI reads input/output and
  # duration.
  defp to_stats(stats) do
    %{
      input_tokens: stats["input_tokens"] || 0,
      output_tokens: stats["output_tokens"] || 0,
      reasoning_tokens: stats["reasoning_tokens"] || 0,
      est_reasoning_tokens: stats["est_reasoning_tokens"] || 0,
      reasoning_source: stats["reasoning_source"],
      cache_tokens: (stats["cache_read_tokens"] || 0) + (stats["cache_creation_tokens"] || 0),
      duration_seconds: stats["duration_seconds"]
    }
  end

  defp to_message(m) do
    %{
      role: m["role"] || "unknown",
      blocks: m |> Map.get("blocks", []) |> Enum.map(&to_block/1)
    }
  end

  defp to_block(b) do
    %{
      type: b["type"],
      text: b["text"],
      tool_name: b["tool_name"],
      tool_input: b["tool_input"],
      tool_result: b["tool_result"]
    }
  end

  @doc "Full-text search over blocks (FTS5). Returns ranked hits with snippets."
  def search(query, limit \\ 50) do
    search_page(query, limit).rows
  end

  @doc "Paginated full-text search hits plus daemon pagination metadata."
  def search_page(query, limit \\ 50, opts \\ []) do
    case Daemon.search(query, page_params(limit, opts)) do
      {:ok, hits, meta} when is_list(hits) ->
        hits
        |> Enum.map(fn h ->
          %{
            session_id: h["session_id"],
            title: h["session_title"],
            tool: h["tool"],
            snippet: h["snippet"]
          }
        end)
        |> page(meta)

      _ ->
        empty_page()
    end
  end

  @doc "Whole-archive rollup, scoped to `filters`."
  def totals(filters \\ %{}) do
    case Daemon.analytics_summary(to_params(filters)) do
      {:ok, t} when is_map(t) ->
        %{
          sessions: t["sessions"] || 0,
          messages: t["messages"] || 0,
          tool_calls: t["tool_calls"] || 0,
          input_tokens: t["input_tokens"] || 0,
          output_tokens: t["output_tokens"] || 0,
          reasoning_tokens: t["reasoning_tokens"] || 0,
          est_reasoning_tokens: t["est_reasoning_tokens"] || 0,
          cost: t["estimated_cost_usd"] || 0.0
        }

      _ ->
        empty_totals()
    end
  end

  defp empty_totals do
    %{
      sessions: 0,
      messages: 0,
      tool_calls: 0,
      input_tokens: 0,
      output_tokens: 0,
      reasoning_tokens: 0,
      est_reasoning_tokens: 0,
      cost: 0.0
    }
  end

  @doc "Per-dimension rollup within `filters`. `dim` is :tool | :model | :project | :day."
  def by_dimension(dim, filters \\ %{}) when dim in [:tool, :model, :project, :day] do
    case Daemon.by_dimension(dim, to_params(filters)) do
      {:ok, rows, _meta} when is_list(rows) ->
        Enum.map(rows, fn r ->
          %{
            key: r["key"] || "",
            sessions: r["sessions"] || 0,
            input_tokens: r["input_tokens"] || 0,
            output_tokens: r["output_tokens"] || 0,
            reasoning_tokens: r["reasoning_tokens"] || 0,
            est_reasoning_tokens: r["est_reasoning_tokens"] || 0,
            cost: r["estimated_cost_usd"] || 0.0,
            worktree_count: r["worktree_count"],
            worktree_label: r["worktree_label"],
            worktree_tool: r["worktree_tool"]
          }
        end)

      _ ->
        []
    end
  end

  @doc """
  File hotspots within `filters`. Options: `group: :path | :ext` (default
  :path), `op: :read | :edit | :write | :delete | nil`, `limit:` (default 100).
  Adds a computed `total` so tables have a rank column.
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

  @doc """
  Per-model daily session counts, aligned to a shared day axis (Tufte small
  multiples / sparklines). Returns `%{model => [count_per_day]}` ordered by the
  sorted distinct days present in `filters`.
  """
  def model_sparklines(filters \\ %{}) do
    case Daemon.model_sparklines(to_params(filters)) do
      {:ok, %{"models" => models}} when is_map(models) -> models
      _ -> %{}
    end
  end

  @doc """
  When sessions happen, for "busiest hour / day" reporting. Returns
  `%{by_hour: [24 counts], by_weekday: [7 counts]}` in the server's local time.
  `by_hour` is indexed 0..23, `by_weekday` 0..6 with 0 = Sunday.
  """
  def activity(filters \\ %{}) do
    case Daemon.activity(to_params(filters)) do
      {:ok, a} when is_map(a) ->
        %{
          by_hour: counts(a["by_hour"], 24),
          by_weekday: counts(a["by_weekday"], 7)
        }

      _ ->
        %{by_hour: List.duplicate(0, 24), by_weekday: List.duplicate(0, 7)}
    end
  end

  # Normalize a histogram to exactly `size` integer buckets (pad/truncate so the
  # UI's 0..size-1 indexing is always safe, even on a malformed payload).
  defp counts(list, size) when is_list(list) do
    list = Enum.map(list, &(&1 || 0))
    Enum.map(0..(size - 1), fn i -> Enum.at(list, i, 0) end)
  end

  defp counts(_other, size), do: List.duplicate(0, size)

  @doc "Per-tool usage (built-in vs MCP) within `filters`, most-called first."
  def tool_usage(filters \\ %{}, limit \\ 50) do
    params = to_params(filters) ++ [limit: limit]

    case Daemon.tools_usage(params) do
      {:ok, rows, _meta} when is_list(rows) ->
        Enum.map(rows, fn r ->
          %{
            tool_name: r["tool_name"] || "",
            kind: r["tool_kind"] || "",
            server: r["mcp_server"],
            calls: r["calls"] || 0,
            errors: r["errors"] || 0
          }
        end)

      _ ->
        []
    end
  end

  @doc "Per-MCP-server usage within `filters`, most-called first."
  def mcp_usage(filters \\ %{}, limit \\ 50) do
    params = to_params(filters) ++ [limit: limit]

    case Daemon.mcp_usage(params) do
      {:ok, rows, _meta} when is_list(rows) ->
        Enum.map(rows, fn r ->
          %{
            server: r["mcp_server"] || "",
            tools: r["tools"] || 0,
            calls: r["calls"] || 0,
            errors: r["errors"] || 0
          }
        end)

      _ ->
        []
    end
  end

  @doc "Min/max session dates (YYYY-MM-DD) for the date-range picker. nil if empty/unreachable."
  def date_bounds do
    case Daemon.date_bounds() do
      {:ok, b} when is_map(b) -> %{min: b["min"], max: b["max"]}
      _ -> %{min: nil, max: nil}
    end
  end

  @doc """
  Unfiltered snapshot for the sidebar: total session count, total estimated
  cost, and the most recent session date (archive freshness).
  """
  def overview do
    t = totals(%{})
    %{sessions: t.sessions, cost: t.cost, last_activity: date_bounds().max}
  end

  defp to_params(filters) do
    [
      from: iso_date(Map.get(filters, :from)),
      to: iso_date(Map.get(filters, :to)),
      tool: present(Map.get(filters, :tool)),
      model: present(Map.get(filters, :model)),
      project: present(Map.get(filters, :project)),
      root: present(Map.get(filters, :root))
    ]
    |> Enum.reject(fn {_k, v} -> is_nil(v) end)
  end

  defp iso_date(nil), do: nil
  defp iso_date(""), do: nil
  defp iso_date(%Date{} = d), do: Date.to_string(d)
  defp iso_date(s) when is_binary(s), do: s

  defp present(v) when v in [nil, ""], do: nil
  defp present(v), do: v

  defp page_params(limit, opts) do
    [limit: limit, cursor: opts[:cursor], sort: opts[:sort]]
    |> Enum.reject(fn {_k, v} -> is_nil(v) or v == "" end)
  end

  defp page(rows, meta), do: %{rows: rows, pagination: pagination(meta)}
  defp empty_page, do: page([], %{})

  defp pagination(meta) when is_map(meta) do
    p = Map.get(meta, "pagination", %{})

    %{
      has_more: Map.get(p, "has_more", false),
      next_cursor: Map.get(p, "next_cursor"),
      page_size: Map.get(p, "page_size"),
      total_count: Map.get(p, "total_count")
    }
  end

  defp pagination(_), do: pagination(%{})
end
