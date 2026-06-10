defmodule DecantWeb.FilesLive do
  use DecantWeb, :live_view

  alias Decant.Archive
  alias DecantWeb.Filters
  alias DecantWeb.TableSort

  @groups ~w(path ext)
  @ops ~w(read edit write delete)

  @impl true
  def mount(_params, _session, socket) do
    {:ok,
     assign(socket,
       bounds: Archive.date_bounds(),
       page_title: "Files",
       files_sort: {:total, :desc}
     )}
  end

  @impl true
  def handle_params(params, _uri, socket) do
    filters = Filters.parse(params)

    group =
      if params["group"] in @groups,
        do: String.to_existing_atom(params["group"]),
        else: :path

    op = if params["op"] in @ops, do: String.to_existing_atom(params["op"]), else: nil

    files =
      Archive.file_hotspots(filters, group: group, op: op)
      |> TableSort.sort(socket.assigns.files_sort)

    {:noreply, assign(socket, filters: filters, group: group, op: op, files: files)}
  end

  @impl true
  def handle_event("sort", %{"table" => "files", "col" => col}, socket) do
    sort = TableSort.toggle(socket.assigns.files_sort, col)

    {:noreply,
     assign(socket, files_sort: sort, files: TableSort.sort(socket.assigns.files, sort))}
  end

  # Group/op chips are patch links carrying the standard filter query, so
  # toggling a view never drops the date/tool/project scope.
  defp view_path(filters, group, op) do
    Filters.url(~p"/files", filters, view_extra(group, op))
  end

  # The page-local view params, in the shape shared controls preserve.
  defp view_extra(group, op), do: [group: group != :path && group, op: op]

  defp basename(nil), do: nil
  defp basename(path), do: path |> String.trim_trailing("/") |> Path.basename()

  defp seg_class(active?) do
    base = "rounded-md px-2.5 py-1 text-sm transition-colors"

    if active? do
      [base, "bg-accent/10 font-medium text-accent"]
    else
      [base, "text-muted hover:bg-elevated hover:text-fg"]
    end
  end

  @impl true
  def render(assigns) do
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:files}
      page_title="Files"
      syncing={@syncing}
      daemon_ready={@daemon_ready}
      metrics={@archive_meta}
    >
      <div class="space-y-5">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 class="text-lg font-semibold tracking-tight">File hotspots</h1>
            <p class="text-sm text-muted">
              What agents touch most. Heavy re-reads with few edits are AGENTS.md / skill
              candidates; heavy edits are churn.
            </p>
          </div>
          <.date_range
            filters={@filters}
            bounds={@bounds}
            path={~p"/files"}
            extra={view_extra(@group, @op)}
          />
        </div>

        <.filter_chips filters={@filters} path={~p"/files"} extra={view_extra(@group, @op)} />

        <div class="flex flex-wrap items-center gap-x-5 gap-y-2">
          <div class="inline-flex items-center gap-1" role="group" aria-label="Group by">
            <.link patch={view_path(@filters, :path, @op)} class={seg_class(@group == :path)}>
              Files
            </.link>
            <.link patch={view_path(@filters, :ext, @op)} class={seg_class(@group == :ext)}>
              Languages
            </.link>
          </div>
          <div class="inline-flex items-center gap-1" role="group" aria-label="Operation">
            <.link patch={view_path(@filters, @group, nil)} class={seg_class(is_nil(@op))}>
              All ops
            </.link>
            <.link
              :for={o <- [:read, :edit, :write, :delete]}
              patch={view_path(@filters, @group, o)}
              class={seg_class(@op == o)}
            >
              {Phoenix.Naming.humanize(o)}
            </.link>
          </div>
        </div>

        <.panel title={if @group == :ext, do: "Languages", else: "Hotspots"} body_class="p-0">
          <:subtitle>Per-operation counts from tool-call evidence, ordered by activity</:subtitle>
          <.empty_state
            :if={@files == []}
            icon="hero-document-text"
            title="No file activity"
            message="Hotspots appear once the daemon has re-ingested the archive with enrichment (automatic after upgrade)."
          />
          <div :if={@files != []} class="overflow-x-auto">
            <table class="w-full text-sm">
              <thead>
                <tr class="border-b border-line text-left text-xs font-medium tracking-wide text-muted uppercase">
                  <.sort_header
                    col="key"
                    label={if @group == :ext, do: "Extension", else: "File"}
                    sort={@files_sort}
                    table="files"
                  />
                  <.sort_header
                    :if={@group == :path}
                    col="project"
                    label="Project"
                    sort={@files_sort}
                    table="files"
                  />
                  <.sort_header
                    col="reads"
                    label="Reads"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                  <.sort_header
                    col="edits"
                    label="Edits"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                  <.sort_header
                    col="writes"
                    label="Writes"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                  <.sort_header
                    col="deletes"
                    label="Deletes"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                  <.sort_header
                    col="sessions"
                    label="Sessions"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                  <.sort_header
                    col="total"
                    label="Total"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                  <.sort_header
                    col="last_touched_at"
                    label="Last touched"
                    sort={@files_sort}
                    table="files"
                    align="right"
                  />
                </tr>
              </thead>
              <tbody>
                <tr
                  :for={r <- @files}
                  class="border-b border-line/60 transition-colors hover:bg-elevated"
                >
                  <td class="px-4 py-2.5 font-mono text-fg">
                    <.link
                      navigate={~p"/search?#{[q: "\"#{r.key}\""]}"}
                      class="hover:text-accent"
                      title={"Search sessions touching #{r.key}"}
                    >
                      {r.key}
                    </.link>
                  </td>
                  <td :if={@group == :path} class="px-4 py-2.5 text-muted" title={r.project}>
                    {basename(r.project) || "·"}
                  </td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.reads)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.edits)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.writes)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.deletes)}</td>
                  <td class="px-4 py-2.5 text-right tabular-nums text-muted">{int(r.sessions)}</td>
                  <td class="px-4 py-2.5 text-right font-medium tabular-nums">{int(r.total)}</td>
                  <td class="px-4 py-2.5 text-right text-muted">
                    {relative_time(r.last_touched_at)}
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
