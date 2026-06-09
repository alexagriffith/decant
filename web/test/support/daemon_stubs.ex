defmodule Decant.DaemonStubs do
  @moduledoc """
  Shared Mimic stubs for `Decant.Daemon`, returning canned API payloads for two
  synthetic sessions the web tests assert on:

    * id 1 — "Fix the failing auth test", `claude_code` / `claude-opus-4-7`,
      started 2026-05-01, project `/Users/dev/proj`, 4 messages, one builtin
      `Read` tool call.
    * id 2 — "List the open TODOs", `codex` / `gpt-5.4`, started 2026-05-02,
      4 messages, one builtin `exec_command` tool call.

  `install/0` wires `Mimic.stub/3` for every `Decant.Daemon` function the
  `Archive` context calls, honoring the `from`/`to`/`tool`/`model`/`project`
  filter opts so filter-scoped assertions still hold. Call it from a test's
  `setup` (the module must already be `Mimic.copy/1`-ed, which `test_helper.exs`
  does for `Decant.Daemon`).
  """

  import Mimic

  @sessions [
    %{
      "id" => 1,
      "tool" => "claude_code",
      "source_session_id" => "sess-claude-1",
      "title" => "Fix the failing auth test",
      "model" => "claude-opus-4-7",
      "project" => "/Users/dev/proj",
      "started_at" => "2026-05-01T09:00:00Z",
      "ended_at" => "2026-05-01T09:20:00Z",
      "message_count" => 4,
      "total_input_tokens" => 1200,
      "total_output_tokens" => 800,
      "total_cache_read_tokens" => 0,
      "total_cache_creation_tokens" => 0,
      "estimated_cost_usd" => 0.42,
      "is_archived" => false
    },
    %{
      "id" => 2,
      "tool" => "codex",
      "source_session_id" => "sess-codex-2",
      "title" => "List the open TODOs",
      "model" => "gpt-5.4",
      "project" => "/Users/dev/proj",
      "started_at" => "2026-05-02T14:00:00Z",
      "ended_at" => "2026-05-02T14:05:00Z",
      "message_count" => 4,
      "total_input_tokens" => 600,
      "total_output_tokens" => 300,
      "total_cache_read_tokens" => 0,
      "total_cache_creation_tokens" => 0,
      "estimated_cost_usd" => 0.18,
      "is_archived" => false
    }
  ]

  # message_count and tool_calls per session, so totals match the old fixture
  # (8 messages, 2 tool calls across the two sessions).
  @tool_calls %{1 => 1, 2 => 1}

  # Representative recommendations the Insights page asserts on: two open
  # signals, a catalog spread across categories (the first Foundations entry is
  # the spotlight), and one already-implemented entry.
  @recommendations [
    %{
      "key" => "signal:error:Read",
      "kind" => "signal",
      "category" => nil,
      "title" => "Read fails 20% of the time",
      "detail" => "4 errors across 20 calls.",
      "suggestion" => "Codify the recovery path as a Skill.",
      "prompt" => "The Read tool is failing often. Investigate and codify a Skill.",
      "url" => "https://code.claude.com/docs/en/skills",
      "link_label" => "Skills guide",
      "icon" => "hero-exclamation-triangle",
      "tone" => "danger",
      "score" => 4.0,
      "status" => "open",
      "status_source" => nil,
      "note" => nil,
      "implemented_at" => nil
    },
    %{
      "key" => "signal:heavy-server:claude_ai_Exa",
      "kind" => "signal",
      "category" => nil,
      "title" => "Heavy reliance on the claude_ai_Exa MCP server",
      "detail" => "120 calls across 6 tools.",
      "suggestion" => "Package the common workflows into a Skill.",
      "prompt" => "We rely heavily on claude_ai_Exa. Create a reusable Skill.",
      "url" => "https://code.claude.com/docs/en/skills",
      "link_label" => "Skills guide",
      "icon" => "hero-cpu-chip",
      "tone" => "accent",
      "score" => 60.0,
      "status" => "open",
      "status_source" => nil,
      "note" => nil,
      "implemented_at" => nil
    },
    %{
      "key" => "catalog:agents-md",
      "kind" => "catalog",
      "category" => "Foundations",
      "title" => "AGENTS.md at the repo root",
      "detail" => "One machine-readable contract every agent reads first.",
      "suggestion" => nil,
      "prompt" => "Create a high-quality AGENTS.md at this repo root.",
      "url" => "https://agents.md",
      "link_label" => "agents.md standard",
      "icon" => "hero-document-text",
      "tone" => nil,
      "score" => 0,
      "status" => "open",
      "status_source" => nil,
      "note" => nil,
      "implemented_at" => nil
    },
    %{
      "key" => "catalog:claude-md",
      "kind" => "catalog",
      "category" => "Foundations",
      "title" => "Project memory (CLAUDE.md)",
      "detail" => "Persistent facts every session loads automatically.",
      "suggestion" => nil,
      "prompt" => "Create a concise CLAUDE.md capturing durable facts.",
      "url" => "https://code.claude.com/docs/en/memory",
      "link_label" => "Memory guide",
      "icon" => "hero-book-open",
      "tone" => nil,
      "score" => 0,
      "status" => "open",
      "status_source" => nil,
      "note" => nil,
      "implemented_at" => nil
    },
    %{
      "key" => "catalog:skills",
      "kind" => "catalog",
      "category" => "Reusable workflows",
      "title" => "Skills",
      "detail" => "Capture a repeated procedure once as a SKILL.md.",
      "suggestion" => nil,
      "prompt" => "Scaffold a reusable Skill for a workflow I repeat often.",
      "url" => "https://code.claude.com/docs/en/skills",
      "link_label" => "Skills guide",
      "icon" => "hero-sparkles",
      "tone" => nil,
      "score" => 0,
      "status" => "open",
      "status_source" => nil,
      "note" => nil,
      "implemented_at" => nil
    },
    %{
      "key" => "catalog:hooks",
      "kind" => "catalog",
      "category" => "Connect and automate",
      "title" => "Hooks that keep the tree green",
      "detail" => "Run format, lint, and tests automatically on agent events.",
      "suggestion" => nil,
      "prompt" => "Set up Claude Code hooks to run format, lint, and tests.",
      "url" => "https://code.claude.com/docs/en/hooks-guide",
      "link_label" => "Hooks guide",
      "icon" => "hero-bolt",
      "tone" => nil,
      "score" => 0,
      "status" => "implemented",
      "status_source" => "agent",
      "note" => nil,
      "implemented_at" => "2026-05-03T10:00:00Z"
    }
  ]

  @messages_1 [
    %{
      "seq" => 0,
      "role" => "user",
      "blocks" => [%{"type" => "text", "text" => "Fix the failing auth test in the suite."}]
    },
    %{
      "seq" => 1,
      "role" => "assistant",
      "blocks" => [
        %{"type" => "text", "text" => "Reading the auth test to see why it fails."},
        %{
          "type" => "tool_use",
          "tool_name" => "Read",
          "tool_input" => "{\"path\":\"test/auth_test.exs\"}"
        }
      ]
    },
    %{
      "seq" => 2,
      "role" => "tool",
      "blocks" => [%{"type" => "tool_result", "tool_result" => "1 test, 1 failure (auth)"}]
    },
    %{
      "seq" => 3,
      "role" => "assistant",
      "blocks" => [
        %{"type" => "text", "text" => "Fixed the auth assertion; the test passes now."}
      ]
    }
  ]

  @messages_2 [
    %{
      "seq" => 0,
      "role" => "user",
      "blocks" => [%{"type" => "text", "text" => "List the open TODOs in this repo."}]
    },
    %{
      "seq" => 1,
      "role" => "assistant",
      "blocks" => [
        %{"type" => "text", "text" => "Scanning for TODO comments."},
        %{
          "type" => "tool_use",
          "tool_name" => "exec_command",
          "tool_input" => "{\"cmd\":\"rg TODO\"}"
        }
      ]
    },
    %{
      "seq" => 2,
      "role" => "tool",
      "blocks" => [%{"type" => "tool_result", "tool_result" => "lib/foo.ex: TODO refactor"}]
    },
    %{
      "seq" => 3,
      "role" => "assistant",
      "blocks" => [%{"type" => "text", "text" => "Found one open TODO."}]
    }
  ]

  @doc "Install default stubs for every `Decant.Daemon` function the Archive uses."
  def install do
    stub(Decant.Daemon, :list_sessions, &list_sessions/1)
    stub(Decant.Daemon, :get_session, &get_session/1)
    stub(Decant.Daemon, :search, &search/2)
    stub(Decant.Daemon, :analytics_summary, &analytics_summary/1)
    stub(Decant.Daemon, :by_dimension, &by_dimension/2)
    stub(Decant.Daemon, :activity, &activity/1)
    stub(Decant.Daemon, :model_sparklines, &model_sparklines/1)
    stub(Decant.Daemon, :tools_usage, &tools_usage/1)
    stub(Decant.Daemon, :mcp_usage, &mcp_usage/1)
    stub(Decant.Daemon, :date_bounds, &date_bounds/0)
    stub(Decant.Daemon, :recommendations, &recommendations/1)
    stub(Decant.Daemon, :health, fn -> {:ok, %{"status" => "ok"}} end)
    :ok
  end

  @doc "The canned sessions (API SessionSummary shape), for tests that want the raw payloads."
  def sessions, do: @sessions

  @doc "The canned recommendations (API Recommendation shape), for tests that want the raw payloads."
  def recommendations, do: @recommendations

  defp list_sessions(opts) do
    rows =
      @sessions
      |> filter(opts)
      |> Enum.sort_by(& &1["started_at"], :desc)
      |> take(opts)

    {:ok, rows, paginated_meta()}
  end

  defp get_session(id) do
    case find_session(id) do
      nil ->
        {:error, {:http, 404, %{"error" => %{"code" => "NOT_FOUND"}}}}

      session ->
        {:ok,
         %{
           "summary" => session,
           "stats" => stats_for(session),
           "messages" => messages_for(session["id"])
         }}
    end
  end

  defp search(q, _opts) do
    hits =
      cond do
        String.contains?(String.downcase(q), "auth") -> [hit(1, "auth")]
        String.contains?(String.downcase(q), "todo") -> [hit(2, "TODO")]
        true -> []
      end

    {:ok, hits, %{"timestamp" => "2026-05-02T00:00:00Z"}}
  end

  defp analytics_summary(opts) do
    rows = filter(@sessions, opts)

    {:ok,
     %{
       "sessions" => length(rows),
       "messages" => Enum.sum(Enum.map(rows, & &1["message_count"])),
       "tool_calls" => Enum.sum(Enum.map(rows, &Map.get(@tool_calls, &1["id"], 0))),
       "input_tokens" => sum(rows, "total_input_tokens"),
       "output_tokens" => sum(rows, "total_output_tokens"),
       "cache_read_tokens" => 0,
       "cache_creation_tokens" => 0,
       "estimated_cost_usd" => sum(rows, "estimated_cost_usd")
     }}
  end

  defp by_dimension(dim, opts) do
    key = dimension_key(dim)

    rows =
      @sessions
      |> filter(opts)
      |> Enum.group_by(&key.(&1))
      |> Enum.map(fn {k, group} ->
        %{
          "key" => k,
          "sessions" => length(group),
          "input_tokens" => sum(group, "total_input_tokens"),
          "output_tokens" => sum(group, "total_output_tokens"),
          "estimated_cost_usd" => sum(group, "estimated_cost_usd")
        }
      end)
      |> Enum.sort_by(& &1["sessions"], :desc)

    {:ok, rows, paginated_meta()}
  end

  defp activity(opts) do
    rows = filter(@sessions, opts)

    # Bucket each session by the hour/weekday of its (UTC) start. The exact
    # bucket is unimportant for the assertions; only the totals and lengths are.
    by_hour = histogram(rows, 24, &start_hour/1)
    by_weekday = histogram(rows, 7, &start_weekday/1)

    {:ok,
     %{
       "by_hour" => by_hour,
       "by_weekday" => by_weekday,
       "timezone" => "UTC+00:00",
       "peak_hour" => peak(by_hour),
       "peak_weekday" => peak(by_weekday)
     }}
  end

  defp model_sparklines(opts) do
    rows = filter(@sessions, opts)
    days = rows |> Enum.map(&day(&1["started_at"])) |> Enum.uniq() |> Enum.sort()

    models =
      rows
      |> Enum.group_by(& &1["model"])
      |> Map.new(fn {model, group} ->
        by_day = Enum.frequencies_by(group, &day(&1["started_at"]))
        {model, Enum.map(days, fn d -> Map.get(by_day, d, 0) end)}
      end)

    {:ok, %{"models" => models, "days" => days}}
  end

  defp tools_usage(opts) do
    rows =
      @sessions
      |> filter(opts)
      |> Enum.map(&tool_row_for/1)
      |> Enum.sort_by(& &1["calls"], :desc)

    {:ok, rows, paginated_meta()}
  end

  defp mcp_usage(_opts), do: {:ok, [], paginated_meta()}

  defp date_bounds, do: {:ok, %{"min" => "2026-05-01", "max" => "2026-05-02"}}

  defp recommendations(status) do
    rows =
      case to_string(status) do
        "open" -> Enum.filter(@recommendations, &(&1["status"] == "open"))
        "implemented" -> Enum.filter(@recommendations, &(&1["status"] == "implemented"))
        _ -> @recommendations
      end

    {:ok, rows}
  end

  defp filter(sessions, opts) do
    opts = Map.new(opts, fn {k, v} -> {to_string(k), v} end)

    Enum.filter(sessions, fn s ->
      match_from?(s, opts["from"]) and match_to?(s, opts["to"]) and
        match_eq?(s["tool"], opts["tool"]) and match_eq?(s["model"], opts["model"]) and
        match_eq?(s["project"], opts["project"])
    end)
  end

  defp match_from?(_s, nil), do: true
  defp match_from?(s, from), do: day(s["started_at"]) >= to_string(from)

  defp match_to?(_s, nil), do: true
  defp match_to?(s, to), do: day(s["started_at"]) <= to_string(to)

  defp match_eq?(_value, nil), do: true
  defp match_eq?(value, expected), do: value == expected

  defp take(rows, opts) do
    case opts |> Map.new(fn {k, v} -> {to_string(k), v} end) |> Map.get("limit") do
      nil -> rows
      limit -> Enum.take(rows, limit)
    end
  end

  defp find_session(id) do
    target = to_string(id)
    Enum.find(@sessions, fn s -> to_string(s["id"]) == target end)
  end

  defp messages_for(1), do: @messages_1
  defp messages_for(2), do: @messages_2

  defp stats_for(session) do
    %{
      "duration_seconds" => 1200,
      "input_tokens" => session["total_input_tokens"],
      "output_tokens" => session["total_output_tokens"],
      "cache_read_tokens" => 0,
      "cache_creation_tokens" => 0,
      "estimated_cost_usd" => session["estimated_cost_usd"],
      "cost_breakdown" => %{
        "input" => session["estimated_cost_usd"],
        "output" => 0.0,
        "cache_read" => 0.0,
        "cache_creation" => 0.0,
        "total" => session["estimated_cost_usd"]
      }
    }
  end

  defp hit(session_id, term) do
    session = find_session(session_id)

    %{
      "session_id" => session_id,
      "session_title" => session["title"],
      "tool" => session["tool"],
      "block_id" => session_id * 10,
      "snippet" => "matched [#{term}] in this session"
    }
  end

  defp tool_row_for(%{"id" => 1}), do: tool_row("Read", "builtin", 1, 0)
  defp tool_row_for(%{"id" => 2}), do: tool_row("exec_command", "builtin", 1, 0)

  defp tool_row(name, kind, calls, errors) do
    %{
      "tool_name" => name,
      "tool_kind" => kind,
      "mcp_server" => nil,
      "calls" => calls,
      "errors" => errors,
      "error_rate" => if(calls == 0, do: 0.0, else: errors / calls)
    }
  end

  defp dimension_key(:tool), do: & &1["tool"]
  defp dimension_key(:model), do: & &1["model"]
  defp dimension_key(:project), do: & &1["project"]
  defp dimension_key(:day), do: &day(&1["started_at"])

  defp histogram(rows, size, bucket_fun) do
    counts = Enum.frequencies_by(rows, bucket_fun)
    Enum.map(0..(size - 1), fn i -> Map.get(counts, i, 0) end)
  end

  defp peak([]), do: nil

  defp peak(buckets) do
    max = Enum.max(buckets)
    if max > 0, do: Enum.find_index(buckets, &(&1 == max))
  end

  defp start_hour(s) do
    {:ok, dt, _} = DateTime.from_iso8601(s["started_at"])
    dt.hour
  end

  defp start_weekday(s) do
    {:ok, dt, _} = DateTime.from_iso8601(s["started_at"])
    # Date.day_of_week is 1 (Mon)..7 (Sun); map to 0 (Sun)..6 (Sat).
    rem(Date.day_of_week(DateTime.to_date(dt)), 7)
  end

  defp day(iso) when is_binary(iso), do: String.slice(iso, 0, 10)

  defp sum(rows, field), do: Enum.reduce(rows, 0, fn r, acc -> acc + (r[field] || 0) end)

  defp paginated_meta do
    %{
      "pagination" => %{
        "has_more" => false,
        "page_size" => 50,
        "total_count" => length(@sessions)
      }
    }
  end
end
