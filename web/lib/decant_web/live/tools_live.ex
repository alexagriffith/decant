defmodule DecantWeb.ToolsLive do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters
  alias DecantWeb.TableSort

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Tools & MCP",
       tools_sort: {:calls, :desc},
       mcp_sort: {:calls, :desc}
     )}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)

    {:noreply,
     assign(socket,
       filters: filters,
       tools: Archive.tool_usage(filters, 100) |> TableSort.sort(socket.assigns.tools_sort),
       mcp: Archive.mcp_usage(filters, 100) |> TableSort.sort(socket.assigns.mcp_sort)
     )}
  end

  @impl true
  def handle_event("sort", %{"table" => "tools", "col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.tools_sort, col)
    {:noreply, assign(socket, tools_sort: sort, tools: TableSort.sort(socket.assigns.tools, sort))}
  end

  def handle_event("sort", %{"table" => "mcp", "col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.mcp_sort, col)
    {:noreply, assign(socket, mcp_sort: sort, mcp: TableSort.sort(socket.assigns.mcp, sort))}
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:tools}
      page_title="Tools & MCP"
      syncing={@syncing}
      metrics={@archive_meta}
    >
      <div class="space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold tracking-tight">Tools &amp; MCP</h1>
            <p class="text-sm text-muted">
              Tool and MCP-server call volume, scoped to the selected range.
            </p>
          </div>
          <.date_range filters={@filters} bounds={@bounds} path={~p"/tools"} />
        </div>

        <.filter_chips filters={@filters} path={~p"/tools"} />

        <.panel title="MCP servers" body_class="p-0">
          <:subtitle>Model Context Protocol servers and their call volume</:subtitle>
          <.empty_state
            :if={@mcp == []}
            icon="hero-cpu-chip"
            title="No MCP servers"
            message="No MCP tool calls in this range."
          />
          <div :if={@mcp != []} class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <.sort_header col="server" label="Server" sort={@mcp_sort} table="mcp" />
                  <.sort_header col="tools" label="Tools" sort={@mcp_sort} table="mcp" align="right" />
                  <.sort_header col="calls" label="Calls" sort={@mcp_sort} table="mcp" align="right" />
                  <.sort_header col="errors" label="Errors" sort={@mcp_sort} table="mcp" align="right" />
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @mcp}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-4 py-2.5">
                    <span class="inline-flex items-center gap-2 font-mono text-fg">
                      <.server_icon server={r.server} class="size-4 shrink-0 text-muted" />{r.server}
                    </span>
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.tools)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums">{int(r.calls)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums">
                    <.badge :if={r.errors > 0} tone={:danger}>{int(r.errors)}</.badge>
                    <span :if={r.errors == 0} class="text-faint">0</span>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </.panel>

        <.panel title="Tools" body_class="p-0">
          <:subtitle>Built-in vs MCP, most-called first</:subtitle>
          <div class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <.sort_header col="tool_name" label="Tool" sort={@tools_sort} table="tools" />
                  <.sort_header col="kind" label="Kind" sort={@tools_sort} table="tools" />
                  <.sort_header col="server" label="Server" sort={@tools_sort} table="tools" />
                  <.sort_header col="calls" label="Calls" sort={@tools_sort} table="tools" align="right" />
                  <.sort_header col="errors" label="Errors" sort={@tools_sort} table="tools" align="right" />
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @tools}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-4 py-2.5 font-mono font-medium text-fg">{r.tool_name}</td>
                  <td class="px-4 py-2.5">
                    <.badge :if={r.kind == "mcp"} tone={:accent}>MCP</.badge>
                    <.badge :if={r.kind != "mcp"} tone={:neutral}>built-in</.badge>
                  </td>
                  <td class="px-4 py-2.5">
                    <span
                      :if={r.server && r.server != ""}
                      class="inline-flex items-center gap-2 font-mono text-muted"
                    >
                      <.server_icon server={r.server} class="size-4 shrink-0" />{r.server}
                    </span>
                    <span :if={!(r.server && r.server != "")} class="text-faint">·</span>
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums">{int(r.calls)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums">
                    <.badge :if={r.errors > 0} tone={:danger}>{int(r.errors)}</.badge>
                    <span :if={r.errors == 0} class="text-faint">0</span>
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
