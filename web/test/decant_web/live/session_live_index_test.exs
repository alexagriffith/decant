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

    assert html =~ "Claude"
    assert html =~ "Codex"
    assert html =~ "claude-opus-4-7"
    assert html =~ "gpt-5.4"
  end

  test "renders no manual sync control — the daemon keeps the archive live", %{conn: conn} do
    {:ok, view, html} = live(conn, ~p"/")

    refute has_element?(view, ~s{button[phx-click="sync"]})
    assert html =~ "Live and auto-syncing"
  end

  test "links each session row to its detail page", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    assert has_element?(view, ~s{a[href="/sessions/1"]})
    assert has_element?(view, ~s{a[href="/sessions/2"]})
  end
end
