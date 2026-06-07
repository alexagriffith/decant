defmodule DecantWeb.AnalyticsLiveTest do
  use DecantWeb.ConnCase, async: true

  import Phoenix.LiveViewTest

  test "renders totals cards", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/analytics")

    assert html =~ "analytics"
    assert html =~ "sessions"
    assert html =~ "messages"
    assert html =~ "tool calls"
    assert html =~ "est. cost"
  end

  test "renders the by-model table with both fixture models and a cost", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/analytics")

    assert html =~ "by model"
    assert html =~ "claude-opus-4-7"
    assert html =~ "gpt-5.4"

    # Cost is rendered with a dollar sign and 2 decimals (best-effort, not the SVG).
    assert html =~ "$"
  end
end
