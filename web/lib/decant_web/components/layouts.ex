defmodule DecantWeb.Layouts do
  @moduledoc """
  App shell and layout components: a responsive sidebar + top bar that wraps
  every page via `app/1`, plus the flash container and theme toggle.
  """
  use DecantWeb, :html

  embed_templates "layouts/*"

  @nav [
    %{key: :sessions, label: "Sessions", path: "/", icon: "hero-rectangle-stack"},
    %{key: :search, label: "Search", path: "/search", icon: "hero-magnifying-glass"},
    %{key: :analytics, label: "Analytics", path: "/analytics", icon: "hero-chart-bar"},
    %{key: :insights, label: "Insights", path: "/insights", icon: "hero-light-bulb"},
    %{key: :tools, label: "Tools & MCP", path: "/tools", icon: "hero-wrench-screwdriver"}
  ]

  @doc """
  The application shell. Wrap each LiveView's content in it:

      <Layouts.app flash={@flash} active={:sessions} page_title="Sessions" syncing={@syncing}>
        ...
        <:actions>...</:actions>
      </Layouts.app>
  """
  attr :flash, :map, required: true, doc: "the map of flash messages"
  attr :active, :atom, default: nil, doc: "the active nav key"
  attr :page_title, :string, default: nil, doc: "title shown in the top bar"
  attr :syncing, :boolean, default: false, doc: "whether a sync is in progress"
  attr :metrics, :map, default: nil, doc: "archive snapshot for the sidebar footer"
  attr :current_scope, :map, default: nil
  slot :inner_block, required: true
  slot :actions, doc: "page-specific top-bar actions"

  def app(assigns) do
    assigns = assign(assigns, :nav, @nav)

    ~H"""
    <div class="min-h-dvh bg-canvas text-fg">
      <div
        id="sidebar-backdrop"
        class="fixed inset-0 z-30 hidden bg-black/60 backdrop-blur-sm md:hidden"
        phx-click={close_sidebar()}
      />

      <aside
        id="sidebar"
        class={[
          "fixed inset-y-0 left-0 z-40 flex w-60 -translate-x-full flex-col",
          "border-r border-line bg-surface transition-transform duration-200 md:translate-x-0"
        ]}
      >
        <div class="flex h-14 items-center gap-2.5 border-b border-line px-5">
          <.link navigate={~p"/"} class="flex items-center gap-2.5">
            <span class="grid size-7 place-items-center rounded-lg bg-accent text-on-accent shadow-sm">
              <.icon name="hero-beaker-solid" class="size-4" />
            </span>
            <span class="text-[15px] font-semibold tracking-tight">decant</span>
          </.link>
          <button
            class="ml-auto rounded-md p-1.5 text-muted hover:bg-elevated hover:text-fg md:hidden"
            phx-click={close_sidebar()}
            aria-label="Close menu"
          >
            <.icon name="hero-x-mark" class="size-5" />
          </button>
        </div>

        <nav class="flex-1 space-y-1 p-3">
          <.nav_item :for={item <- @nav} item={item} active={@active} />
        </nav>

        <div :if={@metrics} class="space-y-1.5 border-t border-line px-4 py-3 text-xs text-muted">
          <div class="flex items-center gap-2" title="Live and auto-syncing">
            <span class="size-1.5 shrink-0 rounded-full bg-success motion-safe:animate-pulse"></span>
            <span>
              <span class="font-medium text-fg tabular-nums">{int(@metrics.sessions)}</span> sessions
            </span>
          </div>
          <div class="flex items-center gap-2">
            <.icon name="hero-banknotes" class="size-3.5 shrink-0 text-faint" />
            <span>
              <span class="font-medium text-fg tabular-nums">{money(@metrics.cost)}</span> tracked
            </span>
          </div>
          <div :if={@metrics.last_activity} class="flex items-center gap-2">
            <.icon name="hero-clock" class="size-3.5 shrink-0 text-faint" />
            <span>latest {fmt_day(@metrics.last_activity)}</span>
          </div>
        </div>
      </aside>

      <div class="flex min-h-dvh flex-col md:pl-60">
        <header class="sticky top-0 z-20 flex h-14 items-center gap-3 border-b border-line bg-canvas/80 px-4 backdrop-blur-md sm:px-6">
          <button
            class="rounded-md p-1.5 text-muted hover:bg-elevated hover:text-fg md:hidden"
            phx-click={open_sidebar()}
            aria-label="Open menu"
          >
            <.icon name="hero-bars-3" class="size-5" />
          </button>

          <h1 :if={@page_title} class="text-sm font-semibold tracking-tight">{@page_title}</h1>

          <div class="flex-1"></div>

          <.link
            navigate={~p"/search"}
            class={[
              "group hidden items-center gap-2 rounded-lg border border-line bg-surface px-3 py-1.5",
              "text-sm text-faint hover:border-line-strong hover:text-muted sm:flex"
            ]}
          >
            <.icon name="hero-magnifying-glass" class="size-4" />
            <span>Search…</span>
            <kbd class="ml-2 rounded border border-line px-1.5 py-0.5 font-mono text-[10px] text-faint">
              /
            </kbd>
          </.link>

          {render_slot(@actions)}

          <button
            phx-click="sync"
            disabled={@syncing}
            class={[
              "inline-flex items-center gap-2 rounded-lg border border-line bg-surface px-3 py-1.5",
              "text-sm font-medium text-fg transition-colors hover:bg-elevated hover:border-line-strong",
              "disabled:opacity-60 disabled:pointer-events-none"
            ]}
          >
            <.icon name="hero-arrow-path" class={["size-4", @syncing && "animate-spin"]} />
            <span class="hidden sm:inline">{(@syncing && "Syncing…") || "Sync"}</span>
          </button>

          <.link
            navigate={~p"/settings"}
            class="inline-flex items-center justify-center rounded-lg border border-line bg-surface p-2 text-muted transition-colors hover:border-line-strong hover:bg-elevated hover:text-fg"
            aria-label="Settings"
            title="Settings"
          >
            <.icon name="hero-cog-6-tooth" class="size-4" />
          </.link>

          <.theme_toggle />
        </header>

        <main class="flex-1 px-4 py-6 sm:px-6 lg:px-8">
          <div class="mx-auto w-full max-w-6xl">
            {render_slot(@inner_block)}
          </div>
        </main>
      </div>

      <.flash_group flash={@flash} />
    </div>
    """
  end

  attr :item, :map, required: true
  attr :active, :atom, default: nil

  defp nav_item(assigns) do
    assigns = assign(assigns, :is_active, assigns.item.key == assigns.active)

    ~H"""
    <.link
      navigate={@item.path}
      aria-current={@is_active && "page"}
      class={[
        "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
        (@is_active && "bg-accent/10 text-accent") ||
          "text-muted hover:bg-elevated hover:text-fg"
      ]}
    >
      <.icon name={@item.icon} class="size-[18px]" />
      {@item.label}
    </.link>
    """
  end

  defp open_sidebar(js \\ %JS{}) do
    js
    |> JS.remove_class("-translate-x-full", to: "#sidebar")
    |> JS.show(to: "#sidebar-backdrop")
  end

  defp close_sidebar(js \\ %JS{}) do
    js
    |> JS.add_class("-translate-x-full", to: "#sidebar")
    |> JS.hide(to: "#sidebar-backdrop")
  end

  # Friendly day label for the sidebar freshness line (e.g. "Jun 07").
  defp fmt_day(iso) when is_binary(iso) do
    case Date.from_iso8601(iso) do
      {:ok, d} -> Calendar.strftime(d, "%b %d")
      _ -> iso
    end
  end

  defp fmt_day(other), do: other

  @doc "Fixed, stacking container for flash messages."
  attr :flash, :map, required: true
  attr :id, :string, default: "flash-group"

  def flash_group(assigns) do
    ~H"""
    <div
      id={@id}
      aria-live="polite"
      class="fixed top-4 right-4 z-50 flex w-80 flex-col gap-2 sm:w-96"
    >
      <.flash kind={:info} flash={@flash} />
      <.flash kind={:error} flash={@flash} />

      <.flash
        id="client-error"
        kind={:error}
        title="Connection lost"
        phx-disconnected={show(".phx-client-error #client-error")}
        phx-connected={hide("#client-error")}
        hidden
      >
        Attempting to reconnect <.icon name="hero-arrow-path" class="ml-1 size-3 animate-spin" />
      </.flash>

      <.flash
        id="server-error"
        kind={:error}
        title="Something went wrong"
        phx-disconnected={show(".phx-server-error #server-error")}
        phx-connected={hide("#server-error")}
        hidden
      >
        Attempting to reconnect <.icon name="hero-arrow-path" class="ml-1 size-3 animate-spin" />
      </.flash>
    </div>
    """
  end

  @doc "Three-way theme toggle (system / light / dark)."
  def theme_toggle(assigns) do
    ~H"""
    <div class="relative flex items-center rounded-lg border border-line bg-surface p-0.5">
      <div class="absolute top-0.5 bottom-0.5 left-0.5 w-7 rounded-md bg-elevated transition-[left] [[data-theme=light]_&]:left-[calc(0.125rem+1.75rem)] [[data-theme=dark]_&]:left-[calc(0.125rem+3.5rem)]" />
      <button
        class="relative z-10 grid size-7 place-items-center rounded-md text-muted hover:text-fg"
        phx-click={JS.dispatch("phx:set-theme")}
        data-phx-theme="system"
        aria-label="System theme"
      >
        <.icon name="hero-computer-desktop-micro" class="size-4" />
      </button>
      <button
        class="relative z-10 grid size-7 place-items-center rounded-md text-muted hover:text-fg"
        phx-click={JS.dispatch("phx:set-theme")}
        data-phx-theme="light"
        aria-label="Light theme"
      >
        <.icon name="hero-sun-micro" class="size-4" />
      </button>
      <button
        class="relative z-10 grid size-7 place-items-center rounded-md text-muted hover:text-fg"
        phx-click={JS.dispatch("phx:set-theme")}
        data-phx-theme="dark"
        aria-label="Dark theme"
      >
        <.icon name="hero-moon-micro" class="size-4" />
      </button>
    </div>
    """
  end
end
