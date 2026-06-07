defmodule DecantWeb.SessionLive.IndexTest do
  use DecantWeb.ConnCase, async: true

  import Phoenix.LiveViewTest

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

  test "renders the sync control without triggering it", %{conn: conn} do
    {:ok, view, html} = live(conn, ~p"/")

    # The control is present but we must NOT click it (it would shell out).
    assert html =~ "Sync"
    assert has_element?(view, "button[phx-click=\"sync\"]")
  end

  test "links each session row to its detail page", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    assert has_element?(view, ~s{a[href="/sessions/1"]})
    assert has_element?(view, ~s{a[href="/sessions/2"]})
  end
end
