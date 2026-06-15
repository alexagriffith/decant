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
    assert html =~ "1 result"
    assert has_element?(view, ~s{a[href="/sessions/1"]})
    refute html =~ "List the open TODOs"
  end

  test "loads the next page of search results", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :search, fn "auth", opts ->
      cursor = opts |> Enum.into(%{}) |> Map.get(:cursor)

      case cursor do
        nil -> {:ok, [hit_payload(1, "First hit")], page_meta(true, "next", nil)}
        "next" -> {:ok, [hit_payload(2, "Second hit")], page_meta(false, nil, nil)}
      end
    end)

    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => "auth"})
      |> render_change()

    assert html =~ "First hit"
    refute html =~ "Second hit"
    assert html =~ "Showing 1 results; more available"

    html = view |> element(~s{button[phx-click="load_more"]}) |> render_click()

    assert html =~ "First hit"
    assert html =~ "Second hit"
    assert html =~ "2 results"
    refute has_element?(view, ~s{button[phx-click="load_more"]})
  end

  test "load_more without a search cursor is a no-op", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search?q=auth")

    html = render_click(view, "load_more")

    assert html =~ "Fix the failing auth test"
    assert html =~ "1 result"
  end

  test "search caption uses daemon total count when available", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :search, fn _q, _opts ->
      {:ok, [hit_payload(1, "Counted hit")], page_meta(true, "next", 40)}
    end)

    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => "auth"})
      |> render_change()

    assert html =~ "Counted hit"
    assert html =~ "Showing 1 of 40 results"
  end

  test "searching 'TODO' surfaces the other session", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search")

    html =
      view
      |> form("form", %{"q" => "TODO"})
      |> render_change()

    assert html =~ "List the open TODOs"
    assert html =~ "1 result"
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

  test "searching a blank-padded query resets to the empty state", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/search?q=auth")

    html =
      view
      |> form("form", %{"q" => "   "})
      |> render_change()

    assert html =~ "Search your archive"
    refute html =~ "Fix the failing auth test"
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

  defp hit_payload(id, title) do
    %{
      "session_id" => id,
      "session_title" => title,
      "tool" => "codex",
      "snippet" => "[auth] result"
    }
  end

  defp page_meta(has_more, next_cursor, total_count) do
    %{
      "pagination" => %{
        "has_more" => has_more,
        "next_cursor" => next_cursor,
        "page_size" => 25,
        "total_count" => total_count
      }
    }
  end
end
