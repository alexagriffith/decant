defmodule Decant.AgentLauncher do
  @moduledoc """
  Turns an Insight into action: opens a coding agent (Claude Code or Codex) in
  the user's preferred terminal, seeded with a prompt, so they can codify a
  Skill on the spot. Also opens a project in the preferred IDE. macOS only
  (uses `osascript` and `open`); elsewhere callers fall back to the copy-able
  command from `command/2`.

  The seeded prompt is written to a temp file and read with `$(cat …)` so prompt
  text never passes through AppleScript/shell parsing. Agent names, terminals,
  and IDEs are all whitelisted.
  """

  @agents %{
    "claude" => %{bin: "claude", label: "Claude"},
    "codex" => %{bin: "codex", label: "Codex"}
  }

  # Ordered so the settings dropdown is stable. Terminal.app and iTerm are
  # driven with AppleScript; the rest are launched through their CLI exec flags.
  @terminals [
    {"terminal", "Terminal"},
    {"iterm", "iTerm"},
    {"ghostty", "Ghostty"},
    {"wezterm", "WezTerm"},
    {"kitty", "kitty"},
    {"alacritty", "Alacritty"}
  ]

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
  def terminals, do: @terminals

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
    launch(agent, prompt, nil)
  end

  @doc """
  Like `launch/2`, but threads a recommendation `key` so the seeded prompt ends
  with an instruction to record the recommendation implemented once the agent
  has finished and verified the work. A `nil` key behaves exactly like `launch/2`.
  """
  def launch(agent, prompt, key) when is_binary(agent) and is_binary(prompt) do
    cond do
      not Map.has_key?(@agents, agent) -> {:error, "Unknown agent."}
      not can_launch?() -> {:error, "Opening a terminal is only supported on macOS right now."}
      true -> do_launch(@agents[agent].bin, with_mark_instruction(prompt, key))
    end
  end

  # Append the mark-implemented handoff line so the agent records the
  # recommendation as done once it has finished and verified the work.
  defp with_mark_instruction(prompt, key) when is_binary(key) and key != "" do
    prompt <>
      "\n\nWhen you have completed and verified this, run: decant recommendations mark #{key}"
  end

  defp with_mark_instruction(prompt, _key), do: prompt

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
    # Read the prompt from the temp file, then remove it in the same subshell so
    # the file never lingers and the prompt text never passes through parsing.
    cmd =
      "cd #{shell_quote(dir)} && #{bin} \"$(cat #{shell_quote(tmp)}; rm -f #{shell_quote(tmp)})\""

    launch_in(Decant.Settings.value(:terminal, "terminal"), cmd)
  end

  # Terminal.app and iTerm are scripted with AppleScript; CLI-capable emulators
  # run the command through their exec flags via `open -na <App> --args …`.
  defp launch_in("iterm", cmd), do: run("osascript", ["-e", iterm_script(cmd)])
  defp launch_in("ghostty", cmd), do: open_args("Ghostty", ["-e", shell(), "-lc", cmd])
  defp launch_in("alacritty", cmd), do: open_args("Alacritty", ["-e", shell(), "-lc", cmd])
  defp launch_in("kitty", cmd), do: open_args("kitty", [shell(), "-lc", cmd])
  defp launch_in("wezterm", cmd), do: open_args("WezTerm", ["start", "--", shell(), "-lc", cmd])
  defp launch_in(_terminal, cmd), do: run("osascript", ["-e", terminal_app_script(cmd)])

  defp open_args(app, args), do: run("open", ["-na", app, "--args" | args])

  defp shell, do: System.get_env("SHELL") || "/bin/zsh"

  defp iterm_script(cmd) do
    """
    tell application "iTerm"
      activate
      set w to (create window with default profile)
      tell current session of w to write text #{applescript_string(cmd)}
    end tell
    """
  end

  defp terminal_app_script(cmd) do
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
