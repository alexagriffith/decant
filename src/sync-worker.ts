import type { Config } from "./config.ts";
import { openDb } from "./db.ts";
import { sync } from "./ingest.ts";

self.addEventListener("message", (event) => {
  const config = event.data as Config;
  const db = openDb(config.dbPath);
  try {
    const report = sync(db, config);
    self.postMessage({ ok: true, report });
  } catch (error) {
    self.postMessage({ ok: false, error: error instanceof Error ? error.message : String(error) });
  } finally {
    db.close();
  }
});
