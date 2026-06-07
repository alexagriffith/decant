defmodule Decant.AgentLauncher do
  @moduledoc """
  Turns an Insight into action: opens a coding agent (Claude Code or Codex) in
  the user's preferred terminal, seeded with a prompt, so they can codify a
  Skill on the spot. Also opens a project in the preferred IDE. macOS only
  (uses `osascript`); elsewhere callers fall back to the copy-able command from
  `command/2`.

  The seeded prompt is written to a temp file and read with `$(cat …)` so prompt
  text never passes through AppleScript/shell parsing. Agent names, terminals,
  and IDEs are all whitelisted.
  """

  @agents %{
    "claude" => %{bin: "claude", label: "Claude"},
    "codex" => %{bin: "codex", label: "Codex"}
  }

  @terminals %{"terminal" => "Terminal", "iterm" => "iTerm"}

  @ides %{
    "vscode" => %{app: "Visual Studio Code", label: "VS Code"},
    "cursor" => %{app: "Cursor", label: "Cursor"},
    "zed" => %{app: "Zed", label: "Zed"},
    "sublime" => %{app: "Sublime Text", label: "Sublime Text"},
    "intellij" => %{app: "IntelliJ IDEA", label: "IntelliJ IDEA"}
  }

  @doc "Agents offered in the UI: [{key, label}]."
  def agents, do: Enum.map(@agents, fn {k, %{label: l}} -> {k, l} end)

  @doc "Terminals offered in settings: [{key, label}]."
  def terminals, do: Enum.map(@terminals, fn {k, l} -> {k, l} end)

  @doc "IDEs offered in settings: [{key, label}]."
  def ides, do: Enum.map(@ides, fn {k, %{label: l}} -> {k, l} end)

  @doc "Preferred agent key launched by the primary CTA (from settings)."
  def default_agent, do: Decant.Settings.value(:agent, "claude")

  @doc "Label for an IDE key (for button text), defaulting to a generic word."
  def ide_label(key), do: get_in(@ides, [key, :label]) || "IDE"

  @doc "True when we can open apps for the user (macOS)."
  def can_launch?, do: match?({:unix, :darwin}, :os.type())

  @doc "Open `agent` in the preferred terminal seeded with `prompt`. :ok | {:error, msg}."
  def launch(agent, prompt) when is_binary(agent) and is_binary(prompt) do
    cond do
      not Map.has_key?(@agents, agent) -> {:error, "Unknown agent."}
      not can_launch?() -> {:error, "Opening a terminal is only supported on macOS right now."}
      true -> do_launch(@agents[agent].bin, prompt)
    end
  end

  @doc "Open `dir` in the preferred IDE. :ok | {:error, msg}."
  def open_ide(dir) when is_binary(dir) do
    ide = Decant.Settings.value(:ide, "vscode")
    app = get_in(@ides, [ide, :app]) || "Visual Studio Code"

    cond do
      not can_launch?() -> {:error, "Opening an IDE is only supported on macOS right now."}
      not File.dir?(dir) -> {:error, "That project folder no longer exists."}
      true -> run("open", ["-a", app, dir])
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
    terminal = Decant.Settings.value(:terminal, "terminal")

    run("osascript", ["-e", terminal_script(terminal, cmd)])
  end

  # iTerm gets a tailored script; everything else uses Terminal.app.
  defp terminal_script("iterm", cmd) do
    """
    tell application "iTerm"
      activate
      set w to (create window with default profile)
      tell current session of w to write text #{applescript_string(cmd)}
    end tell
    """
  end

  defp terminal_script(_terminal, cmd) do
    """
    tell application "Terminal"
      activate
      do script #{applescript_string(cmd)}
    end tell
    """
  end

  defp run(bin, args) do
    case System.cmd(bin, args, stderr_to_stdout: true) do
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
