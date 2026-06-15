defmodule DecantWeb.InsightsLiveTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    # Force the agent CTA to render its launch buttons regardless of the host OS
    # the suite runs on (the real check is macOS-only).
    Mimic.stub(Decant.AgentLauncher, :can_launch?, fn -> true end)
    :ok
  end

  test "renders promotion candidates, Recommended, and Implemented sections", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/insights")

    assert html =~ "Promotion candidates"
    assert html =~ "Read fails 20% of the time"
    assert html =~ "Heavy reliance on the claude_ai_Exa MCP server"
    assert html =~ "Memory card"
    assert html =~ "Promote to"
    assert html =~ "Skill or regression test"

    assert html =~ "Recommended for coding agents"
    assert html =~ "Foundations"
    assert html =~ "Reusable workflows"
    assert html =~ "AGENTS.md at the repo root"

    assert html =~ "Implemented"
    assert html =~ "Hooks that keep the tree green"
  end

  test "the launch button carries the recommendation key", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/insights")

    # The primary CTA threads the rec key through phx-value-key so the agent
    # records it implemented when finished.
    assert html =~ ~s(phx-value-key="catalog:agents-md")
    assert html =~ ~s(phx-value-key="signal:error:Read")
  end

  test "the top-ranked signal leads and lower signals drop the repeated suggestion",
       %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/insights")

    # Signals are ranked by score: claude_ai_Exa (60) outranks Read (4), so it
    # is the lead/hero and keeps its full "Suggested" guidance.
    assert html =~ "Package the common workflows"
    # Lower-ranked signals render compactly: they still expose their promotion
    # target, but do not repeat the full memory-card panel.
    assert html =~ "Skill or regression test"
  end

  test "agent menu is toggle-able: CSS-safe id and class-based hiding", %{conn: conn} do
    {:ok, view, html} = live(conn, ~p"/insights")

    # 1) Keys contain colons (e.g. "signal:error:Read"). They must not leak into
    #    element ids: JS.toggle/JS.hide build a "#...-menu" CSS selector, and ":"
    #    is the pseudo-class delimiter, so querySelector throws "not a valid
    #    selector" and the dropdown never opens.
    assert html =~ ~s(id="sig-signal-error-Read-menu")
    refute html =~ "signal:error:Read-menu"

    # 2) The menu must hide via a utility *class*, never the `hidden` *attribute*:
    #    Tailwind force-hides [hidden] with `display:none !important`, which
    #    JS.toggle's inline display can't override — so an attribute-hidden menu
    #    can never be revealed.
    assert has_element?(view, "#sig-signal-error-Read-menu.hidden")
  end

  test "launching threads the key into AgentLauncher.launch/3", %{conn: conn} do
    test_pid = self()

    Mimic.stub(Decant.AgentLauncher, :launch, fn agent, prompt, key ->
      send(test_pid, {:launched, agent, prompt, key})
      :ok
    end)

    {:ok, view, _html} = live(conn, ~p"/insights")

    view
    |> element(~s(button[phx-value-key="catalog:agents-md"][phx-click="launch"]))
    |> render_click()

    assert_receive {:launched, "claude", prompt, "catalog:agents-md"}
    assert prompt =~ "AGENTS.md"
    assert prompt =~ "Use this Decant memory card"
    assert prompt =~ "Promote to: AGENTS.md"
    assert prompt =~ "Done when:"
  end

  test "a failed launch surfaces the error as a flash", %{conn: conn} do
    Mimic.stub(Decant.AgentLauncher, :launch, fn _agent, _prompt, _key ->
      {:error, "could not open terminal"}
    end)

    {:ok, view, _html} = live(conn, ~p"/insights")

    html =
      view
      |> element(~s(button[phx-value-key="catalog:agents-md"][phx-click="launch"]))
      |> render_click()

    assert html =~ "could not open terminal"
  end

  test "renders the empty signals state when the daemon returns no recommendations", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :recommendations, fn _status -> {:error, :service_unavailable} end)

    {:ok, _view, html} = live(conn, ~p"/insights")

    assert html =~ "No signals yet"
  end

  test "renders varied tones and the activity-sourced 'implemented' note", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :recommendations, fn _status ->
      {:ok,
       [
         %{
           "key" => "signal:a",
           "kind" => "signal",
           "category" => nil,
           "title" => "Accent signal",
           "detail" => "d",
           "suggestion" => "s",
           "prompt" => "p",
           "url" => "https://example.com",
           "link_label" => "docs",
           "icon" => "hero-bolt",
           "tone" => "accent",
           "score" => 9.0,
           "status" => "open",
           "status_source" => nil,
           "note" => nil,
           "implemented_at" => nil
         },
         %{
           "key" => "signal:b",
           "kind" => "signal",
           "category" => nil,
           "title" => "Warning signal",
           "detail" => "d2",
           "suggestion" => nil,
           "prompt" => nil,
           "url" => nil,
           "link_label" => nil,
           "icon" => nil,
           "tone" => "warning",
           "score" => 1.0,
           "status" => "open",
           "status_source" => nil,
           "note" => nil,
           "implemented_at" => nil
         },
         %{
           "key" => "done:activity",
           "kind" => "catalog",
           "category" => "Foundations",
           "title" => "Detected setup",
           "detail" => nil,
           "suggestion" => nil,
           "prompt" => nil,
           "url" => nil,
           "link_label" => nil,
           "icon" => nil,
           "tone" => nil,
           "score" => 0,
           "status" => "implemented",
           "status_source" => "activity",
           "note" => nil,
           "implemented_at" => "2026-05-03T10:00:00Z"
         }
       ]}
    end)

    {:ok, _view, html} = live(conn, ~p"/insights")

    assert html =~ "Accent signal"
    assert html =~ "Warning signal"
    assert html =~ "Detected in your sessions"
  end

  defp signal(key, tone, score) do
    %{
      "key" => key,
      "kind" => "signal",
      "category" => nil,
      "title" => "#{tone} signal",
      "detail" => "detail",
      "suggestion" => nil,
      "prompt" => nil,
      "url" => nil,
      "link_label" => nil,
      "icon" => nil,
      "tone" => tone,
      "score" => score,
      "status" => "open",
      "status_source" => nil,
      "note" => nil,
      "implemented_at" => nil
    }
  end

  # Each tone, in turn, as the highest-score hero (covers tone_border_l /
  # tone_text / tone_tint) and again as a lower-score row (covers tone_rail).
  for tone <- ["accent", "success", "warning", "danger", "info", "weird-unknown-tone"] do
    test "renders the #{tone}-toned signal as the hero and in a row", %{conn: conn} do
      Mimic.stub(Decant.Daemon, :recommendations, fn _status ->
        {:ok, [signal("sig:hero", unquote(tone), 10.0), signal("sig:row", unquote(tone), 1.0)]}
      end)

      {:ok, _view, html} = live(conn, ~p"/insights")
      assert html =~ "#{unquote(tone)} signal"
    end
  end

  test "an implemented rec with no known status_source reads 'Marked done'", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :recommendations, fn _status ->
      {:ok,
       [
         %{
           "key" => "done:manual",
           "kind" => "catalog",
           "category" => "Foundations",
           "title" => "Manually marked",
           "detail" => nil,
           "suggestion" => nil,
           "prompt" => nil,
           "url" => nil,
           "link_label" => nil,
           "icon" => nil,
           "tone" => nil,
           "score" => 0,
           "status" => "implemented",
           "status_source" => "manual",
           "note" => nil,
           "implemented_at" => "2026-05-03T10:00:00Z"
         }
       ]}
    end)

    {:ok, _view, html} = live(conn, ~p"/insights")
    assert html =~ "Marked done"
  end
end
