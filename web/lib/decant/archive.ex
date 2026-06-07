defmodule Decant.Archive do
  @moduledoc """
  Read-only access to the decant SQLite archive. The schema is owned and written
  by the Rust `decant` CLI; this module only reads it (raw SQL → plain maps).

  Most read functions accept a `filters` map so the whole dashboard can scope by
  date range and drill down by dimension:

      %{from: ~D[2026-06-01], to: ~D[2026-06-07], tool: "codex", model: "gpt-5.5", project: "/path"}

  Any key may be omitted or nil. `from`/`to` accept a `Date` or an ISO date
  string; the range is inclusive of both endpoints.
  """
  alias Decant.Repo

  @doc "List sessions matching `filters`, newest first."
  def list_sessions(filters \\ %{}, limit \\ 200) do
    {where, params} = where_clause(filters)

    sql = """
    SELECT s.id, s.tool, s.title, s.model, s.message_count,
           s.estimated_cost_usd, s.started_at, p.path
    FROM session s
    LEFT JOIN project p ON p.id = s.project_id
    WHERE #{where}
    ORDER BY s.started_at DESC
    LIMIT ?
    """

    Repo.query!(sql, params ++ [limit]).rows |> Enum.map(&to_summary/1)
  end

  defp to_summary([id, tool, title, model, mc, cost, started, path]) do
    %{
      id: id,
      tool: tool,
      title: title,
      model: model,
      message_count: mc || 0,
      cost: cost || 0.0,
      started_at: started,
      project: path
    }
  end

  @doc "Full session detail: summary + ordered messages, each with its blocks."
  def get_session(id) do
    summary_sql = """
    SELECT s.id, s.tool, s.title, s.model, s.message_count, s.estimated_cost_usd, s.started_at, p.path
    FROM session s
    LEFT JOIN project p ON p.id = s.project_id
    WHERE s.id = ?
    """

    case Repo.query!(summary_sql, [id]).rows do
      [row] ->
        rows =
          Repo.query!(
            """
            SELECT m.id, m.role, b.type, b.text, b.tool_name, b.tool_input, b.tool_result
            FROM message m
            LEFT JOIN block b ON b.message_id = m.id
            WHERE m.session_id = ?
            ORDER BY m.seq, b.ordinal
            """,
            [id]
          ).rows

        %{summary: to_summary(row), messages: group_messages(rows)}

      _ ->
        nil
    end
  end

  defp group_messages(rows) do
    rows
    |> Enum.chunk_by(fn [mid | _] -> mid end)
    |> Enum.map(fn chunk ->
      [[_mid, role | _] | _] = chunk

      blocks =
        chunk
        |> Enum.reject(fn [_, _, type | _] -> is_nil(type) end)
        |> Enum.map(fn [_, _, type, text, tool_name, tool_input, tool_result] ->
          %{
            type: type,
            text: text,
            tool_name: tool_name,
            tool_input: tool_input,
            tool_result: tool_result
          }
        end)

      %{role: role || "unknown", blocks: blocks}
    end)
  end

  @doc "Full-text search over blocks (FTS5). Returns ranked hits with snippets."
  def search(query, limit \\ 50) do
    sql = """
    SELECT b.session_id, s.title, s.tool,
           snippet(block_fts, 0, '[', ']', '…', 12) AS snip
    FROM block_fts
    JOIN block b ON b.id = block_fts.rowid
    JOIN session s ON s.id = b.session_id
    WHERE block_fts MATCH ?
    ORDER BY bm25(block_fts)
    LIMIT ?
    """

    Repo.query!(sql, [query, limit]).rows
    |> Enum.map(fn [sid, title, tool, snip] ->
      %{session_id: sid, title: title, tool: tool, snippet: snip}
    end)
  end

  @doc "Whole-archive rollup, scoped to `filters`."
  def totals(filters \\ %{}) do
    {where, params} = where_clause(filters)

    [[sessions, intok, outtok, cost]] =
      Repo.query!(
        "SELECT COUNT(*), COALESCE(SUM(s.total_input_tokens),0), " <>
          "COALESCE(SUM(s.total_output_tokens),0), COALESCE(SUM(s.estimated_cost_usd),0.0) " <>
          "FROM session s WHERE #{where}",
        params
      ).rows

    [[messages]] =
      Repo.query!(
        "SELECT COUNT(*) FROM message m JOIN session s ON s.id = m.session_id WHERE #{where}",
        params
      ).rows

    [[tool_calls]] =
      Repo.query!(
        "SELECT COUNT(*) FROM tool_call tc JOIN session s ON s.id = tc.session_id WHERE #{where}",
        params
      ).rows

    %{
      sessions: sessions,
      messages: messages,
      tool_calls: tool_calls,
      input_tokens: intok,
      output_tokens: outtok,
      cost: cost
    }
  end

  @doc "Per-dimension rollup within `filters`. `dim` is :tool | :model | :project | :day."
  def by_dimension(dim, filters \\ %{}) when dim in [:tool, :model, :project, :day] do
    {where, params} = where_clause(filters)

    {expr, join} =
      case dim do
        :tool -> {"s.tool", ""}
        :model -> {"COALESCE(s.model, '(unknown)')", ""}
        :project -> {"COALESCE(p.path, '(none)')", "LEFT JOIN project p ON p.id = s.project_id"}
        :day -> {"substr(s.started_at, 1, 10)", ""}
      end

    sql =
      "SELECT #{expr} AS k, COUNT(*), COALESCE(SUM(s.total_input_tokens),0), " <>
        "COALESCE(SUM(s.total_output_tokens),0), COALESCE(SUM(s.estimated_cost_usd),0.0) " <>
        "FROM session s #{join} WHERE #{where} GROUP BY k ORDER BY 2 DESC"

    Repo.query!(sql, params).rows
    |> Enum.map(fn [k, sessions, intok, outtok, cost] ->
      %{key: k || "", sessions: sessions, input_tokens: intok, output_tokens: outtok, cost: cost}
    end)
  end

  @doc """
  Per-model daily session counts, aligned to a shared day axis (Tufte small
  multiples / sparklines). Returns `%{model => [count_per_day]}` where each list
  is ordered by the sorted distinct days present in `filters`.
  """
  def model_sparklines(filters \\ %{}) do
    {where, params} = where_clause(filters)

    rows =
      Repo.query!(
        "SELECT COALESCE(s.model,'(unknown)') k, substr(s.started_at,1,10) d, COUNT(*) c " <>
          "FROM session s WHERE #{where} AND s.started_at IS NOT NULL GROUP BY k, d",
        params
      ).rows

    days = rows |> Enum.map(fn [_, d, _] -> d end) |> Enum.uniq() |> Enum.sort()

    rows
    |> Enum.group_by(fn [k | _] -> k end)
    |> Map.new(fn {k, krows} ->
      by_day = Map.new(krows, fn [_, d, c] -> {d, c} end)
      {k, Enum.map(days, fn d -> Map.get(by_day, d, 0) end)}
    end)
  end

  @doc "Per-tool usage (built-in vs MCP) within `filters`, most-called first."
  def tool_usage(filters \\ %{}, limit \\ 50) do
    {where, params} = where_clause(filters)

    Repo.query!(
      "SELECT tc.tool_name, tc.tool_kind, tc.mcp_server, COUNT(*), " <>
        "COALESCE(SUM(CASE WHEN tc.is_error = 1 THEN 1 ELSE 0 END), 0) " <>
        "FROM tool_call tc JOIN session s ON s.id = tc.session_id WHERE #{where} " <>
        "GROUP BY tc.tool_name, tc.tool_kind, tc.mcp_server ORDER BY 4 DESC LIMIT ?",
      params ++ [limit]
    ).rows
    |> Enum.map(fn [n, k, srv, calls, errs] ->
      %{tool_name: n || "", kind: k || "", server: srv, calls: calls, errors: errs}
    end)
  end

  @doc "Per-MCP-server usage within `filters`, most-called first."
  def mcp_usage(filters \\ %{}, limit \\ 50) do
    {where, params} = where_clause(filters)

    Repo.query!(
      "SELECT tc.mcp_server, COUNT(DISTINCT tc.tool_name), COUNT(*), " <>
        "COALESCE(SUM(CASE WHEN tc.is_error = 1 THEN 1 ELSE 0 END), 0) " <>
        "FROM tool_call tc JOIN session s ON s.id = tc.session_id " <>
        "WHERE #{where} AND tc.tool_kind = 'mcp' AND tc.mcp_server IS NOT NULL " <>
        "GROUP BY tc.mcp_server ORDER BY 3 DESC LIMIT ?",
      params ++ [limit]
    ).rows
    |> Enum.map(fn [srv, tools, calls, errs] ->
      %{server: srv || "", tools: tools, calls: calls, errors: errs}
    end)
  end

  @doc "Min/max session dates (YYYY-MM-DD) for the date-range picker. nil if empty."
  def date_bounds do
    [[mn, mx]] =
      Repo.query!(
        "SELECT MIN(substr(started_at,1,10)), MAX(substr(started_at,1,10)) " <>
          "FROM session WHERE started_at IS NOT NULL",
        []
      ).rows

    %{min: mn, max: mx}
  end

  @doc "Resolved path of the archive DB (from the Repo config), for shelling out to `decant`."
  def db_path do
    Application.get_env(:decant, Decant.Repo)[:database]
  end

  # Build a parameterized WHERE fragment (on session alias `s`) from a filters map.
  defp where_clause(filters) do
    specs = [
      {:from, "s.started_at >= ?", &iso_date/1},
      {:to, "s.started_at < ?", &day_after/1},
      {:tool, "s.tool = ?", & &1},
      {:model, "s.model = ?", & &1},
      {:project, "s.project_id = (SELECT id FROM project WHERE path = ?)", & &1}
    ]

    {clauses, params} =
      Enum.reduce(specs, {[], []}, fn {key, frag, transform}, {cs, ps} ->
        case Map.get(filters, key) do
          val when val in [nil, ""] -> {cs, ps}
          val -> {[frag | cs], [transform.(val) | ps]}
        end
      end)

    where = if clauses == [], do: "1=1", else: clauses |> Enum.reverse() |> Enum.join(" AND ")
    {where, Enum.reverse(params)}
  end

  defp iso_date(%Date{} = d), do: Date.to_string(d)
  defp iso_date(s) when is_binary(s), do: s

  defp day_after(date) do
    date |> to_date() |> Date.add(1) |> Date.to_string()
  end

  defp to_date(%Date{} = d), do: d
  defp to_date(s) when is_binary(s), do: Date.from_iso8601!(s)
end
