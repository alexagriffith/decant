defmodule DecantWeb.InsightsLive do
  use DecantWeb, :live_view

  alias Decant.AgentLauncher
  alias Decant.Insights
  alias Decant.Archive
  alias DecantWeb.Filters

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Insights",
       can_launch: AgentLauncher.can_launch?(),
       agents: AgentLauncher.agents(),
       default_agent: AgentLauncher.default_agent()
     )}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)

    {:noreply,
     assign(socket,
       filters: filters,
       signals: Insights.signals(filters),
       catalog_groups: Insights.catalog_groups()
     )}
  end

  @impl true
  def handle_event("launch", %{"agent" => agent, "prompt" => prompt}, socket) do
    label = agent_label(socket.assigns.agents, agent)

    case AgentLauncher.launch(agent, prompt) do
      :ok -> {:noreply, put_flash(socket, :info, "Opening #{label} in a new terminal.")}
      {:error, msg} -> {:noreply, put_flash(socket, :error, msg)}
    end
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash} active={:insights} page_title="Insights" syncing={@syncing}>
      <div class="space-y-8">
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
          <div>
            <h2 class="text-sm font-semibold tracking-tight">Signals</h2>
            <p class="text-xs text-muted">Patterns worth acting on, ranked by impact</p>
          </div>

          <.empty_state
            :if={@signals == []}
            icon="hero-light-bulb"
            title="No signals yet"
            message="Sync more sessions to surface patterns."
          />

          <div :if={@signals != []} class="grid grid-cols-1 gap-3 md:grid-cols-2">
            <div :for={{s, i} <- Enum.with_index(@signals)} class="card-surface flex flex-col p-4">
              <div class="flex items-center gap-3">
                <span class={["grid size-9 shrink-0 place-items-center rounded-lg", tone_tint(s.tone)]}>
                  <.icon name={s.icon} class="size-5" />
                </span>
                <span class="text-sm font-semibold">{s.title}</span>
              </div>
              <p class="mt-2 text-sm text-muted">{s.detail}</p>
              <div class="mt-3 border-l-2 border-line pl-3 text-sm text-muted">
                <span class="text-xs font-medium tracking-wide text-faint uppercase">Suggested</span>
                <p class="mt-0.5">{s.suggestion}</p>
              </div>
              <div class="mt-auto flex items-center justify-between gap-2 pt-4">
                <.agent_cta
                  id={"sig-#{i}"}
                  prompt={s.prompt}
                  agents={@agents}
                  default={@default_agent}
                  can_launch={@can_launch}
                />
                <.doc_link :if={s[:url]} url={s.url} label={s[:link_label]} />
              </div>
            </div>
          </div>
        </section>

        <section class="space-y-5">
          <div>
            <h2 class="text-sm font-semibold tracking-tight">Recommended for coding agents</h2>
            <p class="text-xs text-muted">
              Set these up to make your agents faster and more consistent
            </p>
          </div>

          <div :for={{category, items} <- @catalog_groups} class="space-y-3">
            <h3 class="text-xs font-medium tracking-wide text-faint uppercase">{category}</h3>
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <div
                :for={c <- items}
                class={[
                  "card-surface flex flex-col p-4",
                  c[:spotlight] && "ring-1 ring-accent/25 sm:col-span-2 lg:col-span-2"
                ]}
              >
                <div class="flex items-center gap-2.5">
                  <span class={[
                    "grid size-8 shrink-0 place-items-center rounded-lg",
                    (c[:spotlight] && "bg-accent/10 text-accent") || "bg-elevated text-muted"
                  ]}>
                    <.icon name={c.icon} class="size-4" />
                  </span>
                  <h4 class={["font-semibold", (c[:spotlight] && "text-base") || "text-sm"]}>
                    {c.title}
                  </h4>
                </div>
                <p class="mt-2 text-sm text-muted">{c.detail}</p>
                <div class="mt-auto flex items-center justify-between gap-2 pt-4">
                  <.agent_cta
                    id={"rec-#{c.key}"}
                    prompt={c.prompt}
                    agents={@agents}
                    default={@default_agent}
                    can_launch={@can_launch}
                  />
                  <.doc_link url={c.url} label={c[:link_label]} />
                </div>
              </div>
            </div>
          </div>
        </section>
      </div>
    </Layouts.app>
    """
  end

  attr :url, :string, required: true
  attr :label, :string, default: nil

  defp doc_link(assigns) do
    ~H"""
    <a
      href={@url}
      target="_blank"
      rel="noreferrer"
      class="inline-flex shrink-0 items-center gap-1 text-xs font-medium text-muted hover:text-fg"
    >
      {@label || "Docs"} <.icon name="hero-arrow-top-right-on-square" class="size-3.5" />
    </a>
    """
  end

  defp agent_label(agents, key) do
    Enum.find_value(agents, key, fn {k, l} -> if k == key, do: l end)
  end

  defp tone_tint(:accent), do: "bg-accent/10 text-accent"
  defp tone_tint(:success), do: "bg-success/10 text-success"
  defp tone_tint(:warning), do: "bg-warning/10 text-warning"
  defp tone_tint(:danger), do: "bg-danger/10 text-danger"
  defp tone_tint(:info), do: "bg-info/10 text-info"
  defp tone_tint(_), do: "bg-elevated text-muted"
end
