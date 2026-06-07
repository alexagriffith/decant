defmodule DecantWeb.Components.UI do
  @moduledoc """
  decant design-system components: panels, stat cards, badges, empty states,
  and the ECharts chart container. All styled with the semantic tokens from
  `assets/css/app.css` so they theme automatically.
  """
  use Phoenix.Component

  import DecantWeb.CoreComponents, only: [icon: 1]

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
    values: [:neutral, :accent, :success, :warning, :danger, :info]

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

  @doc "Badge for a tool (`claude_code` / `codex`)."
  attr :tool, :string, default: nil

  def tool_badge(assigns) do
    {tone, label} =
      case assigns.tool do
        "claude_code" -> {:accent, "Claude"}
        "codex" -> {:info, "Codex"}
        other -> {:neutral, other || "—"}
      end

    assigns = assign(assigns, tone: tone, label: label)

    ~H"""
    <.badge tone={@tone}>{@label}</.badge>
    """
  end

  @doc "Badge for a model id, colored by family, rendered monospace."
  attr :model, :string, default: nil

  def model_badge(assigns) do
    assigns = assign(assigns, :tone, model_tone(assigns.model))

    ~H"""
    <.badge :if={@model} tone={@tone} mono>{@model}</.badge>
    <span :if={!@model} class="text-xs text-faint">—</span>
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

  ## tone helpers

  def model_tone(nil), do: :neutral

  def model_tone(model) do
    m = String.downcase(model)

    cond do
      String.contains?(m, "opus") -> :accent
      String.contains?(m, "sonnet") -> :info
      String.contains?(m, "haiku") -> :success
      String.contains?(m, "codex") -> :warning
      String.contains?(m, "gpt") -> :warning
      true -> :neutral
    end
  end

  defp tone_soft(:accent), do: "bg-accent/10 text-accent"
  defp tone_soft(:success), do: "bg-success/10 text-success"
  defp tone_soft(:warning), do: "bg-warning/10 text-warning"
  defp tone_soft(:danger), do: "bg-danger/10 text-danger"
  defp tone_soft(:info), do: "bg-info/10 text-info"
  defp tone_soft(_), do: "bg-elevated text-muted"

  defp tone_bar(:success), do: "bg-success"
  defp tone_bar(:warning), do: "bg-warning"
  defp tone_bar(:danger), do: "bg-danger"
  defp tone_bar(:info), do: "bg-info"
  defp tone_bar(_), do: "bg-accent"
end
