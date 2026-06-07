defmodule Decant.ArchiveTest do
  @moduledoc """
  Behavior tests for the read-only archive context against the committed
  fixture DB (test/fixtures/decant.db). These tests only read; they must never
  mutate the fixture.
  """
  use Decant.DataCase, async: true

  alias Decant.Archive

  describe "list_sessions/0,1" do
    test "returns all sessions in the fixture, newest first" do
      sessions = Archive.list_sessions()

      assert length(sessions) == 2

      # started_at: session 2 (2026-05-02) is newer than session 1 (2026-05-01).
      assert [%{id: 2}, %{id: 1}] = sessions

      titles = Enum.map(sessions, & &1.title)
      assert "Fix the failing auth test" in titles
      assert "List the open TODOs" in titles
    end

    test "each summary exposes the expected fields" do
      [_newest, s1] = Archive.list_sessions()

      assert s1.id == 1
      assert s1.tool == "claude_code"
      assert s1.title == "Fix the failing auth test"
      assert s1.model == "claude-opus-4-7"
      assert s1.message_count == 4
      assert s1.cost > 0
      assert s1.project == "/Users/dev/proj"
    end

    test "honors the limit argument" do
      assert [%{id: 2}] = Archive.list_sessions(%{}, 1)
    end
  end

  describe "filters" do
    test "list_sessions scopes by date range" do
      assert [%{id: 1}] = Archive.list_sessions(%{from: ~D[2026-05-01], to: ~D[2026-05-01]})
      assert [%{id: 2}] = Archive.list_sessions(%{from: ~D[2026-05-02], to: ~D[2026-05-02]})
      assert length(Archive.list_sessions(%{from: ~D[2026-05-01], to: ~D[2026-05-02]})) == 2
    end

    test "list_sessions scopes by model and tool" do
      assert [%{id: 1}] = Archive.list_sessions(%{model: "claude-opus-4-7"})
      assert [%{id: 2}] = Archive.list_sessions(%{tool: "codex"})
    end

    test "totals scope to filters" do
      assert Archive.totals(%{tool: "codex"}).sessions == 1
      assert Archive.totals(%{from: ~D[2026-05-02], to: ~D[2026-05-02]}).sessions == 1
    end

    test "date_bounds returns the fixture span" do
      assert %{min: "2026-05-01", max: "2026-05-02"} = Archive.date_bounds()
    end

    test "model_sparklines returns per-model day counts" do
      sparks = Archive.model_sparklines(%{})
      assert is_map(sparks)
      assert Map.has_key?(sparks, "claude-opus-4-7")
    end
  end

  describe "get_session/1" do
    test "returns the summary plus grouped messages with blocks for a valid id" do
      detail = Archive.get_session(1)

      assert detail.summary.id == 1
      assert detail.summary.title == "Fix the failing auth test"

      # Fixture: session 1 has 4 messages.
      assert length(detail.messages) == 4

      # Every message is a map with a role and a list of blocks.
      for m <- detail.messages do
        assert is_binary(m.role)
        assert is_list(m.blocks)
      end

      # At least one block carries the searchable text.
      block_text =
        detail.messages
        |> Enum.flat_map(& &1.blocks)
        |> Enum.map(& &1.text)
        |> Enum.reject(&is_nil/1)
        |> Enum.join(" ")

      assert block_text =~ "auth"
    end

    test "accepts a string id (as passed from route params)" do
      detail = Archive.get_session("1")
      assert detail.summary.id == 1
    end

    test "returns nil for an unknown id" do
      assert Archive.get_session(999_999) == nil
    end
  end

  describe "search/1,2" do
    test "matches the auth session for 'auth'" do
      hits = Archive.search("auth")

      assert hits != []
      assert Enum.all?(hits, &(&1.session_id == 1))
      assert Enum.any?(hits, &(&1.title == "Fix the failing auth test"))

      # Snippet brackets the matched term.
      assert Enum.any?(hits, &(&1.snippet =~ "auth"))
    end

    test "matches the TODO session for 'TODO'" do
      hits = Archive.search("TODO")

      assert hits != []
      assert Enum.all?(hits, &(&1.session_id == 2))
      assert Enum.any?(hits, &(&1.title == "List the open TODOs"))
    end

    test "returns an empty list when nothing matches" do
      assert Archive.search("zzzzznomatchzzzzz") == []
    end
  end

  describe "totals/0" do
    test "rolls up the whole archive" do
      totals = Archive.totals()

      assert totals.sessions == 2
      assert totals.messages == 8
      assert totals.tool_calls == 2
      assert totals.cost > 0
      assert totals.input_tokens > 0
      assert totals.output_tokens > 0
    end
  end

  describe "by_dimension/1" do
    test ":model includes both fixture models" do
      rows = Archive.by_dimension(:model)
      keys = Enum.map(rows, & &1.key)

      assert "claude-opus-4-7" in keys
      assert "gpt-5.4" in keys
      assert Enum.all?(rows, &(&1.sessions >= 1))
    end

    test ":tool includes both fixture tools" do
      keys = Archive.by_dimension(:tool) |> Enum.map(& &1.key)

      assert "claude_code" in keys
      assert "codex" in keys
    end

    test ":project includes the fixture project path" do
      keys = Archive.by_dimension(:project) |> Enum.map(& &1.key)

      assert "/Users/dev/proj" in keys
    end

    test ":day groups by the date portion of started_at" do
      keys = Archive.by_dimension(:day) |> Enum.map(& &1.key)

      assert "2026-05-01" in keys
      assert "2026-05-02" in keys
    end
  end

  describe "tool_usage/0" do
    test "lists the built-in tools used in the fixture" do
      rows = Archive.tool_usage()
      names = Enum.map(rows, & &1.tool_name)

      assert "Read" in names
      assert "exec_command" in names

      read = Enum.find(rows, &(&1.tool_name == "Read"))
      assert read.kind == "builtin"
      assert read.calls >= 1
    end
  end

  describe "mcp_usage/0" do
    test "is empty because the fixture has no MCP tool calls" do
      assert Archive.mcp_usage() == []
    end
  end

  describe "db_path/0" do
    test "points at the committed fixture database" do
      assert Archive.db_path() =~ "test/fixtures/decant.db"
    end
  end
end
