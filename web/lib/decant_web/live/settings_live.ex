defmodule DecantWeb.SettingsLive do
  use DecantWeb, :live_view

  alias Decant.AgentLauncher
  alias Decant.Settings

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       page_title: "Settings",
       settings: Settings.get(),
       agents: AgentLauncher.agents(),
       terminals: AgentLauncher.terminals(),
       ides: AgentLauncher.ides(),
       can_launch: AgentLauncher.can_launch?()
     )}
  end

  @impl true
  def handle_event("save", params, socket) do
    Settings.put(%{
      agent: params["agent"],
      terminal: params["terminal"],
      ide: params["ide"]
    })

    {:noreply, socket |> assign(settings: Settings.get()) |> put_flash(:info, "Settings saved.")}
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:settings}
      page_title="Settings"
      syncing={@syncing}
      metrics={@archive_meta}
    >
      <div class="mx-auto max-w-2xl space-y-6">
        <header class="space-y-1">
          <h1 class="text-xl font-semibold tracking-tight text-fg">Settings</h1>
          <p class="text-sm text-muted">
            How decant opens things on your machine. We start from what we detect and remember your choices.
          </p>
        </header>

        <.panel>
          <form phx-change="save" class="space-y-6">
            <.setting_field
              name="agent"
              label="Preferred agent"
              help="The agent the Run button opens first across Insights."
              options={@agents}
              value={@settings.agent}
            />
            <.setting_field
              name="terminal"
              label="Terminal"
              help="Where a session opens when you run an agent."
              options={@terminals}
              value={@settings.terminal}
            />
            <.setting_field
              name="ide"
              label="Editor"
              help="Which editor Open in editor uses for a session's project."
              options={@ides}
              value={@settings.ide}
            />
          </form>
        </.panel>

        <p :if={!@can_launch} class="text-xs text-faint">
          Opening terminals and editors works on macOS only. Your choices are still saved.
        </p>
      </div>
    </Layouts.app>
    """
  end

  attr :name, :string, required: true
  attr :label, :string, required: true
  attr :help, :string, required: true
  attr :options, :list, required: true
  attr :value, :string, required: true

  defp setting_field(assigns) do
    ~H"""
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="min-w-0">
        <label for={@name} class="text-sm font-medium text-fg">{@label}</label>
        <p class="text-xs text-muted">{@help}</p>
      </div>
      <select
        id={@name}
        name={@name}
        class="w-44 rounded-lg border border-line bg-surface px-3 py-1.5 text-sm text-fg focus:border-accent"
      >
        <option :for={{key, label} <- @options} value={key} selected={@value == key}>
          {label}
        </option>
      </select>
    </div>
    """
  end
end
