defmodule DecantWeb.AnalyticsLiveTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  test "renders totals cards", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/analytics")

    assert html =~ "Analytics"
    assert html =~ "Sessions"
    assert html =~ "Messages"
    assert html =~ "Tool calls"
    assert html =~ "Est. cost"
  end

  test "renders the by-model table with both fixture models and a cost", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/analytics")

    assert html =~ "By model"
    assert html =~ "claude-opus-4-7"
    assert html =~ "gpt-5.4"

    # Cost is rendered with a dollar sign and 2 decimals (best-effort, not the SVG).
    assert html =~ "$"
  end

  # Shared stub for the project worktree tests.
  defp stub_worktree_project(_context) do
    Mimic.stub(Decant.Daemon, :by_dimension, fn
      :project, opts ->
        if Keyword.has_key?(opts, :root) do
          {:ok,
           [
             %{
               "key" => "/home/x/dosu/dosu",
               "sessions" => 3,
               "input_tokens" => 0,
               "output_tokens" => 0,
               "estimated_cost_usd" => 1.0,
               "worktree_label" => nil,
               "worktree_tool" => nil
             },
             %{
               "key" => "/home/x/.warp-worktrees/dosu-agate-spire",
               "sessions" => 2,
               "input_tokens" => 0,
               "output_tokens" => 0,
               "estimated_cost_usd" => 2.0,
               "worktree_label" => "agate-spire",
               "worktree_tool" => "warp"
             }
           ], %{}}
        else
          {:ok,
           [
             %{
               "key" => "/home/x/dosu/dosu",
               "sessions" => 5,
               "input_tokens" => 0,
               "output_tokens" => 0,
               "estimated_cost_usd" => 3.0,
               "worktree_count" => 1
             }
           ], %{}}
        end

      _dim, _opts ->
        {:ok, [], %{}}
    end)

    :ok
  end

  test "rolls projects up by root and expands to per-worktree rows", %{conn: conn} do
    stub_worktree_project(%{})

    {:ok, view, html} = live(conn, ~p"/analytics")

    assert html =~ "By project"
    assert html =~ "dosu"
    assert html =~ "1 wt"
    refute html =~ "agate-spire"

    # Use element form to verify a bound phx-click element exists.
    html = view |> element("button[phx-click*='toggle_project']") |> render_click()
    assert html =~ "agate-spire"
    assert html =~ "warp"
    # Disclosure arrow should flip to ▾ when expanded.
    assert html =~ "▾"
    # The root checkout's own leaf row reads "root" — it is not a worktree.
    assert html =~ ~r/>\s*root\s*</
    refute html =~ "wt: dosu"
  end

  test "collapses per-worktree rows on second click", %{conn: conn} do
    stub_worktree_project(%{})

    {:ok, view, _html} = live(conn, ~p"/analytics")

    # Expand.
    view |> element("button[phx-click*='toggle_project']") |> render_click()
    # Collapse.
    html = view |> element("button[phx-click*='toggle_project']") |> render_click()
    refute html =~ "agate-spire"
  end

  test "resets expansion state when filter changes via patch", %{conn: conn} do
    stub_worktree_project(%{})

    {:ok, view, _html} = live(conn, ~p"/analytics")

    # Expand the root row.
    view |> element("button[phx-click*='toggle_project']") |> render_click()

    # Apply a date-range filter patch (simulates clicking a date preset chip).
    html = render_patch(view, ~p"/analytics?from=2026-01-01")

    # Sub-rows must be gone after the filter change.
    refute html =~ "agate-spire"
  end
end
