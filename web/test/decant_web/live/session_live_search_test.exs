defmodule DecantWeb.SessionLive.SearchTest do
  use DecantWeb.ConnCase, async: true

  import Phoenix.LiveViewTest

  test "renders the empty search form", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/search")

    assert html =~ "full-text search across all sessions"
    # No results before a query is entered.
    refute html =~ "Fix the failing auth test"
  end

  test "searching 'auth' surfaces the matching session", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search")

    # The form fires phx-change="search" with the `q` param.
    html =
      view
      |> form("form", %{"q" => "auth"})
      |> render_change()

    assert html =~ "Fix the failing auth test"
    assert has_element?(view, ~s{a[href="/sessions/1"]})
    refute html =~ "List the open TODOs"
  end

  test "searching 'TODO' surfaces the other session", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => "TODO"})
      |> render_change()

    assert html =~ "List the open TODOs"
    assert has_element?(view, ~s{a[href="/sessions/2"]})
  end

  test "an empty query yields no results", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => ""})
      |> render_change()

    refute html =~ "Fix the failing auth test"
    refute html =~ "List the open TODOs"
  end
end
