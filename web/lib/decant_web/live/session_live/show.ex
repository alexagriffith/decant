defmodule DecantWeb.SessionLive.Show do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(%{"id" => id}, _session, socket) do
    case Archive.get_session(id) do
      nil ->
        {:ok, socket |> put_flash(:error, "No session ##{id}") |> push_navigate(to: ~p"/")}

      detail ->
        {:ok, assign(socket, detail: detail, page_title: detail.summary.title || "session")}
    end
  end

  @impl true
  def render(assigns) do
    ~H"""
    <div class="p-6 max-w-4xl mx-auto">
      <.link navigate={~p"/"} class="text-blue-600 hover:underline">← sessions</.link>

      <h1 class="text-2xl font-bold mt-2">{@detail.summary.title || "(untitled)"}</h1>
      <p class="text-gray-500 text-sm">
        {@detail.summary.tool} · {@detail.summary.model} · {@detail.summary.message_count} msgs · {@detail.summary.project}
      </p>

      <div class="mt-6 space-y-6">
        <div :for={m <- @detail.messages}>
          <div class="font-semibold uppercase text-xs text-gray-500">{m.role}</div>
          <div :for={b <- m.blocks} class="mt-1">
            <pre :if={b.type in ["text", "thinking"]} class="whitespace-pre-wrap font-sans text-sm">{b.text}</pre>
            <div :if={b.type == "tool_use"} class="bg-base-200 p-2 rounded text-xs">
              <div class="font-mono">→ {b.tool_name}</div>
              <pre class="whitespace-pre-wrap">{b.tool_input}</pre>
            </div>
            <pre
              :if={b.type == "tool_result"}
              class="bg-base-200/50 p-2 rounded text-xs whitespace-pre-wrap"
            >{b.tool_result}</pre>
          </div>
        </div>
      </div>
    </div>
    """
  end
end
