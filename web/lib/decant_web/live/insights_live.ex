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

    # Rank signals by score (highest impact first) so the lead/hero is always the
    # top signal regardless of the order the daemon returned them in.
    signals =
      open
      |> Enum.filter(&(&1["kind"] == "signal"))
      |> Enum.sort_by(&(&1["score"] || 0), :desc)

    {:noreply,
     assign(socket,
       signals: signals,
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
    assigns =
      assign(assigns,
        hero: List.first(assigns.signals),
        rest: Enum.drop(assigns.signals, 1)
      )

    ~H"""
    <Layouts.app
      flash={@flash}
      active={:insights}
      page_title="Insights"
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
      <div class="space-y-10">
        <div>
          <h1 class="text-lg font-semibold tracking-tight">Insights</h1>
          <p class="text-sm text-muted">
            What could make your coding agents better, drawn from your archive.
          </p>
        </div>

        <section class="space-y-4">
          <div class="flex items-end justify-between gap-3">
            <div>
              <h2 class="text-sm font-semibold tracking-tight">Promotion candidates</h2>
              <p class="text-xs text-muted">Data-backed lessons ranked by impact</p>
            </div>
            <span :if={@signals != []} class="shrink-0 text-xs text-faint tabular-nums">
              {length(@signals)} active
            </span>
          </div>

          <.empty_state
            :if={@signals == []}
            icon="hero-light-bulb"
            title="No signals yet"
            message="Sync more sessions to surface patterns."
          />

          <.signal_hero
            :if={@hero}
            signal={@hero}
            agents={@agents}
            default={@default_agent}
            can_launch={@can_launch}
          />

          <div :if={@rest != []} class="card-surface divide-y divide-line">
            <.signal_row
              :for={s <- @rest}
              signal={s}
              agents={@agents}
              default={@default_agent}
              can_launch={@can_launch}
            />
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
                <.promotion_panel rec={c} id={"catalog-card-#{c["key"]}"} compact />
                <div class="mt-auto flex items-center justify-between gap-2 pt-4">
                  <.agent_cta
                    :if={c["prompt"]}
                    id={"rec-#{c["key"]}"}
                    prompt={handoff_prompt(c)}
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

  attr :signal, :map, required: true
  attr :agents, :list, required: true
  attr :default, :string, required: true
  attr :can_launch, :boolean, required: true

  defp signal_hero(assigns) do
    ~H"""
    <article class={["card-surface border-l-4 p-5 sm:p-6", tone_border_l(@signal["tone"])]}>
      <div class="flex items-start gap-4">
        <span class={[
          "hidden size-11 shrink-0 place-items-center rounded-xl sm:grid",
          tone_tint(@signal["tone"])
        ]}>
          <.icon name={@signal["icon"] || "hero-light-bulb"} class="size-6" />
        </span>
        <div class="min-w-0 flex-1">
          <span class={[
            "text-[11px] font-semibold tracking-wider uppercase",
            tone_text(@signal["tone"])
          ]}>
            Top signal
          </span>
          <h3 class="mt-1 text-lg font-semibold tracking-tight">{@signal["title"]}</h3>
          <p :if={@signal["detail"]} class="mt-1 text-sm text-muted">{@signal["detail"]}</p>
          <div :if={@signal["suggestion"]} class="mt-3 border-l-2 border-line pl-3">
            <span class="text-[11px] font-semibold tracking-wider text-faint uppercase">
              Suggested
            </span>
            <p class="mt-0.5 text-sm text-muted">{@signal["suggestion"]}</p>
          </div>
          <.promotion_panel rec={@signal} id={"signal-card-#{@signal["key"]}"} />
          <div class="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
            <.agent_cta
              :if={@signal["prompt"]}
              id={"sig-#{@signal["key"]}"}
              prompt={handoff_prompt(@signal)}
              mark_key={@signal["key"]}
              agents={@agents}
              default={@default}
              can_launch={@can_launch}
            />
            <.doc_link :if={@signal["url"]} url={@signal["url"]} label={@signal["link_label"]} />
          </div>
        </div>
      </div>
    </article>
    """
  end

  attr :signal, :map, required: true
  attr :agents, :list, required: true
  attr :default, :string, required: true
  attr :can_launch, :boolean, required: true

  defp signal_row(assigns) do
    ~H"""
    <div class="flex items-center gap-3 px-3 py-3 sm:gap-4 sm:px-4">
      <span class={["h-9 w-1 shrink-0 rounded-full", tone_rail(@signal["tone"])]} aria-hidden="true" />
      <span class={[
        "grid size-8 shrink-0 place-items-center rounded-lg",
        tone_tint(@signal["tone"])
      ]}>
        <.icon name={@signal["icon"] || "hero-light-bulb"} class="size-4" />
      </span>
      <div class="min-w-0 flex-1">
        <p class="truncate text-sm font-medium">{@signal["title"]}</p>
        <p :if={@signal["detail"]} class="truncate text-xs text-faint">{@signal["detail"]}</p>
        <div
          :if={promotion_card?(@signal)}
          class="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-faint"
        >
          <span class="rounded border border-line px-1.5 py-0.5">{@signal["memory_layer"]}</span>
          <span class="truncate">{@signal["promotion_target"]}</span>
        </div>
      </div>
      <div class="shrink-0">
        <.agent_cta
          :if={@signal["prompt"]}
          id={"sig-#{@signal["key"]}"}
          prompt={handoff_prompt(@signal)}
          mark_key={@signal["key"]}
          agents={@agents}
          default={@default}
          can_launch={@can_launch}
          compact
        />
      </div>
    </div>
    """
  end

  attr :url, :string, required: true
  attr :label, :string, default: nil
  attr :class, :any, default: nil

  defp doc_link(assigns) do
    ~H"""
    <a
      href={@url}
      target="_blank"
      rel="noreferrer"
      class={[
        "inline-flex shrink-0 items-center gap-1 text-xs font-medium text-muted hover:text-fg",
        @class
      ]}
    >
      {@label || "Docs"} <.icon name="hero-arrow-top-right-on-square" class="size-3.5" />
    </a>
    """
  end

  attr :rec, :map, required: true
  attr :id, :string, required: true
  attr :compact, :boolean, default: false

  defp promotion_panel(assigns) do
    assigns =
      assigns
      |> assign(:has_card, promotion_card?(assigns.rec))
      |> assign(:copy_id, "#{safe_id(assigns.id)}-copy")
      |> assign(:copy_text, promotion_text(assigns.rec))

    ~H"""
    <div :if={@has_card} class={[(@compact && "mt-3") || "mt-4", "border-t border-line pt-3"]}>
      <div class="mb-2 flex items-center justify-between gap-2">
        <span class="text-[11px] font-semibold tracking-wider text-faint uppercase">
          Memory card
        </span>
        <.copy_button id={@copy_id} text={@copy_text} title="Copy memory card" class="size-6" />
      </div>
      <dl class={[
        "grid gap-x-4 gap-y-2 text-xs",
        (@compact && "grid-cols-1") || "sm:grid-cols-2"
      ]}>
        <div>
          <dt class="font-medium text-faint">Layer</dt>
          <dd class="mt-0.5 text-muted">{@rec["memory_layer"]}</dd>
        </div>
        <div>
          <dt class="font-medium text-faint">Promote to</dt>
          <dd class="mt-0.5 text-muted">{@rec["promotion_target"]}</dd>
        </div>
        <div :if={!@compact} class="sm:col-span-2">
          <dt class="font-medium text-faint">Trigger</dt>
          <dd class="mt-0.5 text-muted">{@rec["trigger"]}</dd>
        </div>
        <div :if={!@compact} class="sm:col-span-2">
          <dt class="font-medium text-faint">Done when</dt>
          <dd class="mt-0.5 text-muted">{@rec["success_metric"]}</dd>
        </div>
      </dl>
    </div>
    """
  end

  defp agent_label(agents, key) do
    Enum.find_value(agents, key, fn {k, l} -> if k == key, do: l end)
  end

  defp promotion_card?(rec) do
    present?(rec["memory_layer"]) or present?(rec["promotion_target"]) or present?(rec["trigger"]) or
      present?(rec["evidence"]) or present?(rec["action"]) or present?(rec["success_metric"])
  end

  defp promotion_text(rec) do
    [
      "# #{rec["title"]}",
      "Key: #{rec["key"]}",
      field("Layer", rec["memory_layer"]),
      field("Promote to", rec["promotion_target"]),
      field("Trigger", rec["trigger"]),
      field("Evidence", rec["evidence"]),
      field("Action", rec["action"]),
      field("Done when", rec["success_metric"])
    ]
    |> Enum.reject(&blank?/1)
    |> Enum.join("\n")
  end

  defp handoff_prompt(rec) do
    [
      rec["prompt"] || rec["action"] || rec["suggestion"],
      "Use this Decant memory card as the evidence and promotion target:\n#{promotion_text(rec)}"
    ]
    |> Enum.reject(&blank?/1)
    |> Enum.join("\n\n")
  end

  defp field(label, value) do
    if present?(value), do: "#{label}: #{value}"
  end

  defp present?(value), do: !blank?(value)
  defp blank?(nil), do: true
  defp blank?(value) when is_binary(value), do: String.trim(value) == ""
  defp blank?(_value), do: false

  defp safe_id(id), do: String.replace(id, ~r/[^A-Za-z0-9_-]/, "-")

  defp spotlight?(0, 0), do: true
  defp spotlight?(_group_index, _card_index), do: false

  defp done_note(%{"status_source" => "agent"}), do: "Done by an agent"
  defp done_note(%{"status_source" => "activity"}), do: "Detected in your sessions"
  defp done_note(_), do: "Marked done"

  defp tone_tint("accent"), do: "bg-accent/10 text-accent"
  defp tone_tint("success"), do: "bg-success/10 text-success"
  defp tone_tint("warning"), do: "bg-warning/10 text-warning"
  defp tone_tint("danger"), do: "bg-danger/10 text-danger"
  defp tone_tint("info"), do: "bg-info/10 text-info"
  defp tone_tint(_), do: "bg-elevated text-muted"

  defp tone_rail("accent"), do: "bg-accent"
  defp tone_rail("success"), do: "bg-success"
  defp tone_rail("warning"), do: "bg-warning"
  defp tone_rail("danger"), do: "bg-danger"
  defp tone_rail("info"), do: "bg-info"
  defp tone_rail(_), do: "bg-line-strong"

  defp tone_text("accent"), do: "text-accent"
  defp tone_text("success"), do: "text-success"
  defp tone_text("warning"), do: "text-warning"
  defp tone_text("danger"), do: "text-danger"
  defp tone_text("info"), do: "text-info"
  defp tone_text(_), do: "text-muted"

  # Tone-colored left border for the hero's severity rail. A border follows the
  # card's rounded corners, so the hero needs no `overflow-hidden` — which would
  # otherwise clip the agent dropdown that extends past the card.
  defp tone_border_l("accent"), do: "border-l-accent"
  defp tone_border_l("success"), do: "border-l-success"
  defp tone_border_l("warning"), do: "border-l-warning"
  defp tone_border_l("danger"), do: "border-l-danger"
  defp tone_border_l("info"), do: "border-l-info"
  defp tone_border_l(_), do: "border-l-line-strong"
end
