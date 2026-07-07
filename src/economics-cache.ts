import type { Database } from "bun:sqlite";
import type { DateFilter } from "./date-filter.ts";
import {
  aggregateEconomicsVectors,
  economicsVectorMatchesFilter,
  type SessionEconomicsVector,
  type TokenEconomics,
} from "./token-economics.ts";

/**
 * Serves /api/analytics/token-economics from precomputed per-session vectors.
 *
 * Recomputing token economics from the archive walks every block row, which
 * takes seconds on multi-GB archives — far beyond any per-request budget. The
 * cache computes per-session vectors once, off the request thread, and then
 * answers any date filter by summing vectors in memory. `PRAGMA data_version`
 * cheaply detects archive writes from other connections (sync workers, CLI
 * runs); a stale cache serves the previous model while a rebuild runs in the
 * background, and onRebuilt lets the server nudge clients to refetch.
 */
export interface EconomicsCacheOptions {
  dbPath: string;
  db: Database;
  computeVectors?: (dbPath: string) => Promise<SessionEconomicsVector[]>;
  onRebuilt?: () => void;
}

export class EconomicsCache {
  #vectors: SessionEconomicsVector[] | null = null;
  #builtDataVersion: number | null = null;
  #building: Promise<void> | null = null;
  #disposed = false;
  readonly #options: EconomicsCacheOptions;

  constructor(options: EconomicsCacheOptions) {
    this.#options = options;
  }

  async get(filter?: DateFilter | null): Promise<TokenEconomics> {
    if (this.#vectors == null) {
      await (this.#building ?? this.#rebuild());
    } else if (this.#dataVersion() !== this.#builtDataVersion) {
      // Stale-while-revalidate: answer from the previous model immediately and
      // refresh in the background; onRebuilt tells clients to refetch.
      void this.#rebuild().catch(() => {});
    }
    const vectors = this.#vectors ?? [];
    return aggregateEconomicsVectors(
      vectors.filter((vector) => economicsVectorMatchesFilter(vector, filter)),
    );
  }

  /** Kick a background rebuild, e.g. right after a sync ingests new sessions. */
  invalidate(): void {
    void this.#rebuild().catch(() => {});
  }

  prewarm(): void {
    void this.#rebuild().catch(() => {});
  }

  /** Resolves once any in-flight rebuild has finished (success or failure). */
  async settled(): Promise<void> {
    while (this.#building != null) {
      await this.#building.catch(() => {});
    }
  }

  dispose(): void {
    this.#disposed = true;
  }

  #dataVersion(): number | null {
    try {
      // data_version only reflects other connections' commits after this
      // connection takes a fresh read snapshot, so touch a tiny table first.
      this.#options.db.query("SELECT 1 FROM schema_migrations LIMIT 1").get();
      const row = this.#options.db.query("PRAGMA data_version").get() as {
        data_version: number;
      } | null;
      return row?.data_version ?? null;
    } catch {
      return null;
    }
  }

  #rebuild(): Promise<void> {
    if (this.#building != null) {
      return this.#building;
    }
    const compute = this.#options.computeVectors ?? computeVectorsInWorker;
    const version = this.#dataVersion();
    this.#building = compute(this.#options.dbPath)
      .then((vectors) => {
        if (this.#disposed) {
          return;
        }
        const replacedModel = this.#vectors != null;
        this.#vectors = vectors;
        this.#builtDataVersion = version;
        if (replacedModel) {
          this.#options.onRebuilt?.();
        }
      })
      .finally(() => {
        this.#building = null;
      });
    return this.#building;
  }
}

function computeVectorsInWorker(dbPath: string): Promise<SessionEconomicsVector[]> {
  return new Promise((resolve, reject) => {
    const worker = new Worker(new URL("./stats-worker.ts", import.meta.url), { type: "module" });
    worker.addEventListener("message", (event) => {
      const data = event.data as
        | { ok: true; vectors: SessionEconomicsVector[] }
        | { ok: false; error: string };
      worker.terminate();
      if (data.ok) {
        resolve(data.vectors);
      } else {
        reject(new Error(data.error));
      }
    });
    worker.addEventListener("error", (event) => {
      worker.terminate();
      reject(event.error instanceof Error ? event.error : new Error(String(event.error)));
    });
    worker.postMessage({ dbPath });
  });
}
