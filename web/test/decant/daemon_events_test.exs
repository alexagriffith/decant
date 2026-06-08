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
    # The process kept running.
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
end
