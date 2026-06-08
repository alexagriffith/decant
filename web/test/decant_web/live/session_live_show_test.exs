defmodule DecantWeb.SessionLive.ShowTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

  test "shows a session's summary and message/block content", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/sessions/1")

    assert html =~ "Fix the failing auth test"

    # Summary line metadata (tool badge + model badge).
    assert html =~ "Claude"
    assert html =~ "claude-opus-4-7"

    # Block text from the conversation is rendered.
    assert html =~ "auth"
  end

  test "redirects to the index for a non-existent session id", %{conn: conn} do
    # Show.mount/3 handles a nil session by flashing an error and
    # push_navigate-ing back to "/", which surfaces as a live_redirect.
    assert {:error, {:live_redirect, %{to: "/"}}} = live(conn, ~p"/sessions/999999")
  end
end
