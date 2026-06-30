defmodule DecantWeb.CoreComponentsTest do
  @moduledoc "Render tests for the core UI primitives."
  use ExUnit.Case, async: true

  import Phoenix.Component, only: [sigil_H: 2]
  import Phoenix.LiveViewTest

  alias DecantWeb.CoreComponents
  alias Phoenix.LiveView.JS

  describe "flash/1" do
    test "renders an info flash with a message from the flash map" do
      html = render_component(&CoreComponents.flash/1, kind: :info, flash: %{"info" => "Saved!"})
      assert html =~ "Saved!"
      assert html =~ "hero-information-circle"
    end

    test "renders an error flash with a title and inner block" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <CoreComponents.flash kind={:error} title="Oops">Bad thing</CoreComponents.flash>
        """)

      assert html =~ "Oops"
      assert html =~ "Bad thing"
      assert html =~ "hero-exclamation-circle"
    end

    test "renders nothing when there is no message" do
      html = render_component(&CoreComponents.flash/1, kind: :info, flash: %{})
      refute html =~ "role=\"alert\""
    end
  end

  describe "button/1" do
    test "renders a <button> by default with the primary variant" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <CoreComponents.button variant="primary">Go</CoreComponents.button>
        """)

      assert html =~ "<button"
      assert html =~ "bg-accent"
      assert html =~ "Go"
    end

    test "renders a link when given navigate, with the danger variant" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <CoreComponents.button variant="danger" navigate="/x">Del</CoreComponents.button>
        """)

      assert html =~ "<a"
      assert html =~ "text-danger"
    end

    test "ghost and default variants render" do
      assigns = %{}

      ghost =
        rendered_to_string(~H"""
        <CoreComponents.button variant="ghost">G</CoreComponents.button>
        """)

      plain =
        rendered_to_string(~H"""
        <CoreComponents.button>P</CoreComponents.button>
        """)

      assert ghost =~ "hover:bg-elevated"
      assert plain =~ "border-line"
    end
  end

  describe "input/1" do
    test "text input renders with a label" do
      html = render_component(&CoreComponents.input/1, name: "email", label: "Email", value: "x")
      assert html =~ "Email"
      assert html =~ ~s(name="email")
    end

    test "hidden input renders" do
      html = render_component(&CoreComponents.input/1, type: "hidden", name: "h", value: "v")
      assert html =~ ~s(type="hidden")
    end

    test "checkbox input renders with the hidden false companion" do
      html =
        render_component(&CoreComponents.input/1,
          type: "checkbox",
          name: "c",
          value: "true",
          label: "Agree"
        )

      assert html =~ ~s(type="checkbox")
      assert html =~ "Agree"
    end

    test "select input renders options and a prompt" do
      html =
        render_component(&CoreComponents.input/1,
          type: "select",
          name: "s",
          label: "Pick",
          prompt: "Choose…",
          options: [{"One", "1"}, {"Two", "2"}],
          value: "1"
        )

      assert html =~ "Choose…"
      assert html =~ "One"
    end

    test "textarea input renders" do
      html =
        render_component(&CoreComponents.input/1,
          type: "textarea",
          name: "t",
          value: "hi",
          label: "Body"
        )

      assert html =~ "<textarea"
      assert html =~ "hi"
    end

    test "renders errors from a used form field" do
      field = %Phoenix.HTML.FormField{
        id: "user_email",
        name: "user[email]",
        value: "x",
        errors: [{"can't be blank", []}],
        field: :email,
        form: %Phoenix.HTML.Form{params: %{"email" => "x"}, source: %{}}
      }

      html = render_component(&CoreComponents.input/1, field: field, type: "text")
      assert html =~ ~s(name="user[email]")
    end
  end

  describe "header/1" do
    test "renders a header with subtitle and actions" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <CoreComponents.header>
          Title
          <:subtitle>Sub</:subtitle>
          <:actions>Act</:actions>
        </CoreComponents.header>
        """)

      assert html =~ "Title"
      assert html =~ "Sub"
      assert html =~ "Act"
    end
  end

  describe "table/1" do
    test "renders rows and an action column" do
      assigns = %{rows: [%{id: 1, name: "a"}, %{id: 2, name: "b"}]}

      html =
        rendered_to_string(~H"""
        <CoreComponents.table id="t" rows={@rows} row_id={fn r -> "row-#{r.id}" end}>
          <:col :let={r} label="Name">{r.name}</:col>
          <:action :let={r}>act-{r.id}</:action>
        </CoreComponents.table>
        """)

      assert html =~ "Name"
      assert html =~ "row-1"
      assert html =~ "act-2"
      assert html =~ "Actions"
    end

    test "derives row ids from a LiveStream and renders with phx-update=stream" do
      stream = %Phoenix.LiveView.LiveStream{
        name: :rows,
        ref: "0",
        inserts: [{"rows-1", -1, %{id: 1, name: "streamed"}, nil, nil}],
        deletes: [],
        reset?: false,
        consumable?: false,
        dom_id: fn _ -> "rows-1" end
      }

      assigns = %{rows: stream}

      html =
        rendered_to_string(~H"""
        <CoreComponents.table id="streamed" rows={@rows}>
          <:col :let={r} label="Name">{r.name}</:col>
        </CoreComponents.table>
        """)

      # The LiveStream branch assigns a row_id deriver and switches the tbody to
      # phx-update="stream" (the rows themselves only materialize inside a live
      # view, which is out of scope for this static render).
      assert html =~ "phx-update=\"stream\""
      assert html =~ "id=\"streamed\""
    end
  end

  describe "icon/1, show/2, hide/2" do
    test "icon renders a hero span" do
      html = render_component(&CoreComponents.icon/1, name: "hero-x-mark", class: "size-4")
      assert html =~ "hero-x-mark"
    end

    test "show/2 and hide/2 build JS commands" do
      assert %JS{} = CoreComponents.show("#x")
      assert %JS{} = CoreComponents.hide("#x")
    end
  end

  describe "translate_error/1 and translate_errors/2" do
    test "interpolates options into the message" do
      assert CoreComponents.translate_error({"is too short (min %{count})", [count: 3]}) ==
               "is too short (min 3)"
    end

    test "translate_errors filters by field" do
      errors = [email: {"is invalid", []}, name: {"too short", []}]
      assert CoreComponents.translate_errors(errors, :email) == ["is invalid"]
    end
  end
end
