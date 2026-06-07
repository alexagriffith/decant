defmodule DecantWeb.Filters do
  @moduledoc """
  Shared dashboard filter state: parse/build the `?from&to&tool&model&project`
  query string, date-range presets, window shifting, and removable chips. Pages
  parse params in `handle_params/3`, pass the filters map to `Decant.Archive`,
  and build drill-down/date links with `url/2`.
  """

  @presets [{"7d", 7}, {"30d", 30}, {"90d", 90}]

  @doc "Parse LiveView `params` into a filters map (Dates for from/to, strings else)."
  def parse(params) do
    %{
      from: parse_date(params["from"]),
      to: parse_date(params["to"]),
      tool: present(params["tool"]),
      model: present(params["model"]),
      project: present(params["project"])
    }
  end

  @doc "Build a path + query string for the given filters (omitting empty keys)."
  def url(path, filters) do
    case query(filters) do
      q when map_size(q) == 0 -> path
      q -> path <> "?" <> URI.encode_query(q)
    end
  end

  @doc "Query map (string values) for the set filter keys."
  def query(filters) do
    [:from, :to, :tool, :model, :project]
    |> Enum.flat_map(fn k ->
      case Map.get(filters, k) do
        v when v in [nil, ""] -> []
        v -> [{k, stringify(v)}]
      end
    end)
    |> Map.new()
  end

  @doc "Preset definitions: list of {key, days}."
  def presets, do: @presets

  @doc "Apply a preset key (\"7d\"/\"30d\"/\"90d\" or \"all\") to filters, using `bounds` as the anchor."
  def apply_preset(filters, "all", _bounds), do: %{filters | from: nil, to: nil}

  def apply_preset(filters, key, bounds) do
    days = preset_days(key)
    to = anchor(bounds)
    %{filters | from: Date.add(to, -(days - 1)), to: to}
  end

  @doc "Which preset (or \"all\"/\"custom\") the current range matches."
  def active_preset(%{from: nil, to: nil}, _bounds), do: "all"

  def active_preset(filters, bounds) do
    Enum.find_value(@presets, "custom", fn {key, _} ->
      r = apply_preset(filters, key, bounds)
      if r.from == filters[:from] and r.to == filters[:to], do: key
    end)
  end

  @doc "Shift the current window earlier (:prev) or later (:next) by its own span."
  def shift(%{from: %Date{} = from, to: %Date{} = to} = filters, dir) do
    span = Date.diff(to, from) + 1
    delta = if dir == :prev, do: -span, else: span
    %{filters | from: Date.add(from, delta), to: Date.add(to, delta)}
  end

  def shift(filters, _dir), do: filters

  @doc "Human label for the active range."
  def label(%{from: %Date{} = f, to: %Date{} = t}), do: "#{fmt(f)} – #{fmt(t)}"
  def label(_), do: "All time"

  @doc "Removable filter chips: list of {key, label}."
  def chips(filters) do
    [
      filters[:model] && {:model, "model: #{filters[:model]}"},
      filters[:tool] && {:tool, "tool: #{tool_label(filters[:tool])}"},
      filters[:project] && {:project, "project: #{Path.basename(filters[:project])}"}
    ]
    |> Enum.filter(& &1)
  end

  @doc "True when no date range and no dimension filter is set."
  def empty?(filters) do
    Enum.all?([:from, :to, :tool, :model, :project], &(Map.get(filters, &1) in [nil, ""]))
  end

  ## helpers

  defp preset_days(key), do: Enum.find_value(@presets, 30, fn {k, d} -> if k == key, do: d end)

  defp anchor(%{max: max}) when is_binary(max), do: Date.from_iso8601!(max)
  defp anchor(_), do: Date.utc_today()

  defp parse_date(s) when s in [nil, ""], do: nil

  defp parse_date(s) do
    case Date.from_iso8601(s) do
      {:ok, d} -> d
      _ -> nil
    end
  end

  defp present(s) when s in [nil, ""], do: nil
  defp present(s), do: s

  defp stringify(%Date{} = d), do: Date.to_string(d)
  defp stringify(s), do: to_string(s)

  defp fmt(%Date{} = d), do: Calendar.strftime(d, "%b %d, %Y")

  defp tool_label("claude_code"), do: "Claude"
  defp tool_label("codex"), do: "Codex"
  defp tool_label(other), do: other
end
