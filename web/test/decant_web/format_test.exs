defmodule DecantWeb.FormatTest do
  @moduledoc "Unit tests for the pure formatting helpers."
  use ExUnit.Case, async: true

  alias DecantWeb.Format

  describe "money/1" do
    test "formats a positive amount with thousands and 2 decimals" do
      assert Format.money(1234.5) == "$1,234.50"
    end

    test "formats nil as zero" do
      assert Format.money(nil) == "$0.00"
    end

    test "formats a negative amount with a leading minus" do
      assert Format.money(-12.3) == "-$12.30"
    end

    test "formats small amounts without separators" do
      assert Format.money(0.42) == "$0.42"
    end
  end

  describe "int/1" do
    test "formats an integer with thousands separators" do
      assert Format.int(187_811) == "187,811"
    end

    test "truncates a float and formats it" do
      assert Format.int(1234.9) == "1,234"
    end

    test "formats nil as 0" do
      assert Format.int(nil) == "0"
    end
  end

  describe "compact/1" do
    test "abbreviates millions" do
      assert Format.compact(2_500_000) == "2.5M"
    end

    test "abbreviates thousands" do
      assert Format.compact(187_811) == "187.8k"
    end

    test "leaves small numbers as-is" do
      assert Format.compact(42) == "42"
    end

    test "formats nil as 0" do
      assert Format.compact(nil) == "0"
    end
  end

  describe "dur/1" do
    test "nil renders the middot" do
      assert Format.dur(nil) == "·"
    end

    test "seconds under a minute" do
      assert Format.dur(45) == "45s"
    end

    test "minutes and seconds" do
      assert Format.dur(83) == "1m23s"
    end

    test "hours and minutes" do
      assert Format.dur(3920) == "1h05m"
    end
  end

  describe "date_only/1" do
    test "nil renders the middot" do
      assert Format.date_only(nil) == "·"
    end

    test "slices the date portion of an ISO timestamp" do
      assert Format.date_only("2026-05-01T09:00:00Z") == "2026-05-01"
    end
  end

  describe "relative_time/1" do
    test "nil renders the middot" do
      assert Format.relative_time(nil) == "·"
    end

    test "a fresh timestamp reads 'just now'" do
      now = DateTime.utc_now() |> DateTime.to_iso8601()
      assert Format.relative_time(now) == "just now"
    end

    test "a future timestamp also reads 'just now'" do
      future = DateTime.utc_now() |> DateTime.add(120, :second) |> DateTime.to_iso8601()
      assert Format.relative_time(future) == "just now"
    end

    test "minutes ago" do
      ts = DateTime.utc_now() |> DateTime.add(-300, :second) |> DateTime.to_iso8601()
      assert Format.relative_time(ts) == "5m ago"
    end

    test "hours ago" do
      ts = DateTime.utc_now() |> DateTime.add(-7200, :second) |> DateTime.to_iso8601()
      assert Format.relative_time(ts) == "2h ago"
    end

    test "days ago" do
      ts = DateTime.utc_now() |> DateTime.add(-3 * 86_400, :second) |> DateTime.to_iso8601()
      assert Format.relative_time(ts) == "3d ago"
    end

    test "months ago" do
      ts = DateTime.utc_now() |> DateTime.add(-90 * 86_400, :second) |> DateTime.to_iso8601()
      assert Format.relative_time(ts) == "3mo ago"
    end

    test "years ago" do
      ts = DateTime.utc_now() |> DateTime.add(-800 * 86_400, :second) |> DateTime.to_iso8601()
      assert Format.relative_time(ts) == "2y ago"
    end

    test "parses a naive (no offset) timestamp" do
      ndt =
        NaiveDateTime.utc_now() |> NaiveDateTime.add(-300, :second) |> NaiveDateTime.to_iso8601()

      assert Format.relative_time(ndt) == "5m ago"
    end

    test "an unparseable string falls back to the date portion" do
      assert Format.relative_time("not-a-timestamp") == "not-a-time"
    end
  end
end
