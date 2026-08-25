import type { TokenEconomics } from "./main.tsx";

export interface PhaseSplitRow {
  phase: "orientation" | "implementation";
  label: string;
  tone: "info" | "success";
  costShare: number;
  timeShare: number;
  estimated_cost_usd: number;
  active_ms: number;
  generation_tokens: number;
  context_window_tokens: number;
}

/** Flattens the phase split into table rows. Returning `[]` when the server
 * omits `phases` lets the panel degrade to today's rendering against an older
 * archive rather than throwing on a missing field. */
export function phaseSplitRows(economics: TokenEconomics | null): PhaseSplitRow[] {
  const phases = economics?.totals.phases;
  if (economics == null || phases == null) {
    return [];
  }
  const totalActiveMs = economics.totals.active_ms;
  return [
    // Orientation echoes the context bucket's tone and implementation the code
    // bucket's, because each phase is dominated by that activity.
    { phase: "orientation", label: "Orientation", tone: "info" } as const,
    { phase: "implementation", label: "Implementation", tone: "success" } as const,
  ].map((row) => {
    const amounts = phases[row.phase];
    return {
      ...row,
      costShare: amounts.cost_share,
      timeShare: totalActiveMs > 0 ? amounts.active_ms / totalActiveMs : 0,
      estimated_cost_usd: amounts.estimated_cost_usd,
      active_ms: amounts.active_ms,
      generation_tokens: amounts.generation_tokens,
      context_window_tokens: amounts.context_window_tokens,
    };
  });
}
