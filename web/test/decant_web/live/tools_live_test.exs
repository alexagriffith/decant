defmodule DecantWeb.ToolsLiveTest do
  use DecantWeb.ConnCase, async: true

  import Phoenix.LiveViewTest

  test "renders the tool usage table with the fixture's built-in tools", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/tools")

    assert html =~ "Tools"
    assert html =~ "Read"
    assert html =~ "exec_command"
    assert html =~ "built-in"
  end

  test "renders the MCP servers section even with no MCP usage", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/tools")

    # The fixture has no MCP tool calls, but the section header still renders.
    assert html =~ "MCP servers"
  end
end
