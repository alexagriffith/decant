defmodule DecantWeb.ResumeTest do
  @moduledoc "Tests the per-tool continue/fork/new shell command builders."
  use ExUnit.Case, async: true

  alias DecantWeb.Resume

  test "claude_code builds resume, fork, and new commands, cd-ing into the project" do
    cmds =
      Resume.commands(%{
        tool: "claude_code",
        source_session_id: "sess-1",
        project: "/Users/dev/proj"
      })

    assert cmds.resume == "cd '/Users/dev/proj' && claude --resume sess-1"
    assert cmds.fork == "cd '/Users/dev/proj' && claude --resume sess-1 --fork-session"
    assert cmds.new == "cd '/Users/dev/proj' && claude"
  end

  test "codex builds resume and new but no fork" do
    cmds = Resume.commands(%{tool: "codex", source_session_id: "sess-2", project: "/p"})

    assert cmds.resume == "cd '/p' && codex resume sess-2"
    assert cmds.fork == nil
    assert cmds.new == "cd '/p' && codex"
  end

  test "an unknown tool yields only a new command from the cd prefix" do
    cmds = Resume.commands(%{tool: "other", source_session_id: "x", project: "/p"})
    assert cmds == %{resume: nil, fork: nil, new: "cd '/p'"}
  end

  test "no project means no cd prefix and a nil new command for unknown tools" do
    cmds = Resume.commands(%{tool: "other", source_session_id: "x", project: nil})
    assert cmds == %{resume: nil, fork: nil, new: nil}
  end

  test "single quotes in the project path are escaped" do
    cmds = Resume.commands(%{tool: "claude_code", source_session_id: "s", project: "/a'b"})
    assert cmds.resume =~ "'/a'\\''b'"
  end
end
