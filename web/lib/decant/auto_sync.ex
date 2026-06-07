defmodule Decant.AutoSync do
  @moduledoc """
  Keeps the archive fresh in realtime. Watches the Claude Code and Codex source
  directories, runs an incremental `decant sync` (debounced) when they change,
  and broadcasts `{:archive_updated, report}` over `Decant.PubSub` so connected
  LiveViews live-reload. Also polls periodically as a safety net, and exposes
  `sync_now/0` for the manual Sync button.

  Enabled when `config :decant, auto_sync: true` (dev/prod, not test). When the
  `decant` binary can't be found, syncs are skipped gracefully.
  """
  use GenServer
  require Logger

  @topic "archive"
  @debounce_ms 1500
  @poll_ms 60_000

  def start_link(opts), do: GenServer.start_link(__MODULE__, opts, name: __MODULE__)

  @doc "Trigger an immediate sync (used by the manual Sync button)."
  def sync_now, do: GenServer.cast(__MODULE__, :sync_now)

  @doc "PubSub topic LiveViews subscribe to for archive updates."
  def topic, do: @topic

  @doc "Resolve the `decant` binary: DECANT_BIN env > repo target > PATH."
  def decant_bin do
    System.get_env("DECANT_BIN") ||
      Enum.find(
        [Path.expand("../target/release/decant"), Path.expand("../target/debug/decant")],
        &File.exists?/1
      ) ||
      System.find_executable("decant") ||
      "decant"
  end

  @impl true
  def init(_opts) do
    if Application.get_env(:decant, :auto_sync, false) do
      dirs = watch_dirs()
      watcher = start_watcher(dirs)
      Process.send_after(self(), :poll, @poll_ms)
      {:ok, %{watcher: watcher, dirs: dirs, timer: nil, last: nil}, {:continue, :initial_sync}}
    else
      {:ok, %{watcher: nil, dirs: [], timer: nil, last: nil}}
    end
  end

  @impl true
  def handle_continue(:initial_sync, state), do: {:noreply, sync(state, false)}

  @impl true
  def handle_cast(:sync_now, state), do: {:noreply, sync(state, true)}

  @impl true
  def handle_info({:file_event, _pid, {_path, _events}}, state) do
    if state.timer, do: Process.cancel_timer(state.timer)
    {:noreply, %{state | timer: Process.send_after(self(), :debounced_sync, @debounce_ms)}}
  end

  def handle_info({:file_event, _pid, :stop}, state), do: {:noreply, state}

  def handle_info(:debounced_sync, state), do: {:noreply, sync(%{state | timer: nil}, false)}

  def handle_info(:poll, state) do
    Process.send_after(self(), :poll, @poll_ms)
    {:noreply, sync(state, false)}
  end

  def handle_info(_msg, state), do: {:noreply, state}

  # Run an incremental sync. Broadcast when something was ingested, or always
  # when `force` (a manual click) so the user sees feedback.
  defp sync(state, force) do
    case Decant.Archive.db_path() do
      nil ->
        state

      db ->
        report = run(decant_bin(), db)

        if force or ingested?(report) do
          Phoenix.PubSub.broadcast(Decant.PubSub, @topic, {:archive_updated, report})
        end

        %{state | last: report}
    end
  end

  defp run(bin, db) do
    case System.cmd(bin, ["--db", db, "sync"], stderr_to_stdout: true) do
      {out, 0} -> String.trim(out)
      {out, 3} -> String.trim(out) <> " (with issues)"
      {out, code} -> "exit #{code}: #{String.slice(out, 0, 200)}"
    end
  rescue
    e -> "error: #{Exception.message(e)} (set DECANT_BIN to the decant binary)"
  end

  defp ingested?(report) do
    case Regex.run(~r/(\d+) ingested/, report) do
      [_, n] -> String.to_integer(n) > 0
      _ -> false
    end
  end

  defp start_watcher([]), do: nil

  defp start_watcher(dirs) do
    case FileSystem.start_link(dirs: dirs) do
      {:ok, pid} ->
        FileSystem.subscribe(pid)
        pid

      other ->
        Logger.warning("AutoSync: could not start file watcher: #{inspect(other)}")
        nil
    end
  end

  defp watch_dirs do
    home = System.user_home() || "."

    [
      System.get_env("DECANT_CLAUDE_DIR") || Path.join(home, ".claude/projects"),
      System.get_env("DECANT_CODEX_DIR") || Path.join(home, ".codex")
    ]
    |> Enum.filter(&File.dir?/1)
  end
end
