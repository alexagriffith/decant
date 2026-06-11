defmodule DecantWeb.Components.IconsTest do
  @moduledoc "Tests for brand icon dispatchers and the brand classifier."
  use ExUnit.Case, async: true

  import Phoenix.LiveViewTest

  alias DecantWeb.Components.Icons

  describe "brand/1" do
    test "nil is :other" do
      assert Icons.brand(nil) == :other
    end

    test "claude family models map to :claude" do
      for m <- ["claude-opus-4-7", "opus", "sonnet", "haiku"] do
        assert Icons.brand(m) == :claude
      end
    end

    test "openai family models map to :openai" do
      for m <- ["gpt-5.4", "codex", "o1-preview", "o3-mini"] do
        assert Icons.brand(m) == :openai
      end
    end

    test "anything else is :other" do
      assert Icons.brand("gemini-pro") == :other
    end
  end

  describe "single brand marks" do
    test "claude/1 renders an svg path" do
      html = render_component(&Icons.claude/1, class: "size-4")
      assert html =~ "<svg"
      assert html =~ "<path"
    end

    test "openai/1 renders an svg path" do
      assert render_component(&Icons.openai/1, %{}) =~ "<path"
    end

    test "anthropic/1 renders an svg path" do
      assert render_component(&Icons.anthropic/1, %{}) =~ "<path"
    end
  end

  describe "model_icon/1" do
    test "renders the Claude mark for a Claude model" do
      assert render_component(&Icons.model_icon/1, model: "claude-opus-4-7") =~ "text-claude"
    end

    test "renders the OpenAI mark for an OpenAI model" do
      assert render_component(&Icons.model_icon/1, model: "gpt-5.4") =~ "<svg"
    end

    test "renders nothing for an unknown model" do
      html = render_component(&Icons.model_icon/1, model: "gemini")
      refute html =~ "<svg"
    end
  end

  describe "tool_icon/1" do
    test "claude_code renders the Claude mark" do
      assert render_component(&Icons.tool_icon/1, tool: "claude_code") =~ "text-claude"
    end

    test "codex renders the OpenAI mark" do
      assert render_component(&Icons.tool_icon/1, tool: "codex") =~ "<svg"
    end
  end

  describe "server_icon/1" do
    test "a known server uses its brand mark" do
      assert render_component(&Icons.server_icon/1, server: "github") =~ "<path"
    end

    test "a matched substring server uses the brand mark" do
      assert render_component(&Icons.server_icon/1, server: "my-linear-mcp") =~ "<path"
    end

    test "nil falls back to the generic MCP mark" do
      assert render_component(&Icons.server_icon/1, server: nil) =~ "<path"
    end

    test "empty string falls back to the generic MCP mark" do
      assert render_component(&Icons.server_icon/1, server: "") =~ "<path"
    end

    test "an unknown server falls back to the generic MCP mark" do
      assert render_component(&Icons.server_icon/1, server: "totally-unknown") =~ "<path"
    end
  end
end
