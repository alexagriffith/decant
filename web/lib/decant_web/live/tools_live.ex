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
    <Layouts.app flash={@flash} active={:tools} page_title="Tools & MCP" syncing={@syncing}>
      <div class="space-y-4">
        <.panel title="MCP servers" body_class="p-0">
          <:subtitle>Model Context Protocol servers and their call volume</:subtitle>

          <.empty_state
            :if={@mcp == []}
            icon="hero-cpu-chip"
            title="No MCP servers"
            message="No MCP tool calls in this archive yet."
          />

          <div :if={@mcp != []} class="overflow-x-auto">
            <table class="w-full border-collapse">
              <thead>
                <tr class="border-b border-line">
                  <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                    Server
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                    Tools
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                    Calls
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                    Errors
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @mcp}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-3 py-2.5 font-mono text-sm text-fg">{r.server}</td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums text-muted">
                    {int(r.tools)}
                  </td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums">{int(r.calls)}</td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums">
                    <.badge :if={r.errors > 0} tone={:danger}>{int(r.errors)}</.badge>
                    <span :if={r.errors == 0} class="text-muted">0</span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>

        <.panel title="Tools" body_class="p-0">
          <:subtitle>Built-in and MCP tools invoked across all sessions</:subtitle>

          <.empty_state
            :if={@tools == []}
            icon="hero-wrench-screwdriver"
            title="No tools"
            message="No tool calls in this archive yet."
          />

          <div :if={@tools != []} class="overflow-x-auto">
            <table class="w-full border-collapse">
              <thead>
                <tr class="border-b border-line">
                  <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                    Tool
                  </th>
                  <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                    Kind
                  </th>
                  <th class="px-3 py-2.5 text-left text-xs font-medium tracking-wide text-muted uppercase">
                    Server
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                    Calls
                  </th>
                  <th class="px-3 py-2.5 text-right text-xs font-medium tracking-wide text-muted uppercase">
                    Errors
                  </th>
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @tools}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-3 py-2.5 font-mono text-sm font-medium text-fg">{r.tool_name}</td>
                  <td class="px-3 py-2.5 text-sm">
                    <.badge tone={(r.kind == "builtin" && :neutral) || :accent}>
                      {(r.kind == "builtin" && "built-in") || "MCP"}
                    </.badge>
                  </td>
                  <td class="px-3 py-2.5 font-mono text-sm text-muted">
                    {(r.server && r.server != "" && r.server) || "—"}
                  </td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums">{int(r.calls)}</td>
                  <td class="px-3 py-2.5 text-right text-sm tabular-nums">
                    <.badge :if={r.errors > 0} tone={:danger}>{int(r.errors)}</.badge>
                    <span :if={r.errors == 0} class="text-muted">0</span>
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
