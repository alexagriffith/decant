defmodule Decant.Insights do
  @moduledoc """
  Derives actionable "what could help my coding agents" signals from the archive
  (error hotspots, heavy tools/servers, cost concentration) and pairs them with a
  small curated catalog of agent enhancements (skills, AGENTS.md, MCP). Read-only.
  """
  alias Decant.Archive

  @doc "Data-derived signals, highest-impact first. Each is a map with title/detail/suggestion/tone/icon and an optional drill (path)."
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
      rate = t.errors / t.calls

      %{
        score: rate * t.calls,
        tone: :danger,
        icon: "hero-exclamation-triangle",
        title: "#{t.tool_name} fails #{round(rate * 100)}% of the time",
        detail: "#{t.errors} errors across #{t.calls} calls#{server_suffix(t.server)}.",
        suggestion:
          "Codify the recovery path as a Skill (or fix the call sites) so agents stop repeating this failure."
      }
    end
  end

  defp heavy_servers(mcp) do
    for s <- Enum.take(mcp, 3), s.calls >= 50 do
      %{
        score: s.calls / 2,
        tone: :accent,
        icon: "hero-cpu-chip",
        title: "Heavy reliance on the #{s.server} MCP server",
        detail: "#{s.calls} calls across #{s.tools} tools.",
        suggestion:
          "Package the common #{s.server} workflows into a reusable Skill so agents use them consistently."
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
        title: "#{t.tool_name} is one of your busiest tools",
        detail: "#{t.calls} calls.",
        suggestion:
          "High-frequency tools are good Skill candidates — capture the patterns agents repeat around #{t.tool_name}."
      }
    end
  end

  defp cost_concentration([top | _] = models) do
    total = Enum.reduce(models, 0.0, &(&2 + (&1.cost || 0)))

    if total > 0 and (top.cost || 0) / total >= 0.4 do
      [
        %{
          score: 5.0,
          tone: :warning,
          icon: "hero-currency-dollar",
          title: "#{round((top.cost || 0) / total * 100)}% of spend is on #{top.key}",
          detail: "#{fmt_usd(top.cost)} of #{fmt_usd(total)} total.",
          suggestion:
            "Consider routing routine sub-tasks to a cheaper model (sub-agents, simpler edits) to cut cost."
        }
      ]
    else
      []
    end
  end

  defp cost_concentration(_), do: []

  @doc "Curated, evergreen enhancements for coding-agent setups."
  def catalog do
    [
      %{
        icon: "hero-document-text",
        title: "AGENTS.md at the repo root",
        detail:
          "A single, machine-readable contract of commands, conventions, and boundaries for any agent.",
        url: "https://agents.md"
      },
      %{
        icon: "hero-sparkles",
        title: "Skills (reusable agent workflows)",
        detail:
          "Package repeated workflows (test-fix, format, review) as Skills so agents apply them the same way every time.",
        url: "https://agents.md"
      },
      %{
        icon: "hero-cpu-chip",
        title: "MCP servers for your tools",
        detail:
          "Give agents typed, auditable access to your services (GitHub, Linear, DBs) via the Model Context Protocol.",
        url: "https://modelcontextprotocol.io"
      },
      %{
        icon: "hero-beaker",
        title: "Subagent-driven development",
        detail:
          "Fan out independent tasks to fresh subagents with two-stage review for higher quality and speed.",
        url: "https://agents.md"
      }
    ]
  end

  defp server_suffix(nil), do: ""
  defp server_suffix(""), do: ""
  defp server_suffix(s), do: " on #{s}"

  defp fmt_usd(n), do: "$" <> :erlang.float_to_binary((n || 0) * 1.0, decimals: 2)
end
