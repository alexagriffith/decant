defmodule DecantWeb.AnalyticsLive do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    totals = Archive.totals()

    by_model =
      Archive.by_dimension(:model)
      |> Enum.sort_by(&(&1.cost || 0), :desc)

    by_day =
      Archive.by_dimension(:day)
      |> Enum.reject(&(&1.key in [nil, ""]))
      |> Enum.sort_by(& &1.key)

    days = Enum.map(by_day, & &1.key)
    session_counts = Enum.map(by_day, & &1.sessions)
    costs = Enum.map(by_day, &(&1.cost || 0))

    sessions_spec = %{
      type: "bar",
      categories: days,
      series: [%{name: "sessions", data: session_counts}],
      y_format: "int"
    }

    cost_spec = %{
      type: "line",
      categories: days,
      series: [%{name: "cost", data: costs}],
      y_format: "money",
      smooth: true
    }

    {:ok,
     assign(socket,
       page_title: "decant — analytics",
       totals: totals,
       by_model: by_model,
       sessions_spec: sessions_spec,
       cost_spec: cost_spec
     )}
  end

  @impl true
  def render(assigns) do
    assigns = assign(assigns, :max_cost, max_cost(assigns.by_model))

    ~H"""
    <Layouts.app flash={@flash} active={:analytics} page_title="Analytics" syncing={@syncing}>
      <div class="space-y-6">
        <div>
          <h2 class="text-base font-semibold tracking-tight">Analytics</h2>
          <p class="mt-1 text-sm text-muted">Usage and cost across every recorded session.</p>
        </div>

        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
          <.stat_card
            label="Sessions"
            value={int(@totals.sessions)}
            hint="all time"
            icon="hero-rectangle-stack"
            tone={:accent}
          />
          <.stat_card
            label="Messages"
            value={int(@totals.messages)}
            icon="hero-chat-bubble-left-right"
            tone={:accent}
          />
          <.stat_card
            label="Tool calls"
            value={int(@totals.tool_calls)}
            icon="hero-bolt"
            tone={:accent}
          />
          <.stat_card
            label="Input tokens"
            value={int(@totals.input_tokens)}
            icon="hero-arrow-down-tray"
            tone={:neutral}
          />
          <.stat_card
            label="Output tokens"
            value={int(@totals.output_tokens)}
            icon="hero-arrow-up-tray"
            tone={:neutral}
          />
          <.stat_card
            label="Est. cost"
            value={money(@totals.cost)}
            icon="hero-currency-dollar"
            tone={:success}
          />
        </div>

        <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <.panel title="Sessions per day">
            <.chart
              :if={@sessions_spec.categories != []}
              id="chart-sessions"
              spec={@sessions_spec}
              class="h-64"
            />
            <.empty_state
              :if={@sessions_spec.categories == []}
              icon="hero-chart-bar"
              title="No data yet"
              message="Session activity will appear here once sessions are recorded."
            />
          </.panel>

          <.panel title="Cost per day">
            <.chart
              :if={@cost_spec.categories != []}
              id="chart-cost"
              spec={@cost_spec}
              class="h-64"
            />
            <.empty_state
              :if={@cost_spec.categories == []}
              icon="hero-currency-dollar"
              title="No data yet"
              message="Daily cost will appear here once sessions are recorded."
            />
          </.panel>
        </div>

        <.panel title="By model" body_class="p-0">
          <div :if={@by_model == []} class="p-4 sm:p-5">
            <.empty_state
              icon="hero-cpu-chip"
              title="No models yet"
              message="Model usage will appear here once sessions are recorded."
            />
          </div>

          <div :if={@by_model != []} class="overflow-x-auto">
            <table class="w-full">
              <thead>
                <tr class="border-b border-line">
                  <th class="px-3 py-2.5 text-left text-xs font-medium uppercase tracking-wide text-muted">
                    Model
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium uppercase tracking-wide text-muted">
                    Sessions
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium uppercase tracking-wide text-muted">
                    In tok
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium uppercase tracking-wide text-muted">
                    Out tok
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium uppercase tracking-wide text-muted">
                    Cost
                  </th>
                  <th class="w-40 px-3 py-2.5 text-left text-xs font-medium uppercase tracking-wide text-muted">
                    Share
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @by_model}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-3 py-2.5 text-sm">
                    <.model_badge model={r.key} />
                  </td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums">{int(r.sessions)}</td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums text-muted">
                    {compact(r.input_tokens)}
                  </td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums text-muted">
                    {compact(r.output_tokens)}
                  </td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums">{money(r.cost)}</td>
                  <td class="px-3 py-2.5">
                    <.bar fraction={(r.cost || 0) / @max_cost} tone={:accent} />
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end

  # Max cost across models, guarded so the SHARE bar never divides by zero.
  defp max_cost([]), do: 1.0e-9

  defp max_cost(by_model) do
    by_model
    |> Enum.map(&(&1.cost || 0))
    |> Enum.max(fn -> 0 end)
    |> max(1.0e-9)
  end
end
