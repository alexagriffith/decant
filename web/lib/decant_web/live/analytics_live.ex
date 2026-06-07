defmodule DecantWeb.AnalyticsLive do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters

  @impl true
  def mount(_params, _session, socket) do
    {:ok, assign(socket, bounds: Archive.date_bounds(), page_title: "Analytics")}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)

    by_model = Archive.by_dimension(:model, filters) |> Enum.sort_by(& &1.cost, :desc)
    by_project = Archive.by_dimension(:project, filters) |> Enum.sort_by(& &1.cost, :desc)
    by_day = Archive.by_dimension(:day, filters) |> reject_blank() |> Enum.sort_by(& &1.key)
    days = Enum.map(by_day, & &1.key)

    {:noreply,
     assign(socket,
       filters: filters,
       totals: Archive.totals(filters),
       by_model: by_model,
       by_project: Enum.take(by_project, 12),
       sparks: Archive.model_sparklines(filters),
       max_cost: max(1.0e-9, Enum.reduce(by_model, 0.0, fn r, a -> max(a, r.cost || 0) end)),
       sessions_spec: %{
         type: "bar",
         categories: days,
         series: [%{name: "sessions", data: Enum.map(by_day, & &1.sessions)}],
         y_format: "int"
       },
       cost_spec: %{
         type: "line",
         categories: days,
         series: [%{name: "cost", data: Enum.map(by_day, &(&1.cost || 0))}],
         y_format: "money",
         smooth: true
       }
     )}
  end

  defp reject_blank(rows), do: Enum.reject(rows, &(&1.key in [nil, ""]))

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash} active={:analytics} page_title="Analytics" syncing={@syncing}>
      <div class="space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold tracking-tight">Analytics</h1>
            <p class="text-sm text-muted">
              Usage and cost across your sessions. Click any row to drill in.
            </p>
          </div>
          <.date_range filters={@filters} bounds={@bounds} path={~p"/analytics"} />
        </div>

        <.filter_chips filters={@filters} path={~p"/analytics"} />

        <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
          <.stat_card
            label="Sessions"
            value={int(@totals.sessions)}
            icon="hero-rectangle-stack"
            tone={:accent}
          />
          <.stat_card
            label="Messages"
            value={int(@totals.messages)}
            icon="hero-chat-bubble-left-right"
            tone={:info}
          />
          <.stat_card
            label="Tool calls"
            value={int(@totals.tool_calls)}
            icon="hero-bolt"
            tone={:warning}
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
              title="No data in range"
              message="Widen the date range."
            />
          </.panel>
          <.panel title="Cost per day">
            <.chart :if={@cost_spec.categories != []} id="chart-cost" spec={@cost_spec} class="h-64" />
            <.empty_state
              :if={@cost_spec.categories == []}
              title="No data in range"
              message="Widen the date range."
            />
          </.panel>
        </div>

        <.panel title="By model" body_class="p-0">
          <:subtitle>Trend = sessions/day over the selected range</:subtitle>
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <th class="px-4 py-2.5">Model</th>
                  <th class="px-4 py-2.5">Trend</th>
                  <th class="px-4 py-2.5 text-right">Sessions</th>
                  <th class="px-4 py-2.5 text-right">In tok</th>
                  <th class="px-4 py-2.5 text-right">Out tok</th>
                  <th class="px-4 py-2.5 text-right">Cost</th>
                  <th class="px-4 py-2.5">Share</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @by_model}
                  phx-click={JS.navigate(Filters.url(~p"/", Map.put(@filters, :model, r.key)))}
                  class="cursor-pointer border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-4 py-2.5"><.model_badge model={r.key} /></td>
                  <td class="px-4 py-2.5">
                    <.sparkline
                      values={@sparks[r.key] || []}
                      tone={model_tone(r.key)}
                      class="h-6 w-28"
                    />
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums">{int(r.sessions)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">
                    {compact(r.input_tokens)}
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">
                    {compact(r.output_tokens)}
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums">{money(r.cost)}</td>
                  <td class="w-40 px-4 py-2.5">
                    <.bar fraction={(r.cost || 0) / @max_cost} tone={model_tone(r.key)} />
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>

        <.panel :if={@by_project != []} title="By project" body_class="p-0">
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <th class="px-4 py-2.5">Project</th>
                  <th class="px-4 py-2.5 text-right">Sessions</th>
                  <th class="px-4 py-2.5 text-right">Cost</th>
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @by_project}
                  phx-click={JS.navigate(Filters.url(~p"/", Map.put(@filters, :project, r.key)))}
                  class="cursor-pointer border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="max-w-xl truncate px-4 py-2.5 font-mono text-xs text-fg">{r.key}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.sessions)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums">{money(r.cost)}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end
end
