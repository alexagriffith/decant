defmodule DecantWeb.SessionLive.Index do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       sessions: Archive.list_sessions(200),
       syncing: false,
       page_title: "decant — sessions"
     )}
  end

  @impl true
  def handle_event("sync", _params, %{assigns: %{syncing: true}} = socket), do: {:noreply, socket}

  def handle_event("sync", _params, socket) do
    bin = System.get_env("DECANT_BIN") || "decant"
    db = Archive.db_path()
    {:noreply, socket |> assign(syncing: true) |> start_async(:sync, fn -> run_sync(bin, db) end)}
  end

  defp run_sync(_bin, nil), do: "error: archive DB not configured (set DECANT_DB)"

  defp run_sync(bin, db) do
    case System.cmd(bin, ["--db", db, "sync"], stderr_to_stdout: true) do
      {out, 0} -> String.trim(out)
      {out, 3} -> String.trim(out) <> " (with issues)"
      {out, code} -> "exit #{code}: #{String.slice(out, 0, 200)}"
    end
  rescue
    e -> "error: #{Exception.message(e)} (is the `decant` binary on PATH? set DECANT_BIN)"
  end

  @impl true
  def handle_async(:sync, {:ok, msg}, socket) do
    {:noreply,
     socket
     |> assign(syncing: false, sessions: Archive.list_sessions(200))
     |> put_flash(:info, "sync: #{msg}")}
  end

  @impl true
  def handle_async(:sync, {:exit, reason}, socket) do
    {:noreply, socket |> assign(syncing: false) |> put_flash(:error, "sync crashed: #{inspect(reason)}")}
  end

  defp money(n), do: :erlang.float_to_binary((n || 0) * 1.0, decimals: 2)

  @impl true
  def render(assigns) do
    ~H"""
    <div class="p-6">
      <div class="flex items-center justify-between flex-wrap gap-2">
        <h1 class="text-2xl font-bold">decant — sessions</h1>
        <div class="flex gap-3 items-center text-sm">
          <.link navigate={~p"/search"} class="text-blue-600 hover:underline">search</.link>
          <.link navigate={~p"/analytics"} class="text-blue-600 hover:underline">analytics</.link>
          <.link navigate={~p"/tools"} class="text-blue-600 hover:underline">tools</.link>
          <button
            phx-click="sync"
            disabled={@syncing}
            class="px-3 py-1 rounded bg-blue-600 text-white disabled:opacity-50"
          >
            {if @syncing, do: "syncing…", else: "Sync now"}
          </button>
        </div>
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
            <td class="p-2 text-right">{money(s.cost)}</td>
            <td class="p-2 text-gray-500">{s.started_at}</td>
          </tr>
        </tbody>
      </table>
    </div>
    """
  end
end
