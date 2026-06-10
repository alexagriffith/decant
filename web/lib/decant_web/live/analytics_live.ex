defmodule DecantWeb.AnalyticsLive do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters
  alias DecantWeb.TableSort

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Analytics",
       model_sort: {:cost, :desc},
       project_sort: {:cost, :desc},
       expanded: MapSet.new(),
       worktree_rows: %{}
     )}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)

    by_model = Archive.by_dimension(:model, filters) |> TableSort.sort(socket.assigns.model_sort)

    by_project =
      Archive.by_dimension(:project, filters)
      |> Enum.sort_by(&(&1.cost || 0), :desc)
      |> Enum.take(12)
      |> TableSort.sort(socket.assigns.project_sort)

    by_day = Archive.by_dimension(:day, filters) |> reject_blank() |> Enum.sort_by(& &1.key)
    days = Enum.map(by_day, & &1.key)
    activity = Archive.activity(filters)

    {:noreply,
     assign(socket,
       filters: filters,
       totals: Archive.totals(filters),
       by_model: by_model,
       by_project: by_project,
       expanded: MapSet.new(),
       worktree_rows: %{},
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
       },
       peak_hour: peak_label(activity.by_hour, &hour_label/1),
       peak_day: peak_label(activity.by_weekday, &weekday_label/1),
       hour_spec: %{
         type: "bar",
         categories: Enum.map(0..23, &hour_label/1),
         series: [%{name: "sessions", data: activity.by_hour}],
         y_format: "int"
       },
       weekday_spec: %{
         type: "bar",
         categories: Enum.map(0..6, &weekday_label/1),
         series: [%{name: "sessions", data: activity.by_weekday}],
         y_format: "int"
       }
     )}
  end

  defp peak_label(counts, labeler) do
    max = Enum.max(counts, fn -> 0 end)

    if max > 0 do
      idx = Enum.find_index(counts, &(&1 == max))
      labeler.(idx)
    end
  end

  defp hour_label(0), do: "12a"
  defp hour_label(12), do: "12p"
  defp hour_label(h) when h < 12, do: "#{h}a"
  defp hour_label(h), do: "#{h - 12}p"

  defp weekday_label(d), do: Enum.at(~w(Sun Mon Tue Wed Thu Fri Sat), d)

  # Display name for a project key: the last path segment, or the key itself for
  # synthetic (path-less) root keys.
  defp basename(key) do
    key |> to_string() |> String.trim_trailing("/") |> String.split("/") |> List.last()
  end

  @impl true
  def handle_event("sort", %{"table" => "model", "col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.model_sort, col)

    {:noreply,
     assign(socket, model_sort: sort, by_model: TableSort.sort(socket.assigns.by_model, sort))}
  end

  def handle_event("sort", %{"table" => "project", "col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.project_sort, col)

    {:noreply,
     assign(socket,
       project_sort: sort,
       by_project: TableSort.sort(socket.assigns.by_project, sort)
     )}
  end

  def handle_event("toggle_project", %{"key" => key}, socket) do
    if MapSet.member?(socket.assigns.expanded, key) do
      {:noreply,
       assign(socket,
         expanded: MapSet.delete(socket.assigns.expanded, key),
         worktree_rows: Map.delete(socket.assigns.worktree_rows, key)
       )}
    else
      rows = Archive.by_dimension(:project, Map.put(socket.assigns.filters, :root, key))

      {:noreply,
       assign(socket,
         expanded: MapSet.put(socket.assigns.expanded, key),
         worktree_rows: Map.put(socket.assigns.worktree_rows, key, rows)
       )}
    end
  end

  defp reject_blank(rows), do: Enum.reject(rows, &(&1.key in [nil, ""]))

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:analytics}
      page_title="Analytics"
      syncing={@syncing}
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
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

        <div :if={@totals.sessions > 0} class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <.panel title="Busiest hours">
            <:subtitle>
              {(@peak_hour && "Local time, you ship most around #{@peak_hour}") ||
                "Sessions by hour, local time"}
            </:subtitle>
            <.chart id="chart-hours" spec={@hour_spec} class="h-56" />
          </.panel>
          <.panel title="Busiest days">
            <:subtitle>
              {(@peak_day && "You ship most on #{@peak_day}") || "Sessions by weekday"}
            </:subtitle>
            <.chart id="chart-weekday" spec={@weekday_spec} class="h-56" />
          </.panel>
        </div>

        <.panel title="By model" body_class="p-0">
          <:subtitle>Trend is sessions per day over the selected range</:subtitle>
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <.sort_header col="key" label="Model" sort={@model_sort} table="model" />
                  <th class="px-4 py-2.5">Trend</th>
                  <.sort_header
                    col="sessions"
                    label="Sessions"
                    sort={@model_sort}
                    table="model"
                    align="right"
                  />
                  <.sort_header
                    col="input_tokens"
                    label="In tok"
                    sort={@model_sort}
                    table="model"
                    align="right"
                  />
                  <.sort_header
                    col="output_tokens"
                    label="Out tok"
                    sort={@model_sort}
                    table="model"
                    align="right"
                  />
                  <.sort_header
                    col="cost"
                    label="Cost"
                    sort={@model_sort}
                    table="model"
                    align="right"
                  />
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
                      tone={brand_tone(r.key)}
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
                    <.bar fraction={(r.cost || 0) / @max_cost} tone={brand_tone(r.key)} />
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
                  <.sort_header col="key" label="Project" sort={@project_sort} table="project" />
                  <.sort_header
                    col="sessions"
                    label="Sessions"
                    sort={@project_sort}
                    table="project"
                    align="right"
                  />
                  <.sort_header
                    col="cost"
                    label="Cost"
                    sort={@project_sort}
                    table="project"
                    align="right"
                  />
                </tr>
              </thead>
              <tbody>
                <%= for r <- @by_project do %>
                  <tr
                    phx-click={JS.navigate(Filters.url(~p"/", Map.put(@filters, :project, r.key)))}
                    class="cursor-pointer border-b border-line/60 transition-colors hover:bg-elevated"
                  >
                    <td class="max-w-xl truncate px-4 py-2.5 font-mono text-xs text-fg" title={r.key}>
                      {basename(r.key)}
                      <%!-- LiveView dispatches the innermost phx-click (closestPhxBinding), so
                           the parent row's JS.navigate does NOT fire when this button is clicked.
                           No onclick="event.stopPropagation()" needed — that would break phx-click. --%>
                      <button
                        :if={(r.worktree_count || 0) > 0}
                        type="button"
                        phx-click={JS.push("toggle_project", value: %{key: r.key})}
                        aria-expanded={to_string(MapSet.member?(@expanded, r.key))}
                        class="ml-2 rounded px-1 text-[10px] text-muted hover:text-fg"
                      >
                        {(MapSet.member?(@expanded, r.key) && "▾") || "▸"} {r.worktree_count} wt
                      </button>
                    </td>
                    <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.sessions)}</td>
                    <td class="px-4 py-2.5 text-right tabular-nums">{money(r.cost)}</td>
                  </tr>
                  <tr
                    :for={w <- Map.get(@worktree_rows, r.key, [])}
                    class="border-b border-line/40 bg-elevated/40"
                  >
                    <td class="truncate px-4 py-2 pl-10 font-mono text-xs text-muted">
                      wt: {w.worktree_label || basename(w.key)}<span :if={w.worktree_tool}>&nbsp;({w.worktree_tool})</span>
                    </td>
                    <td class="px-4 py-2 text-right tabular-nums text-muted">{int(w.sessions)}</td>
                    <td class="px-4 py-2 text-right tabular-nums text-muted">{money(w.cost)}</td>
                  </tr>
                <% end %>
              </tbody>
            </table>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end
end
