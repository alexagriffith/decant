defmodule DecantWeb.SessionLive.IndexTest do
  # Global Mimic mode (so the daemon stub is visible from the LiveView process)
  # requires async: false.
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  test "lists all sessions from the archive", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/")

    assert html =~ "Sessions"
    assert html =~ "Fix the failing auth test"
    assert html =~ "List the open TODOs"

    # Tool badges + model badges render.
    assert html =~ "Claude"
    assert html =~ "Codex"
    assert html =~ "claude-opus-4-7"
    assert html =~ "gpt-5.4"
  end

  test "renders the refresh control without shelling out", %{conn: conn} do
    {:ok, view, html} = live(conn, ~p"/")

    # The control is present; clicking it now just re-fetches the page.
    assert html =~ "Sync"
    assert has_element?(view, "button[phx-click=\"sync\"]")
  end

  test "the sync button re-fetches the page and flashes (no shelling out)", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html =
      view
      |> element("button[phx-click=\"sync\"]")
      |> render_click()

    assert html =~ "Refreshed."
    # Data is still present after the refresh.
    assert html =~ "Fix the failing auth test"
  end

  test "links each session row to its detail page", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    assert has_element?(view, ~s{a[href="/sessions/1"]})
    assert has_element?(view, ~s{a[href="/sessions/2"]})
  end
end
