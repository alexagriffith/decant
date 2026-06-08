defmodule DecantWeb.SessionLive.Index do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters
  alias DecantWeb.TableSort

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Sessions",
       q: "",
       sort: {:started_at, :desc}
     )}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)
    sessions = Archive.list_sessions(filters, 300)

    {:noreply,
     socket
     |> assign(filters: filters, totals: Archive.totals(filters), all: sessions, q: "")
     |> refresh()}
  end

  @impl true
  def handle_event("filter", %{"q" => q}, socket) do
    {:noreply, socket |> assign(q: q) |> refresh()}
  end

  def handle_event("sort", %{"col" => col}, socket) do
    {:noreply, socket |> assign(sort: TableSort.toggle(socket.assigns.sort, col)) |> refresh()}
  end

  # Apply the text filter and the current sort, then re-stream the rows.
  defp refresh(socket) do
    rows =
      socket.assigns.all
      |> filter_all(socket.assigns.q)
      |> TableSort.sort(socket.assigns.sort)

    stream(socket, :sessions, rows, reset: true)
  end

  defp filter_all(sessions, q) do
    case String.trim(q) do
      "" ->
        sessions

      needle ->
        needle = String.downcase(needle)

        Enum.filter(sessions, fn s ->
          String.contains?(String.downcase(s.title || ""), needle) or
            String.contains?(String.downcase(s.model || ""), needle) or
            String.contains?(String.downcase(s.tool || ""), needle)
        end)
    end
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:sessions}
      page_title="Sessions"
      syncing={@syncing}
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
      <div class="space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <.date_range filters={@filters} bounds={@bounds} path={~p"/"} />
          <.filter_chips filters={@filters} path={~p"/"} />
        </div>

        <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
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
            label="Est. cost"
            value={money(@totals.cost)}
            icon="hero-currency-dollar"
            tone={:success}
          />
        </div>

        <.panel title="Sessions" body_class="p-0">
          <:actions>
            <form phx-change="filter" class="w-56 sm:w-72">
              <input
                type="text"
                name="q"
                value={@q}
                phx-debounce="150"
                autocomplete="off"
                placeholder="Filter by title, model, or tool…"
                class="w-full rounded-lg border border-line bg-surface px-3 py-1.5 text-sm text-fg placeholder:text-faint focus:border-accent"
              />
            </form>
          </:actions>

          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <.sort_header col="tool" label="Tool" sort={@sort} />
                  <.sort_header col="title" label="Title" sort={@sort} />
                  <.sort_header col="model" label="Model" sort={@sort} />
                  <.sort_header col="message_count" label="Msgs" sort={@sort} align="right" />
                  <.sort_header col="cost" label="Cost" sort={@sort} align="right" />
                  <.sort_header col="started_at" label="Started" sort={@sort} align="right" />
                </tr>
              </thead>
              <tbody id="sessions" phx-update="stream">
                <tr
                  :for={{id, s} <- @streams.sessions}
                  id={id}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-4 py-2.5"><.tool_badge tool={s.tool} /></td>
                  <td class="max-w-md truncate px-4 py-2.5">
                    <.link
                      navigate={~p"/sessions/#{s.id}"}
                      class="font-medium text-fg hover:text-accent"
                    >
                      {s.title || "(untitled)"}
                    </.link>
                  </td>
                  <td class="px-4 py-2.5"><.model_badge model={s.model} /></td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">
                    {int(s.message_count)}
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums">{money(s.cost)}</td>
                  <td class="px-4 py-2.5 text-right whitespace-nowrap text-muted">
                    {relative_time(s.started_at)}
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end
end
