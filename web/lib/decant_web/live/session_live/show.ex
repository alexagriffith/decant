defmodule DecantWeb.SessionLive.Show do
  use DecantWeb, :live_view

  alias Decant.Archive

  @impl true
  def mount(%{"id" => id}, _session, socket) do
    case Archive.get_session(id) do
      nil ->
        {:ok, socket |> put_flash(:error, "Session not found") |> push_navigate(to: ~p"/")}

      detail ->
        {:ok, assign(socket, detail: detail, page_title: detail.summary.title || "Session")}
    end
  end

  @impl true
  def render(assigns) do
    assigns =
      assigns
      |> assign(:messages, Enum.filter(assigns.detail.messages, &renderable_message?/1))
      |> assign(:resume_cmds, resume_rows(assigns.detail.summary))

    ~H"""
    <Layouts.app
      flash={@flash}
      active={:sessions}
      page_title={@detail.summary.title || "Session"}
      syncing={@syncing}
      metrics={@archive_meta}
    >
      <div class="sticky top-14 z-10 -mx-4 border-b border-line bg-canvas/80 px-4 py-3 backdrop-blur sm:-mx-6 sm:px-6">
        <div class="mx-auto max-w-3xl space-y-2">
          <h1 class="truncate text-base font-semibold tracking-tight text-fg">
            {@detail.summary.title || "Untitled session"}
          </h1>
          <div class="flex flex-wrap items-center gap-2 text-sm">
            <.tool_badge tool={@detail.summary.tool} />
            <.model_badge model={@detail.summary.model} />
            <span class="text-muted tabular-nums">
              {int(@detail.summary.message_count)} messages
            </span>
            <span
              :if={@detail.summary.project}
              class="inline-flex items-center gap-1 font-mono text-xs text-muted"
            >
              <.icon name="hero-folder" class="size-3.5" />
              <span class="truncate max-w-[18rem]">{@detail.summary.project}</span>
            </span>
            <span class="text-muted tabular-nums">{money(@detail.summary.cost)} est. cost</span>
          </div>
        </div>
      </div>

      <div class="mx-auto max-w-3xl">
        <.link
          navigate={~p"/"}
          class="mt-6 inline-flex items-center gap-1 text-sm text-muted hover:text-fg"
        >
          <.icon name="hero-arrow-left" class="size-4" /> Sessions
        </.link>

        <div :if={@resume_cmds != []} class="mt-6">
          <.panel title="Continue this thread">
            <div class="space-y-2">
              <button
                :for={row <- @resume_cmds}
                id={"resume-#{row.key}"}
                phx-hook="Copy"
                data-copy={row.cmd}
                class="flex w-full items-center gap-2 rounded-lg border border-line bg-surface px-3 py-2 text-left font-mono text-xs hover:bg-elevated"
              >
                <span class="shrink-0 rounded bg-elevated px-1.5 py-0.5 text-[10px] font-medium tracking-wide text-muted uppercase">
                  {row.label}
                </span>
                <span class="truncate text-fg">{row.cmd}</span>
                <span data-copy-icon class="hero-clipboard-document ml-auto size-4 shrink-0 text-muted">
                </span>
              </button>
            </div>
            <p class="mt-3 text-xs text-muted">
              Run these in the session's working directory. Threads are tied to their cwd.
            </p>
          </.panel>
        </div>

        <div class="mt-8 space-y-8 pb-16">
          <article :for={m <- @messages} class="space-y-2.5">
            <.badge tone={role_tone(m.role)} mono class="text-[11px] tracking-wide uppercase">
              {m.role}
            </.badge>

            <div class="space-y-3 border-l-2 border-line pl-4">
              <%= for b <- m.blocks do %>
                <p
                  :if={b.type == "text"}
                  class="whitespace-pre-wrap text-[15px] leading-relaxed text-fg"
                >
                  {b.text || ""}
                </p>

                <details :if={b.type == "thinking" and trimmed?(b.text)} class="group">
                  <summary class="cursor-pointer select-none text-xs text-muted hover:text-fg">
                    <.icon
                      name="hero-chevron-right"
                      class="inline size-3 transition-transform group-open:rotate-90"
                    /> Thinking
                  </summary>
                  <p class="mt-2 whitespace-pre-wrap text-[15px] italic leading-relaxed text-muted">
                    {b.text || ""}
                  </p>
                </details>

                <div
                  :if={b.type == "tool_use"}
                  class="rounded-lg border border-line bg-surface px-3 py-2"
                >
                  <div class="flex items-center gap-2 text-sm">
                    <.icon name="hero-bolt" class="size-4 shrink-0 text-accent" />
                    <span class="font-mono text-fg">{b.tool_name}</span>
                    <span class="text-xs text-muted">tool call</span>
                  </div>
                  <details :if={trimmed?(b.tool_input)} open={short?(b.tool_input)} class="mt-1">
                    <summary class="cursor-pointer select-none text-xs text-muted hover:text-fg">
                      arguments
                    </summary>
                    <pre class="mt-2 overflow-x-auto font-mono text-xs whitespace-pre-wrap text-muted">{b.tool_input}</pre>
                  </details>
                </div>

                <details
                  :if={b.type == "tool_result" and trimmed?(b.tool_result)}
                  class="rounded-lg border border-line bg-elevated px-3 py-2"
                >
                  <summary class="cursor-pointer select-none text-xs text-muted hover:text-fg">
                    result
                  </summary>
                  <pre class="mt-2 overflow-x-auto font-mono text-xs whitespace-pre-wrap text-fg">{b.tool_result}</pre>
                </details>
              <% end %>
            </div>
          </article>
        </div>
      </div>
    </Layouts.app>
    """
  end

  # Build copy-able command rows from the resume commands, dropping any that the
  # tool doesn't support (nil). Order: resume, fork, new.
  defp resume_rows(summary) do
    cmds = DecantWeb.Resume.commands(summary)

    [
      {:resume, "Resume", cmds.resume},
      {:fork, "Fork", cmds.fork},
      {:new, "New session", cmds.new}
    ]
    |> Enum.flat_map(fn
      {_key, _label, nil} -> []
      {key, label, cmd} -> [%{key: key, label: label, cmd: cmd}]
    end)
  end

  defp role_tone("assistant"), do: :accent
  defp role_tone("user"), do: :neutral
  defp role_tone("tool"), do: :info
  defp role_tone(_), do: :neutral

  defp trimmed?(s), do: is_binary(s) and String.trim(s) != ""

  # Short payloads default to open; long ones stay collapsed so they don't
  # dominate the reading column.
  defp short?(s), do: is_binary(s) and String.length(s) <= 240

  # Skip meta/empty messages (e.g. summary or system markers) that carry no
  # renderable text or tool blocks, so the transcript reads cleanly.
  defp renderable_message?(m) do
    Enum.any?(m.blocks, fn b ->
      case b.type do
        t when t in ["text", "thinking"] -> is_binary(b.text) and String.trim(b.text) != ""
        t when t in ["tool_use", "tool_result"] -> true
        _ -> false
      end
    end)
  end
end
