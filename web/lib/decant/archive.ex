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
    params = to_params(filters) ++ [limit: limit]

    case Daemon.list_sessions(params) do
      {:ok, rows, _meta} when is_list(rows) -> Enum.map(rows, &to_summary/1)
      _ -> []
    end
  end

  # Map an API SessionSummary (string keys) to the dashboard's atom-keyed shape.
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
    case Daemon.search(query, limit: limit) do
      {:ok, hits, _meta} when is_list(hits) ->
        Enum.map(hits, fn h ->
          %{
            session_id: h["session_id"],
            title: h["session_title"],
            tool: h["tool"],
            snippet: h["snippet"]
          }
        end)

      _ ->
        []
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
          cost: t["estimated_cost_usd"] || 0.0
        }

      _ ->
        empty_totals()
    end
  end

  defp empty_totals do
    %{sessions: 0, messages: 0, tool_calls: 0, input_tokens: 0, output_tokens: 0, cost: 0.0}
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
            cost: r["estimated_cost_usd"] || 0.0
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

  # Build the daemon's query params (string values) from a filters map. `from`
  # and `to` become `YYYY-MM-DD`; nils are dropped by the client.
  defp to_params(filters) do
    [
      from: iso_date(Map.get(filters, :from)),
      to: iso_date(Map.get(filters, :to)),
      tool: present(Map.get(filters, :tool)),
      model: present(Map.get(filters, :model)),
      project: present(Map.get(filters, :project))
    ]
    |> Enum.reject(fn {_k, v} -> is_nil(v) end)
  end

  defp iso_date(nil), do: nil
  defp iso_date(""), do: nil
  defp iso_date(%Date{} = d), do: Date.to_string(d)
  defp iso_date(s) when is_binary(s), do: s

  defp present(v) when v in [nil, ""], do: nil
  defp present(v), do: v
end
