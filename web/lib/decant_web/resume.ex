defmodule DecantWeb.Resume do
  @moduledoc """
  Builds the shell commands to continue (resume), branch (fork), or start a new
  thread from an archived session, per tool. The web app can't open an
  interactive TTY, so the UI surfaces these copy-able commands (run them in the
  session's working directory — sessions are cwd-bound).
  """

  @doc "Returns %{resume, fork, new} shell commands; nil where unsupported."
  def commands(summary) do
    cwd = summary[:project]
    sid = summary[:source_session_id]
    cd = if is_binary(cwd) and cwd != "", do: "cd #{quote_path(cwd)} && ", else: ""
    build(summary[:tool], sid, cd)
  end

  defp build("claude_code", sid, cd) when is_binary(sid) do
    %{
      resume: cd <> "claude --resume " <> sid,
      fork: cd <> "claude --resume " <> sid <> " --fork-session",
      new: cd <> "claude"
    }
  end

  defp build("codex", sid, cd) when is_binary(sid) do
    %{resume: cd <> "codex resume " <> sid, fork: nil, new: cd <> "codex"}
  end

  defp build(_tool, _sid, cd) do
    %{resume: nil, fork: nil, new: if(cd != "", do: String.trim_trailing(cd, " && "), else: nil)}
  end

  defp quote_path(p), do: "'" <> String.replace(p, "'", "'\\''") <> "'"
end
