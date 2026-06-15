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
    assert html =~ "Showing 2 of 2 sessions"
  end

  test "local table filter makes the loaded-row scope explicit", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html =
      view
      |> form("form", %{"q" => "auth"})
      |> render_change()

    assert html =~ "Fix the failing auth test"
    refute html =~ "List the open TODOs"
    assert html =~ "Showing 1 matching loaded row from 2 loaded sessions"
  end

  test "local table filter pluralizes the loaded-row scope", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html =
      view
      |> form("form", %{"q" => "t"})
      |> render_change()

    assert html =~ "Showing 2 matching loaded rows from 2 loaded sessions"
  end

  test "local-only table sort makes the loaded-row scope explicit", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html =
      view
      |> element(~s{button[phx-value-col="title"]})
      |> render_click()

    assert html =~ "Showing 2 loaded sessions sorted locally of 2 total"
  end

  test "server-supported table sorts keep the global-session scope", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    started_html =
      view
      |> element(~s{button[phx-value-col="started_at"]})
      |> render_click()

    assert started_html =~ "Showing 2 of 2 sessions"

    cost_html =
      view
      |> element(~s{button[phx-value-col="cost"]})
      |> render_click()

    assert cost_html =~ "Showing 2 of 2 sessions"
  end

  test "loads the next sessions page", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :list_sessions, fn opts ->
      cursor = opts |> Enum.into(%{}) |> Map.get(:cursor)

      case cursor do
        nil -> {:ok, [session_payload(1, "First page")], page_meta(true, "next", 2)}
        "next" -> {:ok, [session_payload(2, "Second page")], page_meta(false, nil, 2)}
      end
    end)

    {:ok, view, html} = live(conn, ~p"/")

    assert html =~ "First page"
    refute html =~ "Second page"
    assert html =~ "Showing 1 of 2 sessions"

    html = view |> element(~s{button[phx-click="load_more"]}) |> render_click()

    assert html =~ "First page"
    assert html =~ "Second page"
    assert html =~ "Showing 2 of 2 sessions"
    refute has_element?(view, ~s{button[phx-click="load_more"]})
  end

  test "load_more without a cursor is a no-op", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/")

    html = render_click(view, "load_more")

    assert html =~ "Fix the failing auth test"
    assert html =~ "Showing 2 of 2 sessions"
  end

  test "caption handles daemon pagination without a total count", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :list_sessions, fn _opts ->
      {:ok, [session_payload(1, "Untotaled page")], %{"pagination" => %{"has_more" => false}}}
    end)

    {:ok, view, html} = live(conn, ~p"/")

    assert html =~ "Untotaled page"
    assert html =~ "Showing 1 sessions"

    html =
      view
      |> element(~s{button[phx-value-col="title"]})
      |> render_click()

    assert html =~ "Showing 1 loaded sessions sorted locally"
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

  defp session_payload(id, title) do
    %{
      "id" => id,
      "tool" => "codex",
      "source_session_id" => "sess-#{id}",
      "title" => title,
      "model" => "gpt-5.4",
      "project" => "/Users/dev/proj",
      "started_at" => "2026-05-0#{id}T09:00:00Z",
      "ended_at" => "2026-05-0#{id}T09:05:00Z",
      "message_count" => 4,
      "estimated_cost_usd" => 0.1
    }
  end

  defp page_meta(has_more, next_cursor, total_count) do
    %{
      "pagination" => %{
        "has_more" => has_more,
        "next_cursor" => next_cursor,
        "page_size" => 100,
        "total_count" => total_count
      }
    }
  end
end
