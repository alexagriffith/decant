defmodule DecantWeb.TableSort do
  @moduledoc """
  Tiny helpers for clickable, sortable tables. A LiveView keeps a
  `{column, direction}` tuple per table in its assigns, toggles it on the
  "sort" event, and re-sorts the already-loaded rows. Pairs with the
  `UI.sort_header/1` component.
  """

  @type sort :: {atom(), :asc | :desc}

  @doc """
  Toggle sort state for a clicked column name (a string). Clicking the active
  column flips its direction; clicking a new column starts it descending. Uses
  `String.to_existing_atom/1`, so columns must be real field atoms.
  """
  def toggle(current, col_string) when is_binary(col_string) do
    col = String.to_existing_atom(col_string)

    case current do
      {^col, :desc} -> {col, :asc}
      {^col, :asc} -> {col, :desc}
      _ -> {col, :desc}
    end
  end

  @doc "Sort a list of row maps by the `{column, direction}` tuple."
  def sort(rows, {col, dir}) when dir in [:asc, :desc] do
    Enum.sort_by(rows, fn row -> normalize(Map.get(row, col)) end, dir)
  end

  def sort(rows, _), do: rows

  # Case-insensitive for text; everything else compares as-is.
  defp normalize(v) when is_binary(v), do: String.downcase(v)
  defp normalize(v), do: v
end
