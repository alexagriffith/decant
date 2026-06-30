defmodule DecantWeb.SessionLive.Index do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters
  alias DecantWeb.TableSort

  @page_size 100

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
    page = fetch_sessions(filters, socket.assigns.sort, nil)

    {:noreply,
     socket
     |> assign(
       filters: filters,
       totals: Archive.totals(filters),
       all: page.rows,
       pagination: page.pagination,
       q: ""
     )
     |> refresh()}
  end

  @impl true
  def handle_event("filter", %{"q" => q}, socket) do
    {:noreply, socket |> assign(q: q) |> refresh()}
  end

  def handle_event("sort", %{"col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.sort, col)
    page = fetch_sessions(socket.assigns.filters, sort, nil)

    {:noreply,
     socket
     |> assign(sort: sort, all: page.rows, pagination: page.pagination)
     |> refresh()}
  end

  def handle_event("load_more", _params, socket) do
    case socket.assigns.pagination.next_cursor do
      nil ->
        {:noreply, socket}

      cursor ->
        page = fetch_sessions(socket.assigns.filters, socket.assigns.sort, cursor)

        {:noreply,
         socket
         |> assign(all: socket.assigns.all ++ page.rows, pagination: page.pagination)
         |> refresh()}
    end
  end

  defp refresh(socket) do
    rows =
      socket.assigns.all
      |> filter_all(socket.assigns.q)
      |> TableSort.sort(socket.assigns.sort)

    socket
    |> assign(visible_count: length(rows), loaded_count: length(socket.assigns.all))
    |> stream(:sessions, rows, reset: true)
  end

  defp fetch_sessions(filters, sort, cursor) do
    Archive.list_sessions_page(filters, @page_size, sort: server_sort(sort), cursor: cursor)
  end

  defp server_sort({:started_at, :asc}), do: "started_at_asc"
  defp server_sort({:started_at, :desc}), do: "started_at_desc"
  defp server_sort({:cost, :desc}), do: "cost_desc"
  defp server_sort(_sort), do: "started_at_desc"

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

  defp sessions_caption(q, sort, visible_count, loaded_count, pagination) do
    total_count = Map.get(pagination || %{}, :total_count)

    cond do
      String.trim(q || "") != "" ->
        "Showing #{format_count(visible_count)} matching loaded #{row_word(visible_count)} from #{format_count(loaded_count)} loaded sessions"

      !server_sort_supported?(sort) and is_integer(total_count) ->
        "Showing #{format_count(loaded_count)} loaded sessions sorted locally of #{format_count(total_count)} total"

      !server_sort_supported?(sort) ->
        "Showing #{format_count(loaded_count)} loaded sessions sorted locally"

      is_integer(total_count) ->
        "Showing #{format_count(loaded_count)} of #{format_count(total_count)} sessions"

      true ->
        "Showing #{format_count(loaded_count)} sessions"
    end
  end

  defp format_count(n) when is_integer(n), do: Integer.to_string(n)
  defp format_count(_), do: "0"
  defp row_word(1), do: "row"
  defp row_word(_), do: "rows"
  defp server_sort_supported?({:started_at, dir}) when dir in [:asc, :desc], do: true
  defp server_sort_supported?({:cost, :desc}), do: true
  defp server_sort_supported?(_sort), do: false

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:sessions}
      page_title="Sessions"
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
            <form id="session-filter-form" phx-change="filter" class="w-56 sm:w-72">
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

          <div class="flex flex-wrap items-center justify-between gap-3 border-t border-line px-4 py-3 text-xs text-muted">
            <span>{sessions_caption(@q, @sort, @visible_count, @loaded_count, @pagination)}</span>
            <button
              :if={@pagination.has_more}
              type="button"
              phx-click="load_more"
              class="inline-flex items-center gap-1.5 rounded-md border border-line bg-surface px-2.5 py-1 font-medium text-fg transition-colors hover:bg-elevated"
            >
              <.icon name="hero-arrow-down-tray" class="size-3.5" /> Load more
            </button>
          </div>
        </.panel>
      </div>
    </Layouts.app>
    """
  end
end
