defmodule DecantWeb.CoreComponents do
  @moduledoc """
  Core UI primitives (flash, button, input, table, icon) styled with the
  decant design system defined in `assets/css/app.css`.

  Templates use semantic tokens — `bg-surface`, `text-fg`, `border-line`,
  `text-accent`, `text-muted`, status colors — which switch automatically
  between light and dark themes, so no `dark:` variants are needed.

    * [Tailwind CSS](https://tailwindcss.com) — utilities for layout/spacing.
    * [Heroicons](https://heroicons.com) — see `icon/1`.
    * [Phoenix.Component](https://hexdocs.pm/phoenix_live_view/Phoenix.Component.html).
  """
  use Phoenix.Component

  alias Phoenix.LiveView.JS

  @doc """
  Renders flash notices.

  ## Examples

      <.flash kind={:info} flash={@flash} />
      <.flash id="welcome-back" kind={:info} phx-mounted={show("#welcome-back")}>
        Welcome Back!
      </.flash>
  """
  attr :id, :string, doc: "the optional id of flash container"
  attr :flash, :map, default: %{}, doc: "the map of flash messages to display"
  attr :title, :string, default: nil
  attr :kind, :atom, values: [:info, :error], doc: "used for styling and flash lookup"
  attr :rest, :global, doc: "the arbitrary HTML attributes to add to the flash container"

  slot :inner_block, doc: "the optional inner block that renders the flash message"

  def flash(assigns) do
    assigns = assign_new(assigns, :id, fn -> "flash-#{assigns.kind}" end)

    ~H"""
    <div
      :if={msg = render_slot(@inner_block) || Phoenix.Flash.get(@flash, @kind)}
      id={@id}
      phx-click={JS.push("lv:clear-flash", value: %{key: @kind}) |> hide("##{@id}")}
      role="alert"
      class={[
        "w-full cursor-pointer card-surface shadow-lg p-3.5 flex items-start gap-3 border-l-4",
        @kind == :info && "border-l-info",
        @kind == :error && "border-l-danger"
      ]}
      {@rest}
    >
      <.icon
        :if={@kind == :info}
        name="hero-information-circle"
        class="size-5 shrink-0 text-info"
      />
      <.icon
        :if={@kind == :error}
        name="hero-exclamation-circle"
        class="size-5 shrink-0 text-danger"
      />
      <div class="min-w-0 flex-1">
        <p :if={@title} class="text-sm font-semibold">{@title}</p>
        <p class="text-sm text-muted break-words">{msg}</p>
      </div>
      <.icon name="hero-x-mark" class="size-4 shrink-0 text-faint hover:text-fg" />
    </div>
    """
  end

  @doc """
  Renders a button (or link styled as a button).

  ## Examples

      <.button>Send!</.button>
      <.button variant="primary" phx-click="go">Send!</.button>
      <.button navigate={~p"/"}>Home</.button>
  """
  attr :rest, :global, include: ~w(href navigate patch method download name value disabled)
  attr :class, :any, default: nil
  attr :variant, :string, default: nil, values: [nil, "primary", "ghost", "danger"]
  slot :inner_block, required: true

  def button(%{rest: rest} = assigns) do
    base =
      "inline-flex items-center justify-center gap-2 rounded-lg px-3.5 py-2 text-sm font-medium " <>
        "transition-colors disabled:opacity-50 disabled:pointer-events-none select-none"

    variant =
      case assigns[:variant] do
        "primary" -> "bg-accent text-on-accent hover:bg-accent-hover shadow-sm"
        "danger" -> "bg-danger/10 text-danger hover:bg-danger/20"
        "ghost" -> "text-muted hover:text-fg hover:bg-elevated"
        _ -> "border border-line bg-surface text-fg hover:bg-elevated hover:border-line-strong"
      end

    assigns = assign(assigns, :class, [base, variant, assigns[:class]])

    if rest[:href] || rest[:navigate] || rest[:patch] do
      ~H"""
      <.link class={@class} {@rest}>{render_slot(@inner_block)}</.link>
      """
    else
      ~H"""
      <button class={@class} {@rest}>{render_slot(@inner_block)}</button>
      """
    end
  end

  @doc """
  Renders an input with label and error messages.

  A `Phoenix.HTML.FormField` may be passed as argument, which is used to
  retrieve the input name, id, and values. Otherwise all attributes may be
  passed explicitly.
  """
  attr :id, :any, default: nil
  attr :name, :any
  attr :label, :string, default: nil
  attr :value, :any

  attr :type, :string,
    default: "text",
    values: ~w(checkbox color date datetime-local email file month number password
               search select tel text textarea time url week hidden)

  attr :field, Phoenix.HTML.FormField,
    doc: "a form field struct retrieved from the form, for example: @form[:email]"

  attr :errors, :list, default: []
  attr :checked, :boolean, doc: "the checked flag for checkbox inputs"
  attr :prompt, :string, default: nil, doc: "the prompt for select inputs"
  attr :options, :list, doc: "the options to pass to Phoenix.HTML.Form.options_for_select/2"
  attr :multiple, :boolean, default: false, doc: "the multiple flag for select inputs"
  attr :class, :any, default: nil, doc: "the input class to use over defaults"
  attr :error_class, :any, default: nil, doc: "the input error class to use over defaults"

  attr :rest, :global,
    include: ~w(accept autocomplete capture cols disabled form list max maxlength min minlength
                multiple pattern placeholder readonly required rows size step)

  def input(%{field: %Phoenix.HTML.FormField{} = field} = assigns) do
    errors = if Phoenix.Component.used_input?(field), do: field.errors, else: []

    assigns
    |> assign(field: nil, id: assigns.id || field.id)
    |> assign(:errors, Enum.map(errors, &translate_error(&1)))
    |> assign_new(:name, fn -> if assigns.multiple, do: field.name <> "[]", else: field.name end)
    |> assign_new(:value, fn -> field.value end)
    |> input()
  end

  def input(%{type: "hidden"} = assigns) do
    ~H"""
    <input type="hidden" id={@id} name={@name} value={@value} {@rest} />
    """
  end

  def input(%{type: "checkbox"} = assigns) do
    assigns =
      assign_new(assigns, :checked, fn ->
        Phoenix.HTML.Form.normalize_value("checkbox", assigns[:value])
      end)

    ~H"""
    <div class="mb-3">
      <label for={@id} class="flex items-center gap-2 text-sm text-fg">
        <input
          type="hidden"
          name={@name}
          value="false"
          disabled={@rest[:disabled]}
          form={@rest[:form]}
        />
        <input
          type="checkbox"
          id={@id}
          name={@name}
          value="true"
          checked={@checked}
          class={@class || "size-4 rounded border-line bg-surface text-accent focus:ring-accent"}
          {@rest}
        />{@label}
      </label>
      <.error :for={msg <- @errors}>{msg}</.error>
    </div>
    """
  end

  def input(%{type: "select"} = assigns) do
    ~H"""
    <div class="mb-3">
      <label :if={@label} for={@id} class="mb-1 block text-sm font-medium text-muted">{@label}</label>
      <select
        id={@id}
        name={@name}
        class={[@class || field_base(), @errors != [] && (@error_class || "border-danger")]}
        multiple={@multiple}
        {@rest}
      >
        <option :if={@prompt} value="">{@prompt}</option>
        {Phoenix.HTML.Form.options_for_select(@options, @value)}
      </select>
      <.error :for={msg <- @errors}>{msg}</.error>
    </div>
    """
  end

  def input(%{type: "textarea"} = assigns) do
    ~H"""
    <div class="mb-3">
      <label :if={@label} for={@id} class="mb-1 block text-sm font-medium text-muted">{@label}</label>
      <textarea
        id={@id}
        name={@name}
        class={[@class || field_base(), @errors != [] && (@error_class || "border-danger")]}
        {@rest}
      >{Phoenix.HTML.Form.normalize_value("textarea", @value)}</textarea>
      <.error :for={msg <- @errors}>{msg}</.error>
    </div>
    """
  end

  def input(assigns) do
    ~H"""
    <div class="mb-3">
      <label :if={@label} for={@id} class="mb-1 block text-sm font-medium text-muted">{@label}</label>
      <input
        type={@type}
        name={@name}
        id={@id}
        value={Phoenix.HTML.Form.normalize_value(@type, @value)}
        class={[@class || field_base(), @errors != [] && (@error_class || "border-danger")]}
        {@rest}
      />
      <.error :for={msg <- @errors}>{msg}</.error>
    </div>
    """
  end

  # Shared base classes for text-like inputs, selects, and textareas.
  defp field_base do
    "w-full rounded-lg border border-line bg-surface px-3 py-2 text-sm text-fg " <>
      "placeholder:text-faint shadow-sm transition-colors focus:border-accent"
  end

  # Helper used by inputs to generate form errors.
  defp error(assigns) do
    ~H"""
    <p class="mt-1.5 flex items-center gap-2 text-sm text-danger">
      <.icon name="hero-exclamation-circle" class="size-5" />
      {render_slot(@inner_block)}
    </p>
    """
  end

  @doc """
  Renders a section header with an optional subtitle and actions.
  """
  slot :inner_block, required: true
  slot :subtitle
  slot :actions

  def header(assigns) do
    ~H"""
    <header class={[@actions != [] && "flex items-center justify-between gap-6", "pb-4"]}>
      <div>
        <h1 class="text-lg font-semibold leading-8">{render_slot(@inner_block)}</h1>
        <p :if={@subtitle != []} class="text-sm text-muted">{render_slot(@subtitle)}</p>
      </div>
      <div class="flex-none">{render_slot(@actions)}</div>
    </header>
    """
  end

  @doc """
  Renders a table with generic styling.

  ## Examples

      <.table id="users" rows={@users}>
        <:col :let={user} label="id">{user.id}</:col>
        <:col :let={user} label="username">{user.username}</:col>
      </.table>
  """
  attr :id, :string, required: true
  attr :rows, :list, required: true
  attr :row_id, :any, default: nil, doc: "the function for generating the row id"
  attr :row_click, :any, default: nil, doc: "the function for handling phx-click on each row"

  attr :row_item, :any,
    default: &Function.identity/1,
    doc: "the function for mapping each row before calling the :col and :action slots"

  slot :col, required: true do
    attr :label, :string
  end

  slot :action, doc: "the slot for showing user actions in the last table column"

  def table(assigns) do
    assigns =
      with %{rows: %Phoenix.LiveView.LiveStream{}} <- assigns do
        assign(assigns, row_id: assigns.row_id || fn {id, _item} -> id end)
      end

    ~H"""
    <div class="overflow-x-auto">
      <table class="w-full text-sm">
        <thead>
          <tr class="border-b border-line text-left">
            <th :for={col <- @col} class="px-3 py-2 font-medium text-muted">{col[:label]}</th>
            <th :if={@action != []} class="px-3 py-2"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        <tbody id={@id} phx-update={is_struct(@rows, Phoenix.LiveView.LiveStream) && "stream"}>
          <tr
            :for={row <- @rows}
            id={@row_id && @row_id.(row)}
            class="border-b border-line/60 hover:bg-elevated"
          >
            <td
              :for={col <- @col}
              phx-click={@row_click && @row_click.(row)}
              class={["px-3 py-2.5", @row_click && "cursor-pointer"]}
            >
              {render_slot(col, @row_item.(row))}
            </td>
            <td :if={@action != []} class="px-3 py-2.5 text-right">
              <div class="flex justify-end gap-3">
                <%= for action <- @action do %>
                  {render_slot(action, @row_item.(row))}
                <% end %>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
    """
  end

  @doc """
  Renders a [Heroicon](https://heroicons.com).

  ## Examples

      <.icon name="hero-x-mark" />
      <.icon name="hero-arrow-path" class="ml-1 size-3 motion-safe:animate-spin" />
  """
  attr :name, :string, required: true
  attr :class, :any, default: "size-4"

  def icon(%{name: "hero-" <> _} = assigns) do
    ~H"""
    <span class={[@name, @class]} />
    """
  end

  ## JS Commands

  def show(js \\ %JS{}, selector) do
    JS.show(js,
      to: selector,
      time: 200,
      transition:
        {"transition-all ease-out duration-200", "opacity-0 translate-y-1",
         "opacity-100 translate-y-0"}
    )
  end

  def hide(js \\ %JS{}, selector) do
    JS.hide(js,
      to: selector,
      time: 150,
      transition:
        {"transition-all ease-in duration-150", "opacity-100 translate-y-0",
         "opacity-0 translate-y-1"}
    )
  end

  @doc """
  Translates an error message.
  """
  def translate_error({msg, opts}) do
    Enum.reduce(opts, msg, fn {key, value}, acc ->
      String.replace(acc, "%{#{key}}", fn _ -> to_string(value) end)
    end)
  end

  @doc """
  Translates the errors for a field from a keyword list of errors.
  """
  def translate_errors(errors, field) when is_list(errors) do
    for {^field, {msg, opts}} <- errors, do: translate_error({msg, opts})
  end
end
