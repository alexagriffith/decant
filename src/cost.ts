import type { TokenUsage } from "./model.ts";

export interface Price {
  inputPerMtok: number;
  outputPerMtok: number;
  cacheReadPerMtok: number;
  cacheWritePerMtok: number;
}

export function defaultPricing(): Map<string, Price> {
  return new Map<string, Price>([
    [
      "claude-fable",
      {
        inputPerMtok: 10.0,
        outputPerMtok: 50.0,
        cacheReadPerMtok: 1.0,
        cacheWritePerMtok: 12.5,
      },
    ],
    [
      "claude-opus",
      {
        inputPerMtok: 5.0,
        outputPerMtok: 25.0,
        cacheReadPerMtok: 0.5,
        cacheWritePerMtok: 6.25,
      },
    ],
    [
      "claude-sonnet",
      {
        inputPerMtok: 3.0,
        outputPerMtok: 15.0,
        cacheReadPerMtok: 0.3,
        cacheWritePerMtok: 3.75,
      },
    ],
    [
      "claude-haiku",
      {
        inputPerMtok: 1.0,
        outputPerMtok: 5.0,
        cacheReadPerMtok: 0.1,
        cacheWritePerMtok: 1.25,
      },
    ],
    [
      "gpt-5",
      {
        inputPerMtok: 1.25,
        outputPerMtok: 10.0,
        cacheReadPerMtok: 0.125,
        cacheWritePerMtok: 1.25,
      },
    ],
    [
      "gpt-5.4",
      {
        inputPerMtok: 2.5,
        outputPerMtok: 15.0,
        cacheReadPerMtok: 0.25,
        cacheWritePerMtok: 2.5,
      },
    ],
    [
      "gpt-5.4-mini",
      {
        inputPerMtok: 0.75,
        outputPerMtok: 4.5,
        cacheReadPerMtok: 0.075,
        cacheWritePerMtok: 0.75,
      },
    ],
    [
      "gpt-5.4-nano",
      {
        inputPerMtok: 0.2,
        outputPerMtok: 1.25,
        cacheReadPerMtok: 0.02,
        cacheWritePerMtok: 0.2,
      },
    ],
    [
      "gpt-5.5",
      {
        inputPerMtok: 5.0,
        outputPerMtok: 30.0,
        cacheReadPerMtok: 0.5,
        cacheWritePerMtok: 5.0,
      },
    ],
    [
      "gpt-5.3-codex",
      {
        inputPerMtok: 1.75,
        outputPerMtok: 14.0,
        cacheReadPerMtok: 0.175,
        cacheWritePerMtok: 1.75,
      },
    ],
  ]);
}

function canonicalModel(raw: string): string | null {
  const model = raw.toLowerCase();

  if (
    model.includes("claude") ||
    model === "opus" ||
    model === "sonnet" ||
    model === "haiku" ||
    model === "fable"
  ) {
    if (model.includes("fable") || model.includes("mythos")) {
      return "claude-fable";
    }
    if (model.includes("opus")) {
      return "claude-opus";
    }
    if (model.includes("sonnet")) {
      return "claude-sonnet";
    }
    if (model.includes("haiku")) {
      return "claude-haiku";
    }
    return null;
  }

  if (model.startsWith("codex-auto-review")) {
    return "gpt-5.3-codex";
  }
  if (model.startsWith("gpt-5.4-nano")) {
    return "gpt-5.4-nano";
  }
  if (model.startsWith("gpt-5.4-mini")) {
    return "gpt-5.4-mini";
  }
  if (model.startsWith("gpt-5.4")) {
    return "gpt-5.4";
  }
  if (model.startsWith("gpt-5.5")) {
    return "gpt-5.5";
  }
  if (model.startsWith("gpt-5.3-codex") || model.startsWith("gpt-5.2")) {
    return "gpt-5.3-codex";
  }
  if (model.startsWith("gpt-5")) {
    return "gpt-5";
  }

  return null;
}

export function isPriceable(model: string): boolean {
  return canonicalModel(model) !== null;
}

export function estimateCost(
  model: string | null | undefined,
  usage: TokenUsage,
  pricing: ReadonlyMap<string, Price>,
): number {
  if (model == null) {
    return 0.0;
  }
  const key = canonicalModel(model);
  if (key == null) {
    return 0.0;
  }
  const price = pricing.get(key);
  if (price == null) {
    return 0.0;
  }

  const per = (tokens: number, rate: number): number => (tokens * rate) / 1_000_000.0;
  return (
    per(usage.input, price.inputPerMtok) +
    per(usage.output, price.outputPerMtok) +
    per(usage.cacheRead, price.cacheReadPerMtok) +
    per(usage.cacheCreation, price.cacheWritePerMtok)
  );
}
