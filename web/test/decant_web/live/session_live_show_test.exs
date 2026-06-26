defmodule DecantWeb.SessionLive.ShowTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  test "shows a session's summary and message/block content", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/sessions/1")

    assert html =~ "Fix the failing auth test"

    assert html =~ "Claude"
    assert html =~ "claude-opus-4-7"

    assert html =~ "auth"

    # Claude has no exact reasoning count, so the header shows the estimate (≈).
    assert html =~ "reasoning (est)"
    assert html =~ "≈"
  end

  test "redirects to the index for a non-existent session id", %{conn: conn} do
    # Show.mount/3 handles a nil session by flashing an error and
    # push_navigate-ing back to "/", which surfaces as a live_redirect.
    assert {:error, {:live_redirect, %{to: "/"}}} = live(conn, ~p"/sessions/999999")
  end

  describe "open in editor" do
    setup do
      Mimic.stub(Decant.AgentLauncher, :can_launch?, fn -> true end)
      :ok
    end

    test "the Open in editor button launches the IDE and flashes", %{conn: conn} do
      Mimic.stub(Decant.AgentLauncher, :open_ide, fn _dir -> :ok end)

      {:ok, view, html} = live(conn, ~p"/sessions/1")
      assert html =~ "Open in"

      html = view |> element("button[phx-click=\"open_ide\"]") |> render_click()
      assert html =~ "Opening"
    end

    test "an IDE launch failure surfaces the error", %{conn: conn} do
      Mimic.stub(Decant.AgentLauncher, :open_ide, fn _dir -> {:error, "that folder is gone"} end)

      {:ok, view, _html} = live(conn, ~p"/sessions/1")
      html = view |> element("button[phx-click=\"open_ide\"]") |> render_click()
      assert html =~ "that folder is gone"
    end
  end

  describe "transcript rendering for varied block types" do
    setup do
      # A codex session with a thinking block, a short tool_use (auto-open
      # arguments), a long tool_use (collapsed), and a tool result. This drives
      # the render branches the two canned fixtures don't reach.
      long_input = String.duplicate("x", 300)

      detail = %{
        "summary" => %{
          "id" => 42,
          "tool" => "codex",
          "title" => "Rich transcript",
          "model" => "gpt-5.4",
          "project" => "/Users/dev/proj",
          "source_session_id" => "sess-codex-42",
          "message_count" => 3,
          "estimated_cost_usd" => 0.5,
          "started_at" => "2026-05-02T14:00:00Z"
        },
        "stats" => %{
          "duration_seconds" => 60,
          "input_tokens" => 10,
          "output_tokens" => 5,
          "cache_read_tokens" => 0,
          "cache_creation_tokens" => 0
        },
        "messages" => [
          %{
            "seq" => 0,
            "role" => "user",
            # A long first line (>70 chars) is truncated in the TOC label; the
            # user turn also carries a tool_use so the TOC shows a tool count.
            "blocks" => [
              %{
                "type" => "text",
                "text" =>
                  "Please refactor the entire parser module very carefully and double-check every edge case before you continue."
              },
              %{"type" => "tool_use", "tool_name" => "Grep", "tool_input" => "{\"q\":1}"},
              # An unknown block type exercises the renderable filter's fallthrough.
              %{"type" => "mystery", "text" => nil}
            ]
          },
          %{
            "seq" => 1,
            "role" => "assistant",
            "blocks" => [
              %{"type" => "thinking", "text" => "I should read the parser first."},
              %{"type" => "tool_use", "tool_name" => "Read", "tool_input" => "{\"p\":1}"},
              %{"type" => "tool_use", "tool_name" => "Edit", "tool_input" => long_input}
            ]
          },
          %{
            "seq" => 2,
            "role" => "tool",
            "blocks" => [%{"type" => "tool_result", "tool_result" => "done"}]
          },
          # An unusual role exercises the role_tone fallback clause.
          %{
            "seq" => 3,
            "role" => "system",
            "blocks" => [%{"type" => "text", "text" => "system note"}]
          },
          # A non-renderable message (only blank text) must be filtered out.
          %{
            "seq" => 4,
            "role" => "assistant",
            "blocks" => [%{"type" => "text", "text" => "   "}]
          },
          # A message whose only block has an unknown type is dropped too (this
          # drives the renderable filter's catch-all clause to its conclusion).
          %{
            "seq" => 5,
            "role" => "assistant",
            "blocks" => [%{"type" => "mystery", "text" => nil}]
          }
        ]
      }

      Mimic.stub(Decant.Daemon, :get_session, fn _id -> {:ok, detail} end)
      :ok
    end

    test "renders thinking, tool calls, and a result, plus codex resume commands", %{conn: conn} do
      {:ok, _view, html} = live(conn, ~p"/sessions/42")

      assert html =~ "Rich transcript"
      assert html =~ "Thinking"
      assert html =~ "tool call"
      assert html =~ "result"
      # Codex resume command appears in the "Continue this thread" panel.
      assert html =~ "codex resume sess-codex-42"
      # The unusual-role message still renders (role_tone fallback).
      assert html =~ "system note"
      # The long TOC label is truncated with an ellipsis.
      assert html =~ "…"
    end
  end

  test "renders the empty-toc note when no user prompts have text", %{conn: conn} do
    detail = %{
      "summary" => %{
        "id" => 7,
        "tool" => "claude_code",
        "title" => "No prompts",
        "model" => "claude-opus-4-7",
        "project" => nil,
        "source_session_id" => nil,
        "message_count" => 1,
        "estimated_cost_usd" => 0.0,
        "started_at" => "2026-05-01T09:00:00Z"
      },
      "stats" => %{"duration_seconds" => nil, "input_tokens" => 0, "output_tokens" => 0},
      "messages" => [
        %{"seq" => 0, "role" => "assistant", "blocks" => [%{"type" => "text", "text" => "Hi."}]}
      ]
    }

    Mimic.stub(Decant.Daemon, :get_session, fn _id -> {:ok, detail} end)

    {:ok, _view, html} = live(conn, ~p"/sessions/7")
    assert html =~ "No prompts to list"
  end
end
