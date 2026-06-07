defmodule Decant.Insights do
  @moduledoc """
  Derives actionable "what could help my coding agents" signals from the archive
  (error hotspots, heavy tools/servers, cost concentration) and pairs them with a
  curated catalog of agent enhancements. Each signal and catalog entry carries a
  `prompt` so the UI can open a coding agent seeded to act on it. Read-only.
  """
  alias Decant.Archive

  @doc "Data-derived signals, highest-impact first. Each has title/detail/suggestion/tone/icon/prompt."
  def signals(filters \\ %{}) do
    tools = Archive.tool_usage(filters, 500)
    mcp = Archive.mcp_usage(filters, 500)
    models = Archive.by_dimension(:model, filters) |> Enum.sort_by(& &1.cost, :desc)

    (error_hotspots(tools) ++
       heavy_servers(mcp) ++ heavy_tools(tools) ++ cost_concentration(models))
    |> Enum.sort_by(& &1.score, :desc)
    |> Enum.take(12)
    |> Enum.map(&Map.delete(&1, :score))
  end

  defp error_hotspots(tools) do
    for t <- tools, t.calls >= 20, t.errors / t.calls >= 0.12 do
      pct = round(t.errors / t.calls * 100)

      %{
        score: t.errors / t.calls * t.calls,
        tone: :danger,
        icon: "hero-exclamation-triangle",
        url: "https://code.claude.com/docs/en/skills",
        link_label: "Skills guide",
        title: "#{t.tool_name} fails #{pct}% of the time",
        detail: "#{t.errors} errors across #{t.calls} calls#{server_suffix(t.server)}.",
        suggestion:
          "Codify the recovery path as a Skill (or fix the call sites) so agents stop repeating this failure.",
        prompt:
          "The #{t.tool_name} tool is failing about #{pct}% of the time (#{t.errors} errors in #{t.calls} calls)#{server_suffix(t.server)}. Investigate the common failure mode and codify a reusable Skill (or guardrail) so agents handle it consistently. Follow this repo's AGENTS.md and Skill conventions."
      }
    end
  end

  defp heavy_servers(mcp) do
    for s <- Enum.take(mcp, 3), s.calls >= 50 do
      %{
        score: s.calls / 2,
        tone: :accent,
        icon: "hero-cpu-chip",
        url: "https://code.claude.com/docs/en/skills",
        link_label: "Skills guide",
        title: "Heavy reliance on the #{s.server} MCP server",
        detail: "#{s.calls} calls across #{s.tools} tools.",
        suggestion:
          "Package the common #{s.server} workflows into a reusable Skill so agents use them consistently.",
        prompt:
          "We rely heavily on the #{s.server} MCP server (#{s.calls} calls across #{s.tools} tools). Create a reusable Skill that packages our most common #{s.server} workflows so agents use them consistently. Follow this repo's Skill conventions."
      }
    end
  end

  defp heavy_tools(tools) do
    builtin = Enum.filter(tools, &(&1.kind != "mcp"))

    for t <- Enum.take(builtin, 2), t.calls >= 200 do
      %{
        score: t.calls / 4,
        tone: :info,
        icon: "hero-bolt",
        url: "https://code.claude.com/docs/en/skills",
        link_label: "Skills guide",
        title: "#{t.tool_name} is one of your busiest tools",
        detail: "#{t.calls} calls.",
        suggestion:
          "High-frequency tools make good Skill candidates. Capture the patterns agents repeat around #{t.tool_name}.",
        prompt:
          "We use the #{t.tool_name} tool very frequently (#{t.calls} calls). Identify the patterns we repeat around #{t.tool_name} and codify them into a reusable Skill, following this repo's conventions."
      }
    end
  end

  defp cost_concentration([top | _] = models) do
    total = Enum.reduce(models, 0.0, &(&2 + (&1.cost || 0)))

    if total > 0 and (top.cost || 0) / total >= 0.4 do
      pct = round((top.cost || 0) / total * 100)

      [
        %{
          score: 5.0,
          tone: :warning,
          icon: "hero-currency-dollar",
          title: "#{pct}% of spend is on #{top.key}",
          detail: "#{fmt_usd(top.cost)} of #{fmt_usd(total)} total.",
          suggestion:
            "Consider routing routine sub-tasks to a cheaper model (sub-agents, simpler edits) to cut cost.",
          prompt:
            "About #{pct}% of our agent spend is on #{top.key}. Propose and set up a model-routing strategy that uses cheaper models or subagents for routine work, and document it as guidance for this repo."
        }
      ]
    else
      []
    end
  end

  defp cost_concentration(_), do: []

  @doc """
  Curated, evergreen enhancements for coding-agent setups, grouped into a few
  categories. Each entry links to the specific authoritative doc (not a generic
  homepage) and carries a setup `prompt` so the UI can open an agent ready to
  wire it up. Exactly one entry is the `spotlight`, rendered larger.
  """
  def catalog do
    [
      %{
        key: "agents-md",
        category: "Foundations",
        spotlight: true,
        icon: "hero-document-text",
        title: "AGENTS.md at the repo root",
        detail:
          "One machine-readable contract of build and test commands, conventions, and boundaries that every agent reads first. Start here.",
        url: "https://agents.md",
        link_label: "agents.md standard",
        prompt:
          "Create a high-quality AGENTS.md at this repo root following the agents.md standard. Include the exact build, test, and lint commands, the conventions, and the boundaries. Keep it concise and command-first."
      },
      %{
        key: "claude-md",
        category: "Foundations",
        icon: "hero-book-open",
        title: "Project memory (CLAUDE.md)",
        detail:
          "Persistent facts and conventions every session loads automatically so you stop re-explaining the same context.",
        url: "https://code.claude.com/docs/en/memory",
        link_label: "Memory guide",
        prompt:
          "Create a concise CLAUDE.md capturing this repo's durable facts and conventions (architecture, commands, gotchas), following the Claude Code memory guide."
      },
      %{
        key: "skills",
        category: "Reusable workflows",
        icon: "hero-sparkles",
        title: "Skills",
        detail:
          "Capture a repeated procedure once as a SKILL.md. The agent loads it only when relevant and applies it the same way every time.",
        url: "https://code.claude.com/docs/en/skills",
        link_label: "Skills guide",
        prompt:
          "Scaffold a reusable Skill (SKILL.md) for a workflow I repeat often in this repo, following the Agent Skills standard and the Claude Code Skills guide."
      },
      %{
        key: "slash-commands",
        category: "Reusable workflows",
        icon: "hero-command-line",
        title: "Custom slash commands",
        detail:
          "Turn your most frequent multi-step requests into one-word commands your whole team can run.",
        url: "https://code.claude.com/docs/en/commands",
        link_label: "Commands reference",
        prompt:
          "Create custom slash commands for the requests I make most often in this repo, following the Claude Code commands reference."
      },
      %{
        key: "subagents",
        category: "Reusable workflows",
        icon: "hero-squares-2x2",
        title: "Subagent-driven development",
        detail:
          "Fan out independent tasks to fresh subagents with isolated context, then review. Higher quality and faster iteration.",
        url: "https://code.claude.com/docs/en/sub-agents",
        link_label: "Subagents guide",
        prompt:
          "Set up a subagent-driven development workflow for this repo (a fresh subagent per task with a two-stage spec and quality review), following the Claude Code subagents guide."
      },
      %{
        key: "mcp",
        category: "Connect and automate",
        icon: "hero-cpu-chip",
        title: "MCP servers for your tools",
        detail:
          "Typed, auditable access to GitHub, Linear, databases and more through the Model Context Protocol. No more pasting data into chat.",
        url: "https://code.claude.com/docs/en/mcp",
        link_label: "MCP setup guide",
        prompt:
          "Recommend MCP servers for the tools and services this project uses, then help me configure them following the Claude Code MCP guide."
      },
      %{
        key: "hooks",
        category: "Connect and automate",
        icon: "hero-bolt",
        title: "Hooks that keep the tree green",
        detail:
          "Run format, lint, and tests automatically on agent events such as before each commit, so the working tree never drifts.",
        url: "https://code.claude.com/docs/en/hooks-guide",
        link_label: "Hooks guide",
        prompt:
          "Set up Claude Code hooks for this repo to run format, lint, and tests automatically (for example on file edits and pre-commit), following the hooks guide."
      }
    ]
  end

  @doc "Catalog grouped into ordered category sections: [{category, [entries]}]."
  def catalog_groups do
    catalog()
    |> Enum.chunk_by(& &1.category)
    |> Enum.map(fn [%{category: c} | _] = items -> {c, items} end)
  end

  defp server_suffix(nil), do: ""
  defp server_suffix(""), do: ""
  defp server_suffix(s), do: " on #{s}"

  defp fmt_usd(n), do: "$" <> :erlang.float_to_binary((n || 0) * 1.0, decimals: 2)
end
