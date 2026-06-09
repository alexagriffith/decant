defmodule DecantWeb.Components.UI do
  @moduledoc """
  decant design-system components: panels, stat cards, badges, empty states,
  and the ECharts chart container. All styled with the semantic tokens from
  `assets/css/app.css` so they theme automatically.
  """
  use Phoenix.Component

  import DecantWeb.CoreComponents, only: [icon: 1]
  import DecantWeb.Components.Icons

  alias DecantWeb.Filters
  alias Phoenix.LiveView.JS

  @doc """
  A bordered surface panel. Optional `title`, `subtitle`, and `actions` slots
  render a header row.

      <.panel title="By model"><table>…</table></.panel>
  """
  attr :class, :any, default: nil
  attr :body_class, :any, default: "p-4 sm:p-5"
  attr :title, :string, default: nil
  slot :subtitle
  slot :actions
  slot :inner_block, required: true

  def panel(assigns) do
    ~H"""
    <section class={["card-surface overflow-hidden", @class]}>
      <header
        :if={@title || @subtitle != [] || @actions != []}
        class="flex items-center justify-between gap-4 border-b border-line px-4 py-3 sm:px-5"
      >
        <div class="min-w-0">
          <h2 :if={@title} class="text-sm font-semibold tracking-tight">{@title}</h2>
          <p :if={@subtitle != []} class="text-xs text-muted">{render_slot(@subtitle)}</p>
        </div>
        <div :if={@actions != []} class="flex shrink-0 items-center gap-2">
          {render_slot(@actions)}
        </div>
      </header>
      <div class={@body_class}>{render_slot(@inner_block)}</div>
    </section>
    """
  end

  @doc """
  A metric/stat card: label, large value, optional hint and icon, optional
  `chart` slot for a sparkline.
  """
  attr :label, :string, required: true
  attr :value, :string, required: true
  attr :hint, :string, default: nil
  attr :icon, :string, default: nil

  attr :tone, :atom,
    default: :accent,
    values: [:accent, :neutral, :success, :warning, :danger, :info]

  slot :chart

  def stat_card(assigns) do
    ~H"""
    <div class="card-surface p-4 sm:p-5">
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0">
          <div class="text-xs font-medium tracking-wide text-muted uppercase">{@label}</div>
          <div class="mt-1.5 text-2xl font-semibold tracking-tight tabular-nums">{@value}</div>
          <div :if={@hint} class="mt-1 text-xs text-faint">{@hint}</div>
        </div>
        <span
          :if={@icon}
          class={["grid size-9 shrink-0 place-items-center rounded-lg", tone_soft(@tone)]}
        >
          <.icon name={@icon} class="size-[18px]" />
        </span>
      </div>
      <div :if={@chart != []} class="mt-3">{render_slot(@chart)}</div>
    </div>
    """
  end

  @doc "A small pill badge."
  attr :tone, :atom,
    default: :neutral,
    values: [:neutral, :accent, :success, :warning, :danger, :info, :claude, :openai]

  attr :class, :any, default: nil
  attr :mono, :boolean, default: false
  slot :inner_block, required: true

  def badge(assigns) do
    ~H"""
    <span class={[
      "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-xs font-medium",
      tone_soft(@tone),
      @mono && "font-mono",
      @class
    ]}>
      {render_slot(@inner_block)}
    </span>
    """
  end

  @doc "Badge for a tool (`claude_code` / `codex`), tinted with the brand color."
  attr :tool, :string, default: nil

  def tool_badge(assigns) do
    {tone, label} =
      case assigns.tool do
        "claude_code" -> {:claude, "Claude"}
        "codex" -> {:openai, "Codex"}
        other -> {:neutral, other || "·"}
      end

    assigns = assign(assigns, tone: tone, label: label)

    ~H"""
    <.badge tone={@tone}>
      <.tool_icon :if={@tool in ["claude_code", "codex"]} tool={@tool} class="size-3.5" />{@label}
    </.badge>
    """
  end

  @doc "Badge for a model id, tinted by brand (Claude clay, OpenAI mono), monospace."
  attr :model, :string, default: nil

  def model_badge(assigns) do
    assigns = assign(assigns, :tone, brand_tone(assigns.model))

    ~H"""
    <.badge :if={@model} tone={@tone} mono>
      <.model_icon model={@model} class="size-3.5" />{@model}
    </.badge>
    <span :if={!@model} class="text-xs text-faint">·</span>
    """
  end

  @doc "Brand tone for a model id: Claude family -> clay, OpenAI family -> mono, else neutral."
  def brand_tone(model) do
    case DecantWeb.Components.Icons.brand(model) do
      :claude -> :claude
      :openai -> :openai
      _ -> :neutral
    end
  end

  @doc """
  A clickable, sortable table header cell. Pushes a "sort" event carrying the
  column and table name; shows a direction chevron when this column is active.
  Pair with `DecantWeb.TableSort` in the LiveView.
  """
  attr :col, :string, required: true
  attr :label, :string, required: true
  attr :sort, :any, required: true, doc: "current {column_atom, :asc | :desc}"
  attr :table, :string, default: ""
  attr :align, :string, default: "left", values: ["left", "right"]

  def sort_header(assigns) do
    {scol, sdir} = assigns.sort
    assigns = assign(assigns, active: to_string(scol) == assigns.col, dir: sdir)

    ~H"""
    <th class={["px-4 py-2.5", @align == "right" && "text-right"]}>
      <button
        type="button"
        phx-click="sort"
        phx-value-table={@table}
        phx-value-col={@col}
        class={[
          "group inline-flex items-center gap-1 hover:text-fg",
          @align == "right" && "flex-row-reverse"
        ]}
      >
        <span>{@label}</span>
        <.icon
          name={(@active && @dir == :asc && "hero-chevron-up") || "hero-chevron-down"}
          class={[
            "size-3 transition-opacity",
            (@active && "text-fg opacity-100") || "opacity-0 group-hover:opacity-40"
          ]}
        />
      </button>
    </th>
    """
  end

  @doc "A friendly empty / no-results state."
  attr :icon, :string, default: "hero-inbox"
  attr :title, :string, required: true
  attr :message, :string, default: nil
  slot :inner_block

  def empty_state(assigns) do
    ~H"""
    <div class="flex flex-col items-center justify-center rounded-xl border border-dashed border-line bg-grid px-6 py-16 text-center">
      <span class="grid size-12 place-items-center rounded-xl bg-elevated text-muted">
        <.icon name={@icon} class="size-6" />
      </span>
      <h3 class="mt-4 text-sm font-semibold">{@title}</h3>
      <p :if={@message} class="mt-1 max-w-sm text-sm text-muted">{@message}</p>
      <div :if={@inner_block != []} class="mt-4">{render_slot(@inner_block)}</div>
    </div>
    """
  end

  @doc """
  An interactive ECharts container. Pass a normalized `spec` (no ECharts API
  knowledge needed); the `Chart` JS hook builds the option, injects theme-aware
  colors from CSS variables, and re-themes on theme change. Render it only once
  data is ready so the hook mounts with data.

  `spec` shape:

      %{
        type: "bar" | "line",
        categories: ["2026-06-01", ...],
        series: [%{name: "sessions", data: [1, 2, 3], type: "bar" | "line"}],
        y_format: "int" | "money",   # optional, default "int"
        smooth: true                  # optional (line charts)
      }

      <.chart id="sessions-day" spec={@day_chart} class="h-72" />
  """
  attr :id, :string, required: true
  attr :spec, :map, required: true
  attr :class, :any, default: "h-72"

  def chart(assigns) do
    assigns = assign(assigns, :json, Jason.encode!(assigns.spec))

    ~H"""
    <div id={@id} phx-hook="Chart" phx-update="ignore" data-spec={@json} class={["w-full", @class]} />
    """
  end

  @doc "A simple horizontal proportion bar (e.g. cost share in a table)."
  attr :fraction, :float, required: true
  attr :tone, :atom, default: :accent
  attr :class, :any, default: nil

  def bar(assigns) do
    pct = max(0.0, min(1.0, assigns.fraction || 0.0)) * 100

    assigns = assign(assigns, :pct, pct)

    ~H"""
    <div class={["h-1.5 w-full overflow-hidden rounded-full bg-elevated", @class]}>
      <div class={["h-full rounded-full", tone_bar(@tone)]} style={"width: #{@pct}%"} />
    </div>
    """
  end

  @doc """
  Copy-to-clipboard icon button. The clipboard glyph swaps to a check on click
  (handled by the `Copy` JS hook). Use this everywhere instead of a "Copy" word.
  """
  attr :id, :string, required: true
  attr :text, :string, required: true
  attr :title, :string, default: "Copy"
  attr :class, :any, default: nil

  def copy_button(assigns) do
    ~H"""
    <button
      id={@id}
      type="button"
      phx-hook="Copy"
      data-copy={@text}
      aria-label={@title}
      title={@title}
      class={[
        "inline-grid size-7 shrink-0 place-items-center rounded-md text-muted transition-colors hover:bg-elevated hover:text-fg",
        @class
      ]}
    >
      <span data-copy-icon class="hero-clipboard-document size-4"></span>
    </button>
    """
  end

  @doc """
  Primary call to action that opens a coding agent in a terminal seeded with a
  prompt. Renders a split button. The left segment launches the preferred
  agent in one click; the chevron opens a small menu for the others. When
  opening a terminal is not available it falls back to a copy button for the
  prompt. Requires a parent LiveView that handles the "launch" event.
  """
  attr :id, :string, required: true
  attr :prompt, :string, required: true
  attr :agents, :list, required: true, doc: "list of {key, label}"
  attr :default, :string, required: true, doc: "preferred agent key"
  attr :can_launch, :boolean, default: false

  attr :mark_key, :string,
    default: nil,
    doc: "recommendation key to mark implemented after launch"

  def agent_cta(assigns) do
    assigns =
      assign(
        assigns,
        :default_label,
        Enum.find_value(assigns.agents, "agent", fn {k, l} -> if k == assigns.default, do: l end)
      )

    ~H"""
    <div :if={@can_launch} class="relative inline-flex">
      <button
        type="button"
        phx-click="launch"
        phx-value-agent={@default}
        phx-value-prompt={@prompt}
        phx-value-key={@mark_key}
        class="inline-flex items-center gap-1.5 rounded-l-lg border border-r-0 border-line bg-surface px-3 py-1.5 text-sm font-medium text-fg transition-colors hover:bg-elevated"
      >
        <.tool_icon tool={agent_tool(@default)} class="size-4" /> Run in {@default_label}
      </button>
      <button
        type="button"
        phx-click={JS.toggle(to: "##{@id}-menu")}
        aria-haspopup="true"
        aria-label="Choose another agent"
        class="grid place-items-center rounded-r-lg border border-line bg-surface px-1.5 text-muted transition-colors hover:bg-elevated hover:text-fg"
      >
        <.icon name="hero-chevron-down" class="size-4" />
      </button>
      <div
        id={"#{@id}-menu"}
        hidden
        phx-click-away={JS.hide(to: "##{@id}-menu")}
        class="absolute top-full right-0 z-20 mt-1 w-44 overflow-hidden rounded-lg border border-line bg-overlay py-1 shadow-lg"
      >
        <button
          :for={{key, label} <- @agents}
          type="button"
          phx-click={
            JS.hide(to: "##{@id}-menu")
            |> JS.push("launch", value: %{agent: key, prompt: @prompt, key: @mark_key})
          }
          class="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm text-fg hover:bg-elevated"
        >
          <.tool_icon tool={agent_tool(key)} class="size-4" /> Run in {label}
        </button>
      </div>
    </div>

    <.copy_button
      :if={!@can_launch}
      id={"#{@id}-copy"}
      text={@prompt}
      title="Copy the setup prompt"
    />
    """
  end

  # Map an agent key to the tool id its brand mark is keyed on.
  defp agent_tool("claude"), do: "claude_code"
  defp agent_tool(_), do: "codex"

  @doc "Date-range navigator with previous and next window, presets, and a range label."
  attr :filters, :map, required: true
  attr :bounds, :map, required: true
  attr :path, :string, required: true

  def date_range(assigns) do
    assigns = assign(assigns, :active, Filters.active_preset(assigns.filters, assigns.bounds))

    ~H"""
    <div class="flex flex-wrap items-center gap-2">
      <div class="flex items-center rounded-lg border border-line bg-surface p-0.5 text-sm">
        <.link
          :if={@filters.from}
          patch={Filters.url(@path, Filters.shift(@filters, :prev))}
          class="grid size-7 place-items-center rounded-md text-muted hover:bg-elevated hover:text-fg"
          aria-label="Previous period"
        >
          <.icon name="hero-chevron-left" class="size-4" />
        </.link>
        <.link
          :for={{key, _days} <- Filters.presets()}
          patch={Filters.url(@path, Filters.apply_preset(@filters, key, @bounds))}
          class={[
            "rounded-md px-2.5 py-1 font-medium",
            (@active == key && "bg-elevated text-fg") || "text-muted hover:text-fg"
          ]}
        >
          {key}
        </.link>
        <.link
          patch={Filters.url(@path, %{@filters | from: nil, to: nil})}
          class={[
            "rounded-md px-2.5 py-1 font-medium",
            (@active == "all" && "bg-elevated text-fg") || "text-muted hover:text-fg"
          ]}
        >
          All
        </.link>
        <.link
          :if={@filters.to}
          patch={Filters.url(@path, Filters.shift(@filters, :next))}
          class="grid size-7 place-items-center rounded-md text-muted hover:bg-elevated hover:text-fg"
          aria-label="Next period"
        >
          <.icon name="hero-chevron-right" class="size-4" />
        </.link>
      </div>
      <span class="text-xs text-muted">{Filters.label(@filters)}</span>
    </div>
    """
  end

  @doc "Removable active-filter chips (drill-down state)."
  attr :filters, :map, required: true
  attr :path, :string, required: true

  def filter_chips(assigns) do
    assigns = assign(assigns, :chips, Filters.chips(assigns.filters))

    ~H"""
    <div :if={@chips != []} class="flex flex-wrap items-center gap-2">
      <span class="text-xs text-faint">Filtered by</span>
      <.link
        :for={{key, label} <- @chips}
        patch={Filters.url(@path, Map.put(@filters, key, nil))}
        class="inline-flex items-center gap-1 rounded-md bg-accent/10 px-2 py-0.5 text-xs font-medium text-accent hover:bg-accent/20"
      >
        {label} <.icon name="hero-x-mark" class="size-3" />
      </.link>
    </div>
    """
  end

  @doc "A tiny inline-SVG sparkline (Tufte small multiple). `values` is a list of numbers."
  attr :values, :list, default: []
  attr :class, :any, default: "h-6 w-24"
  attr :tone, :atom, default: :accent

  def sparkline(assigns) do
    assigns = assign(assigns, :points, spark_points(assigns.values))

    ~H"""
    <svg
      :if={@points}
      viewBox="0 0 100 24"
      preserveAspectRatio="none"
      class={[@class, spark_color(@tone)]}
      aria-hidden="true"
    >
      <polyline
        points={@points}
        fill="none"
        stroke="currentColor"
        stroke-width="1.5"
        vector-effect="non-scaling-stroke"
      />
    </svg>
    <span :if={!@points} class="text-faint">·</span>
    """
  end

  defp spark_points(values) do
    vals = Enum.map(values || [], fn v -> (v || 0) * 1.0 end)

    if length(vals) < 2 do
      nil
    else
      max = Enum.max(vals)
      max = if max <= 0, do: 1.0, else: max
      step = 100 / (length(vals) - 1)

      vals
      |> Enum.with_index()
      |> Enum.map(fn {v, i} ->
        "#{Float.round(i * step, 2)},#{Float.round(23 - v / max * 22, 2)}"
      end)
      |> Enum.join(" ")
    end
  end

  defp spark_color(:claude), do: "text-claude"
  defp spark_color(:openai), do: "text-muted"
  defp spark_color(:success), do: "text-success"
  defp spark_color(:warning), do: "text-warning"
  defp spark_color(:danger), do: "text-danger"
  defp spark_color(:info), do: "text-info"
  defp spark_color(_), do: "text-accent"

  ## tone helpers

  defp tone_soft(:accent), do: "bg-accent/10 text-accent"
  defp tone_soft(:success), do: "bg-success/10 text-success"
  defp tone_soft(:warning), do: "bg-warning/10 text-warning"
  defp tone_soft(:danger), do: "bg-danger/10 text-danger"
  defp tone_soft(:info), do: "bg-info/10 text-info"
  defp tone_soft(:claude), do: "bg-claude/10 text-claude"
  defp tone_soft(:openai), do: "bg-elevated text-fg"
  defp tone_soft(_), do: "bg-elevated text-muted"

  defp tone_bar(:claude), do: "bg-claude"
  defp tone_bar(:openai), do: "bg-muted"
  defp tone_bar(:success), do: "bg-success"
  defp tone_bar(:warning), do: "bg-warning"
  defp tone_bar(:danger), do: "bg-danger"
  defp tone_bar(:info), do: "bg-info"
  defp tone_bar(_), do: "bg-accent"
end
