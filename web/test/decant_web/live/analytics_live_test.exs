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

  test "rolls projects up by root and expands to per-worktree rows", %{conn: conn} do
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

    {:ok, view, html} = live(conn, ~p"/analytics")

    assert html =~ "By project"
    assert html =~ "dosu"
    assert html =~ "1 wt"
    refute html =~ "agate-spire"

    html = render_click(view, "toggle_project", %{"key" => "/home/x/dosu/dosu"})
    assert html =~ "agate-spire"
    assert html =~ "warp"
  end
end
