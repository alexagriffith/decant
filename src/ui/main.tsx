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

type Activity = {
  by_hour: number[];
  by_weekday: number[];
  timezone: string;
  peak_hour: number | null;
  peak_weekday: number | null;
};

type ModelSparklines = {
  models: Record<string, number[]>;
  days: string[];
};

type DateBounds = {
  min: string | null;
  max: string | null;
};

type NowView = {
  today: Summary;
  active_sessions: unknown[];
  last_sync_at: string | null;
  sync_in_progress: boolean;
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
  category: string | null;
  title: string;
  detail: string | null;
  suggestion: string | null;
  tone: string | null;
  action: string | null;
};

type ConfigView = {
  dbPath: string;
  claudeDir: string;
  codexDir: string;
};

type UserSettings = {
  agent: string;
  terminal: string;
  ide: string;
};

type SettingsInfo = {
  settings: UserSettings;
  path: string;
  can_launch: boolean;
  options: {
    agents: [string, string][];
    terminals: [string, string][];
    ides: [string, string][];
  };
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
  settings: SettingsInfo | null;
  activity: Activity | null;
  modelSparklines: ModelSparklines | null;
  now: NowView | null;
  dateBounds: DateBounds | null;
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
  settings: null,
  activity: null,
  modelSparklines: null,
  now: null,
  dateBounds: null,
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
const SESSION_PAGE_SIZE = 50;

function App() {
  const [path, setPath] = useState(() => window.location.pathname);
  const [data, setData] = useState<DashboardData>(emptyData);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [sessionPage, setSessionPage] = useState(0);

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
      getJson<SessionSummary[]>(
        `/api/sessions?limit=${SESSION_PAGE_SIZE}&offset=${sessionPage * SESSION_PAGE_SIZE}`,
      ),
      getJson<DimensionRow[]>("/api/stats/by-dimension?dim=tool"),
      getJson<ToolRow[]>("/api/tools/usage?limit=10"),
      getJson<McpRow[]>("/api/tools/mcp-usage?limit=10"),
      getJson<FileRow[]>("/api/files?group=path&limit=10"),
      getJson<Recommendation[]>("/api/recommendations?status=open"),
      getJson<ConfigView>("/api/config"),
      getJson<SettingsInfo>("/api/settings"),
      getJson<Activity>("/api/analytics/activity"),
      getJson<ModelSparklines>("/api/analytics/model-sparklines"),
      getJson<NowView>("/api/analytics/now"),
      getJson<DateBounds>("/api/date-bounds"),
    ])
      .then(
        ([
          summary,
          sessions,
          byTool,
          tools,
          mcp,
          files,
          recommendations,
          config,
          settings,
          activity,
          modelSparklines,
          now,
          dateBounds,
        ]) => {
          if (cancelled) {
            return;
          }
          setData({
            summary,
            sessions,
            byTool,
            tools,
            mcp,
            files,
            recommendations,
            config,
            settings,
            activity,
            modelSparklines,
            now,
            dateBounds,
          });
          setError(null);
        },
      )
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
  }, [reloadKey, sessionPage]);

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
              void fetch("/api/sync", { method: "POST" }).then(() =>
                setReloadKey((key) => key + 1),
              );
            }}
          >
            Sync
          </button>
        </header>
        {error != null ? <div className="notice danger">{error}</div> : null}
        {loading ? (
          <div className="notice">Loading archive data...</div>
        ) : (
          renderView(active, path, data, {
            refresh: () => setReloadKey((key) => key + 1),
            sessionPage,
            setSessionPage,
          })
        )}
      </main>
    </div>
  );
}

function renderView(
  active: string,
  path: string,
  data: DashboardData,
  actions: {
    refresh: () => void;
    sessionPage: number;
    setSessionPage: (page: number) => void;
  },
) {
  if (path.startsWith("/sessions/")) {
    return <SessionDetailView id={Number(path.split("/").at(-1))} />;
  }
  switch (active) {
    case "Sessions":
      return (
        <SessionsView
          data={data}
          page={actions.sessionPage}
          pageSize={SESSION_PAGE_SIZE}
          onPageChange={actions.setSessionPage}
        />
      );
    case "Search":
      return <SearchView />;
    case "Analytics":
      return <AnalyticsView data={data} />;
    case "Insights":
      return <InsightsView rows={data.recommendations} onMarked={actions.refresh} />;
    case "Tools":
      return <ToolsView data={data} />;
    case "Files":
      return <FilesView rows={data.files} />;
    case "Settings":
      return <SettingsView config={data.config} settingsInfo={data.settings} />;
    default:
      return (
        <SessionsView
          data={data}
          page={actions.sessionPage}
          pageSize={SESSION_PAGE_SIZE}
          onPageChange={actions.setSessionPage}
        />
      );
  }
}

function SessionsView({
  data,
  page,
  pageSize,
  onPageChange,
}: {
  data: DashboardData;
  page: number;
  pageSize: number;
  onPageChange: (page: number) => void;
}) {
  const total = data.summary?.sessions ?? data.sessions.length;
  const start = total === 0 ? 0 : page * pageSize + 1;
  const end = Math.min(total, page * pageSize + data.sessions.length);
  const hasPrevious = page > 0;
  const hasNext = end < total;
  return (
    <>
      <MetricGrid summary={data.summary} />
      <section className="section">
        <div className="section-heading">
          <h2>Recent Sessions</h2>
          <span>
            {start}-{end} of {total}
          </span>
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
            {data.sessions.length === 0 ? (
              <tr>
                <td colSpan={5}>No sessions ingested yet.</td>
              </tr>
            ) : null}
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
        <div className="pager">
          <button disabled={!hasPrevious} type="button" onClick={() => onPageChange(page - 1)}>
            Previous
          </button>
          <span>Page {page + 1}</span>
          <button disabled={!hasNext} type="button" onClick={() => onPageChange(page + 1)}>
            Next
          </button>
        </div>
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
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runSearch = () => {
    const trimmed = query.trim();
    if (trimmed === "") {
      setHits([]);
      setError(null);
      return;
    }
    setSearching(true);
    setError(null);
    void getJson<SearchHit[]>("/api/search", {
      method: "POST",
      body: JSON.stringify({ query: trimmed, limit: 20 }),
    })
      .then(setHits)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setSearching(false));
  };

  useEffect(() => {
    void getJson<SearchHit[]>("/api/search", {
      method: "POST",
      body: JSON.stringify({ query: "auth", limit: 20 }),
    })
      .then(setHits)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  return (
    <section className="section">
      <div className="search-row">
        <input value={query} onChange={(event) => setQuery(event.target.value)} />
        <button type="button" onClick={runSearch}>
          Search
        </button>
      </div>
      {error != null ? <div className="notice danger inline-notice">{error}</div> : null}
      <div className="result-list">
        {searching ? <p className="empty">Searching...</p> : null}
        {!searching && hits.length === 0 ? <p className="empty">No matching blocks.</p> : null}
        {hits.map((hit) => (
          <article className="result" key={`${hit.session_id}-${hit.snippet}`}>
            <a href={`/sessions/${hit.session_id}`}>
              {hit.session_title ?? `Session ${hit.session_id}`}
            </a>
            <p>
              <HighlightedSnippet snippet={hit.snippet} />
            </p>
          </article>
        ))}
      </div>
    </section>
  );
}

function HighlightedSnippet({ snippet }: { snippet: string }) {
  return (
    <>
      {snippetParts(snippet).map((part) =>
        part.match ? (
          <mark key={part.key}>{part.text}</mark>
        ) : (
          <span key={part.key}>{part.text}</span>
        ),
      )}
    </>
  );
}

function AnalyticsView({ data }: { data: DashboardData }) {
  const maxSessions = Math.max(1, ...data.byTool.map((row) => row.sessions));
  const maxCost = Math.max(1, ...data.byTool.map((row) => row.estimated_cost_usd));
  return (
    <>
      <MetricGrid summary={data.summary} />
      <div className="split">
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
        <section className="section">
          <div className="section-heading">
            <h2>Cost By Tool</h2>
            <span>{dateRange(data.dateBounds)}</span>
          </div>
          <div className="bar-list">
            {data.byTool.map((row) => (
              <div className="bar-row" key={row.key}>
                <span>{row.key}</span>
                <div>
                  <i style={{ width: `${(row.estimated_cost_usd / maxCost) * 100}%` }} />
                </div>
                <strong>${row.estimated_cost_usd.toFixed(2)}</strong>
              </div>
            ))}
          </div>
        </section>
      </div>
      <div className="split">
        <ActivityPanel activity={data.activity} />
        <NowPanel now={data.now} />
      </div>
      <ModelSparklinePanel sparklines={data.modelSparklines} />
    </>
  );
}

function ActivityPanel({ activity }: { activity: Activity | null }) {
  const weekdayLabels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  return (
    <section className="section">
      <div className="section-heading">
        <h2>Activity</h2>
        <span>{activity?.timezone ?? "-"}</span>
      </div>
      <div className="chart-stack">
        <Histogram
          labels={Array.from({ length: 24 }, (_, hour) => String(hour).padStart(2, "0"))}
          values={activity?.by_hour ?? []}
        />
        <Histogram labels={weekdayLabels} values={activity?.by_weekday ?? []} />
      </div>
    </section>
  );
}

function NowPanel({ now }: { now: NowView | null }) {
  return (
    <section className="section">
      <div className="section-heading">
        <h2>Today</h2>
        <span>{now?.sync_in_progress === true ? "syncing" : "idle"}</span>
      </div>
      <div className="today-grid">
        <MetricMini label="Sessions" value={now?.today.sessions ?? 0} />
        <MetricMini label="Messages" value={now?.today.messages ?? 0} />
        <MetricMini label="Tool Calls" value={now?.today.tool_calls ?? 0} />
        <MetricMini label="Cost" value={`$${(now?.today.estimated_cost_usd ?? 0).toFixed(2)}`} />
      </div>
      <p className="settings-note">Last sync: {now?.last_sync_at ?? "-"}</p>
    </section>
  );
}

function ModelSparklinePanel({ sparklines }: { sparklines: ModelSparklines | null }) {
  const rows = Object.entries(sparklines?.models ?? {});
  return (
    <section className="section">
      <div className="section-heading">
        <h2>Model Activity</h2>
        <span>{sparklines?.days.length ?? 0} days</span>
      </div>
      <div className="sparkline-list">
        {rows.length === 0 ? (
          <p className="empty">No dated model activity yet.</p>
        ) : (
          rows.map(([model, values]) => (
            <div className="sparkline-row" key={model}>
              <span>{model}</span>
              <Sparkline values={values} />
              <strong>{values.reduce((sum, value) => sum + value, 0)}</strong>
            </div>
          ))
        )}
      </div>
    </section>
  );
}

function Histogram({ labels, values }: { labels: string[]; values: number[] }) {
  const max = Math.max(1, ...values);
  return (
    <div className="histogram">
      {labels.map((label, index) => {
        const value = values[index] ?? 0;
        return (
          <span key={label} title={`${label}: ${value}`}>
            <i style={{ height: `${Math.max(3, (value / max) * 100)}%` }} />
            <b>{label}</b>
          </span>
        );
      })}
    </div>
  );
}

function Sparkline({ values }: { values: number[] }) {
  const max = Math.max(1, ...values);
  const width = 160;
  const height = 36;
  const points = values
    .map((value, index) => {
      const x = values.length <= 1 ? 0 : (index / (values.length - 1)) * width;
      const y = height - (value / max) * (height - 4) - 2;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg aria-hidden="true" className="sparkline" viewBox={`0 0 ${width} ${height}`}>
      <polyline points={points} />
    </svg>
  );
}

function MetricMini({ label, value }: { label: string; value: string | number }) {
  return (
    <div className="metric-mini">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function InsightsView({ rows, onMarked }: { rows: Recommendation[]; onMarked: () => void }) {
  const [pending, setPending] = useState<string | null>(null);
  const markImplemented = (key: string) => {
    setPending(key);
    void getJson<{ ok: boolean }>("/api/recommendations/mark", {
      method: "POST",
      body: JSON.stringify({ key, source: "ui" }),
    })
      .then(onMarked)
      .finally(() => setPending(null));
  };

  return (
    <section className="insight-grid">
      {rows.length === 0 ? <p className="empty">No open recommendations.</p> : null}
      {rows.map((row) => (
        <article className={`insight tone-${row.tone ?? "neutral"}`} key={row.key}>
          <span>{row.category ?? row.status}</span>
          <h2>{row.title}</h2>
          {row.detail != null ? <p>{row.detail}</p> : null}
          {row.suggestion != null ? <p>{row.suggestion}</p> : null}
          {row.action != null ? <strong>{row.action}</strong> : null}
          <button
            disabled={pending === row.key}
            type="button"
            onClick={() => markImplemented(row.key)}
          >
            {pending === row.key ? "Saving" : "Mark Implemented"}
          </button>
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

function SettingsView({
  config,
  settingsInfo,
}: {
  config: ConfigView | null;
  settingsInfo: SettingsInfo | null;
}) {
  const [settings, setSettings] = useState<UserSettings | null>(settingsInfo?.settings ?? null);

  useEffect(() => {
    setSettings(settingsInfo?.settings ?? null);
  }, [settingsInfo]);

  const save = (patch: Partial<UserSettings>) => {
    const next = { ...(settings ?? settingsInfo?.settings), ...patch } as UserSettings;
    setSettings(next);
    void getJson<SettingsInfo>("/api/settings", {
      method: "POST",
      body: JSON.stringify(next),
    }).then((response) => setSettings(response.settings));
  };

  return (
    <div className="split">
      <section className="section settings">
        <h2>Launch Preferences</h2>
        <div className="setting-list">
          <SettingSelect
            label="Agent"
            options={settingsInfo?.options.agents ?? []}
            value={settings?.agent ?? "claude"}
            onChange={(agent) => save({ agent })}
          />
          <SettingSelect
            label="Terminal"
            options={settingsInfo?.options.terminals ?? []}
            value={settings?.terminal ?? "terminal"}
            onChange={(terminal) => save({ terminal })}
          />
          <SettingSelect
            label="Editor"
            options={settingsInfo?.options.ides ?? []}
            value={settings?.ide ?? "vscode"}
            onChange={(ide) => save({ ide })}
          />
        </div>
        <p className="settings-note">
          {settingsInfo?.can_launch === true
            ? "Native launcher is available on this Mac."
            : "Native launcher is unavailable on this platform."}
        </p>
      </section>
      <section className="section settings">
        <h2>Local Paths</h2>
        <dl>
          <dt>Settings</dt>
          <dd>{settingsInfo?.path ?? "-"}</dd>
          <dt>Archive</dt>
          <dd>{config?.dbPath ?? "-"}</dd>
          <dt>Claude</dt>
          <dd>{config?.claudeDir ?? "-"}</dd>
          <dt>Codex</dt>
          <dd>{config?.codexDir ?? "-"}</dd>
        </dl>
      </section>
    </div>
  );
}

function SettingSelect({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: [string, string][];
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="setting-select">
      <span>{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map(([key, name]) => (
          <option key={key} value={key}>
            {name}
          </option>
        ))}
      </select>
    </label>
  );
}

function SessionDetailView({ id }: { id: number }) {
  const [detail, setDetail] = useState<{
    summary: SessionSummary;
    messages: {
      role: string;
      timestamp: string | null;
      model: string | null;
      blocks: {
        ordinal: number;
        block_type: string;
        text: string | null;
        tool_name: string | null;
        tool_input: string | null;
        tool_result: string | null;
      }[];
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
          <header>
            <strong>{message.role}</strong>
            <span>{message.model ?? message.timestamp ?? ""}</span>
          </header>
          {message.blocks.map((block) => (
            <TranscriptBlock block={block} key={`${block.ordinal}-${block.block_type}`} />
          ))}
        </article>
      ))}
    </section>
  );
}

function TranscriptBlock({
  block,
}: {
  block: {
    ordinal: number;
    block_type: string;
    text: string | null;
    tool_name: string | null;
    tool_input: string | null;
    tool_result: string | null;
  };
}) {
  if (block.block_type === "tool_use") {
    return (
      <div className="tool-block">
        <span>{block.tool_name ?? "tool_use"}</span>
        <pre>{prettyJson(block.tool_input)}</pre>
      </div>
    );
  }
  if (block.block_type === "tool_result") {
    return (
      <div className="tool-block result-block">
        <span>result</span>
        <pre>{block.tool_result ?? ""}</pre>
      </div>
    );
  }
  if (block.block_type === "thinking") {
    return <p className="thinking">{block.text ?? ""}</p>;
  }
  return <p>{block.text ?? block.block_type}</p>;
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
        {rows.length === 0 ? (
          <tr>
            <td colSpan={headers.length}>No rows.</td>
          </tr>
        ) : null}
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

function dateRange(bounds: DateBounds | null): string {
  if (bounds?.min == null || bounds.max == null) {
    return "all time";
  }
  return bounds.min === bounds.max ? bounds.min : `${bounds.min}..${bounds.max}`;
}

function prettyJson(value: string | null): string {
  if (value == null || value === "") {
    return "";
  }
  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
}

function snippetParts(snippet: string): { key: string; text: string; match: boolean }[] {
  const parts: { key: string; text: string; match: boolean }[] = [];
  let remaining = snippet;
  let offset = 0;
  while (remaining !== "") {
    const start = remaining.indexOf("[");
    if (start < 0) {
      parts.push({ key: `text-${offset}`, text: remaining, match: false });
      break;
    }
    if (start > 0) {
      parts.push({ key: `text-${offset}`, text: remaining.slice(0, start), match: false });
    }
    const close = remaining.indexOf("]", start + 1);
    if (close < 0) {
      parts.push({ key: `text-${offset + start}`, text: remaining.slice(start), match: false });
      break;
    }
    parts.push({
      key: `match-${offset + start}`,
      text: remaining.slice(start + 1, close),
      match: true,
    });
    const consumed = close + 1;
    offset += consumed;
    remaining = remaining.slice(consumed);
  }
  return parts;
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
