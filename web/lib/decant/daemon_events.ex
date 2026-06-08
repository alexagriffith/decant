defmodule Decant.DaemonEvents do
  @moduledoc """
  Holds a single long-lived Server-Sent Events connection to the daemon's
  `GET /api/v1/events` change-stream and rebroadcasts each `archive_updated`
  event to the rest of the app.

  On every `archive_updated` event it broadcasts `{:archive_updated, report}` on
  the `"archive"` `Phoenix.PubSub` topic — the *same* message and topic that
  `Decant.AutoSync` uses — so `DecantWeb.SyncHook` live-reloads connected
  LiveViews without any change. `report` is a short human string derived from the
  event (its `ingested` count and `last_sync_at`).

  The actual streaming runs in a monitored, *unlinked* worker process (a
  blocking `Req.get` with `into:` a chunk handler that feeds `ServerSentEvents`).
  Spawning it unlinked is deliberate: a connection failure exits the worker and
  reaches the GenServer only as a `:DOWN` message — it never crashes the
  GenServer. When the worker ends — clean stream close, connection refused, or
  any error — the GenServer reconnects with capped exponential backoff and keeps
  retrying quietly while the daemon is unreachable.

  Started by the application supervisor and gated behind
  `config :decant, :daemon_client` so the test suite does not open network
  connections.
  """
  use GenServer
  require Logger

  @topic "archive"
  @path "/api/v1/events"
  @backoff_initial_ms 1_000
  @backoff_max_ms 30_000
  # No receive timeout: the stream is meant to stay open indefinitely.

  def start_link(opts) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  @doc "PubSub topic broadcast to on archive updates (shared with `Decant.AutoSync`)."
  def topic, do: @topic

  @impl true
  def init(opts) do
    pubsub = Keyword.get(opts, :pubsub, Decant.PubSub)
    state = %{pubsub: pubsub, worker_ref: nil, backoff: @backoff_initial_ms}

    # `connect_on_start: false` lets tests drive the message handlers without
    # opening a real connection. Production always connects on start.
    if Keyword.get(opts, :connect_on_start, true) do
      {:ok, state, {:continue, :connect}}
    else
      {:ok, state}
    end
  end

  @impl true
  def handle_continue(:connect, state) do
    {:noreply, connect(state)}
  end

  # An SSE event decoded by the streaming worker. Only `archive_updated` is
  # rebroadcast; other event types (e.g. `resync`) are logged and ignored —
  # SyncHook re-fetches on the broadcast we already send for updates.
  @impl true
  def handle_info({:sse_event, %{event: "archive_updated"} = event}, state) do
    report = report_for(event)
    Phoenix.PubSub.broadcast(state.pubsub, @topic, {:archive_updated, report})
    {:noreply, state}
  end

  def handle_info({:sse_event, %{event: other}}, state) do
    Logger.debug("DaemonEvents: ignoring SSE event #{inspect(other)}")
    {:noreply, state}
  end

  # A successful connection delivered at least one event: reset the backoff so a
  # later drop reconnects promptly.
  def handle_info(:sse_connected, state) do
    {:noreply, %{state | backoff: @backoff_initial_ms}}
  end

  # The streaming worker ended (clean close, refusal, or error). Schedule a
  # reconnect with backoff. The worker is spawned unlinked (`spawn_monitor`), so
  # its exit reaches us only as this :DOWN message and never crashes us — that is
  # what keeps the reconnect loop quiet while the daemon is unreachable.
  def handle_info({:DOWN, ref, :process, _pid, reason}, %{worker_ref: ref} = state) do
    Logger.debug("DaemonEvents: stream ended (#{inspect(reason)}); reconnecting")
    {:noreply, schedule_reconnect(%{state | worker_ref: nil})}
  end

  def handle_info(:reconnect, state), do: {:noreply, connect(state)}

  def handle_info(_msg, state), do: {:noreply, state}

  # --- internals -------------------------------------------------------------

  # Spawn the blocking streamer *unlinked* and monitor it. A connection failure
  # exits the worker; we receive only the :DOWN and reconnect with backoff.
  defp connect(state) do
    parent = self()
    {_pid, ref} = spawn_monitor(fn -> stream(parent) end)
    %{state | worker_ref: ref}
  end

  defp schedule_reconnect(state) do
    Process.send_after(self(), :reconnect, state.backoff)
    %{state | backoff: min(state.backoff * 2, @backoff_max_ms)}
  end

  # Blocking: open the SSE connection and feed decoded events back to `parent`.
  # Runs in the spawned worker. Returns normally on a clean end-of-stream; exits
  # abnormally on a transport error, surfaced to the GenServer as the worker's
  # :DOWN reason (which triggers a backoff reconnect).
  defp stream(parent) do
    send(parent, :sse_connected)

    req =
      Req.new(
        method: :get,
        base_url: Decant.Daemon.base_url(),
        url: @path,
        finch: Decant.Finch,
        headers: sse_headers(),
        # Keep the connection open; SSE is a never-ending body.
        receive_timeout: :infinity,
        retry: false,
        into: stream_handler(parent)
      )

    case Req.request(req) do
      {:ok, _resp} ->
        :ok

      {:error, reason} ->
        # Exit abnormally so the GenServer's DOWN handler triggers a backoff
        # reconnect rather than treating this as a clean stream close.
        exit({:sse_error, reason})
    end
  end

  # Req `into:` callback: decode each body chunk into SSE events and forward
  # them to the GenServer. The incremental `ServerSentEvents.Parser` state is
  # carried across chunks in the response's `:private` so a frame split across
  # two chunks is reassembled correctly.
  defp stream_handler(parent) do
    fn {:data, data}, {req, resp} ->
      parser = resp.private[:sse_parser] || ServerSentEvents.Parser.new()
      {events, parser} = ServerSentEvents.Parser.parse(parser, data)

      Enum.each(events, fn event -> send(parent, {:sse_event, event}) end)

      resp = put_in(resp.private[:sse_parser], parser)
      {:cont, {req, resp}}
    end
  end

  defp sse_headers do
    base = [
      {"x-requested-api-version", "1"},
      {"accept", "text/event-stream"}
    ]

    case Decant.Daemon.token() do
      nil -> base
      tok -> [{"authorization", "Bearer " <> tok} | base]
    end
  end

  defp report_for(%{data: data}) when is_binary(data) do
    case Jason.decode(data) do
      {:ok, decoded} -> report_from_payload(decoded)
      {:error, _} -> "archive updated"
    end
  end

  defp report_for(_), do: "archive updated"

  defp report_from_payload(%{"ingested" => n} = payload) do
    suffix =
      case payload["last_sync_at"] do
        nil -> ""
        at -> " at #{at}"
      end

    "#{n} ingested#{suffix}"
  end

  defp report_from_payload(_), do: "archive updated"
end
