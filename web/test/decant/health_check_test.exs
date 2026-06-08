defmodule Decant.HealthCheckTest do
  @moduledoc """
  Tests that `Decant.HealthCheck.ready?/0` reflects the outcome of the mocked
  `Decant.Daemon.health/0`. Uses Mimic in global mode so the stub is visible
  from the GenServer's own process (hence `async: false`).
  """
  use ExUnit.Case, async: false

  use Mimic

  setup :set_mimic_global

  test "ready? is true after a successful poll" do
    test_pid = self()

    Mimic.stub(Decant.Daemon, :health, fn ->
      send(test_pid, :polled)
      {:ok, %{"status" => "ok"}}
    end)

    pid = start_supervised!({Decant.HealthCheck, poll_ms: 10_000})

    # Wait until the GenServer has performed its initial poll.
    assert_receive :polled, 1_000
    # Ensure the continue/poll result has been applied before asserting.
    _ = :sys.get_state(pid)

    assert Decant.HealthCheck.ready?() == true

    status = Decant.HealthCheck.status()
    assert status.ready? == true
    assert status.data == %{"status" => "ok"}
    assert %DateTime{} = status.checked_at
  end

  test "ready? is false after a failed poll and never crashes the process" do
    test_pid = self()

    Mimic.stub(Decant.Daemon, :health, fn ->
      send(test_pid, :polled)
      {:error, :service_unavailable}
    end)

    pid = start_supervised!({Decant.HealthCheck, poll_ms: 10_000})

    assert_receive :polled, 1_000
    _ = :sys.get_state(pid)

    assert Decant.HealthCheck.ready?() == false
    assert Process.alive?(pid)
  end

  test "ready? is false when the daemon call raises (poll is resilient)" do
    test_pid = self()

    Mimic.stub(Decant.Daemon, :health, fn ->
      send(test_pid, :polled)
      raise "kaboom"
    end)

    pid = start_supervised!({Decant.HealthCheck, poll_ms: 10_000})

    assert_receive :polled, 1_000
    _ = :sys.get_state(pid)

    assert Decant.HealthCheck.ready?() == false
    assert Process.alive?(pid)
  end

  test "ready? is false when the GenServer is not running" do
    refute Decant.HealthCheck.ready?()
  end
end
