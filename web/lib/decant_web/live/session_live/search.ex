defmodule DecantWeb.SessionLive.Search do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    {:ok, assign(socket, q: "", hits: [])}
  end

  # Deep links (`/search?q=…`, e.g. from a Files hotspot row) pre-fill and run
  # the search; typing in the form goes through handle_event without patching.
  # A URL without `q` (back-navigation) resets, so the UI never shows results
  # the URL no longer claims.
  @impl true
  def handle_params(params, _uri, socket) do
    case String.trim(params["q"] || "") do
      "" -> {:noreply, assign(socket, q: "", hits: [])}
      q -> {:noreply, assign(socket, q: q, hits: safe_search(q))}
    end
  end

  @impl true
  def handle_event("search", %{"q" => q}, socket) do
    hits = if String.trim(q) == "", do: [], else: safe_search(q)
    {:noreply, assign(socket, q: q, hits: hits)}
  end

  # FTS5 MATCH can raise on malformed query syntax; degrade to no results.
  defp safe_search(q) do
    Archive.search(q, 50)
  rescue
    _ -> []
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:search}
      page_title="Search"
      syncing={@syncing}
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
            {(length(@hits) == 1 && "1 result") || "#{int(length(@hits))} results"}
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
