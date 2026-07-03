import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

type Summary = {
  sessions: number;
  messages: number;
  tool_calls: number;
  input_tokens: number;
  output_tokens: number;
  estimated_cost_usd: number;
};

type SessionSummary = {
  id: number;
  tool: string;
  source_session_id: string;
  title: string | null;
  project_path: string | null;
  model: string | null;
  started_at: string | null;
  message_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  estimated_cost_usd: number;
};

type SearchHit = {
  session_id: number;
  session_title: string | null;
  tool: string;
  snippet: string;
};

type DimensionRow = {
  key: string;
  sessions: number;
  estimated_cost_usd: number;
};

type ToolRow = {
  tool_name: string;
  tool_kind: string;
  mcp_server: string | null;
  calls: number;
  errors: number;
};

type McpRow = {
  mcp_server: string;
  tools: number;
  calls: number;
  errors: number;
};

type FileRow = {
  key: string;
  project: string | null;
  reads: number;
  edits: number;
  writes: number;
  deletes: number;
  sessions: number;
};

type Recommendation = {
  key: string;
  status: string;
  category: string;
  title: string;
  detail: string;
  suggestion: string;
  tone: string;
  action: string | null;
};

type ConfigView = {
  dbPath: string;
  claudeDir: string;
  codexDir: string;
};

type DashboardData = {
  summary: Summary | null;
  sessions: SessionSummary[];
  byTool: DimensionRow[];
  tools: ToolRow[];
  mcp: McpRow[];
  files: FileRow[];
  recommendations: Recommendation[];
  config: ConfigView | null;
};

const emptyData: DashboardData = {
  summary: null,
  sessions: [],
  byTool: [],
  tools: [],
  mcp: [],
  files: [],
  recommendations: [],
  config: null,
};

const navItems = [
  ["/", "Sessions"],
  ["/search", "Search"],
  ["/analytics", "Analytics"],
  ["/insights", "Insights"],
  ["/tools", "Tools"],
  ["/files", "Files"],
  ["/settings", "Settings"],
] as const;

function App() {
  const [path, setPath] = useState(() => window.location.pathname);
  const [data, setData] = useState<DashboardData>(emptyData);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);

  useEffect(() => {
    const onPop = () => setPath(window.location.pathname);
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(reloadKey === 0);
    Promise.all([
      getJson<Summary>("/api/stats/summary"),
      getJson<SessionSummary[]>("/api/sessions?limit=12"),
      getJson<DimensionRow[]>("/api/stats/by-dimension?dim=tool"),
      getJson<ToolRow[]>("/api/tools/usage?limit=10"),
      getJson<McpRow[]>("/api/tools/mcp-usage?limit=10"),
      getJson<FileRow[]>("/api/files?group=path&limit=10"),
      getJson<Recommendation[]>("/api/recommendations?status=open"),
      getJson<ConfigView>("/api/config"),
    ])
      .then(([summary, sessions, byTool, tools, mcp, files, recommendations, config]) => {
        if (cancelled) {
          return;
        }
        setData({ summary, sessions, byTool, tools, mcp, files, recommendations, config });
        setError(null);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [reloadKey]);

  useEffect(() => {
    const events = new EventSource("/api/events");
    const refresh = () => setReloadKey((key) => key + 1);
    events.addEventListener("sync", refresh);
    events.addEventListener("archive_updated", refresh);
    return () => {
      events.removeEventListener("sync", refresh);
      events.removeEventListener("archive_updated", refresh);
      events.close();
    };
  }, []);

  const active = activeRoute(path);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand">
          <span className="brand-mark" />
          <span>decant</span>
        </div>
        <nav aria-label="Primary">
          {navItems.map(([href, label]) => (
            <a
              aria-current={active === activeRoute(href) ? "page" : undefined}
              href={href}
              key={href}
              onClick={(event) => {
                event.preventDefault();
                window.history.pushState(null, "", href);
                setPath(href);
              }}
            >
              {label}
            </a>
          ))}
        </nav>
      </aside>
      <main className="workspace">
        <header className="topbar">
          <div>
            <p>{active}</p>
            <h1>{titleFor(active)}</h1>
          </div>
          <button
            type="button"
            onClick={() => {
              void fetch("/api/sync", { method: "POST" }).then(() => window.location.reload());
            }}
          >
            Sync
          </button>
        </header>
        {error != null ? <div className="notice danger">{error}</div> : null}
        {loading ? (
          <div className="notice">Loading archive data...</div>
        ) : (
          renderView(active, path, data)
        )}
      </main>
    </div>
  );
}

function renderView(active: string, path: string, data: DashboardData) {
  if (path.startsWith("/sessions/")) {
    return <SessionDetailView id={Number(path.split("/").at(-1))} />;
  }
  switch (active) {
    case "Sessions":
      return <SessionsView data={data} />;
    case "Search":
      return <SearchView />;
    case "Analytics":
      return <AnalyticsView data={data} />;
    case "Insights":
      return <InsightsView rows={data.recommendations} />;
    case "Tools":
      return <ToolsView data={data} />;
    case "Files":
      return <FilesView rows={data.files} />;
    case "Settings":
      return <SettingsView config={data.config} />;
    default:
      return <SessionsView data={data} />;
  }
}

function SessionsView({ data }: { data: DashboardData }) {
  return (
    <>
      <MetricGrid summary={data.summary} />
      <section className="section">
        <div className="section-heading">
          <h2>Recent Sessions</h2>
          <span>{data.sessions.length} loaded</span>
        </div>
        <table>
          <thead>
            <tr>
              <th>Title</th>
              <th>Tool</th>
              <th>Model</th>
              <th>Project</th>
              <th className="numeric">Cost</th>
            </tr>
          </thead>
          <tbody>
            {data.sessions.map((session) => (
              <tr key={session.id}>
                <td>
                  <a href={`/sessions/${session.id}`}>
                    {session.title ?? session.source_session_id}
                  </a>
                </td>
                <td>{session.tool}</td>
                <td>{session.model ?? "-"}</td>
                <td>{basename(session.project_path)}</td>
                <td className="numeric">${session.estimated_cost_usd.toFixed(2)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </section>
    </>
  );
}

function MetricGrid({ summary }: { summary: Summary | null }) {
  const metrics = [
    ["Sessions", summary?.sessions ?? 0],
    ["Messages", summary?.messages ?? 0],
    ["Tool Calls", summary?.tool_calls ?? 0],
    ["Cost", `$${(summary?.estimated_cost_usd ?? 0).toFixed(2)}`],
  ];
  return (
    <div className="metric-grid">
      {metrics.map(([label, value]) => (
        <section className="metric" key={label}>
          <span>{label}</span>
          <strong>{value}</strong>
        </section>
      ))}
    </div>
  );
}

function SearchView() {
  const [query, setQuery] = useState("auth");
  const [hits, setHits] = useState<SearchHit[]>([]);

  const runSearch = () => {
    void getJson<SearchHit[]>("/api/search", {
      method: "POST",
      body: JSON.stringify({ query, limit: 20 }),
    }).then(setHits);
  };

  useEffect(() => {
    void getJson<SearchHit[]>("/api/search", {
      method: "POST",
      body: JSON.stringify({ query: "auth", limit: 20 }),
    }).then(setHits);
  }, []);

  return (
    <section className="section">
      <div className="search-row">
        <input value={query} onChange={(event) => setQuery(event.target.value)} />
        <button type="button" onClick={runSearch}>
          Search
        </button>
      </div>
      <div className="result-list">
        {hits.map((hit) => (
          <article className="result" key={`${hit.session_id}-${hit.snippet}`}>
            <a href={`/sessions/${hit.session_id}`}>
              {hit.session_title ?? `Session ${hit.session_id}`}
            </a>
            <p>{hit.snippet}</p>
          </article>
        ))}
      </div>
    </section>
  );
}

function AnalyticsView({ data }: { data: DashboardData }) {
  const maxSessions = Math.max(1, ...data.byTool.map((row) => row.sessions));
  return (
    <>
      <MetricGrid summary={data.summary} />
      <section className="section">
        <div className="section-heading">
          <h2>Sessions By Tool</h2>
          <span>{data.byTool.length} groups</span>
        </div>
        <div className="bar-list">
          {data.byTool.map((row) => (
            <div className="bar-row" key={row.key}>
              <span>{row.key}</span>
              <div>
                <i style={{ width: `${(row.sessions / maxSessions) * 100}%` }} />
              </div>
              <strong>{row.sessions}</strong>
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

function InsightsView({ rows }: { rows: Recommendation[] }) {
  return (
    <section className="insight-grid">
      {rows.map((row) => (
        <article className={`insight tone-${row.tone}`} key={row.key}>
          <span>{row.category}</span>
          <h2>{row.title}</h2>
          <p>{row.detail}</p>
          <p>{row.suggestion}</p>
          {row.action != null ? <strong>{row.action}</strong> : null}
        </article>
      ))}
    </section>
  );
}

function ToolsView({ data }: { data: DashboardData }) {
  return (
    <div className="split">
      <section className="section">
        <div className="section-heading">
          <h2>Tool Calls</h2>
          <span>{data.tools.length} tools</span>
        </div>
        <TableRows
          headers={["Tool", "Kind", "Server", "Calls", "Errors"]}
          rows={data.tools.map((row) => [
            row.tool_name,
            row.tool_kind,
            row.mcp_server ?? "-",
            row.calls,
            row.errors,
          ])}
        />
      </section>
      <section className="section">
        <div className="section-heading">
          <h2>MCP Servers</h2>
          <span>{data.mcp.length} servers</span>
        </div>
        <TableRows
          headers={["Server", "Tools", "Calls", "Errors"]}
          rows={data.mcp.map((row) => [row.mcp_server, row.tools, row.calls, row.errors])}
        />
      </section>
    </div>
  );
}

function FilesView({ rows }: { rows: FileRow[] }) {
  return (
    <section className="section">
      <div className="section-heading">
        <h2>Hot Files</h2>
        <span>{rows.length} paths</span>
      </div>
      <TableRows
        headers={["Path", "Project", "Reads", "Edits", "Writes", "Deletes", "Sessions"]}
        rows={rows.map((row) => [
          row.key,
          basename(row.project),
          row.reads,
          row.edits,
          row.writes,
          row.deletes,
          row.sessions,
        ])}
      />
    </section>
  );
}

function SettingsView({ config }: { config: ConfigView | null }) {
  return (
    <section className="section settings">
      <h2>Local Paths</h2>
      <dl>
        <dt>Archive</dt>
        <dd>{config?.dbPath ?? "-"}</dd>
        <dt>Claude</dt>
        <dd>{config?.claudeDir ?? "-"}</dd>
        <dt>Codex</dt>
        <dd>{config?.codexDir ?? "-"}</dd>
      </dl>
    </section>
  );
}

function SessionDetailView({ id }: { id: number }) {
  const [detail, setDetail] = useState<{
    summary: SessionSummary;
    messages: {
      role: string;
      timestamp: string | null;
      blocks: { block_type: string; text: string | null }[];
    }[];
  } | null>(null);

  useEffect(() => {
    if (Number.isFinite(id)) {
      void getJson<typeof detail>(`/api/sessions/${id}`).then(setDetail);
    }
  }, [id]);

  if (detail == null) {
    return <div className="notice">Loading session...</div>;
  }

  return (
    <section className="section transcript">
      <div className="section-heading">
        <h2>{detail.summary.title ?? `Session ${id}`}</h2>
        <span>{detail.messages.length} messages</span>
      </div>
      {detail.messages.map((message) => (
        <article
          className="message"
          key={`${message.role}-${message.timestamp ?? "none"}-${message.blocks.length}`}
        >
          <strong>{message.role}</strong>
          {message.blocks.map((block) => (
            <p key={`${block.block_type}-${block.text ?? "empty"}`}>
              {block.text ?? block.block_type}
            </p>
          ))}
        </article>
      ))}
    </section>
  );
}

function TableRows({ headers, rows }: { headers: string[]; rows: (string | number)[][] }) {
  return (
    <table>
      <thead>
        <tr>
          {headers.map((header) => (
            <th key={header}>{header}</th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.map((cell) => String(cell)).join("|")}>
            {row.map((cell, cellIndex) => (
              <td key={`${headers[cellIndex] ?? "cell"}-${String(cell)}`}>{cell}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  );
}

async function getJson<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    headers: { "content-type": "application/json" },
    ...init,
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}`);
  }
  return (await response.json()) as T;
}

function activeRoute(path: string): string {
  if (path.startsWith("/sessions/")) {
    return "Sessions";
  }
  const match = navItems.find(([href]) => href === path);
  return match?.[1] ?? "Sessions";
}

function titleFor(active: string): string {
  return active === "Sessions" ? "Session Archive" : active;
}

function basename(path: string | null | undefined): string {
  if (path == null || path === "") {
    return "-";
  }
  return path.split("/").filter(Boolean).at(-1) ?? path;
}

const root = document.getElementById("root");
if (root == null) {
  throw new Error("missing #root");
}
createRoot(root).render(<App />);
