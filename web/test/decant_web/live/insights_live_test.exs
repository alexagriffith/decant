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

  test "renders the Signals, Recommended, and Implemented sections", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/insights")

    assert html =~ "Signals"
    assert html =~ "Read fails 20% of the time"
    assert html =~ "Heavy reliance on the claude_ai_Exa MCP server"

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
    # Lower-ranked signals render compactly — the repeated suggestion blockquote
    # (the main source of visual noise) is dropped from them.
    refute html =~ "Codify the recovery path"
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
  end
end
