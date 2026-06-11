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

  test "the in-page filter narrows the streamed rows by title/model/tool", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html =
      view
      |> form("form[phx-change=\"filter\"]", %{"q" => "auth"})
      |> render_change()

    assert html =~ "Fix the failing auth test"
    refute html =~ "List the open TODOs"
  end

  test "clicking a column header re-sorts the rows", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html =
      view
      |> element(~s(button[phx-value-col="title"]))
      |> render_click()

    # Both rows still render after the sort toggle.
    assert html =~ "Fix the failing auth test"
    assert html =~ "List the open TODOs"
  end

  test "an archive_updated PubSub broadcast live-reloads the page", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    # SyncHook subscribes connected LiveViews to the "archive" topic and
    # push_patches the current page on each archive_updated broadcast.
    Phoenix.PubSub.broadcast(Decant.PubSub, "archive", {:archive_updated, "1 ingested"})

    html = render(view)
    assert html =~ "Fix the failing auth test"
  end

  test "an unrelated info message is ignored by the sync hook", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")
    send(view.pid, :some_other_info)
    assert render(view) =~ "Sessions"
  end

  test "the sidebar tolerates a malformed last-activity date string", %{conn: conn} do
    # The sidebar's freshness comes from Archive.overview/0; a malformed date
    # there must fall back to the raw string, not crash the layout.
    Mimic.stub(Decant.Archive, :overview, fn ->
      %{sessions: 2, cost: 0.6, last_activity: "not-a-real-date"}
    end)

    {:ok, _view, html} = live(conn, ~p"/")
    assert html =~ "not-a-real-date"
  end

  test "the sidebar tolerates a non-string last-activity value", %{conn: conn} do
    Mimic.stub(Decant.Archive, :overview, fn ->
      %{sessions: 2, cost: 0.6, last_activity: 20_260_501}
    end)

    {:ok, _view, html} = live(conn, ~p"/")
    assert html =~ "20260501"
  end
end
