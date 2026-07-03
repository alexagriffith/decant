import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { Config } from "./config.ts";
import { openDb } from "./db.ts";
import type { Operation } from "./enrich.ts";
import { sync as ingestSync } from "./ingest.ts";
import { getSession, listSessions, search } from "./query.ts";
import {
  list as listRecommendations,
  markImplemented,
  parseStatusFilter,
  regenerate as regenerateRecommendations,
} from "./recommendations.ts";
import {
  byDimension,
  fileHotspots,
  mcpUsage,
  parseDimension,
  parseFileGroup,
  toolUsage,
  totals,
} from "./stats.ts";
import uiBundle from "./ui/index.html";

export interface ServeOptions {
  config: Config;
  port?: number;
  hostname?: string;
}

type Db = ReturnType<typeof openDb>;

export async function handleRequest(request: Request, config: Config): Promise<Response> {
  const url = new URL(request.url);
  try {
    if (request.method === "GET" && url.pathname === "/") {
      return html(indexHtml());
    }
    if (request.method === "GET" && url.pathname === "/api/health") {
      return json({ ok: true });
    }
    if (request.method === "GET" && url.pathname === "/api/config") {
      return json({
        dbPath: config.dbPath,
        claudeDir: config.claudeDir,
        codexDir: config.codexDir,
      });
    }
    if (request.method === "POST" && url.pathname === "/api/sync") {
      return withDb(config, (db) => json(ingestSync(db, config)));
    }
    if (request.method === "GET" && url.pathname === "/api/sessions") {
      return withDb(config, (db) =>
        json(
          listSessions(db, {
            tool: url.searchParams.get("tool"),
            limit: integerParam(url, "limit", 50),
          }),
        ),
      );
    }
    const sessionMatch = url.pathname.match(/^\/api\/sessions\/(\d+)$/);
    if (request.method === "GET" && sessionMatch != null) {
      return withDb(config, (db) => {
        const detail = getSession(db, Number(sessionMatch[1]));
        return detail == null ? json({ error: "session not found" }, 404) : json(detail);
      });
    }
    if (request.method === "POST" && url.pathname === "/api/search") {
      const body = await readJson<{ query?: string; limit?: number }>(request);
      if (body.query == null || body.query.trim() === "") {
        return json({ error: "query is required" }, 400);
      }
      return withDb(config, (db) => json(search(db, body.query as string, body.limit ?? 30)));
    }
    if (request.method === "GET" && url.pathname === "/api/stats/summary") {
      return withDb(config, (db) => json(totals(db)));
    }
    if (request.method === "GET" && url.pathname === "/api/stats/by-dimension") {
      const dimension = parseDimension(url.searchParams.get("dim") ?? "");
      if (dimension == null) {
        return json({ error: "unknown dimension" }, 400);
      }
      return withDb(config, (db) => json(byDimension(db, dimension)));
    }
    if (request.method === "GET" && url.pathname === "/api/files") {
      const group = parseFileGroup(url.searchParams.get("group") ?? "path");
      const op = parseOperation(url.searchParams.get("op"));
      if (group == null || op === false) {
        return json({ error: "invalid files query" }, 400);
      }
      return withDb(config, (db) =>
        json(fileHotspots(db, group, op, integerParam(url, "limit", 25))),
      );
    }
    if (request.method === "GET" && url.pathname === "/api/tools/usage") {
      return withDb(config, (db) =>
        json(
          toolUsage(
            db,
            url.searchParams.get("errors_only") === "true",
            integerParam(url, "limit", 50),
          ),
        ),
      );
    }
    if (request.method === "GET" && url.pathname === "/api/tools/mcp-usage") {
      return withDb(config, (db) => json(mcpUsage(db, integerParam(url, "limit", 50))));
    }
    if (request.method === "GET" && url.pathname === "/api/recommendations") {
      const status = parseStatusFilter(url.searchParams.get("status") ?? "open");
      if (status == null) {
        return json({ error: "unknown status" }, 400);
      }
      return withDb(config, (db) => {
        regenerateRecommendations(db);
        return json(listRecommendations(db, status));
      });
    }
    if (request.method === "POST" && url.pathname === "/api/recommendations/mark") {
      const body = await readJson<{ key?: string; source?: string; note?: string }>(request);
      if (body.key == null || body.key.trim() === "") {
        return json({ error: "key is required" }, 400);
      }
      return withDb(config, (db) => {
        const ok = markImplemented(db, body.key as string, body.source ?? "agent", body.note);
        return ok
          ? json({ ok: true, key: body.key, status: "implemented" })
          : json({ ok: false, key: body.key, error: "recommendation not found" }, 404);
      });
    }
    if (request.method === "GET" && isUiPath(url.pathname)) {
      return html(indexHtml());
    }
    return json({ error: "not found" }, 404);
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : String(error) }, 500);
  }
}

export function serve(options: ServeOptions): ReturnType<typeof Bun.serve> {
  const hostname = options.hostname ?? "127.0.0.1";
  const port = options.port ?? 4577;
  return Bun.serve({
    hostname,
    port,
    routes: {
      "/": uiBundle,
      "/sessions/:id": uiBundle,
      "/search": uiBundle,
      "/analytics": uiBundle,
      "/insights": uiBundle,
      "/tools": uiBundle,
      "/files": uiBundle,
      "/settings": uiBundle,
    },
    fetch: (request) => handleRequest(request, options.config),
  });
}

function withDb(config: Config, callback: (db: Db) => Response): Response {
  mkdirSync(dirname(config.dbPath), { recursive: true });
  const db = openDb(config.dbPath);
  try {
    return callback(db);
  } finally {
    db.close();
  }
}

function json(value: unknown, status = 200): Response {
  return new Response(JSON.stringify(value, null, 2), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function html(value: string): Response {
  return new Response(value, {
    headers: { "content-type": "text/html; charset=utf-8" },
  });
}

async function readJson<T>(request: Request): Promise<T> {
  try {
    return (await request.json()) as T;
  } catch {
    return {} as T;
  }
}

function integerParam(url: URL, name: string, fallback: number): number {
  const raw = url.searchParams.get(name);
  if (raw == null) {
    return fallback;
  }
  const parsed = Number.parseInt(raw, 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function parseOperation(value: string | null): Operation | null | false {
  if (value == null || value === "") {
    return null;
  }
  return value === "read" || value === "edit" || value === "write" || value === "delete"
    ? value
    : false;
}

function isUiPath(pathname: string): boolean {
  return (
    pathname === "/" ||
    pathname === "/search" ||
    pathname === "/analytics" ||
    pathname === "/insights" ||
    pathname === "/tools" ||
    pathname === "/files" ||
    pathname === "/settings" ||
    /^\/sessions\/\d+$/.test(pathname)
  );
}

function indexHtml(): string {
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>decant</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/ui/main.tsx"></script>
  </body>
</html>
`;
}
