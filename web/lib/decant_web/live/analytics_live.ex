defmodule DecantWeb.AnalyticsLive do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    by_day = Archive.by_dimension(:day) |> Enum.sort_by(& &1.key)

    {:ok,
     assign(socket,
       page_title: "decant — analytics",
       totals: Archive.totals(),
       by_model: Archive.by_dimension(:model),
       chart: day_chart(by_day)
     )}
  end

  # Best-effort SVG bar chart of sessions/day; degrade to nil on any contex issue.
  defp day_chart(by_day) do
    rows =
      by_day
      |> Enum.reject(&(&1.key in [nil, ""]))
      |> Enum.map(&[&1.key, &1.sessions])

    if rows == [] do
      nil
    else
      Contex.Dataset.new(rows, ["day", "sessions"])
      |> Contex.Plot.new(Contex.BarChart, 820, 280)
      |> Contex.Plot.to_svg()
    end
  rescue
    _ -> nil
  end

  defp money(n), do: :erlang.float_to_binary((n || 0) * 1.0, decimals: 2)

  @impl true
  def render(assigns) do
    ~H"""
    <div class="p-6 max-w-5xl mx-auto">
      <.link navigate={~p"/"} class="text-blue-600 hover:underline">← sessions</.link>
      <h1 class="text-2xl font-bold mt-2">analytics</h1>

      <div class="grid grid-cols-3 gap-3 mt-4">
        <div class="p-3 rounded bg-base-200">
          <div class="text-xs text-gray-500">sessions</div>
          <div class="text-xl font-bold">{@totals.sessions}</div>
        </div>
        <div class="p-3 rounded bg-base-200">
          <div class="text-xs text-gray-500">messages</div>
          <div class="text-xl font-bold">{@totals.messages}</div>
        </div>
        <div class="p-3 rounded bg-base-200">
          <div class="text-xs text-gray-500">tool calls</div>
          <div class="text-xl font-bold">{@totals.tool_calls}</div>
        </div>
        <div class="p-3 rounded bg-base-200">
          <div class="text-xs text-gray-500">input tokens</div>
          <div class="text-xl font-bold">{@totals.input_tokens}</div>
        </div>
        <div class="p-3 rounded bg-base-200">
          <div class="text-xs text-gray-500">output tokens</div>
          <div class="text-xl font-bold">{@totals.output_tokens}</div>
        </div>
        <div class="p-3 rounded bg-base-200">
          <div class="text-xs text-gray-500">est. cost</div>
          <div class="text-xl font-bold">${money(@totals.cost)}</div>
        </div>
      </div>

      <div :if={@chart} class="mt-6 overflow-x-auto">
        <h2 class="font-semibold mb-2">sessions per day</h2>
        {@chart}
      </div>

      <h2 class="font-semibold mt-6 mb-2">by model</h2>
      <table class="w-full text-sm border-collapse">
        <thead>
          <tr class="text-left border-b">
            <th class="p-2">model</th>
            <th class="p-2 text-right">sessions</th>
            <th class="p-2 text-right">in tok</th>
            <th class="p-2 text-right">out tok</th>
            <th class="p-2 text-right">cost$</th>
          </tr>
        </thead>
        <tbody>
          <tr :for={r <- @by_model} class="border-b">
            <td class="p-2">{r.key}</td>
            <td class="p-2 text-right">{r.sessions}</td>
            <td class="p-2 text-right">{r.input_tokens}</td>
            <td class="p-2 text-right">{r.output_tokens}</td>
            <td class="p-2 text-right">{money(r.cost)}</td>
          </tr>
        </tbody>
      </table>
    </div>
    """
  end
end
