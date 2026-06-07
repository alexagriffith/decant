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
    ~H"""
    <Layouts.app
      flash={@flash}
      active={:sessions}
      page_title={@detail.summary.title || "Session"}
      syncing={@syncing}
    >
      <div class="space-y-8">
        <.link
          navigate={~p"/"}
          class="inline-flex items-center gap-1 text-sm text-muted hover:text-fg"
        >
          <.icon name="hero-arrow-left" class="size-4" /> Sessions
        </.link>

        <header class="space-y-3">
          <h1 class="text-xl font-semibold tracking-tight text-fg">
            {@detail.summary.title || "Untitled session"}
          </h1>
          <div class="flex flex-wrap items-center gap-2 text-sm">
            <.tool_badge tool={@detail.summary.tool} />
            <.model_badge model={@detail.summary.model} />
            <span class="text-muted">{int(@detail.summary.message_count)} messages</span>
            <span
              :if={@detail.summary.project}
              class="inline-flex items-center gap-1 font-mono text-muted"
            >
              <.icon name="hero-folder" class="size-4" />
              {@detail.summary.project}
            </span>
            <span class="text-muted">{money(@detail.summary.cost)} est. cost</span>
          </div>
        </header>

        <div class="mx-auto max-w-3xl space-y-6">
          <article :for={m <- Enum.filter(@detail.messages, &renderable_message?/1)} class="space-y-2">
            <.badge tone={role_tone(m.role)} mono class="uppercase">{m.role}</.badge>

            <div class="space-y-2">
              <%= for b <- m.blocks do %>
                <div
                  :if={b.type in ["text", "thinking"]}
                  class={[
                    "whitespace-pre-wrap text-sm leading-relaxed text-fg",
                    b.type == "thinking" && "italic text-muted"
                  ]}
                >
                  {b.text || ""}
                </div>

                <details
                  :if={b.type == "tool_use"}
                  open
                  class="rounded-lg border border-line bg-surface p-3"
                >
                  <summary class="flex cursor-pointer items-center gap-2 text-sm">
                    <.icon name="hero-bolt" class="size-4 text-accent" />
                    <span class="font-mono text-accent">{b.tool_name}</span>
                    <span class="text-muted">tool call</span>
                  </summary>
                  <pre class="mt-2 overflow-x-auto font-mono text-xs text-muted whitespace-pre-wrap">{b.tool_input}</pre>
                </details>

                <details
                  :if={b.type == "tool_result"}
                  class="rounded-lg border border-line bg-elevated p-3"
                >
                  <summary class="cursor-pointer text-sm text-muted">result</summary>
                  <pre class="mt-2 overflow-x-auto font-mono text-xs whitespace-pre-wrap">{b.tool_result}</pre>
                </details>
              <% end %>
            </div>
          </article>
        </div>
      </div>
    </Layouts.app>
    """
  end

  defp role_tone("assistant"), do: :accent
  defp role_tone("user"), do: :neutral
  defp role_tone("tool"), do: :info
  defp role_tone(_), do: :neutral

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
