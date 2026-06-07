defmodule DecantWeb.SessionLive.Index do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       sessions: Archive.list_sessions(200),
       page_title: "decant — sessions"
     )}
  end

  @impl true
  def render(assigns) do
    ~H"""
    <div class="p-6">
      <div class="flex items-center justify-between">
        <h1 class="text-2xl font-bold">decant — sessions</h1>
        <.link navigate={~p"/search"} class="text-blue-600 hover:underline">search →</.link>
      </div>

      <table class="mt-4 w-full text-sm border-collapse">
        <thead>
          <tr class="text-left border-b">
            <th class="p-2">tool</th>
            <th class="p-2">title</th>
            <th class="p-2">model</th>
            <th class="p-2 text-right">msgs</th>
            <th class="p-2 text-right">cost$</th>
            <th class="p-2">started</th>
          </tr>
        </thead>
        <tbody>
          <tr :for={s <- @sessions} class="border-b hover:bg-base-200">
            <td class="p-2">{s.tool}</td>
            <td class="p-2 max-w-xl truncate">
              <.link navigate={~p"/sessions/#{s.id}"} class="text-blue-600 hover:underline">
                {s.title || "(untitled)"}
              </.link>
            </td>
            <td class="p-2">{s.model}</td>
            <td class="p-2 text-right">{s.message_count}</td>
            <td class="p-2 text-right">{:erlang.float_to_binary(s.cost * 1.0, decimals: 2)}</td>
            <td class="p-2 text-gray-500">{s.started_at}</td>
          </tr>
        </tbody>
      </table>
    </div>
    """
  end
end
