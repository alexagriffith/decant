defmodule Decant.AgentLauncher do
  @moduledoc """
  Turns an Insight into action: opens a coding agent (Claude Code or Codex) in a
  local terminal, seeded with a prompt, so the user can codify a Skill on the
  spot. macOS only (uses `osascript` to drive Terminal.app); elsewhere callers
  fall back to the copy-able command from `command/2`.

  The seeded prompt is written to a temp file and read with `$(cat …)` so prompt
  text never passes through AppleScript/shell parsing. Agent names are
  whitelisted, so only `claude`/`codex` can be launched.
  """

  @agents %{
    "claude" => %{bin: "claude", label: "Claude"},
    "codex" => %{bin: "codex", label: "Codex"}
  }

  @doc "Agents offered in the UI: [{key, label}]."
  def agents, do: Enum.map(@agents, fn {k, %{label: l}} -> {k, l} end)

  @doc "Preferred agent key launched by the primary CTA. Override with DECANT_DEFAULT_AGENT."
  def default_agent do
    env = System.get_env("DECANT_DEFAULT_AGENT")
    if is_binary(env) and Map.has_key?(@agents, env), do: env, else: "claude"
  end

  @doc "True when we can open a terminal for the user (macOS)."
  def can_launch?, do: match?({:unix, :darwin}, :os.type())

  @doc "Open `agent` in Terminal.app seeded with `prompt`. Returns :ok | {:error, msg}."
  def launch(agent, prompt) when is_binary(agent) and is_binary(prompt) do
    cond do
      not Map.has_key?(@agents, agent) -> {:error, "Unknown agent."}
      not can_launch?() -> {:error, "Opening a terminal is only supported on macOS right now."}
      true -> do_launch(@agents[agent].bin, prompt)
    end
  end

  @doc "A copy-able shell command that starts `agent` with `prompt` (fallback)."
  def command(agent, prompt) do
    case Map.fetch(@agents, agent) do
      {:ok, %{bin: bin}} -> "#{bin} #{shell_quote(prompt)}"
      :error -> nil
    end
  end

  defp do_launch(bin, prompt) do
    tmp =
      Path.join(System.tmp_dir!(), "decant-prompt-#{:erlang.unique_integer([:positive])}.txt")

    File.write!(tmp, prompt)
    dir = System.get_env("DECANT_SKILLS_DIR") || System.user_home() || "."
    cmd = "cd #{shell_quote(dir)} && #{bin} \"$(cat #{shell_quote(tmp)})\""

    apple = """
    tell application "Terminal"
      activate
      do script #{applescript_string(cmd)}
    end tell
    """

    case System.cmd("osascript", ["-e", apple], stderr_to_stdout: true) do
      {_, 0} -> :ok
      {out, _} -> {:error, String.trim(out)}
    end
  rescue
    e -> {:error, Exception.message(e)}
  end

  # Single-quote for POSIX shells.
  defp shell_quote(s), do: "'" <> String.replace(s, "'", "'\\''") <> "'"

  # Quote + escape for an AppleScript string literal.
  defp applescript_string(s) do
    "\"" <> (s |> String.replace("\\", "\\\\") |> String.replace("\"", "\\\"")) <> "\""
  end
end
