defmodule DecantWeb.SessionLive.Show do
  use DecantWeb, :live_view

  alias Decant.AgentLauncher
  alias Decant.Archive
  alias Decant.Settings

  @impl true
  def mount(%{"id" => id}, _session, socket) do
    case Archive.get_session(id) do
      nil ->
        {:ok, socket |> put_flash(:error, "Session not found") |> push_navigate(to: ~p"/")}

      detail ->
        {:ok,
         assign(socket,
           detail: detail,
           page_title: detail.summary.title || "Session",
           can_launch: AgentLauncher.can_launch?(),
           ide_label: AgentLauncher.ide_label(Settings.value(:ide, "vscode"))
         )}
    end
  end

  @impl true
  def handle_event("open_ide", _params, socket) do
    case AgentLauncher.open_ide(socket.assigns.detail.summary.project || "") do
      :ok -> {:noreply, put_flash(socket, :info, "Opening #{socket.assigns.ide_label}.")}
      {:error, msg} -> {:noreply, put_flash(socket, :error, msg)}
    end
  end

  @impl true
  def render(assigns) do
    messages = Enum.filter(assigns.detail.messages, &renderable_message?/1)
    toc = build_toc(messages)

    assigns =
      assigns
      |> assign(:indexed, Enum.with_index(messages))
      |> assign(:toc, toc)
      |> assign(:stats, thread_stats(messages, toc, assigns.detail.stats))
      |> assign(:resume_cmds, resume_rows(assigns.detail.summary))

    ~H"""
    <Layouts.app
      flash={@flash}
      active={:sessions}
      page_title={@detail.summary.title || "Session"}
      syncing={@syncing}
      metrics={@archive_meta}
    >
      <div class="sticky top-14 z-10 -mx-4 border-b border-line bg-canvas/85 px-4 py-3 backdrop-blur sm:-mx-6 sm:px-6">
        <div class="mx-auto max-w-6xl space-y-2">
          <h1 class="truncate text-base font-semibold tracking-tight text-fg">
            {@detail.summary.title || "Untitled session"}
          </h1>
          <div class="flex flex-wrap items-center gap-2 text-sm">
            <.tool_badge tool={@detail.summary.tool} />
            <.model_badge model={@detail.summary.model} />
            <span
              :if={@detail.summary.project}
              class="inline-flex items-center gap-1 font-mono text-xs text-muted"
            >
              <.icon name="hero-folder" class="size-3.5" />
              <span class="max-w-[18rem] truncate">{@detail.summary.project}</span>
            </span>
            <button
              :if={@can_launch and @detail.summary.project}
              phx-click="open_ide"
              class="inline-flex items-center gap-1 rounded-md border border-line bg-surface px-2 py-0.5 text-xs text-muted transition-colors hover:border-line-strong hover:bg-elevated hover:text-fg"
            >
              <.icon name="hero-code-bracket" class="size-3.5" /> Open in {@ide_label}
            </button>
          </div>
          <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted">
            <.stat n={@stats.turns} label="turns" />
            <.stat n={@stats.replies} label="replies" />
            <.stat n={@stats.tool_calls} label="tool calls" />
            <span>
              <span class="font-medium text-fg tabular-nums">
                {compact(@stats.input_tokens + @stats.output_tokens)}
              </span>
              tokens
            </span>
            <span class="font-medium text-fg tabular-nums">{money(@detail.summary.cost)}</span>
            <span :if={@stats.duration} class="inline-flex items-center gap-1">
              <.icon name="hero-clock" class="size-3.5 text-faint" />
              <span class="tabular-nums">{dur(@stats.duration)}</span>
            </span>
          </div>
        </div>
      </div>

      <div class="mx-auto mt-6 max-w-6xl">
        <.link
          navigate={~p"/"}
          class="inline-flex items-center gap-1 text-sm text-muted hover:text-fg"
        >
          <.icon name="hero-arrow-left" class="size-4" /> Sessions
        </.link>

        <div
          id="transcript"
          phx-hook="TranscriptNav"
          class="mt-6 lg:grid lg:grid-cols-[220px_minmax(0,1fr)] lg:gap-8"
        >
          <aside class="hidden lg:block">
            <div class="sticky top-40 max-h-[calc(100dvh-11rem)] space-y-2 overflow-auto pr-1">
              <div class="text-xs font-medium tracking-wide text-faint uppercase">In this thread</div>
              <nav :if={@toc != []} class="space-y-0.5">
                <a
                  :for={t <- @toc}
                  href={"#turn-#{t.i}"}
                  data-toc={t.i}
                  class="flex items-start gap-2 rounded-md px-2 py-1 text-xs text-muted transition-colors hover:bg-elevated hover:text-fg data-[active=true]:bg-elevated data-[active=true]:text-fg"
                >
                  <span class="mt-1 size-1.5 shrink-0 rounded-full bg-line-strong"></span>
                  <span class="line-clamp-2 leading-snug">{t.label}</span>
                  <span
                    :if={t.tools > 0}
                    class="ml-auto shrink-0 text-[10px] tabular-nums text-faint"
                    title="tool calls"
                  >
                    {t.tools}
                  </span>
                </a>
              </nav>
              <p :if={@toc == []} class="text-xs text-faint">No prompts to list</p>
            </div>
          </aside>

          <div class="min-w-0 max-w-3xl">
            <div class="mb-4 flex items-center justify-end gap-1">
              <button
                type="button"
                data-expand-all
                class="rounded-md border border-line bg-surface px-2 py-1 text-xs text-muted transition-colors hover:bg-elevated hover:text-fg"
              >
                Expand all
              </button>
              <button
                type="button"
                data-collapse-all
                class="rounded-md border border-line bg-surface px-2 py-1 text-xs text-muted transition-colors hover:bg-elevated hover:text-fg"
              >
                Collapse all
              </button>
            </div>

            <div :if={@resume_cmds != []} class="mb-8">
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
                    <span
                      class="hero-clipboard-document ml-auto size-4 shrink-0 text-muted"
                      data-copy-icon
                    >
                    </span>
                  </button>
                </div>
                <p class="mt-3 text-xs text-muted">
                  Run these in the session's working directory. Threads are tied to their cwd.
                </p>
              </.panel>
            </div>

            <div class="space-y-8 pb-16">
              <article :for={{m, i} <- @indexed} id={"turn-#{i}"} class="scroll-mt-44 space-y-2.5">
                <.badge tone={role_tone(m.role)} mono class="text-[11px] tracking-wide uppercase">
                  {m.role}
                </.badge>

                <div class="space-y-3 border-l-2 border-line pl-4">
                  <%= for b <- m.blocks do %>
                    <p
                      :if={b.type == "text"}
                      class="text-[15px] leading-relaxed whitespace-pre-wrap text-fg"
                    >
                      {b.text || ""}
                    </p>

                    <details :if={b.type == "thinking" and trimmed?(b.text)} class="group">
                      <summary class="cursor-pointer text-xs text-muted select-none hover:text-fg">
                        <.icon
                          name="hero-chevron-right"
                          class="inline size-3 transition-transform group-open:rotate-90"
                        /> Thinking
                      </summary>
                      <p class="mt-2 text-[15px] leading-relaxed whitespace-pre-wrap text-muted italic">
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
                        <summary class="cursor-pointer text-xs text-muted select-none hover:text-fg">
                          arguments
                        </summary>
                        <pre class="mt-2 overflow-x-auto font-mono text-xs whitespace-pre-wrap text-muted">{b.tool_input}</pre>
                      </details>
                    </div>

                    <details
                      :if={b.type == "tool_result" and trimmed?(b.tool_result)}
                      class="rounded-lg border border-line bg-elevated px-3 py-2"
                    >
                      <summary class="cursor-pointer text-xs text-muted select-none hover:text-fg">
                        result
                      </summary>
                      <pre class="mt-2 overflow-x-auto font-mono text-xs whitespace-pre-wrap text-fg">{b.tool_result}</pre>
                    </details>
                  <% end %>
                </div>
              </article>
            </div>
          </div>
        </div>
      </div>
    </Layouts.app>
    """
  end

  attr :n, :integer, required: true
  attr :label, :string, required: true

  defp stat(assigns) do
    ~H"""
    <span><span class="font-medium text-fg tabular-nums">{int(@n)}</span> {@label}</span>
    """
  end

  # The user prompts that form the navigable spine of the thread.
  defp build_toc(messages) do
    messages
    |> Enum.with_index()
    |> Enum.filter(fn {m, _i} -> m.role == "user" and has_text?(m) end)
    |> Enum.map(fn {m, i} ->
      %{i: i, label: turn_label(m), tools: Enum.count(m.blocks || [], &(&1.type == "tool_use"))}
    end)
  end

  defp thread_stats(messages, toc, session_stats) do
    %{
      turns: length(toc),
      replies: Enum.count(messages, &(&1.role == "assistant")),
      tool_calls:
        Enum.sum(
          Enum.map(messages, fn m -> Enum.count(m.blocks || [], &(&1.type == "tool_use")) end)
        ),
      input_tokens: session_stats.input_tokens,
      output_tokens: session_stats.output_tokens,
      duration: session_stats.duration_seconds
    }
  end

  defp has_text?(m), do: Enum.any?(m.blocks || [], &(&1.type == "text" and trimmed?(&1.text)))

  defp turn_label(m) do
    (m.blocks || [])
    |> Enum.find_value("", fn b -> if b.type == "text" and trimmed?(b.text), do: b.text end)
    |> String.trim()
    |> String.split("\n", parts: 2)
    |> List.first()
    |> truncate(70)
  end

  defp truncate(s, n) when is_binary(s) and byte_size(s) > n, do: String.slice(s, 0, n) <> "…"
  defp truncate(s, _n), do: s

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
    Enum.any?(m.blocks || [], fn b ->
      case b.type do
        t when t in ["text", "thinking"] -> is_binary(b.text) and String.trim(b.text) != ""
        t when t in ["tool_use", "tool_result"] -> true
        _ -> false
      end
    end)
  end
end
