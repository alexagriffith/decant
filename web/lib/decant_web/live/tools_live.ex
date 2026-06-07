defmodule DecantWeb.ToolsLive do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       page_title: "decant — tools & MCP",
       tools: Archive.tool_usage(50),
       mcp: Archive.mcp_usage(50)
     )}
  end

  @impl true
  def render(assigns) do
    ~H"""
    <div class="p-6 max-w-5xl mx-auto">
      <.link navigate={~p"/"} class="text-blue-600 hover:underline">← sessions</.link>
      <h1 class="text-2xl font-bold mt-2">tools &amp; MCP</h1>

      <h2 class="font-semibold mt-4 mb-2">MCP servers</h2>
      <table class="w-full text-sm border-collapse">
        <thead>
          <tr class="text-left border-b">
            <th class="p-2">server</th>
            <th class="p-2 text-right">tools</th>
            <th class="p-2 text-right">calls</th>
            <th class="p-2 text-right">errors</th>
          </tr>
        </thead>
        <tbody>
          <tr :for={r <- @mcp} class="border-b">
            <td class="p-2">{r.server}</td>
            <td class="p-2 text-right">{r.tools}</td>
            <td class="p-2 text-right">{r.calls}</td>
            <td class="p-2 text-right">{r.errors}</td>
          </tr>
        </tbody>
      </table>

      <h2 class="font-semibold mt-6 mb-2">tools (built-in vs MCP)</h2>
      <table class="w-full text-sm border-collapse">
        <thead>
          <tr class="text-left border-b">
            <th class="p-2">tool</th>
            <th class="p-2">kind</th>
            <th class="p-2">server</th>
            <th class="p-2 text-right">calls</th>
            <th class="p-2 text-right">errors</th>
          </tr>
        </thead>
        <tbody>
          <tr :for={r <- @tools} class="border-b">
            <td class="p-2">{r.tool_name}</td>
            <td class="p-2">{r.kind}</td>
            <td class="p-2">{r.server}</td>
            <td class="p-2 text-right">{r.calls}</td>
            <td class="p-2 text-right">{r.errors}</td>
          </tr>
        </tbody>
      </table>
    </div>
    """
  end
end
