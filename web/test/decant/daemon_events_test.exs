defmodule Decant.DaemonEventsTest do
  @moduledoc """
  Tests the SSE consumer's contract with the rest of the app *without* opening a
  network connection: the GenServer is started with `connect_on_start: false`
  and we drive its message handlers directly.

  The critical invariant: on an `archive_updated` SSE event it must broadcast
  `{:archive_updated, report}` on the `"archive"` PubSub topic that
  `DecantWeb.SyncHook` subscribes to, so the dashboard live-reloads.
  """
  use ExUnit.Case, async: true

  alias Decant.DaemonEvents

  setup do
    # Subscribe to the same topic SyncHook listens on.
    Phoenix.PubSub.subscribe(Decant.PubSub, DaemonEvents.topic())
    pid = start_supervised!({DaemonEvents, connect_on_start: false})
    %{pid: pid}
  end

  test "topic/0 is the shared \"archive\" topic" do
    assert DaemonEvents.topic() == "archive"
  end

  test "an archive_updated event is rebroadcast as {:archive_updated, report}", %{pid: pid} do
    event = %{
      event: "archive_updated",
      data:
        Jason.encode!(%{
          "type" => "archive_updated",
          "ingested" => 3,
          "last_sync_at" => "2026-06-07T12:00:00Z"
        })
    }

    send(pid, {:sse_event, event})

    assert_receive {:archive_updated, report}
    assert report =~ "3 ingested"
    assert report =~ "2026-06-07T12:00:00Z"
  end

  test "an archive_updated event with no parseable payload still broadcasts", %{pid: pid} do
    send(pid, {:sse_event, %{event: "archive_updated", data: "not json"}})

    assert_receive {:archive_updated, report}
    assert is_binary(report)
  end

  test "non-archive_updated events (e.g. resync) are ignored, no broadcast", %{pid: pid} do
    send(pid, {:sse_event, %{event: "resync", data: "{\"type\":\"resync\"}"}})

    refute_receive {:archive_updated, _}, 100
    assert Process.alive?(pid)
  end

  test "a worker DOWN does not crash the GenServer (resilient reconnect)", %{pid: pid} do
    # Simulate the streaming worker dying with a connection error. The state
    # carries no live worker_ref (connect_on_start: false), so this exercises
    # the catch-all and the process must survive.
    send(pid, {:DOWN, make_ref(), :process, self(), {:sse_error, :econnrefused}})
    _ = :sys.get_state(pid)
    assert Process.alive?(pid)
  end

  test ":sse_connected resets the backoff", %{pid: pid} do
    send(pid, :sse_connected)
    state = :sys.get_state(pid)
    assert state.backoff == 1_000
  end

  test "a matched worker DOWN clears the ref and schedules a reconnect", %{pid: pid} do
    ref = make_ref()
    :sys.replace_state(pid, fn s -> %{s | worker_ref: ref, backoff: 1_000} end)
    send(pid, {:DOWN, ref, :process, self(), :normal})
    state = :sys.get_state(pid)
    assert state.worker_ref == nil
    # Backoff doubled by schedule_reconnect.
    assert state.backoff == 2_000
  end

  test "an unrelated message is ignored", %{pid: pid} do
    send(pid, :something_else)
    _ = :sys.get_state(pid)
    assert Process.alive?(pid)
  end

  test ":reconnect spawns a fresh worker", %{pid: pid} do
    send(pid, :reconnect)
    state = :sys.get_state(pid)
    # connect/1 set a new monitor ref; the GenServer remains alive even when the
    # spawned worker immediately fails to reach the (absent) daemon.
    assert is_reference(state.worker_ref)
    assert Process.alive?(pid)
  end

  test "a payload with no ingested count broadcasts the generic report", %{pid: pid} do
    send(pid, {:sse_event, %{event: "archive_updated", data: Jason.encode!(%{"type" => "x"})}})
    assert_receive {:archive_updated, "archive updated"}
  end

  test "a payload with ingested but no last_sync_at omits the suffix", %{pid: pid} do
    send(pid, {:sse_event, %{event: "archive_updated", data: Jason.encode!(%{"ingested" => 2})}})
    assert_receive {:archive_updated, "2 ingested"}
  end

  test "an event with non-binary data still broadcasts the generic report", %{pid: pid} do
    send(pid, {:sse_event, %{event: "archive_updated", data: %{not: "binary"}}})
    assert_receive {:archive_updated, "archive updated"}
  end
end

defmodule Decant.DaemonEventsStreamTest do
  @moduledoc """
  Drives the streaming worker (`connect_on_start: true`) with a mocked
  `Req.request/1`, so the connect/continue/stream/handler path is exercised
  without a real network connection.
  """
  use ExUnit.Case, async: false
  use Mimic

  alias Decant.DaemonEvents

  setup :set_mimic_global

  test "connecting streams a chunk through the SSE parser and broadcasts" do
    Phoenix.PubSub.subscribe(Decant.PubSub, DaemonEvents.topic())
    test_pid = self()

    # Capture the streaming request, feed a single SSE `archive_updated` frame
    # into its `into:` handler, then return cleanly so the worker exits.
    Mimic.stub(Req, :request, fn req ->
      handler = req.into
      resp = %Req.Response{status: 200, headers: %{}, body: "", private: %{}}

      frame = "event: archive_updated\ndata: {\"ingested\":1}\n\n"
      {:cont, {_req, _resp}} = handler.({:data, frame}, {req, resp})

      send(test_pid, :streamed)
      {:ok, resp}
    end)

    pid = start_supervised!({DaemonEvents, connect_on_start: true})
    assert_receive :streamed, 1_000
    assert_receive {:archive_updated, "1 ingested"}, 1_000
    assert Process.alive?(pid)
  end

  test "sse_headers include a bearer token when one is configured" do
    System.put_env("DECANT_DAEMON_TOKEN", "secret-token")
    on_exit(fn -> System.delete_env("DECANT_DAEMON_TOKEN") end)
    test_pid = self()

    Mimic.stub(Req, :request, fn req ->
      send(test_pid, {:headers, req.headers})
      {:ok, %Req.Response{status: 200, headers: %{}, body: "", private: %{}}}
    end)

    start_supervised!({DaemonEvents, connect_on_start: true})
    assert_receive {:headers, headers}, 1_000
    assert Map.get(headers, "authorization") == ["Bearer secret-token"]
  end

  test "a transport error from the stream exits the worker and the GenServer survives" do
    System.delete_env("DECANT_DAEMON_TOKEN")
    test_pid = self()

    Mimic.stub(Req, :request, fn _req ->
      send(test_pid, {:worker, self()})
      {:error, %Req.TransportError{reason: :econnrefused}}
    end)

    pid = start_supervised!({DaemonEvents, connect_on_start: true})

    # Synchronize on the worker's exit deterministically (no Process.sleep): the
    # unlinked worker connects, fails, and exits abnormally.
    assert_receive {:worker, worker_pid}, 1_000
    ref = Process.monitor(worker_pid)
    assert_receive {:DOWN, ^ref, :process, ^worker_pid, _reason}, 1_000

    # A successful state read proves the GenServer already processed the worker's
    # own :DOWN (ahead of this call in the mailbox) and is still alive, reconnecting.
    assert %{worker_ref: _} = :sys.get_state(pid)
  end
end
