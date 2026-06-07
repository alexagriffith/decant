defmodule DecantWeb.Format do
  @moduledoc """
  Small, pure formatting helpers used across templates (imported in
  `DecantWeb` html helpers, so call them directly in HEEx).
  """

  @doc ~s|Format a USD amount: `money(1234.5) => "$1,234.50"`.|
  def money(n) do
    n = (n || 0) * 1.0
    sign = if n < 0, do: "-", else: ""
    [whole, frac] = abs(n) |> :erlang.float_to_binary(decimals: 2) |> String.split(".")
    "#{sign}$#{thousands(whole)}.#{frac}"
  end

  @doc ~s|Integer with thousands separators: `int(187811) => "187,811"`.|
  def int(n) when is_integer(n), do: n |> Integer.to_string() |> thousands()
  def int(n) when is_float(n), do: n |> trunc() |> int()
  def int(nil), do: "0"

  @doc ~s|Compact number for dense spots: `compact(187811) => "187.8k"`.|
  def compact(n) when is_number(n) do
    n = trunc(n)

    cond do
      n >= 1_000_000 -> "#{Float.round(n / 1_000_000, 1)}M"
      n >= 1_000 -> "#{Float.round(n / 1_000, 1)}k"
      true -> Integer.to_string(n)
    end
  end

  def compact(nil), do: "0"

  @doc "First 10 chars of an ISO timestamp (the date)."
  def date_only(nil), do: "·"
  def date_only(s) when is_binary(s), do: String.slice(s, 0, 10)

  @doc "Humanized age of an ISO8601 timestamp, e.g. `3d ago`."
  def relative_time(nil), do: "·"

  def relative_time(iso) when is_binary(iso) do
    case parse(iso) do
      {:ok, seconds} -> humanize(seconds)
      :error -> date_only(iso)
    end
  end

  defp parse(iso) do
    case DateTime.from_iso8601(iso) do
      {:ok, dt, _} ->
        {:ok, DateTime.diff(DateTime.utc_now(), dt, :second)}

      _ ->
        case NaiveDateTime.from_iso8601(iso) do
          {:ok, ndt} -> {:ok, NaiveDateTime.diff(NaiveDateTime.utc_now(), ndt, :second)}
          _ -> :error
        end
    end
  end

  defp humanize(s) when s < 0, do: "just now"
  defp humanize(s) when s < 60, do: "just now"
  defp humanize(s) when s < 3_600, do: "#{div(s, 60)}m ago"
  defp humanize(s) when s < 86_400, do: "#{div(s, 3_600)}h ago"
  defp humanize(s) when s < 2_592_000, do: "#{div(s, 86_400)}d ago"
  defp humanize(s) when s < 31_536_000, do: "#{div(s, 2_592_000)}mo ago"
  defp humanize(s), do: "#{div(s, 31_536_000)}y ago"

  defp thousands(digits) when is_binary(digits) do
    digits
    |> String.reverse()
    |> String.replace(~r/(\d{3})(?=\d)/, "\\1,")
    |> String.reverse()
  end
end
