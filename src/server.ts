import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import type { Config } from "./config.ts";
import { openDb } from "./db.ts";
import type { Operation } from "./enrich.ts";
import { sync as ingestSync } from "./ingest.ts";
import { canLaunch, launchAgent, command as launchCommand, openIde } from "./launcher.ts";
import { getSession, listSessions, search } from "./query.ts";
import {
  list as listRecommendations,
  markImplemented,
  parseStatusFilter,
  regenerate as regenerateRecommendations,
} from "./recommendations.ts";
import {
  agentOptions,
  getSettings,
  ideOptions,
  saveSettings,
  settingsPath,
  terminalOptions,
} from "./settings.ts";
import {
  activity as activityStats,
  byDimension,
  dateBounds,
  fileHotspots,
  mcpUsage,
  modelSparklines,
  parseDimension,
  parseFileGroup,
  todayTotals,
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
type ServerEvent = { type: string };

const syncStatus = {
  last_sync_at: null as string | null,
  in_progress: false,
  last_report: null as string | null,
  last_error: null as string | null,
  ingested_count: null as number | null,
};
const eventClients = new Set<(event: ServerEvent) => void>();

export function publishServerEvent<T extends ServerEvent>(event: T): void {
  for (const send of eventClients) {
    send(event);
  }
}

export async function handleRequest(request: Request, config: Config): Promise<Response> {
  const url = new URL(request.url);
  try {
    if (request.method === "GET" && url.pathname === "/") {
      return html(indexHtml());
    }
    if (request.method === "GET" && url.pathname === "/api/health") {
      return json({ ok: true });
    }
    if (request.method === "GET" && url.pathname === "/api/events") {
      return eventStream();
    }
    if (request.method === "GET" && url.pathname === "/api/config") {
      return json({
        dbPath: config.dbPath,
        claudeDir: config.claudeDir,
        codexDir: config.codexDir,
      });
    }
    if (request.method === "GET" && url.pathname === "/api/settings") {
      return json(settingsResponse());
    }
    if (request.method === "POST" && url.pathname === "/api/settings") {
      const body = await readJson<Record<string, unknown>>(request);
      return json({ ...settingsResponse(saveSettings(body)), saved: true });
    }
    if (request.method === "POST" && url.pathname === "/api/launch/agent") {
      const body = await readJson<{ agent?: string; prompt?: string; key?: string }>(request);
      if (body.agent == null || body.prompt == null || body.prompt.trim() === "") {
        return json({ ok: false, error: "agent and prompt are required" }, 400);
      }
      const result = launchAgent(body.agent, body.prompt, body.key ?? null, getSettings());
      return json(
        result.ok
          ? result
          : { ...result, command: result.command ?? launchCommand(body.agent, body.prompt) },
        result.ok ? 200 : 400,
      );
    }
    if (request.method === "POST" && url.pathname === "/api/launch/ide") {
      const body = await readJson<{ dir?: string }>(request);
      if (body.dir == null || body.dir.trim() === "") {
        return json({ ok: false, error: "dir is required" }, 400);
      }
      const result = openIde(body.dir, getSettings());
      return json(result, result.ok ? 200 : 400);
    }
    if (
      request.method === "GET" &&
      (url.pathname === "/api/sync-status" || url.pathname === "/api/metadata/sync-status")
    ) {
      return json({
        ...syncStatus,
        timestamp: new Date().toISOString(),
      });
    }
    if (request.method === "POST" && url.pathname === "/api/sync") {
      return syncNow(config);
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
    if (request.method === "GET" && url.pathname === "/api/analytics/activity") {
      return withDb(config, (db) => json(activityStats(db)));
    }
    if (request.method === "GET" && url.pathname === "/api/analytics/model-sparklines") {
      return withDb(config, (db) => json(modelSparklines(db)));
    }
    if (request.method === "GET" && url.pathname === "/api/analytics/now") {
      return withDb(config, (db) =>
        json({
          today: todayTotals(db),
          active_sessions: [],
          last_sync_at: syncStatus.last_sync_at,
          sync_in_progress: syncStatus.in_progress,
        }),
      );
    }
    if (
      request.method === "GET" &&
      (url.pathname === "/api/date-bounds" || url.pathname === "/api/metadata/date-bounds")
    ) {
      return withDb(config, (db) => json(dateBounds(db)));
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

function settingsResponse(settings = getSettings()): Record<string, unknown> {
  return {
    settings,
    path: settingsPath(),
    can_launch: canLaunch(),
    options: {
      agents: agentOptions,
      terminals: terminalOptions,
      ides: ideOptions,
    },
  };
}

function syncNow(config: Config): Response {
  syncStatus.in_progress = true;
  syncStatus.last_error = null;
  try {
    return withDb(config, (db) => {
      const report = ingestSync(db, config);
      syncStatus.in_progress = false;
      syncStatus.last_sync_at = new Date().toISOString();
      syncStatus.last_report =
        `scanned ${report.scanned}, ingested ${report.ingested}, skipped ${report.skipped}, ` +
        `issues ${report.issues}, failed ${report.failed}`;
      syncStatus.ingested_count = report.ingested;
      publishServerEvent({ type: "sync", reason: "manual", report, status: { ...syncStatus } });
      if (report.ingested > 0) {
        publishServerEvent({
          type: "archive_updated",
          ingested: report.ingested,
          last_sync_at: syncStatus.last_sync_at,
        });
      }
      return json(report);
    });
  } catch (error) {
    syncStatus.in_progress = false;
    syncStatus.last_sync_at = new Date().toISOString();
    syncStatus.last_error = error instanceof Error ? error.message : String(error);
    throw error;
  }
}

function eventStream(): Response {
  const encoder = new TextEncoder();
  let client: ((event: ServerEvent) => void) | null = null;
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const send = (event: ServerEvent): void => {
        controller.enqueue(encoder.encode(formatSse(event)));
      };
      client = send;
      eventClients.add(send);
      send({ type: "hello", timestamp: new Date().toISOString() } as ServerEvent);
    },
    cancel() {
      if (client != null) {
        eventClients.delete(client);
      }
    },
  });
  return new Response(stream, {
    headers: {
      "content-type": "text/event-stream; charset=utf-8",
      "cache-control": "no-cache",
      connection: "keep-alive",
    },
  });
}

function formatSse(event: ServerEvent): string {
  return `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`;
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
