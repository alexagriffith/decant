defmodule DecantWeb.SessionLive.Search do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    {:ok, assign(socket, q: "", hits: [], page_title: "decant — search")}
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
    <div class="p-6 max-w-4xl mx-auto">
      <.link navigate={~p"/"} class="text-blue-600 hover:underline">← sessions</.link>

      <form phx-change="search" class="mt-3">
        <input
          type="text"
          name="q"
          value={@q}
          placeholder="full-text search across all sessions…"
          phx-debounce="200"
          autocomplete="off"
          class="w-full border rounded p-2"
        />
      </form>

      <ul class="mt-4 space-y-3">
        <li :for={h <- @hits} class="text-sm">
          <.link navigate={~p"/sessions/#{h.session_id}"} class="text-blue-600 hover:underline">
            {h.title || "(untitled)"}
          </.link>
          <span class="text-gray-400">[{h.tool}]</span>
          <div class="text-gray-600">{h.snippet}</div>
        </li>
      </ul>
    </div>
    """
  end
end
