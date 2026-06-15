defmodule DecantWeb.SessionLive.Search do
  use DecantWeb, :live_view

  alias Decant.Archive

  @page_size 25

  @impl true
  def mount(_params, _session, socket) do
    {:ok, assign(socket, q: "", hits: [], pagination: empty_pagination())}
  end

  # Deep links (`/search?q=…`, e.g. from a Files hotspot row) pre-fill and run
  # the search; typing in the form goes through handle_event without patching.
  # A URL without `q` (back-navigation) resets, so the UI never shows results
  # the URL no longer claims.
  @impl true
  def handle_params(params, _uri, socket) do
    case String.trim(params["q"] || "") do
      "" -> {:noreply, assign(socket, q: "", hits: [], pagination: empty_pagination())}
      q -> {:noreply, reset_search(socket, q)}
    end
  end

  @impl true
  def handle_event("search", %{"q" => q}, socket) do
    {:noreply, reset_search(socket, q)}
  end

  def handle_event("load_more", _params, socket) do
    case socket.assigns.pagination.next_cursor do
      nil ->
        {:noreply, socket}

      cursor ->
        page = safe_search(socket.assigns.q, cursor)

        {:noreply,
         assign(socket,
           hits: socket.assigns.hits ++ page.rows,
           pagination: page.pagination
         )}
    end
  end

  # FTS5 MATCH can raise on malformed query syntax; degrade to no results.
  defp safe_search(q, cursor \\ nil) do
    Archive.search_page(q, @page_size, cursor: cursor)
  rescue
    _ -> %{rows: [], pagination: empty_pagination()}
  end

  defp reset_search(socket, q) do
    case String.trim(q || "") do
      "" ->
        assign(socket, q: "", hits: [], pagination: empty_pagination())

      trimmed ->
        page = safe_search(trimmed)
        assign(socket, q: trimmed, hits: page.rows, pagination: page.pagination)
    end
  end

  defp search_caption(hits, pagination) do
    count = length(hits)

    cond do
      is_integer(pagination.total_count) ->
        "Showing #{format_count(count)} of #{format_count(pagination.total_count)} results"

      pagination.has_more ->
        "Showing #{format_count(count)} results; more available"

      true ->
        "#{format_count(count)} #{result_word(count)}"
    end
  end

  defp empty_pagination do
    %{has_more: false, next_cursor: nil, page_size: nil, total_count: nil}
  end

  defp format_count(n) when is_integer(n), do: Integer.to_string(n)
  defp result_word(1), do: "result"
  defp result_word(_), do: "results"

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:search}
      page_title="Search"
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
      <div class="mx-auto max-w-3xl space-y-6">
        <header class="space-y-1">
          <h1 class="text-xl font-semibold tracking-tight text-fg">Search</h1>
          <p class="text-sm text-muted">
            Full-text search across every message and tool call in your archive.
          </p>
        </header>

        <div class="space-y-1.5">
          <form phx-change="search">
            <div class="relative">
              <.icon
                name="hero-magnifying-glass"
                class="size-5 text-faint absolute left-3.5 top-1/2 -translate-y-1/2"
              />
              <input
                name="q"
                value={@q}
                phx-debounce="200"
                autocomplete="off"
                placeholder="Search across all sessions and tool calls…"
                class="w-full rounded-xl border border-line bg-surface pl-11 pr-3 py-3 text-sm text-fg placeholder:text-faint focus:border-accent shadow-sm"
              />
            </div>
          </form>

          <p :if={String.trim(@q) != ""} class="px-1 text-xs text-faint tabular-nums">
            {search_caption(@hits, @pagination)}
          </p>
        </div>

        <div>
          <%= cond do %>
            <% String.trim(@q) == "" -> %>
              <.empty_state
                icon="hero-magnifying-glass"
                title="Search your archive"
                message="Find any message or tool call across every session by keyword."
              />
            <% @hits == [] -> %>
              <.empty_state
                icon="hero-inbox"
                title="No matches"
                message="Nothing matched your search. Try a different term."
              />
            <% true -> %>
              <ul class="space-y-2">
                <li :for={h <- @hits}>
                  <.link
                    navigate={~p"/sessions/#{h.session_id}"}
                    class="block card-surface p-3 hover:bg-elevated transition-colors"
                  >
                    <div class="flex items-start justify-between gap-3">
                      <span class="text-sm font-medium text-fg">
                        {h.title || "(untitled)"}
                      </span>
                      <.tool_badge tool={h.tool} />
                    </div>
                    <p class="mt-1.5 text-sm text-muted leading-relaxed">
                      {highlight(h.snippet)}
                    </p>
                  </.link>
                </li>
              </ul>
              <div :if={@pagination.has_more} class="mt-4 flex justify-center">
                <button
                  type="button"
                  phx-click="load_more"
                  class="inline-flex items-center gap-1.5 rounded-md border border-line bg-surface px-3 py-1.5 text-sm font-medium text-fg transition-colors hover:bg-elevated"
                >
                  <.icon name="hero-arrow-down-tray" class="size-4" /> Load more
                </button>
              </div>
          <% end %>
        </div>
      </div>
    </Layouts.app>
    """
  end

  # Matched terms in a snippet are delimited by literal square brackets (the only
  # place brackets appear). Escape the snippet, then wrap each bracketed segment
  # in a <mark> highlight without trusting the source as raw HTML.
  defp highlight(snippet) do
    snippet
    |> Phoenix.HTML.html_escape()
    |> Phoenix.HTML.safe_to_string()
    |> then(
      &Regex.replace(~r/\[([^\]]*)\]/, &1, fn _, inner ->
        ~s(<mark class="rounded bg-accent/20 px-0.5 text-fg">#{inner}</mark>)
      end)
    )
    |> Phoenix.HTML.raw()
  end
end
