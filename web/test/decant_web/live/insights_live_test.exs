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

    # Signals section (open kind=="signal").
    assert html =~ "Signals"
    assert html =~ "Read fails 20% of the time"
    assert html =~ "Heavy reliance on the claude_ai_Exa MCP server"

    # Recommended bento (open kind=="catalog"), grouped by category.
    assert html =~ "Recommended for coding agents"
    assert html =~ "Foundations"
    assert html =~ "Reusable workflows"
    assert html =~ "AGENTS.md at the repo root"

    # Implemented section (status=="implemented"), quiet, no CTA for it.
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
