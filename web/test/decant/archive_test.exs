defmodule Decant.ArchiveTest do
  @moduledoc """
  Behavior tests for the read-only archive context. `Decant.Archive` now reads
  through the daemon HTTP client (`Decant.Daemon`), so these tests stub the
  client with `Decant.DaemonStubs` (canned payloads equivalent to the old
  fixture: 2 sessions) and assert the context maps them into the atom-keyed
  shapes the LiveViews consume. All `Archive` calls happen in this test process,
  so private Mimic stubs (async-safe) are sufficient.
  """
  use ExUnit.Case, async: true
  use Mimic

  alias Decant.Archive

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  describe "list_sessions/0,1" do
    test "returns all sessions, newest first" do
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
      assert s1.source_session_id == "sess-claude-1"
    end

    test "honors the limit argument" do
      assert [%{id: 2}] = Archive.list_sessions(%{}, 1)
    end

    test "list_sessions_page exposes daemon pagination metadata" do
      page = Archive.list_sessions_page(%{}, 1)

      assert [%{id: 2}] = page.rows
      assert page.pagination.has_more
      assert page.pagination.next_cursor == "1"
      assert page.pagination.page_size == 1
      assert page.pagination.total_count == 2
    end

    test "list_sessions_page tolerates malformed daemon pagination metadata" do
      stub(Decant.Daemon, :list_sessions, fn _opts ->
        {:ok, [], []}
      end)

      page = Archive.list_sessions_page(%{}, 1)

      assert page.rows == []
      refute page.pagination.has_more
      assert is_nil(page.pagination.next_cursor)
    end

    test "returns an empty list when the daemon is unreachable" do
      stub(Decant.Daemon, :list_sessions, fn _opts -> {:error, :service_unavailable} end)
      assert Archive.list_sessions() == []
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

      assert length(detail.messages) == 4

      for m <- detail.messages do
        assert is_binary(m.role)
        assert is_list(m.blocks)
      end

      block_text =
        detail.messages
        |> Enum.flat_map(& &1.blocks)
        |> Enum.map(& &1.text)
        |> Enum.reject(&is_nil/1)
        |> Enum.join(" ")

      assert block_text =~ "auth"
    end

    test "exposes stats with token totals and duration" do
      detail = Archive.get_session(1)

      assert detail.stats.input_tokens == 1200
      assert detail.stats.output_tokens == 800
      assert detail.stats.reasoning_tokens == 0
      # Claude (session 1) has no exact count -> inferred estimate.
      assert detail.stats.est_reasoning_tokens == 250
      assert detail.stats.reasoning_source == "inferred"
      assert detail.stats.duration_seconds == 1200
    end

    test "surfaces codex reasoning tokens on the stats" do
      # Session 2 (codex) reports reasoning=120 exactly; nothing inferred.
      detail = Archive.get_session(2)
      assert detail.stats.reasoning_tokens == 120
      assert detail.stats.reasoning_tokens <= detail.stats.output_tokens
      assert detail.stats.est_reasoning_tokens == 0
      assert detail.stats.reasoning_source == "reported"
    end

    test "accepts a string id (as passed from route params)" do
      detail = Archive.get_session("1")
      assert detail.summary.id == 1
    end

    test "returns nil for an unknown id" do
      assert Archive.get_session(999_999) == nil
    end

    test "returns nil when the daemon is unreachable" do
      stub(Decant.Daemon, :get_session, fn _id -> {:error, :service_unavailable} end)
      assert Archive.get_session(1) == nil
    end
  end

  describe "search/1,2" do
    test "matches the auth session for 'auth'" do
      hits = Archive.search("auth")

      assert hits != []
      assert Enum.all?(hits, &(&1.session_id == 1))
      assert Enum.any?(hits, &(&1.title == "Fix the failing auth test"))

      assert Enum.any?(hits, &(&1.snippet =~ "auth"))
    end

    test "search_page returns hits with pagination metadata" do
      page = Archive.search_page("auth", 1)

      assert [%{session_id: 1}] = page.rows
      refute page.pagination.has_more
      assert page.pagination.page_size == 1
      assert is_nil(page.pagination.total_count)
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
      # Codex reports reasoning exactly (120); Claude is estimated (250).
      assert totals.reasoning_tokens == 120
      assert totals.est_reasoning_tokens == 250
    end

    test "returns zeroed totals when the daemon is unreachable" do
      stub(Decant.Daemon, :analytics_summary, fn _opts -> {:error, :service_unavailable} end)

      assert Archive.totals() == %{
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
  end

  describe "by_dimension/1" do
    test ":model includes both fixture models" do
      rows = Archive.by_dimension(:model)
      keys = Enum.map(rows, & &1.key)

      assert "claude-opus-4-7" in keys
      assert "gpt-5.4" in keys
      assert Enum.all?(rows, &(&1.sessions >= 1))
      # Reasoning rolls up per model: gpt-5.4 (codex) exact 120; claude estimated.
      gpt = Enum.find(rows, &(&1.key == "gpt-5.4"))
      assert gpt.reasoning_tokens == 120
      assert gpt.reasoning_tokens <= gpt.output_tokens
      assert gpt.est_reasoning_tokens == 0
      claude = Enum.find(rows, &(&1.key == "claude-opus-4-7"))
      assert claude.reasoning_tokens == 0
      assert claude.est_reasoning_tokens == 250
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

    test "returns an empty list when the daemon is unreachable" do
      stub(Decant.Daemon, :by_dimension, fn _dim, _opts -> {:error, :service_unavailable} end)
      assert Archive.by_dimension(:model) == []
    end
  end

  describe "activity/0,1" do
    test "returns 24 hour buckets and 7 weekday buckets that sum to the session count" do
      a = Archive.activity()

      assert length(a.by_hour) == 24
      assert length(a.by_weekday) == 7
      assert Enum.sum(a.by_hour) == 2
      assert Enum.sum(a.by_weekday) == 2
    end

    test "scopes to filters" do
      a = Archive.activity(%{tool: "codex"})
      assert Enum.sum(a.by_hour) == 1
    end

    test "returns zeroed buckets when the daemon is unreachable" do
      stub(Decant.Daemon, :activity, fn _opts -> {:error, :service_unavailable} end)
      a = Archive.activity()
      assert length(a.by_hour) == 24
      assert length(a.by_weekday) == 7
      assert Enum.sum(a.by_hour) == 0
    end
  end

  describe "overview/0" do
    test "returns an unfiltered snapshot with sessions, cost, and freshness" do
      o = Archive.overview()

      assert o.sessions == 2
      assert o.cost > 0
      assert o.last_activity == "2026-05-02"
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

    test "maps server rows when the daemon returns them" do
      stub(Decant.Daemon, :mcp_usage, fn _opts ->
        {:ok, [%{"mcp_server" => "github", "tools" => 4, "calls" => 12, "errors" => 1}], %{}}
      end)

      assert [row] = Archive.mcp_usage()
      assert row.server == "github"
      assert row.tools == 4
      assert row.calls == 12
      assert row.errors == 1
    end

    test "returns [] when the daemon is unreachable" do
      stub(Decant.Daemon, :mcp_usage, fn _opts -> {:error, :service_unavailable} end)
      assert Archive.mcp_usage() == []
    end
  end

  describe "graceful degradation and mapping edge cases" do
    test "search returns [] when the daemon is unreachable" do
      stub(Decant.Daemon, :search, fn _q, _opts -> {:error, :service_unavailable} end)
      assert Archive.search("auth") == []
    end

    test "tool_usage returns [] when the daemon is unreachable" do
      stub(Decant.Daemon, :tools_usage, fn _opts -> {:error, :service_unavailable} end)
      assert Archive.tool_usage() == []
    end

    test "model_sparklines returns %{} when the daemon is unreachable" do
      stub(Decant.Daemon, :model_sparklines, fn _opts -> {:error, :service_unavailable} end)
      assert Archive.model_sparklines() == %{}
    end

    test "date_bounds returns nils when the daemon is unreachable" do
      stub(Decant.Daemon, :date_bounds, fn -> {:error, :service_unavailable} end)
      assert Archive.date_bounds() == %{min: nil, max: nil}
    end

    test "activity pads/zeros a malformed (non-list) histogram payload" do
      stub(Decant.Daemon, :activity, fn _opts ->
        {:ok, %{"by_hour" => "nope", "by_weekday" => nil}}
      end)

      a = Archive.activity()
      assert a.by_hour == List.duplicate(0, 24)
      assert a.by_weekday == List.duplicate(0, 7)
    end

    test "list_sessions accepts an ISO date string for from/to" do
      assert [%{id: 1}] = Archive.list_sessions(%{from: "2026-05-01", to: "2026-05-01"})
    end

    test "list_sessions treats an empty-string date as no bound" do
      assert length(Archive.list_sessions(%{from: "", to: ""})) == 2
    end
  end
end
