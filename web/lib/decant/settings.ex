defmodule Decant.Settings do
  @moduledoc """
  User preferences for the web app: the preferred coding agent, terminal, and
  IDE used when opening things from the dashboard. Stored as JSON outside the
  archive (the archive is read-only and owned by the CLI), at
  `~/.config/decant/settings.json` (override the directory with
  `DECANT_CONFIG_DIR`).

  When a preference is unset we infer a sensible default from the environment
  the server runs in: the terminal from `TERM_PROGRAM`, the IDE from what is
  installed in /Applications, and the agent from what the archive is mostly
  made of.
  """

  @valid_agents ~w(claude codex)
  @valid_terminals ~w(terminal iterm ghostty wezterm kitty alacritty)
  @valid_ides ~w(vscode cursor zed sublime intellij)

  @doc "Effective settings: inferred defaults overlaid with any saved choices."
  def get, do: Map.merge(detected(), load())

  @doc "A single saved preference, falling back to `default` (no inference)."
  def value(key, default) when key in [:agent, :terminal, :ide] do
    Map.get(load(), key) || default
  end

  @doc "Persist a subset of preferences, ignoring unknown keys and values."
  def put(attrs) do
    merged = Map.merge(load(), sanitize(attrs))
    File.mkdir_p!(Path.dirname(path()))
    File.write!(path(), Jason.encode!(stringify(merged), pretty: true))
    _ = File.chmod(path(), 0o600)
    merged
  end

  @doc "Defaults inferred from the running environment and the archive."
  def detected do
    %{agent: detect_agent(), terminal: detect_terminal(), ide: detect_ide()}
  end

  @doc "Path to the settings file."
  def path do
    dir =
      System.get_env("DECANT_CONFIG_DIR") ||
        Path.join(System.user_home() || ".", ".config/decant")

    Path.expand(Path.join(dir, "settings.json"))
  end

  ## inference

  defp detect_terminal do
    case System.get_env("TERM_PROGRAM") do
      "iTerm.app" -> "iterm"
      "ghostty" -> "ghostty"
      "WezTerm" -> "wezterm"
      "Apple_Terminal" -> "terminal"
      _ -> if System.get_env("TERM") == "xterm-kitty", do: "kitty", else: "terminal"
    end
  end

  defp detect_ide do
    Enum.find_value(
      [
        {"Cursor", "cursor"},
        {"Visual Studio Code", "vscode"},
        {"Zed", "zed"},
        {"Sublime Text", "sublime"},
        {"IntelliJ IDEA", "intellij"}
      ],
      "vscode",
      fn {app, key} -> if File.dir?("/Applications/#{app}.app"), do: key end
    )
  end

  defp detect_agent do
    case Decant.Archive.by_dimension(:tool) do
      [%{key: "codex"} | _] -> "codex"
      _ -> "claude"
    end
  rescue
    _ -> "claude"
  end

  ## storage

  defp load do
    with {:ok, body} <- File.read(path()),
         {:ok, json} when is_map(json) <- Jason.decode(body) do
      sanitize(for {k, v} <- json, into: %{}, do: {to_atom(k), v})
    else
      _ -> %{}
    end
  end

  defp sanitize(attrs) do
    attrs
    |> Map.new(fn {k, v} -> {to_atom(k), v} end)
    |> Map.take([:agent, :terminal, :ide])
    |> Enum.filter(fn
      {:agent, v} -> v in @valid_agents
      {:terminal, v} -> v in @valid_terminals
      {:ide, v} -> v in @valid_ides
      _ -> false
    end)
    |> Map.new()
  end

  defp stringify(map), do: Map.new(map, fn {k, v} -> {to_string(k), v} end)

  defp to_atom(k) when is_atom(k), do: k
  defp to_atom("agent"), do: :agent
  defp to_atom("terminal"), do: :terminal
  defp to_atom("ide"), do: :ide
  defp to_atom(other) when is_binary(other), do: :__ignored__
end
