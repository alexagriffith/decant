defmodule DecantWeb.InsightsLive do
  use DecantWeb, :live_view

  alias Decant.AgentLauncher
  alias Decant.Daemon

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       page_title: "Insights",
       can_launch: AgentLauncher.can_launch?(),
       agents: AgentLauncher.agents(),
       default_agent: AgentLauncher.default_agent()
     )}
  end

  # Recommendations are global (no date range / filters), and we fetch them in
  # handle_params so the SyncHook's push_patch on archive_updated refreshes them.
  @impl true
  def handle_params(_params, _uri, socket) do
    recs =
      case Daemon.recommendations("all") do
        {:ok, list} when is_list(list) -> list
        _ -> []
      end

    open = Enum.filter(recs, &(&1["status"] == "open"))

    {:noreply,
     assign(socket,
       signals: Enum.filter(open, &(&1["kind"] == "signal")),
       catalog_groups: catalog_groups(Enum.filter(open, &(&1["kind"] == "catalog"))),
       implemented: Enum.filter(recs, &(&1["status"] == "implemented"))
     )}
  end

  @impl true
  def handle_event("launch", %{"agent" => agent, "prompt" => prompt} = params, socket) do
    label = agent_label(socket.assigns.agents, agent)
    key = params["key"]

    case AgentLauncher.launch(agent, prompt, key) do
      :ok -> {:noreply, put_flash(socket, :info, "Opening #{label} in a new terminal.")}
      {:error, msg} -> {:noreply, put_flash(socket, :error, msg)}
    end
  end

  # Group catalog recs into ordered category sections, preserving the daemon's
  # ranking order: [{category, [recs]}].
  defp catalog_groups(catalog) do
    catalog
    |> Enum.chunk_by(& &1["category"])
    |> Enum.map(fn [%{"category" => c} | _] = items -> {c, items} end)
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:insights}
      page_title="Insights"
      syncing={@syncing}
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
      <div class="space-y-8">
        <div>
          <h1 class="text-lg font-semibold tracking-tight">Insights</h1>
          <p class="text-sm text-muted">
            What could make your coding agents better, drawn from your archive.
          </p>
        </div>

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
            <div :for={s <- @signals} class="card-surface flex flex-col p-4">
              <div class="flex items-center gap-3">
                <span class={[
                  "grid size-9 shrink-0 place-items-center rounded-lg",
                  tone_tint(s["tone"])
                ]}>
                  <.icon name={s["icon"] || "hero-light-bulb"} class="size-5" />
                </span>
                <span class="text-sm font-semibold">{s["title"]}</span>
              </div>
              <p :if={s["detail"]} class="mt-2 text-sm text-muted">{s["detail"]}</p>
              <div :if={s["suggestion"]} class="mt-3 border-l-2 border-line pl-3 text-sm text-muted">
                <span class="text-xs font-medium tracking-wide text-faint uppercase">Suggested</span>
                <p class="mt-0.5">{s["suggestion"]}</p>
              </div>
              <div class="mt-auto flex items-center justify-between gap-2 pt-4">
                <.agent_cta
                  :if={s["prompt"]}
                  id={"sig-#{s["key"]}"}
                  prompt={s["prompt"]}
                  mark_key={s["key"]}
                  agents={@agents}
                  default={@default_agent}
                  can_launch={@can_launch}
                />
                <.doc_link :if={s["url"]} url={s["url"]} label={s["link_label"]} />
              </div>
            </div>
          </div>
        </section>

        <section :if={@catalog_groups != []} class="space-y-5">
          <div>
            <h2 class="text-sm font-semibold tracking-tight">Recommended for coding agents</h2>
            <p class="text-xs text-muted">
              Set these up to make your agents faster and more consistent
            </p>
          </div>

          <div :for={{{category, items}, gi} <- Enum.with_index(@catalog_groups)} class="space-y-3">
            <h3 class="text-xs font-medium tracking-wide text-faint uppercase">{category}</h3>
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <div
                :for={{c, ci} <- Enum.with_index(items)}
                class={[
                  "card-surface flex flex-col p-4",
                  spotlight?(gi, ci) && "ring-1 ring-accent/25 sm:col-span-2 lg:col-span-2"
                ]}
              >
                <div class="flex items-center gap-2.5">
                  <span class={[
                    "grid size-8 shrink-0 place-items-center rounded-lg",
                    (spotlight?(gi, ci) && "bg-accent/10 text-accent") || "bg-elevated text-muted"
                  ]}>
                    <.icon name={c["icon"] || "hero-sparkles"} class="size-4" />
                  </span>
                  <h4 class={["font-semibold", (spotlight?(gi, ci) && "text-base") || "text-sm"]}>
                    {c["title"]}
                  </h4>
                </div>
                <p :if={c["detail"]} class="mt-2 text-sm text-muted">{c["detail"]}</p>
                <div class="mt-auto flex items-center justify-between gap-2 pt-4">
                  <.agent_cta
                    :if={c["prompt"]}
                    id={"rec-#{c["key"]}"}
                    prompt={c["prompt"]}
                    mark_key={c["key"]}
                    agents={@agents}
                    default={@default_agent}
                    can_launch={@can_launch}
                  />
                  <.doc_link :if={c["url"]} url={c["url"]} label={c["link_label"]} />
                </div>
              </div>
            </div>
          </div>
        </section>

        <section :if={@implemented != []} class="space-y-3">
          <div>
            <h2 class="text-sm font-semibold tracking-tight text-muted">Implemented</h2>
            <p class="text-xs text-faint">Recommendations you have already set up</p>
          </div>

          <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <div
              :for={r <- @implemented}
              class="flex items-start gap-2.5 rounded-lg border border-line bg-elevated/40 p-3"
            >
              <span class="mt-0.5 grid size-6 shrink-0 place-items-center rounded-full bg-success/10 text-success">
                <.icon name="hero-check" class="size-4" />
              </span>
              <div class="min-w-0">
                <p class="text-sm font-medium text-muted">{r["title"]}</p>
                <p class="text-xs text-faint">{done_note(r)}</p>
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

  # The first card of the first category group is the wide spotlight.
  defp spotlight?(0, 0), do: true
  defp spotlight?(_group_index, _card_index), do: false

  # A short past-tense note for the Implemented section.
  defp done_note(%{"status_source" => "agent"}), do: "Done by an agent"
  defp done_note(%{"status_source" => "activity"}), do: "Detected in your sessions"
  defp done_note(_), do: "Marked done"

  defp tone_tint("accent"), do: "bg-accent/10 text-accent"
  defp tone_tint("success"), do: "bg-success/10 text-success"
  defp tone_tint("warning"), do: "bg-warning/10 text-warning"
  defp tone_tint("danger"), do: "bg-danger/10 text-danger"
  defp tone_tint("info"), do: "bg-info/10 text-info"
  defp tone_tint(_), do: "bg-elevated text-muted"
end
