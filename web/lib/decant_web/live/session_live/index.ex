defmodule DecantWeb.SessionLive.Index do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    sessions = Archive.list_sessions(200)
    totals = Archive.totals()

    {:ok,
     socket
     |> assign(totals: totals, all: sessions, page_title: "Sessions")
     |> stream(:sessions, sessions)}
  end

  @impl true
  def handle_event("filter", %{"q" => q}, socket) do
    filtered = filter_sessions(socket.assigns.all, q)
    {:noreply, stream(socket, :sessions, filtered, reset: true)}
  end

  defp filter_sessions(sessions, q) do
    case String.trim(q || "") do
      "" ->
        sessions

      term ->
        needle = String.downcase(term)

        Enum.filter(sessions, fn s ->
          matches?(s.title, needle) or matches?(s.model, needle) or matches?(s.tool, needle)
        end)
    end
  end

  defp matches?(nil, _needle), do: false
  defp matches?(value, needle), do: String.contains?(String.downcase(value), needle)

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app flash={@flash} active={:sessions} page_title="Sessions" syncing={@syncing}>
      <div class="space-y-6">
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <.stat_card
            label="Sessions"
            value={int(@totals.sessions)}
            hint="all time"
            icon="hero-rectangle-stack"
            tone={:accent}
          />
          <.stat_card
            label="Messages"
            value={int(@totals.messages)}
            hint="all time"
            icon="hero-chat-bubble-left-right"
            tone={:info}
          />
          <.stat_card
            label="Est. cost"
            value={money(@totals.cost)}
            hint="all time"
            icon="hero-currency-dollar"
            tone={:success}
          />
        </div>

        <.panel title="Sessions" body_class="p-0">
          <:actions>
            <form phx-change="filter">
              <input
                name="q"
                phx-debounce="150"
                autocomplete="off"
                placeholder="Filter by title, model, or tool…"
                class="rounded-lg border border-line bg-surface px-3 py-1.5 text-sm text-fg placeholder:text-faint focus:border-accent"
              />
            </form>
          </:actions>

          <table class="w-full">
            <thead>
              <tr class="border-b border-line">
                <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                  Tool
                </th>
                <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                  Title
                </th>
                <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                  Model
                </th>
                <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                  Msgs
                </th>
                <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                  Cost
                </th>
                <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                  Started
                </th>
              </tr>
            </thead>
            <tbody id="sessions" phx-update="stream">
              <tr
                :for={{id, s} <- @streams.sessions}
                id={id}
                class="border-b border-line/60 transition-colors hover:bg-elevated"
              >
                <td class="px-3 py-2.5 text-sm">
                  <.tool_badge tool={s.tool} />
                </td>
                <td class="max-w-md px-3 py-2.5 text-sm">
                  <.link
                    navigate={~p"/sessions/#{s.id}"}
                    class="block truncate font-medium text-fg hover:text-accent"
                  >
                    {s.title || "(untitled)"}
                  </.link>
                </td>
                <td class="px-3 py-2.5 text-sm">
                  <.model_badge model={s.model} />
                </td>
                <td class="px-3 py-2.5 text-right text-sm text-muted tabular-nums">
                  {int(s.message_count)}
                </td>
                <td class="px-3 py-2.5 text-right text-sm tabular-nums">
                  {money(s.cost)}
                </td>
                <td class="px-3 py-2.5 text-sm text-muted">
                  {relative_time(s.started_at)}
                </td>
              </tr>
            </tbody>
          </table>
        </.panel>
      </div>
    </Layouts.app>
    """
  end
end
