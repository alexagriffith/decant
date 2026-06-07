defmodule DecantWeb.InsightsLive do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias Decant.Insights
  alias DecantWeb.Filters

  @impl true
  def mount(_params, _session, socket) do
    {:ok, assign(socket, bounds: Archive.date_bounds(), page_title: "Insights")}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)

    {:noreply,
     assign(socket,
       filters: filters,
       signals: Insights.signals(filters),
       catalog: Insights.catalog()
     )}
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash} active={:insights} page_title="Insights" syncing={@syncing}>
      <div class="space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold tracking-tight">Insights</h1>
            <p class="text-sm text-muted">
              What could make your coding agents better, drawn from your archive.
            </p>
          </div>
          <.date_range filters={@filters} bounds={@bounds} path={~p"/insights"} />
        </div>

        <.filter_chips filters={@filters} path={~p"/insights"} />

        <section class="space-y-3">
          <h2 class="text-sm font-semibold tracking-tight">Signals</h2>

          <.empty_state
            :if={@signals == []}
            icon="hero-light-bulb"
            title="No signals yet"
            message="Sync more sessions to surface patterns."
          />

          <div :if={@signals != []} class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div :for={s <- @signals} class="card-surface p-4">
              <div class="flex items-center gap-3">
                <span class={["grid size-9 place-items-center rounded-lg", tone_tint(s.tone)]}>
                  <.icon name={s.icon} class="size-5" />
                </span>
                <span class="font-semibold text-sm">{s.title}</span>
              </div>
              <p class="text-sm text-muted mt-1">{s.detail}</p>
              <div class="mt-3 border-l-2 border-accent/40 pl-3 text-sm text-muted">
                <span class="text-xs font-medium tracking-wide text-faint uppercase">Suggested</span>
                <p>{s.suggestion}</p>
              </div>
            </div>
          </div>
        </section>

        <.panel title="Recommended for coding agents">
          <:subtitle>Tools and integrations worth wiring into your agents</:subtitle>
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <a
              :for={c <- @catalog}
              href={c.url}
              target="_blank"
              rel="noreferrer"
              class="block card-surface p-4 hover:bg-elevated transition-colors"
            >
              <div class="flex items-center gap-2 font-medium">
                <.icon name={c.icon} class="size-4 shrink-0 text-muted" />
                <span>{c.title}</span>
                <.icon name="hero-arrow-top-right-on-square" class="size-3 text-faint" />
              </div>
              <p class="text-sm text-muted mt-1">{c.detail}</p>
            </a>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end

  defp tone_tint(:accent), do: "bg-accent/10 text-accent"
  defp tone_tint(:success), do: "bg-success/10 text-success"
  defp tone_tint(:warning), do: "bg-warning/10 text-warning"
  defp tone_tint(:danger), do: "bg-danger/10 text-danger"
  defp tone_tint(:info), do: "bg-info/10 text-info"
  defp tone_tint(_), do: "bg-elevated text-muted"
end
