defmodule Decant.SettingsTest do
  @moduledoc """
  Tests the user-preference store. It persists JSON under `DECANT_CONFIG_DIR`,
  which we point at a per-test temp dir, and infers defaults from the
  environment and the archive (the latter via the mocked daemon).
  """
  use ExUnit.Case, async: false
  use Mimic
  import Bitwise

  alias Decant.Settings

  setup do
    dir = Path.join(System.tmp_dir!(), "decant-settings-#{System.unique_integer([:positive])}")
    File.mkdir_p!(dir)
    prev = System.get_env("DECANT_CONFIG_DIR")
    System.put_env("DECANT_CONFIG_DIR", dir)

    on_exit(fn ->
      File.rm_rf!(dir)

      if prev,
        do: System.put_env("DECANT_CONFIG_DIR", prev),
        else: System.delete_env("DECANT_CONFIG_DIR")
    end)

    %{dir: dir}
  end

  describe "path/0" do
    test "honors DECANT_CONFIG_DIR", %{dir: dir} do
      assert Settings.path() == Path.expand(Path.join(dir, "settings.json"))
    end

    test "falls back to ~/.config/decant when unset" do
      System.delete_env("DECANT_CONFIG_DIR")
      assert Settings.path() =~ ".config/decant/settings.json"
    end
  end

  describe "put/1 and value/2" do
    test "persists valid prefs and ignores unknown keys/values" do
      merged =
        Settings.put(%{agent: "codex", terminal: "iterm", ide: "zed", bogus: "x", agent2: 1})

      assert merged == %{agent: "codex", terminal: "iterm", ide: "zed"}
      assert Settings.value(:agent, "claude") == "codex"
      assert Settings.value(:terminal, "terminal") == "iterm"
      assert Settings.value(:ide, "vscode") == "zed"
    end

    test "rejects out-of-whitelist values" do
      Settings.put(%{agent: "rogue-agent", terminal: "nope"})
      assert Settings.value(:agent, "claude") == "claude"
      assert Settings.value(:terminal, "terminal") == "terminal"
    end

    test "value/2 falls back to the default when unset" do
      assert Settings.value(:ide, "vscode") == "vscode"
    end

    test "writes the file with 0600 perms", %{dir: _dir} do
      Settings.put(%{agent: "claude"})
      stat = File.stat!(Settings.path())
      assert (stat.mode &&& 0o777) == 0o600
    end
  end

  describe "load via get/0" do
    test "merges detected defaults with saved choices" do
      Mimic.stub(Decant.Archive, :by_dimension, fn :tool -> [%{key: "claude_code"}] end)
      Settings.put(%{ide: "cursor"})

      s = Settings.get()
      assert s.ide == "cursor"
      assert s.agent in ~w(claude codex)
      assert s.terminal in ~w(terminal iterm ghostty wezterm kitty alacritty)
    end

    test "ignores a malformed (non-map) settings file" do
      File.write!(Settings.path(), "[]")
      assert Settings.value(:agent, "claude") == "claude"
    end

    test "ignores invalid JSON" do
      File.write!(Settings.path(), "{not json")
      assert Settings.value(:agent, "claude") == "claude"
    end

    test "loads known keys and drops unknown ones from a saved file" do
      File.write!(
        Settings.path(),
        Jason.encode!(%{"agent" => "codex", "wat" => "x", "ide" => "bogus-ide"})
      )

      # agent is valid → kept; the unknown "wat" key and invalid ide are dropped.
      assert Settings.value(:agent, "claude") == "codex"
      assert Settings.value(:ide, "vscode") == "vscode"
    end
  end

  describe "detected/0" do
    test "infers the agent from the archive's dominant tool (codex)" do
      Mimic.stub(Decant.Archive, :by_dimension, fn :tool -> [%{key: "codex"}] end)
      assert Settings.detected().agent == "codex"
    end

    test "infers claude when the archive leads with claude" do
      Mimic.stub(Decant.Archive, :by_dimension, fn :tool -> [%{key: "claude_code"}] end)
      assert Settings.detected().agent == "claude"
    end

    test "falls back to claude when the archive lookup raises" do
      Mimic.stub(Decant.Archive, :by_dimension, fn :tool -> raise "down" end)
      assert Settings.detected().agent == "claude"
    end

    test "detects the terminal from TERM_PROGRAM/TERM" do
      cases = [
        {"iTerm.app", nil, "iterm"},
        {"ghostty", nil, "ghostty"},
        {"WezTerm", nil, "wezterm"},
        {"Apple_Terminal", nil, "terminal"},
        {"unknown", "xterm-kitty", "kitty"},
        {"unknown", "xterm", "terminal"}
      ]

      prev_program = System.get_env("TERM_PROGRAM")
      prev_term = System.get_env("TERM")

      on_exit(fn ->
        restore("TERM_PROGRAM", prev_program)
        restore("TERM", prev_term)
      end)

      for {program, term, expected} <- cases do
        System.put_env("TERM_PROGRAM", program)
        if term, do: System.put_env("TERM", term), else: System.delete_env("TERM")
        assert Settings.detected().terminal == expected
      end
    end
  end

  defp restore(key, nil), do: System.delete_env(key)
  defp restore(key, val), do: System.put_env(key, val)
end
