defmodule DecantWeb.SessionLive.SearchTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  test "renders the empty search form", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/search")

    assert html =~ "Search your archive"
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

  test "a non-matching query shows the 'No matches' empty state", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => "zzzznomatch"})
      |> render_change()

    assert html =~ "No matches"
  end

  test "a deep link with ?q= pre-runs the search", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/search?q=auth")
    assert html =~ "Fix the failing auth test"
  end

  test "a raising search degrades to no results", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :search, fn _q, _opts -> raise "bad FTS syntax" end)

    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => "auth"})
      |> render_change()

    assert html =~ "No matches"
  end
end
