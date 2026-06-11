defmodule DecantWeb.ToolsLiveTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()
    :ok
  end

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

  test "renders MCP rows (with errors badge) when servers are present", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :mcp_usage, fn _opts ->
      {:ok,
       [
         %{"mcp_server" => "github", "tools" => 4, "calls" => 30, "errors" => 2},
         %{"mcp_server" => "linear", "tools" => 2, "calls" => 5, "errors" => 0}
       ], %{}}
    end)

    {:ok, _view, html} = live(conn, ~p"/tools")

    assert html =~ "github"
    assert html =~ "linear"
  end

  test "renders an MCP tool row with its server icon", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :tools_usage, fn _opts ->
      {:ok,
       [
         %{
           "tool_name" => "search",
           "tool_kind" => "mcp",
           "mcp_server" => "github",
           "calls" => 9,
           "errors" => 1,
           "error_rate" => 0.1
         }
       ], %{}}
    end)

    {:ok, _view, html} = live(conn, ~p"/tools")
    assert html =~ "search"
    assert html =~ "github"
    assert html =~ "MCP"
  end

  test "clicking a tools-table header toggles the sort", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/tools")

    html =
      view
      |> element(~s(button[phx-value-table="tools"][phx-value-col="calls"]))
      |> render_click()

    assert html =~ "Read"
  end

  test "clicking an mcp-table header toggles the sort", %{conn: conn} do
    Mimic.stub(Decant.Daemon, :mcp_usage, fn _opts ->
      {:ok, [%{"mcp_server" => "github", "tools" => 1, "calls" => 3, "errors" => 0}], %{}}
    end)

    {:ok, view, _html} = live(conn, ~p"/tools")

    html =
      view
      |> element(~s(button[phx-value-table="mcp"][phx-value-col="calls"]))
      |> render_click()

    assert html =~ "github"
  end
end
