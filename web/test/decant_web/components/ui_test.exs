defmodule DecantWeb.Components.UITest do
  @moduledoc "Render tests for the design-system components."
  use ExUnit.Case, async: true

  import Phoenix.Component, only: [sigil_H: 2]
  import Phoenix.LiveViewTest

  alias DecantWeb.Components.UI

  describe "panel/1 and stat_card/1" do
    test "panel renders a header (title/subtitle/actions) and body" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.panel title="By model">
          <:subtitle>sub</:subtitle>
          <:actions>act</:actions>
          body
        </UI.panel>
        """)

      assert html =~ "By model"
      assert html =~ "sub"
      assert html =~ "act"
      assert html =~ "body"
    end

    test "stat_card renders label/value/hint/icon and a chart slot" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.stat_card
          label="Cost"
          value="$1.00"
          hint="all time"
          icon="hero-currency-dollar"
          tone={:success}
        >
          <:chart>spark</:chart>
        </UI.stat_card>
        """)

      assert html =~ "Cost"
      assert html =~ "$1.00"
      assert html =~ "all time"
      assert html =~ "spark"
    end
  end

  describe "badge/1 across tones" do
    for tone <- [:neutral, :accent, :success, :warning, :danger, :info, :claude, :openai] do
      test "badge renders the #{tone} tone" do
        assigns = %{tone: unquote(tone)}

        html =
          rendered_to_string(~H"""
          <UI.badge tone={@tone} mono>x</UI.badge>
          """)

        assert html =~ "x"
      end
    end
  end

  describe "tool_badge/1 and model_badge/1" do
    test "claude_code and codex map to brand labels" do
      assigns = %{}

      claude =
        rendered_to_string(~H"""
        <UI.tool_badge tool="claude_code" />
        """)

      codex =
        rendered_to_string(~H"""
        <UI.tool_badge tool="codex" />
        """)

      other =
        rendered_to_string(~H"""
        <UI.tool_badge tool={nil} />
        """)

      assert claude =~ "Claude"
      assert codex =~ "Codex"
      assert other =~ "·"
    end

    test "model_badge renders for a model and the placeholder for nil" do
      assigns = %{}

      with_model =
        rendered_to_string(~H"""
        <UI.model_badge model="claude-opus-4-7" />
        """)

      without =
        rendered_to_string(~H"""
        <UI.model_badge model={nil} />
        """)

      assert with_model =~ "claude-opus-4-7"
      assert without =~ "·"
    end

    test "brand_tone classifies models" do
      assert UI.brand_tone("claude-opus-4-7") == :claude
      assert UI.brand_tone("gpt-5.4") == :openai
      assert UI.brand_tone("gemini") == :neutral
    end
  end

  describe "sort_header/1" do
    test "renders an active ascending header (chevron up)" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.sort_header col="calls" label="Calls" sort={{:calls, :asc}} table="t" align="right" />
        """)

      assert html =~ "Calls"
      assert html =~ "hero-chevron-up"
    end

    test "renders an inactive header (chevron down)" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.sort_header col="calls" label="Calls" sort={{:errors, :desc}} />
        """)

      assert html =~ "hero-chevron-down"
    end
  end

  describe "empty_state/1, chart/1, bar/1, copy_button/1" do
    test "empty_state renders icon/title/message and inner block" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.empty_state icon="hero-inbox" title="Nothing" message="here">cta</UI.empty_state>
        """)

      assert html =~ "Nothing"
      assert html =~ "here"
      assert html =~ "cta"
    end

    test "chart serializes the spec into a data attribute" do
      assigns = %{spec: %{type: "bar", series: []}}

      html =
        rendered_to_string(~H"""
        <UI.chart id="c" spec={@spec} />
        """)

      assert html =~ "phx-hook=\"Chart\""
      assert html =~ "data-spec"
    end

    test "bar clamps the fraction to 0..100%" do
      assigns = %{}

      over =
        rendered_to_string(~H"""
        <UI.bar fraction={2.0} />
        """)

      assert over =~ "width: 100"
    end

    for tone <- [:claude, :openai, :success, :warning, :danger, :info, :accent] do
      test "bar renders the #{tone} tone" do
        assigns = %{tone: unquote(tone)}

        html =
          rendered_to_string(~H"""
          <UI.bar fraction={0.5} tone={@tone} />
          """)

        assert html =~ "width: 50"
      end
    end

    test "copy_button renders a hooked clipboard button" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.copy_button id="cp" text="copy me" />
        """)

      assert html =~ "phx-hook=\"Copy\""
      assert html =~ "copy me"
    end
  end

  describe "agent_cta/1" do
    test "renders the split launch button when launching is available" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.agent_cta
          id="signal:error:Read"
          prompt="do it"
          agents={[{"claude", "Claude"}, {"codex", "Codex"}]}
          default="claude"
          can_launch={true}
          mark_key="signal:error:Read"
        />
        """)

      # The colon-bearing id is sanitized for CSS selectors.
      assert html =~ "signal-error-Read-menu"
      assert html =~ "Run in Claude"
    end

    test "falls back to a copy button when launching is unavailable" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.agent_cta
          id="rec-1"
          prompt="do it"
          agents={[{"codex", "Codex"}]}
          default="codex"
          can_launch={false}
        />
        """)

      assert html =~ "phx-hook=\"Copy\""
    end
  end

  describe "date_range/1 and filter_chips/1" do
    test "date_range renders prev/next when a range is set" do
      assigns = %{
        filters: %{from: ~D[2026-05-01], to: ~D[2026-05-07]},
        bounds: %{max: "2026-05-10"}
      }

      html =
        rendered_to_string(~H"""
        <UI.date_range filters={@filters} bounds={@bounds} path="/analytics" />
        """)

      assert html =~ "Previous period"
      assert html =~ "Next period"
      assert html =~ "7d"
    end

    test "filter_chips renders removable chips" do
      assigns = %{filters: %{model: "gpt-5.4", tool: "codex", project: "/p"}}

      html =
        rendered_to_string(~H"""
        <UI.filter_chips filters={@filters} path="/" />
        """)

      assert html =~ "Filtered by"
      assert html =~ "gpt-5.4"
    end
  end

  describe "sparkline/1" do
    for tone <- [:claude, :openai, :success, :warning, :danger, :info, :accent] do
      test "renders a polyline for the #{tone} tone" do
        assigns = %{tone: unquote(tone)}

        html =
          rendered_to_string(~H"""
          <UI.sparkline values={[1, 4, 2, 5]} tone={@tone} />
          """)

        assert html =~ "polyline"
      end
    end

    test "renders a placeholder when there is too little data" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.sparkline values={[1]} />
        """)

      assert html =~ "·"
      refute html =~ "polyline"
    end

    test "handles an all-zero series without dividing by zero" do
      assigns = %{}

      html =
        rendered_to_string(~H"""
        <UI.sparkline values={[0, 0, 0]} />
        """)

      assert html =~ "polyline"
    end
  end
end
