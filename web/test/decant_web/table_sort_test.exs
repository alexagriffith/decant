defmodule DecantWeb.TableSortTest do
  @moduledoc "Unit tests for the sortable-table helpers."
  use ExUnit.Case, async: true

  alias DecantWeb.TableSort

  describe "toggle/2" do
    test "a new column starts descending" do
      assert TableSort.toggle({:calls, :desc}, "errors") == {:errors, :desc}
    end

    test "the active descending column flips to ascending" do
      assert TableSort.toggle({:calls, :desc}, "calls") == {:calls, :asc}
    end

    test "the active ascending column flips to descending" do
      assert TableSort.toggle({:calls, :asc}, "calls") == {:calls, :desc}
    end

    test "an unknown (non-existing atom) column is ignored" do
      current = {:calls, :desc}
      assert TableSort.toggle(current, "definitely_not_a_field_atom_zzz") == current
    end
  end

  describe "sort/2" do
    test "sorts ascending, case-insensitive on strings" do
      rows = [%{name: "beta"}, %{name: "Alpha"}, %{name: "gamma"}]

      assert TableSort.sort(rows, {:name, :asc}) == [
               %{name: "Alpha"},
               %{name: "beta"},
               %{name: "gamma"}
             ]
    end

    test "sorts descending on numbers" do
      rows = [%{n: 1}, %{n: 3}, %{n: 2}]
      assert TableSort.sort(rows, {:n, :desc}) == [%{n: 3}, %{n: 2}, %{n: 1}]
    end

    test "returns rows unchanged for an invalid sort tuple" do
      rows = [%{n: 1}, %{n: 2}]
      assert TableSort.sort(rows, :nonsense) == rows
    end
  end
end
