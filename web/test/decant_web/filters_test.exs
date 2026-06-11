defmodule DecantWeb.FiltersTest do
  @moduledoc "Unit tests for the dashboard filter query/preset helpers."
  use ExUnit.Case, async: true

  alias DecantWeb.Filters

  describe "parse/1" do
    test "parses dates and present strings, dropping empties" do
      params = %{
        "from" => "2026-05-01",
        "to" => "2026-05-02",
        "tool" => "codex",
        "model" => "",
        "project" => nil
      }

      assert Filters.parse(params) == %{
               from: ~D[2026-05-01],
               to: ~D[2026-05-02],
               tool: "codex",
               model: nil,
               project: nil
             }
    end

    test "an invalid date parses to nil" do
      assert %{from: nil} = Filters.parse(%{"from" => "nope"})
    end

    test "an empty param map yields all-nil filters" do
      assert Filters.parse(%{}) == %{from: nil, to: nil, tool: nil, model: nil, project: nil}
    end
  end

  describe "url/3" do
    test "returns the bare path when no filters are set" do
      assert Filters.url("/files", %{from: nil, to: nil}) == "/files"
    end

    test "encodes set filters as a query string" do
      url = Filters.url("/", %{from: ~D[2026-05-01], to: ~D[2026-05-02], tool: "codex"})
      assert url =~ "/?"
      assert url =~ "from=2026-05-01"
      assert url =~ "tool=codex"
    end

    test "merges extra view params and drops nil/false ones" do
      url = Filters.url("/files", %{}, group: "ext", op: nil, hidden: false)
      assert url == "/files?group=ext"
    end
  end

  describe "presets and apply_preset/3" do
    test "presets/0 lists the known presets" do
      assert Filters.presets() == [{"7d", 7}, {"30d", 30}, {"90d", 90}]
    end

    test "\"all\" clears the date range" do
      assert Filters.apply_preset(%{from: ~D[2026-01-01], to: ~D[2026-01-02]}, "all", %{}) ==
               %{from: nil, to: nil}
    end

    test "a day preset anchors on bounds max" do
      r = Filters.apply_preset(%{from: nil, to: nil}, "7d", %{max: "2026-05-10"})
      assert r.to == ~D[2026-05-10]
      assert r.from == ~D[2026-05-04]
    end

    test "anchors on today when bounds have no max" do
      r = Filters.apply_preset(%{from: nil, to: nil}, "30d", %{})
      assert r.to == Date.utc_today()
    end

    test "an unknown preset key falls back to 30 days" do
      r = Filters.apply_preset(%{from: nil, to: nil}, "bogus", %{max: "2026-05-10"})
      assert Date.diff(r.to, r.from) == 29
    end
  end

  describe "active_preset/2" do
    test "all-time when no range is set" do
      assert Filters.active_preset(%{from: nil, to: nil}, %{}) == "all"
    end

    test "recognizes a matching preset window" do
      bounds = %{max: "2026-05-10"}
      filters = Filters.apply_preset(%{from: nil, to: nil}, "7d", bounds)
      assert Filters.active_preset(filters, bounds) == "7d"
    end

    test "custom when the range matches no preset" do
      assert Filters.active_preset(%{from: ~D[2026-01-01], to: ~D[2026-01-03]}, %{
               max: "2026-05-10"
             }) == "custom"
    end
  end

  describe "shift/2" do
    test "shifts the window earlier by its own span" do
      filters = %{from: ~D[2026-05-04], to: ~D[2026-05-10]}
      assert Filters.shift(filters, :prev) == %{from: ~D[2026-04-27], to: ~D[2026-05-03]}
    end

    test "shifts the window later by its own span" do
      filters = %{from: ~D[2026-05-04], to: ~D[2026-05-10]}
      assert Filters.shift(filters, :next) == %{from: ~D[2026-05-11], to: ~D[2026-05-17]}
    end

    test "leaves an open range unchanged" do
      assert Filters.shift(%{from: nil, to: nil}, :prev) == %{from: nil, to: nil}
    end
  end

  describe "label/1" do
    test "renders the active date range" do
      assert Filters.label(%{from: ~D[2026-05-01], to: ~D[2026-05-02]}) ==
               "May 01, 2026 to May 02, 2026"
    end

    test "renders all-time for an open range" do
      assert Filters.label(%{from: nil, to: nil}) == "All time"
    end
  end

  describe "chips/1" do
    test "builds chips for set dimensions, with human labels" do
      chips =
        Filters.chips(%{model: "gpt-5.4", tool: "claude_code", project: "/Users/dev/proj"})

      assert {:model, "model · gpt-5.4"} in chips
      assert {:tool, "tool · Claude"} in chips
      assert {:project, "project · proj"} in chips
    end

    test "codex and unknown tools get their own labels" do
      assert [{:tool, "tool · Codex"}] = Filters.chips(%{tool: "codex"})
      assert [{:tool, "tool · other"}] = Filters.chips(%{tool: "other"})
    end

    test "no chips for an empty filter set" do
      assert Filters.chips(%{}) == []
    end
  end

  describe "empty?/1" do
    test "true when nothing is set" do
      assert Filters.empty?(%{from: nil, to: nil, tool: nil, model: nil, project: nil})
    end

    test "false when any filter is set" do
      refute Filters.empty?(%{tool: "codex"})
    end
  end
end
