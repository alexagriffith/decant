defmodule Decant.HealthCheck do
  @moduledoc """
  Polls the local decant daemon's `/health` endpoint on a fixed interval and
  caches the latest outcome so the UI can show daemon readiness without making a
  request on every render.

  `ready?/0` returns `true` only when the most recent poll succeeded. The poller
  never crashes if the daemon is down — a failed poll simply records "not ready"
  and the next tick retries.

  Started by the application supervisor and gated behind
  `config :decant, :daemon_client` so the test suite does not open network
  connections.
  """
  use GenServer
  require Logger

  @poll_ms 5_000

  def start_link(opts) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "Whether the last `/health` poll succeeded. Defaults to `false` if not running."
  def ready? do
    case GenServer.whereis(__MODULE__) do
      nil -> false
      _pid -> GenServer.call(__MODULE__, :ready?)
    end
  end

  @doc "Latest cached status map: `%{ready?: boolean, data: map | nil, checked_at: DateTime | nil}`."
  def status do
    case GenServer.whereis(__MODULE__) do
      nil -> %{ready?: false, data: nil, checked_at: nil}
      _pid -> GenServer.call(__MODULE__, :status)
    end
  end

  @impl true
  def init(opts) do
    poll_ms = Keyword.get(opts, :poll_ms, @poll_ms)
    state = %{ready?: false, data: nil, checked_at: nil, poll_ms: poll_ms}
    # Poll immediately on startup, then on the interval.
    {:ok, state, {:continue, :poll}}
  end

  @impl true
  def handle_continue(:poll, state) do
    {:noreply, poll(state)}
  end

  @impl true
  def handle_call(:ready?, _from, state), do: {:reply, state.ready?, state}
  def handle_call(:status, _from, state), do: {:reply, public_status(state), state}

  @impl true
  def handle_info(:poll, state), do: {:noreply, poll(state)}
  def handle_info(_msg, state), do: {:noreply, state}

  # Run one poll, update the cache, and schedule the next tick. Any error is
  # caught and recorded as not-ready so the process never dies.
  defp poll(state) do
    schedule(state.poll_ms)

    new_state =
      try do
        case Decant.Daemon.health() do
          {:ok, data} ->
            %{state | ready?: true, data: data, checked_at: DateTime.utc_now()}

          {:error, reason} ->
            Logger.debug("HealthCheck: daemon not ready (#{inspect(reason)})")
            %{state | ready?: false, checked_at: DateTime.utc_now()}
        end
      rescue
        e ->
          Logger.debug("HealthCheck: poll raised #{inspect(e)}")
          %{state | ready?: false, checked_at: DateTime.utc_now()}
      end

    new_state
  end

  defp schedule(poll_ms), do: Process.send_after(self(), :poll, poll_ms)

  defp public_status(state), do: Map.take(state, [:ready?, :data, :checked_at])
end
