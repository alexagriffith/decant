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
end
