defmodule Decant.Archive do
  @moduledoc """
  Read-only access to the decant SQLite archive. The schema is owned and written
  by the Rust `decant` CLI; this module only reads it (raw SQL → plain maps).
  """
  alias Decant.Repo

  @doc "List sessions, newest first."
  def list_sessions(limit \\ 200) do
    sql = """
    SELECT s.id, s.tool, s.title, s.model, s.message_count,
           s.estimated_cost_usd, s.started_at, p.path
    FROM session s
    LEFT JOIN project p ON p.id = s.project_id
    ORDER BY s.started_at DESC
    LIMIT ?
    """

    Repo.query!(sql, [limit]).rows |> Enum.map(&to_summary/1)
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
          %{type: type, text: text, tool_name: tool_name, tool_input: tool_input, tool_result: tool_result}
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

  @doc "Whole-archive rollup."
  def totals do
    [[sessions, messages, tool_calls, intok, outtok, cost]] =
      Repo.query!(
        """
        SELECT (SELECT COUNT(*) FROM session),
               (SELECT COUNT(*) FROM message),
               (SELECT COUNT(*) FROM tool_call),
               (SELECT COALESCE(SUM(total_input_tokens),0) FROM session),
               (SELECT COALESCE(SUM(total_output_tokens),0) FROM session),
               (SELECT COALESCE(SUM(estimated_cost_usd),0.0) FROM session)
        """,
        []
      ).rows

    %{sessions: sessions, messages: messages, tool_calls: tool_calls,
      input_tokens: intok, output_tokens: outtok, cost: cost}
  end

  @doc "Per-dimension rollup. `dim` is one of :tool, :model, :project, :day (fixed SQL, no user text)."
  def by_dimension(dim) when dim in [:tool, :model, :project, :day] do
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
        "FROM session s #{join} GROUP BY k ORDER BY 2 DESC"

    Repo.query!(sql, []).rows
    |> Enum.map(fn [k, sessions, intok, outtok, cost] ->
      %{key: k || "", sessions: sessions, input_tokens: intok, output_tokens: outtok, cost: cost}
    end)
  end

  @doc "Per-tool usage (built-in vs MCP), most-called first."
  def tool_usage(limit \\ 50) do
    Repo.query!(
      """
      SELECT tool_name, tool_kind, mcp_server, COUNT(*),
             COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0)
      FROM tool_call GROUP BY tool_name, tool_kind, mcp_server ORDER BY 4 DESC LIMIT ?
      """,
      [limit]
    ).rows
    |> Enum.map(fn [n, k, srv, calls, errs] ->
      %{tool_name: n || "", kind: k || "", server: srv, calls: calls, errors: errs}
    end)
  end

  @doc "Per-MCP-server usage, most-called first."
  def mcp_usage(limit \\ 50) do
    Repo.query!(
      """
      SELECT mcp_server, COUNT(DISTINCT tool_name), COUNT(*),
             COALESCE(SUM(CASE WHEN is_error = 1 THEN 1 ELSE 0 END), 0)
      FROM tool_call WHERE tool_kind = 'mcp' AND mcp_server IS NOT NULL
      GROUP BY mcp_server ORDER BY 3 DESC LIMIT ?
      """,
      [limit]
    ).rows
    |> Enum.map(fn [srv, tools, calls, errs] ->
      %{server: srv || "", tools: tools, calls: calls, errors: errs}
    end)
  end

  @doc "Resolved path of the archive DB (from the Repo config), for shelling out to `decant`."
  def db_path do
    Application.get_env(:decant, Decant.Repo)[:database]
  end
end
