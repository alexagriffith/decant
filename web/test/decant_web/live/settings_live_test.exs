defmodule DecantWeb.SettingsLiveTest do
  use DecantWeb.ConnCase, async: false
  use Mimic

  import Phoenix.LiveViewTest

  setup :set_mimic_global

  setup do
    Decant.DaemonStubs.install()

    dir =
      Path.join(System.tmp_dir!(), "decant-settings-live-#{System.unique_integer([:positive])}")

    File.mkdir_p!(dir)
    prev = System.get_env("DECANT_CONFIG_DIR")
    System.put_env("DECANT_CONFIG_DIR", dir)

    on_exit(fn ->
      File.rm_rf!(dir)

      if prev,
        do: System.put_env("DECANT_CONFIG_DIR", prev),
        else: System.delete_env("DECANT_CONFIG_DIR")
    end)

    :ok
  end

  test "renders the settings form with agent/terminal/editor selectors", %{conn: conn} do
    {:ok, _view, html} = live(conn, ~p"/settings")

    assert html =~ "Settings"
    assert html =~ "Preferred agent"
    assert html =~ "Terminal"
    assert html =~ "Editor"
    # Option labels from AgentLauncher.
    assert html =~ "Claude"
    assert html =~ "VS Code"
  end

  test "saving a preference persists it and flashes", %{conn: conn} do
    {:ok, view, _html} = live(conn, ~p"/settings")

    html =
      view
      |> form("form[phx-change=\"save\"]", %{
        "agent" => "codex",
        "terminal" => "iterm",
        "ide" => "zed"
      })
      |> render_change()

    assert html =~ "Settings saved."
    assert Decant.Settings.value(:agent, "claude") == "codex"
    assert Decant.Settings.value(:terminal, "terminal") == "iterm"
    assert Decant.Settings.value(:ide, "vscode") == "zed"
  end

  test "shows the macOS-only hint when launching is unavailable", %{conn: conn} do
    Mimic.stub(Decant.AgentLauncher, :can_launch?, fn -> false end)

    {:ok, _view, html} = live(conn, ~p"/settings")

    assert html =~ "macOS only"
  end
end
