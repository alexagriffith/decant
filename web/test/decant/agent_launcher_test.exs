defmodule Decant.AgentLauncherTest do
  @moduledoc """
  Tests the OS-launch helper. The terminal/IDE-spawning paths are macOS-only
  (guarded by `can_launch?/0`); on every other platform `launch/3` and
  `open_ide/1` short-circuit to a friendly `{:error, _}`. We cover the option
  lists, label/command builders, the mark-implemented handoff, and both
  not-supported branches.
  """
  use ExUnit.Case, async: false
  use Mimic

  alias Decant.AgentLauncher

  describe "option lists" do
    test "agents/0 lists claude and codex with labels" do
      agents = AgentLauncher.agents()
      assert {"claude", "Claude"} in agents
      assert {"codex", "Codex"} in agents
    end

    test "terminals/0 is the stable ordered list" do
      assert [{"terminal", "Terminal"} | _] = AgentLauncher.terminals()
      assert length(AgentLauncher.terminals()) == 6
    end

    test "ides/0 lists the supported editors" do
      ides = AgentLauncher.ides()
      assert {"vscode", "VS Code"} in ides
      assert {"zed", "Zed"} in ides
    end
  end

  describe "default_agent/0 and ide_label/1" do
    test "default_agent reads the saved preference" do
      Mimic.stub(Decant.Settings, :value, fn :agent, "claude" -> "codex" end)
      assert AgentLauncher.default_agent() == "codex"
    end

    test "ide_label returns the editor label, or a generic fallback" do
      assert AgentLauncher.ide_label("vscode") == "VS Code"
      assert AgentLauncher.ide_label("zed") == "Zed"
      assert AgentLauncher.ide_label("unknown") == "IDE"
    end
  end

  describe "command/2" do
    test "builds a shell command for a known agent, quoting the prompt" do
      assert AgentLauncher.command("claude", "do a thing") == "claude 'do a thing'"
    end

    test "escapes single quotes in the prompt" do
      assert AgentLauncher.command("codex", "it's me") == "codex 'it'\\''s me'"
    end

    test "returns nil for an unknown agent" do
      assert AgentLauncher.command("rogue", "x") == nil
    end
  end

  describe "launch/2,3 (launch unavailable)" do
    setup do
      Application.put_env(:decant, :can_launch, false)
      on_exit(fn -> Application.delete_env(:decant, :can_launch) end)
      :ok
    end

    test "an unknown agent is rejected before any OS check" do
      assert {:error, "Unknown agent."} = AgentLauncher.launch("rogue", "prompt")
    end

    test "launch/2 reports the macOS-only limitation" do
      assert {:error, msg} = AgentLauncher.launch("claude", "prompt")
      assert msg =~ "macOS"
    end

    test "launch/3 with a key behaves like launch/2" do
      assert {:error, msg} = AgentLauncher.launch("claude", "prompt", "signal:error:Read")
      assert msg =~ "macOS"
    end
  end

  describe "open_ide/1 (launch unavailable)" do
    setup do
      Application.put_env(:decant, :can_launch, false)
      on_exit(fn -> Application.delete_env(:decant, :can_launch) end)
      :ok
    end

    test "reports the macOS-only limitation" do
      assert {:error, msg} = AgentLauncher.open_ide("/tmp")
      assert msg =~ "macOS"
    end
  end

  describe "can_launch?/0" do
    test "reflects the host platform when no override is set" do
      assert AgentLauncher.can_launch?() == match?({:unix, :darwin}, :os.type())
    end

    test "honors the :can_launch application-env override" do
      Application.put_env(:decant, :can_launch, true)
      on_exit(fn -> Application.delete_env(:decant, :can_launch) end)
      assert AgentLauncher.can_launch?() == true
    end
  end

  # Exercise the macOS-only spawn paths on any host by forcing `can_launch?` true
  # (the `:can_launch` env override) and mocking the actual process spawn
  # (`System.cmd/3`). Each terminal preference routes through a different
  # `launch_in/2` clause; we drive them all.
  describe "spawn paths (forced can_launch)" do
    setup do
      Application.put_env(:decant, :can_launch, true)
      on_exit(fn -> Application.delete_env(:decant, :can_launch) end)
      :ok
    end

    for {pref, expected_bin} <- [
          {"terminal", "osascript"},
          {"iterm", "osascript"},
          {"ghostty", "open"},
          {"alacritty", "open"},
          {"kitty", "open"},
          {"wezterm", "open"}
        ] do
      test "launch/2 spawns via #{pref} (#{expected_bin})" do
        Mimic.stub(Decant.Settings, :value, fn :terminal, "terminal" -> unquote(pref) end)
        test_pid = self()

        Mimic.stub(System, :cmd, fn bin, _args, _opts ->
          send(test_pid, {:cmd, bin})
          {"", 0}
        end)

        assert AgentLauncher.launch("claude", "do a thing") == :ok
        assert_received {:cmd, unquote(expected_bin)}
      end
    end

    test "launch/3 threads the mark-implemented handoff through the spawned command" do
      Mimic.stub(Decant.Settings, :value, fn :terminal, "terminal" -> "terminal" end)
      test_pid = self()

      Mimic.stub(System, :cmd, fn _bin, args, _opts ->
        send(test_pid, {:args, args})
        {"", 0}
      end)

      assert AgentLauncher.launch("claude", "prompt", "signal:error:Read") == :ok
      assert_received {:args, args}
      # The prompt (plus handoff line) is written to a temp file the subshell
      # cats; the AppleScript therefore carries a `cat <tmp>` read-back.
      assert Enum.any?(args, &(is_binary(&1) and &1 =~ "cat "))
    end

    test "a non-zero exit surfaces the trimmed command output as an error" do
      Mimic.stub(Decant.Settings, :value, fn :terminal, "terminal" -> "terminal" end)
      Mimic.stub(System, :cmd, fn _bin, _args, _opts -> {"  it failed\n", 1} end)

      assert {:error, "it failed"} = AgentLauncher.launch("claude", "prompt")
    end

    test "a raising spawn is caught and surfaced as an error message" do
      Mimic.stub(Decant.Settings, :value, fn :terminal, "terminal" -> "terminal" end)
      Mimic.stub(System, :cmd, fn _bin, _args, _opts -> raise "enoent" end)

      assert {:error, msg} = AgentLauncher.launch("claude", "prompt")
      assert msg =~ "enoent"
    end

    test "open_ide/1 spawns `open -a <app> <dir>` for an existing directory" do
      Mimic.stub(Decant.Settings, :value, fn :ide, "vscode" -> "cursor" end)
      test_pid = self()

      Mimic.stub(System, :cmd, fn bin, args, _opts ->
        send(test_pid, {:open, bin, args})
        {"", 0}
      end)

      assert AgentLauncher.open_ide(System.tmp_dir!()) == :ok
      assert_received {:open, "open", ["-a", "Cursor", _dir]}
    end

    test "open_ide/1 falls back to VS Code for an unknown saved ide" do
      Mimic.stub(Decant.Settings, :value, fn :ide, "vscode" -> "weird" end)
      test_pid = self()

      Mimic.stub(System, :cmd, fn _bin, args, _opts ->
        send(test_pid, {:args, args})
        {"", 0}
      end)

      assert AgentLauncher.open_ide(System.tmp_dir!()) == :ok
      assert_received {:args, ["-a", "Visual Studio Code", _]}
    end

    test "open_ide/1 errors when the project folder no longer exists" do
      assert {:error, msg} =
               AgentLauncher.open_ide("/no/such/dir/decant-#{System.unique_integer([:positive])}")

      assert msg =~ "no longer exists"
    end
  end
end
