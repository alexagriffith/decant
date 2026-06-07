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
end
