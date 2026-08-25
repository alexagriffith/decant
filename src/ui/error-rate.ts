/** How the Tools summary presents a tool-call error rate.
 *
 * The alert flag is derived from the RENDERED label rather than from the raw
 * rate, so the two cannot disagree. A rate of 0.0004% rounds to "0.0%", and a
 * card reading "0.0%" in the danger colour is a false alarm — the reader sees a
 * red zero and has no way to tell it apart from a real problem.
 */
export interface ErrorRateDisplay {
  /** What the card shows. "—" when nothing has been called yet. */
  label: string;
  /** Whether to render the card in the danger colour. */
  alert: boolean;
}

export function errorRateDisplay(totalCalls: number, errorRate: number): ErrorRateDisplay {
  const label = totalCalls === 0 ? "—" : `${errorRate.toFixed(1)}%`;
  return { label, alert: label !== "—" && label !== "0.0%" };
}
