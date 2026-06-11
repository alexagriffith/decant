defmodule DecantWeb.TelemetryTest do
  @moduledoc "Smoke test that the metrics list is well-formed."
  use ExUnit.Case, async: true

  test "metrics/0 returns a non-empty list of telemetry metrics" do
    metrics = DecantWeb.Telemetry.metrics()
    assert is_list(metrics)
    refute Enum.empty?(metrics)
    assert Enum.all?(metrics, &is_struct/1)
  end
end
