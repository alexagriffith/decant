import { type MouseEvent, type ReactNode, useEffect, useState } from "react";
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
  input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  est_reasoning_tokens: number;
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
  last_touched_at: string | null;
};

type Recommendation = {
  key: string;
  kind: "signal" | "catalog";
  status: string;
  category: string | null;
  title: string;
  detail: string | null;
  suggestion: string | null;
  prompt: string | null;
  url: string | null;
  link_label: string | null;
  icon: string | null;
  tone: string | null;
  score: number;
  action: string | null;
  memory_layer: string | null;
  promotion_target: string | null;
  trigger: string | null;
  evidence: string | null;
  success_metric: string | null;
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
  byModel: DimensionRow[];
  byProject: DimensionRow[];
  byDay: DimensionRow[];
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
  byModel: [],
  byProject: [],
  byDay: [],
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

type NavItem = {
  key: string;
  href: string;
  label: string;
  icon: IconName;
};

const navItems: NavItem[] = [
  { key: "sessions", href: "/", label: "Sessions", icon: "sessions" },
  { key: "search", href: "/search", label: "Search", icon: "search" },
  { key: "analytics", href: "/analytics", label: "Analytics", icon: "chart" },
  { key: "insights", href: "/insights", label: "Insights", icon: "lightbulb" },
  { key: "tools", href: "/tools", label: "Tools & MCP", icon: "tools" },
  { key: "files", href: "/files", label: "Files", icon: "file" },
];

const CLAUDE_ICON_PATH =
  "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z";
const OPENAI_ICON_PATH =
  "M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z";
const ANTHROPIC_ICON_PATH =
  "M17.3041 3.541h-3.6718l6.696 16.918H24Zm-10.6082 0L0 20.459h3.7442l1.3693-3.5527h7.0052l1.3693 3.5528h3.7442L10.5363 3.5409Zm-.3712 10.2232 2.2914-5.9456 2.2914 5.9456Z";

const SESSION_PAGE_SIZE = 100;
type ThemeChoice = "system" | "light" | "dark";

function App() {
  const [path, setPath] = useState(locationPath);
  const [data, setData] = useState<DashboardData>(emptyData);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [sessionLimit, setSessionLimit] = useState(SESSION_PAGE_SIZE);
  const [menuOpen, setMenuOpen] = useState(false);
  const [theme, setTheme] = useState<ThemeChoice>(() => {
    const stored = localStorage.getItem("decant-theme");
    return stored === "light" || stored === "dark" ? stored : "system";
  });

  useEffect(() => {
    const onPop = () => setPath(locationPath());
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  useEffect(() => {
    if (theme === "system") {
      document.documentElement.removeAttribute("data-theme");
      localStorage.removeItem("decant-theme");
    } else {
      document.documentElement.dataset.theme = theme;
      localStorage.setItem("decant-theme", theme);
    }
  }, [theme]);

  useEffect(() => {
    let cancelled = false;
    setLoading(reloadKey === 0);
    Promise.all([
      getJson<Summary>("/api/stats/summary"),
      getJson<SessionSummary[]>(`/api/sessions?limit=${sessionLimit}&offset=0`),
      getJson<DimensionRow[]>("/api/stats/by-dimension?dim=tool"),
      getJson<DimensionRow[]>("/api/stats/by-dimension?dim=model"),
      getJson<DimensionRow[]>("/api/stats/by-dimension?dim=project"),
      getJson<DimensionRow[]>("/api/stats/by-dimension?dim=day"),
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
          byModel,
          byProject,
          byDay,
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
            byModel,
            byProject,
            byDay,
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
  }, [reloadKey, sessionLimit]);

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
  const activeKey = activeRouteKey(path);
  const metrics = data.summary;
  const lastActivity = latestSessionDay(data.sessions);

  return (
    <div className="app-shell">
      <button
        aria-label="Close menu"
        className={`sidebar-backdrop${menuOpen ? " is-open" : ""}`}
        onClick={() => setMenuOpen(false)}
        type="button"
      />
      <aside className={`sidebar${menuOpen ? " is-open" : ""}`}>
        <div className="brand-row">
          <a className="brand" href="/" onClick={(event) => navigate(event, "/", setPath)}>
            <span className="brand-icon">
              <Icon name="beaker" />
            </span>
            <span>decant</span>
          </a>
          <button
            aria-label="Close menu"
            className="icon-button mobile-only"
            onClick={() => setMenuOpen(false)}
            type="button"
          >
            <Icon name="x" />
          </button>
        </div>
        <nav aria-label="Primary">
          {navItems.map((item) => (
            <a
              aria-current={activeKey === item.key ? "page" : undefined}
              href={item.href}
              key={item.href}
              onClick={(event) => {
                setMenuOpen(false);
                navigate(event, item.href, setPath);
              }}
            >
              <Icon name={item.icon} />
              <span>{item.label}</span>
            </a>
          ))}
        </nav>
        <div className="sidebar-footer">
          <div className="sidebar-stat" title="Live and auto-syncing">
            <span className="live-dot" />
            <span>
              <strong>{formatInt(metrics?.sessions ?? 0)}</strong> sessions
            </span>
          </div>
          <div className="sidebar-stat">
            <Icon name="money" />
            <span>
              <strong>{money(metrics?.estimated_cost_usd ?? 0)}</strong> tracked
            </span>
          </div>
          {lastActivity != null ? (
            <div className="sidebar-stat">
              <Icon name="clock" />
              <span>latest {lastActivity}</span>
            </div>
          ) : null}
        </div>
      </aside>
      <div className="workspace">
        <header className="topbar">
          <button
            aria-label="Open menu"
            className="icon-button mobile-only"
            onClick={() => setMenuOpen(true)}
            type="button"
          >
            <Icon name="menu" />
          </button>
          <h1>{titleFor(active)}</h1>
          <div className="topbar-spacer" />
          <a
            className="search-shortcut"
            href="/search"
            onClick={(event) => navigate(event, "/search", setPath)}
          >
            <Icon name="search" />
            <span>Search...</span>
            <kbd>/</kbd>
          </a>
          <a
            aria-label="Settings"
            className="icon-button"
            href="/settings"
            onClick={(event) => navigate(event, "/settings", setPath)}
            title="Settings"
          >
            <Icon name="settings" />
          </a>
          <fieldset className="theme-toggle">
            <legend>Theme</legend>
            {(["system", "light", "dark"] as const).map((choice) => (
              <button
                aria-label={`${choice} theme`}
                aria-pressed={theme === choice}
                key={choice}
                onClick={() => setTheme(choice)}
                type="button"
              >
                <Icon
                  name={choice === "system" ? "desktop" : choice === "light" ? "sun" : "moon"}
                />
              </button>
            ))}
          </fieldset>
        </header>
        <main className="content">
          <div className="content-wrap">
            {error != null ? <div className="notice danger">{error}</div> : null}
            {loading ? (
              <div className="notice">Loading archive data...</div>
            ) : (
              renderView(active, path, data, {
                refresh: () => setReloadKey((key) => key + 1),
                sessionLimit,
                setSessionLimit,
              })
            )}
          </div>
        </main>
      </div>
    </div>
  );
}

function renderView(
  active: string,
  path: string,
  data: DashboardData,
  actions: {
    refresh: () => void;
    sessionLimit: number;
    setSessionLimit: (limit: number) => void;
  },
) {
  const pathname = pathOnly(path);
  if (pathname.startsWith("/sessions/")) {
    return <SessionDetailView id={Number(pathname.split("/").at(-1))} />;
  }
  switch (active) {
    case "Sessions":
      return (
        <SessionsView
          data={data}
          limit={actions.sessionLimit}
          onLimitChange={actions.setSessionLimit}
        />
      );
    case "Search":
      return <SearchView path={path} />;
    case "Analytics":
      return <AnalyticsView data={data} />;
    case "Insights":
      return (
        <InsightsView
          rows={data.recommendations}
          settingsInfo={data.settings}
          onMarked={actions.refresh}
        />
      );
    case "Tools & MCP":
      return <ToolsView data={data} />;
    case "Files":
      return <FilesView rows={data.files} />;
    case "Settings":
      return <SettingsView config={data.config} settingsInfo={data.settings} />;
    default:
      return (
        <SessionsView
          data={data}
          limit={actions.sessionLimit}
          onLimitChange={actions.setSessionLimit}
        />
      );
  }
}

function SessionsView({
  data,
  limit,
  onLimitChange,
}: {
  data: DashboardData;
  limit: number;
  onLimitChange: (limit: number) => void;
}) {
  const [query, setQuery] = useState("");
  const total = data.summary?.sessions ?? data.sessions.length;
  const filtered = filterSessions(data.sessions, query);
  const hasMore = data.sessions.length < total;
  return (
    <div className="view-stack">
      <div className="stat-grid sessions-stat-grid">
        <StatCard
          icon="sessions"
          label="Sessions"
          tone="accent"
          value={formatInt(data.summary?.sessions ?? 0)}
        />
        <StatCard
          icon="messages"
          label="Messages"
          tone="info"
          value={formatInt(data.summary?.messages ?? 0)}
        />
        <StatCard
          icon="money"
          label="Est. cost"
          tone="success"
          value={money(data.summary?.estimated_cost_usd ?? 0)}
        />
      </div>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>Sessions</h2>
          </div>
          <input
            aria-label="Filter sessions"
            className="session-filter"
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter by title, model, or tool..."
            value={query}
          />
        </div>
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Tool</th>
                <th>Title</th>
                <th>Model</th>
                <th className="numeric">Msgs</th>
                <th className="numeric">Cost</th>
                <th className="numeric">Started</th>
              </tr>
            </thead>
            <tbody>
              {filtered.length === 0 ? (
                <tr>
                  <td colSpan={6}>No sessions ingested yet.</td>
                </tr>
              ) : null}
              {filtered.map((session) => (
                <tr key={session.id}>
                  <td>
                    <ToolBadge tool={session.tool} />
                  </td>
                  <td className="truncate-cell">
                    <a href={`/sessions/${session.id}`}>
                      {session.title ?? session.source_session_id ?? "(untitled)"}
                    </a>
                  </td>
                  <td>
                    <ModelBadge model={session.model} />
                  </td>
                  <td className="numeric muted">{formatInt(session.message_count)}</td>
                  <td className="numeric">{money(session.estimated_cost_usd)}</td>
                  <td className="numeric muted">{relativeTime(session.started_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="panel-footer">
          <span>{sessionsCaption(query, filtered.length, data.sessions.length, total)}</span>
          {hasMore ? (
            <button
              className="secondary-button"
              onClick={() => onLimitChange(limit + SESSION_PAGE_SIZE)}
              type="button"
            >
              <Icon name="download" />
              Load more
            </button>
          ) : null}
        </div>
      </section>
    </div>
  );
}

function filterSessions(sessions: SessionSummary[], query: string): SessionSummary[] {
  const needle = query.trim().toLowerCase();
  if (needle === "") {
    return sessions;
  }
  return sessions.filter((session) =>
    [session.title, session.model, session.tool, session.project_path]
      .filter((value): value is string => value != null)
      .some((value) => value.toLowerCase().includes(needle)),
  );
}

function sessionsCaption(query: string, visible: number, loaded: number, total: number): string {
  if (query.trim() !== "") {
    return `Showing ${formatInt(visible)} matching loaded ${visible === 1 ? "row" : "rows"} from ${formatInt(loaded)} loaded sessions`;
  }
  return `Showing ${formatInt(loaded)} of ${formatInt(total)} sessions`;
}

function SearchView({ path }: { path: string }) {
  const initialQuery = new URLSearchParams(path.split("?")[1] ?? "").get("q") ?? "";
  const [query, setQuery] = useState(initialQuery);
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
      body: JSON.stringify({ query: trimmed, limit: 25 }),
    })
      .then(setHits)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setSearching(false));
  };

  useEffect(() => {
    setQuery(initialQuery);
    if (initialQuery.trim() === "") {
      setHits([]);
      return;
    }
    setSearching(true);
    setError(null);
    void getJson<SearchHit[]>("/api/search", {
      method: "POST",
      body: JSON.stringify({ query: initialQuery, limit: 25 }),
    })
      .then(setHits)
      .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
      .finally(() => setSearching(false));
  }, [initialQuery]);

  return (
    <div className="search-page">
      <header className="page-heading">
        <h1>Search</h1>
        <p>Full-text search across every message and tool call in your archive.</p>
      </header>

      <form
        className="search-form"
        onSubmit={(event) => {
          event.preventDefault();
          runSearch();
        }}
      >
        <Icon name="search" />
        <input
          autoComplete="off"
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search across all sessions and tool calls..."
          value={query}
        />
      </form>

      {query.trim() !== "" ? (
        <p className="result-caption">
          {formatInt(hits.length)} {hits.length === 1 ? "result" : "results"}
        </p>
      ) : null}
      {error != null ? <div className="notice danger">{error}</div> : null}

      <div className="search-results">
        {searching ? <div className="empty-state">Searching...</div> : null}
        {!searching && query.trim() === "" ? (
          <EmptyState
            icon="search"
            message="Find any message or tool call across every session by keyword."
            title="Search your archive"
          />
        ) : null}
        {!searching && query.trim() !== "" && hits.length === 0 ? (
          <EmptyState
            icon="inbox"
            message="Nothing matched your search. Try a different term."
            title="No matches"
          />
        ) : null}
        {hits.map((hit) => (
          <a
            className="result-card"
            href={`/sessions/${hit.session_id}`}
            key={`${hit.session_id}-${hit.snippet}`}
          >
            <div className="result-card-heading">
              <span>{hit.session_title ?? `Session ${hit.session_id}`}</span>
            </div>
            <p>
              <HighlightedSnippet snippet={hit.snippet} />
            </p>
          </a>
        ))}
      </div>
    </div>
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
  const byDay = data.byDay
    .filter((row) => row.key !== "")
    .slice()
    .sort((left, right) => left.key.localeCompare(right.key));
  const maxModelCost = Math.max(1.0e-9, ...data.byModel.map((row) => row.estimated_cost_usd));
  return (
    <div className="view-stack">
      <header className="page-heading inline-heading">
        <div>
          <h1>Analytics</h1>
          <p>Usage and cost across your sessions.</p>
        </div>
        <span>{dateRange(data.dateBounds)}</span>
      </header>

      <div className="stat-grid analytics-stat-grid">
        <StatCard
          icon="sessions"
          label="Sessions"
          tone="accent"
          value={formatInt(data.summary?.sessions ?? 0)}
        />
        <StatCard
          icon="messages"
          label="Messages"
          tone="info"
          value={formatInt(data.summary?.messages ?? 0)}
        />
        <StatCard
          icon="bolt"
          label="Tool calls"
          tone="warning"
          value={formatInt(data.summary?.tool_calls ?? 0)}
        />
        <StatCard
          icon="download"
          label="Input tokens"
          tone="neutral"
          value={compact(data.summary?.input_tokens ?? 0)}
        />
        <StatCard
          icon="upload"
          label="Output tokens"
          tone="neutral"
          value={compact(data.summary?.output_tokens ?? 0)}
        />
        <StatCard
          icon="money"
          label="Est. cost"
          tone="success"
          value={money(data.summary?.estimated_cost_usd ?? 0)}
        />
      </div>

      <div className="split">
        <DailyPanel rows={byDay} metric="sessions" title="Sessions per day" />
        <DailyPanel rows={byDay} metric="cost" title="Cost per day" />
      </div>

      <div className="split">
        <ActivityPanel activity={data.activity} />
        <WeekdayPanel activity={data.activity} />
      </div>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>By model</h2>
            <p>Trend is sessions per day over the selected range</p>
          </div>
        </div>
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Model</th>
                <th>Trend</th>
                <th className="numeric">Sessions</th>
                <th className="numeric">In tok</th>
                <th className="numeric">Out tok</th>
                <th className="numeric">Reason tok</th>
                <th className="numeric">Cost</th>
                <th>Share</th>
              </tr>
            </thead>
            <tbody>
              {data.byModel.length === 0 ? (
                <tr>
                  <td colSpan={8}>No model activity.</td>
                </tr>
              ) : null}
              {data.byModel.map((row) => (
                <tr key={row.key}>
                  <td>
                    <ModelBadge model={row.key} />
                  </td>
                  <td>
                    <Sparkline
                      tone={brandTone(row.key)}
                      values={data.modelSparklines?.models[row.key] ?? []}
                    />
                  </td>
                  <td className="numeric">{formatInt(row.sessions)}</td>
                  <td className="numeric muted">{compact(row.input_tokens)}</td>
                  <td className="numeric muted">{compact(row.output_tokens)}</td>
                  <td className="numeric muted">
                    {row.reasoning_tokens > 0
                      ? compact(row.reasoning_tokens)
                      : row.est_reasoning_tokens > 0
                        ? `~${compact(row.est_reasoning_tokens)}`
                        : "-"}
                  </td>
                  <td className="numeric">{money(row.estimated_cost_usd)}</td>
                  <td className="share-cell">
                    <Bar
                      fraction={row.estimated_cost_usd / maxModelCost}
                      tone={brandTone(row.key)}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>

      {data.byProject.length > 0 ? (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <h2>By project</h2>
            </div>
          </div>
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Project</th>
                  <th className="numeric">Sessions</th>
                  <th className="numeric">Cost</th>
                </tr>
              </thead>
              <tbody>
                {data.byProject.slice(0, 12).map((row) => (
                  <tr key={row.key}>
                    <td className="mono truncate-cell" title={row.key}>
                      {basename(row.key)}
                    </td>
                    <td className="numeric muted">{formatInt(row.sessions)}</td>
                    <td className="numeric">{money(row.estimated_cost_usd)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
      ) : null}
    </div>
  );
}

function ActivityPanel({ activity }: { activity: Activity | null }) {
  const labels = Array.from({ length: 24 }, (_, hour) => hourLabel(hour));
  const peak = activity?.peak_hour ?? peakIndex(activity?.by_hour ?? []);
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Busiest hours</h2>
          <p>
            {peak == null
              ? "Sessions by hour, local time"
              : `Local time, you ship most around ${hourLabel(peak)}`}
          </p>
        </div>
      </div>
      <div className="panel-body chart-panel-body">
        <AnalyticsChart
          labels={labels}
          metric="int"
          values={activity?.by_hour ?? []}
          variant="bar"
        />
      </div>
    </section>
  );
}

function WeekdayPanel({ activity }: { activity: Activity | null }) {
  const labels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const peak = activity?.peak_weekday ?? peakIndex(activity?.by_weekday ?? []);
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>Busiest days</h2>
          <p>{peak == null ? "Sessions by weekday" : `You ship most on ${weekdayLabel(peak)}`}</p>
        </div>
      </div>
      <div className="panel-body chart-panel-body">
        <AnalyticsChart
          labels={labels}
          metric="int"
          values={activity?.by_weekday ?? []}
          variant="bar"
        />
      </div>
    </section>
  );
}

function DailyPanel({
  rows,
  metric,
  title,
}: {
  rows: DimensionRow[];
  metric: "sessions" | "cost";
  title: string;
}) {
  const values = rows.map((row) => (metric === "sessions" ? row.sessions : row.estimated_cost_usd));
  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <h2>{title}</h2>
        </div>
      </div>
      <div className="panel-body">
        {rows.length === 0 ? (
          <EmptyState icon="chart" message="Widen the date range." title="No data in range" />
        ) : (
          <AnalyticsChart
            labels={rows.map((row) => row.key)}
            metric={metric === "cost" ? "money" : "int"}
            values={values}
            variant={metric === "cost" ? "line" : "bar"}
          />
        )}
      </div>
    </section>
  );
}

type AnalyticsChartMetric = "int" | "money";
type AnalyticsChartVariant = "bar" | "line";

function AnalyticsChart({
  labels,
  metric,
  values,
  variant,
}: {
  labels: string[];
  metric: AnalyticsChartMetric;
  values: number[];
  variant: AnalyticsChartVariant;
}) {
  const width = 640;
  const height = 240;
  const left = 44;
  const right = 16;
  const top = 18;
  const bottom = 30;
  const plotWidth = width - left - right;
  const plotHeight = height - top - bottom;
  const cleanValues = labels.map((_, index) => Math.max(0, values[index] ?? 0));
  const max = niceMax(Math.max(1, ...cleanValues));
  const yTicks = [max, max / 2, 0];
  const xIndexes = chartLabelIndexes(labels.length);
  const bandwidth = labels.length === 0 ? plotWidth : plotWidth / labels.length;
  const barWidth = Math.max(3, Math.min(26, bandwidth * 0.62));
  const points = cleanValues.map((value, index) => ({
    label: labels[index] ?? "",
    value,
    x:
      labels.length <= 1
        ? left + plotWidth / 2
        : variant === "bar"
          ? left + bandwidth * (index + 0.5)
          : left + (index / (labels.length - 1)) * plotWidth,
    y: top + plotHeight - (value / max) * plotHeight,
  }));
  const linePath = points
    .map((point, index) => `${index === 0 ? "M" : "L"} ${point.x.toFixed(2)} ${point.y.toFixed(2)}`)
    .join(" ");
  const areaPath =
    points.length > 1
      ? `${linePath} L ${(points.at(-1)?.x ?? left).toFixed(2)} ${top + plotHeight} L ${left} ${top + plotHeight} Z`
      : "";

  return (
    <svg
      aria-label="Analytics chart"
      className={`analytics-chart is-${variant}`}
      preserveAspectRatio="none"
      role="img"
      viewBox={`0 0 ${width} ${height}`}
    >
      {yTicks.map((tick) => {
        const y = top + plotHeight - (tick / max) * plotHeight;
        return (
          <g key={tick}>
            <line className="chart-grid" x1={left} x2={width - right} y1={y} y2={y} />
            <text className="chart-y-label" x={left - 10} y={y + 4}>
              {chartValue(tick, metric)}
            </text>
          </g>
        );
      })}
      <line
        className="chart-axis"
        x1={left}
        x2={width - right}
        y1={top + plotHeight}
        y2={top + plotHeight}
      />
      {variant === "bar"
        ? points.map((point) => (
            <rect
              className="chart-bar"
              height={Math.max(2, top + plotHeight - point.y)}
              key={point.label}
              rx="3"
              width={barWidth}
              x={point.x - barWidth / 2}
              y={Math.min(top + plotHeight - 2, point.y)}
            >
              <title>{`${point.label}: ${chartTooltipValue(point.value, metric)}`}</title>
            </rect>
          ))
        : null}
      {variant === "line" && areaPath !== "" ? <path className="chart-area" d={areaPath} /> : null}
      {variant === "line" && linePath !== "" ? (
        <path className="chart-line" d={linePath}>
          <title>
            {points
              .map((point) => `${point.label}: ${chartTooltipValue(point.value, metric)}`)
              .join("\n")}
          </title>
        </path>
      ) : null}
      {xIndexes.map((index) => {
        const point = points[index];
        if (point == null) {
          return null;
        }
        return (
          <text className="chart-x-label" key={point.label} x={point.x} y={height - 8}>
            {chartLabel(point.label)}
          </text>
        );
      })}
    </svg>
  );
}

function Sparkline({ tone = "accent", values }: { tone?: BadgeTone; values: number[] }) {
  const points = sparkPoints(values);
  if (points == null) {
    return <span className="spark-empty">-</span>;
  }
  return (
    <svg
      aria-hidden="true"
      className={`sparkline tone-${tone}`}
      preserveAspectRatio="none"
      viewBox="0 0 100 24"
    >
      <polyline points={points} />
    </svg>
  );
}

function sparkPoints(values: number[]): string | null {
  const cleanValues = values.map((value) => Math.max(0, value));
  if (cleanValues.length < 2) {
    return null;
  }
  const max = Math.max(1, ...cleanValues);
  return cleanValues
    .map((value, index) => {
      const x = (index / (cleanValues.length - 1)) * 100;
      const y = 23 - (value / max) * 22;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

function hourLabel(hour: number): string {
  if (hour === 0) {
    return "12a";
  }
  if (hour === 12) {
    return "12p";
  }
  return hour < 12 ? `${hour}a` : `${hour - 12}p`;
}

function weekdayLabel(day: number): string {
  return ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][day] ?? String(day);
}

function peakIndex(values: number[]): number | null {
  const max = Math.max(0, ...values);
  return max > 0 ? values.indexOf(max) : null;
}

function niceMax(value: number): number {
  if (value <= 1) {
    return 1;
  }
  const exponent = Math.floor(Math.log10(value));
  const base = 10 ** exponent;
  const normalized = value / base;
  const nice = normalized <= 2 ? 2 : normalized <= 5 ? 5 : 10;
  return nice * base;
}

function chartLabelIndexes(count: number): number[] {
  if (count <= 0) {
    return [];
  }
  if (count <= 7) {
    return Array.from({ length: count }, (_, index) => index);
  }
  const maxLabels = count <= 14 ? 7 : count <= 31 ? 8 : 10;
  const step = Math.max(1, Math.ceil((count - 1) / (maxLabels - 1)));
  const indexes = new Set<number>([0, count - 1]);
  for (let index = 0; index < count; index += step) {
    indexes.add(index);
  }
  return [...indexes].sort((left, right) => left - right);
}

function chartLabel(value: string): string {
  return /^\d{4}-\d{2}-\d{2}/.test(value) ? value.slice(5, 10) : value;
}

function chartValue(value: number, metric: AnalyticsChartMetric): string {
  const formatted = compactAxis(value);
  return metric === "money" ? `$${formatted}` : formatted;
}

function chartTooltipValue(value: number, metric: AnalyticsChartMetric): string {
  return metric === "money" ? money(value) : formatInt(value);
}

function compactAxis(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 1_000_000) {
    return `${trimNumber(value / 1_000_000)}M`;
  }
  if (abs >= 1_000) {
    return `${trimNumber(value / 1_000)}K`;
  }
  return trimNumber(value);
}

function trimNumber(value: number): string {
  return Number.isInteger(value) ? formatInt(value) : value.toFixed(2).replace(/\.?0+$/, "");
}

type BadgeTone =
  | "neutral"
  | "accent"
  | "success"
  | "warning"
  | "danger"
  | "info"
  | "claude"
  | "openai";

type IconName =
  | "anthropic"
  | "arrowLeft"
  | "beaker"
  | "bolt"
  | "chart"
  | "check"
  | "claude"
  | "clock"
  | "cpu"
  | "desktop"
  | "download"
  | "file"
  | "folder"
  | "inbox"
  | "lightbulb"
  | "menu"
  | "messages"
  | "money"
  | "moon"
  | "openai"
  | "search"
  | "sessions"
  | "settings"
  | "sun"
  | "tools"
  | "upload"
  | "x";

function StatCard({
  icon,
  label,
  tone,
  value,
}: {
  icon: IconName;
  label: string;
  tone: BadgeTone;
  value: string;
}) {
  return (
    <div className="stat-card">
      <div>
        <span>{label}</span>
        <strong>{value}</strong>
      </div>
      <span className={`stat-icon tone-${tone}`}>
        <Icon name={icon} />
      </span>
    </div>
  );
}

function Badge({
  children,
  className,
  mono = false,
  tone = "neutral",
}: {
  children: ReactNode;
  className?: string;
  mono?: boolean;
  tone?: BadgeTone;
}) {
  return (
    <span
      className={`badge tone-${tone}${mono ? " is-mono" : ""}${className ? ` ${className}` : ""}`}
    >
      {children}
    </span>
  );
}

function ToolBadge({ tool }: { tool: string | null | undefined }) {
  if (tool === "claude_code") {
    return (
      <Badge tone="claude">
        <Icon name="claude" />
        Claude
      </Badge>
    );
  }
  if (tool === "codex") {
    return (
      <Badge tone="openai">
        <Icon name="openai" />
        Codex
      </Badge>
    );
  }
  return <Badge>{tool ?? "-"}</Badge>;
}

function ModelBadge({ model }: { model: string | null | undefined }) {
  if (model == null || model === "") {
    return <span className="faint">-</span>;
  }
  const tone = brandTone(model);
  const icon = modelBrandIcon(model, tone);
  return (
    <Badge mono tone={tone}>
      {icon == null ? null : <Icon name={icon} />}
      {model}
    </Badge>
  );
}

function EmptyState({ icon, message, title }: { icon: IconName; message: string; title: string }) {
  return (
    <div className="empty-state">
      <span>
        <Icon name={icon} />
      </span>
      <h3>{title}</h3>
      <p>{message}</p>
    </div>
  );
}

function Bar({ fraction, tone }: { fraction: number; tone: BadgeTone }) {
  const pct = Math.max(0, Math.min(1, Number.isFinite(fraction) ? fraction : 0)) * 100;
  return (
    <div className="bar">
      <span className={`tone-${tone}`} style={{ width: `${pct}%` }} />
    </div>
  );
}

function RecommendationHero({
  canLaunch,
  onComplete,
  pending,
  row,
}: {
  canLaunch: boolean;
  onComplete: (row: Recommendation) => void;
  pending: string | null;
  row: Recommendation;
}) {
  return (
    <article className={`signal-hero tone-${toneName(row.tone)}`}>
      <span className={`signal-icon tone-${toneName(row.tone)}`}>
        <Icon name={recommendationIcon(row)} />
      </span>
      <div>
        <span className={`signal-kicker tone-${toneName(row.tone)}`}>Top signal</span>
        <h3>{row.title}</h3>
        {row.detail != null ? <p>{row.detail}</p> : null}
        {row.suggestion != null ? (
          <div className="suggestion-block">
            <span>Suggested</span>
            <p>{row.suggestion}</p>
          </div>
        ) : null}
        <PromotionPanel row={row} />
        <RecommendationActions
          canLaunch={canLaunch}
          onComplete={onComplete}
          pending={pending}
          row={row}
        />
      </div>
    </article>
  );
}

function RecommendationRow({
  canLaunch,
  onComplete,
  pending,
  row,
}: {
  canLaunch: boolean;
  onComplete: (row: Recommendation) => void;
  pending: string | null;
  row: Recommendation;
}) {
  return (
    <div className="signal-row">
      <span className={`signal-rail tone-${toneName(row.tone)}`} />
      <span className={`signal-icon tone-${toneName(row.tone)}`}>
        <Icon name={recommendationIcon(row)} />
      </span>
      <div>
        <p>{row.title}</p>
        {row.detail != null ? <small>{row.detail}</small> : null}
        <PromotionMeta row={row} />
      </div>
      <RecommendationActions
        canLaunch={canLaunch}
        compact
        onComplete={onComplete}
        pending={pending}
        row={row}
      />
    </div>
  );
}

function RecommendationCard({
  canLaunch,
  featured,
  onComplete,
  pending,
  row,
}: {
  canLaunch: boolean;
  featured: boolean;
  onComplete: (row: Recommendation) => void;
  pending: string | null;
  row: Recommendation;
}) {
  return (
    <article className={`catalog-card${featured ? " is-featured" : ""}`}>
      <div>
        <span className={`signal-icon tone-${toneName(row.tone)}`}>
          <Icon name={recommendationIcon(row)} />
        </span>
        <h4>{row.title}</h4>
      </div>
      {row.detail != null ? <p>{row.detail}</p> : null}
      <PromotionPanel compact row={row} />
      <RecommendationActions
        canLaunch={canLaunch}
        onComplete={onComplete}
        pending={pending}
        row={row}
      />
    </article>
  );
}

function RecommendationActions({
  canLaunch,
  compact = false,
  onComplete,
  pending,
  row,
}: {
  canLaunch: boolean;
  compact?: boolean;
  onComplete: (row: Recommendation) => void;
  pending: string | null;
  row: Recommendation;
}) {
  return (
    <div className="recommendation-actions">
      {row.prompt != null || row.action != null || row.suggestion != null ? (
        <button
          className="secondary-button"
          disabled={pending === row.key}
          onClick={() => onComplete(row)}
          type="button"
        >
          <Icon name={canLaunch ? "bolt" : "check"} />
          {pending === row.key
            ? "Saving"
            : compact
              ? "Run"
              : canLaunch
                ? "Run"
                : "Copy setup prompt"}
        </button>
      ) : null}
      {row.url != null ? (
        <a href={row.url} rel="noreferrer" target="_blank">
          {row.link_label ?? "Docs"}
        </a>
      ) : null}
    </div>
  );
}

function PromotionPanel({ compact = false, row }: { compact?: boolean; row: Recommendation }) {
  if (!hasPromotion(row)) {
    return null;
  }
  return (
    <div className={`promotion-panel${compact ? " is-compact" : ""}`}>
      <span>Memory card</span>
      <dl>
        {row.memory_layer != null ? (
          <div>
            <dt>Layer</dt>
            <dd>{row.memory_layer}</dd>
          </div>
        ) : null}
        {row.promotion_target != null ? (
          <div>
            <dt>Promote to</dt>
            <dd>{row.promotion_target}</dd>
          </div>
        ) : null}
        {!compact && row.trigger != null ? (
          <div>
            <dt>Trigger</dt>
            <dd>{row.trigger}</dd>
          </div>
        ) : null}
        {!compact && row.success_metric != null ? (
          <div>
            <dt>Done when</dt>
            <dd>{row.success_metric}</dd>
          </div>
        ) : null}
      </dl>
    </div>
  );
}

function PromotionMeta({ row }: { row: Recommendation }) {
  if (!hasPromotion(row)) {
    return null;
  }
  return (
    <div className="promotion-meta">
      {row.memory_layer != null ? <span>{row.memory_layer}</span> : null}
      {row.promotion_target != null ? <span>{row.promotion_target}</span> : null}
    </div>
  );
}

function Icon({ name }: { name: IconName }) {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      {iconPath(name)}
    </svg>
  );
}

function iconPath(name: IconName) {
  switch (name) {
    case "anthropic":
      return <path d={ANTHROPIC_ICON_PATH} />;
    case "arrowLeft":
      return <path d="M10 19 3 12l7-7v4h11v6H10v4Z" />;
    case "beaker":
      return (
        <path d="M9 3h6v2l-1 1v4.3l5 7.9A2 2 0 0 1 17.3 21H6.7A2 2 0 0 1 5 18.2l5-7.9V6L9 5V3Zm1.8 9-4.1 6.5h10.6L13.2 12h-2.4Z" />
      );
    case "bolt":
      return <path d="m13 2-8 12h6l-1 8 9-13h-6l1-7Z" />;
    case "chart":
      return <path d="M4 20V10h4v10H4Zm6 0V4h4v16h-4Zm6 0v-7h4v7h-4Z" />;
    case "check":
      return <path d="m9.2 16.2-4-4L3.8 13.6l5.4 5.4L20.5 7.7 19.1 6.3 9.2 16.2Z" />;
    case "claude":
      return <path d={CLAUDE_ICON_PATH} />;
    case "clock":
      return <path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20Zm1 5h-2v6l5 3 1-1.7-4-2.3V7Z" />;
    case "cpu":
      return (
        <path d="M8 2h2v3h4V2h2v3h3v3h3v2h-3v4h3v2h-3v3h-3v3h-2v-3h-4v3H8v-3H5v-3H2v-2h3v-4H2V8h3V5h3V2Zm-1 5v10h10V7H7Zm3 3h4v4h-4v-4Z" />
      );
    case "desktop":
      return <path d="M3 4h18v12H3V4Zm7 14h4v2h4v2H6v-2h4v-2Z" />;
    case "download":
      return <path d="M11 3h2v9l3-3 1.4 1.4L12 15.8l-5.4-5.4L8 9l3 3V3ZM5 19h14v2H5v-2Z" />;
    case "file":
      return <path d="M6 2h8l4 4v16H6V2Zm7 1.5V7h3.5L13 3.5ZM8 11h8v2H8v-2Zm0 4h8v2H8v-2Z" />;
    case "folder":
      return <path d="M3 6h7l2 2h9v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6Z" />;
    case "inbox":
      return <path d="M5 4h14l3 9v7H2v-7l3-9Zm1.4 2-2 6H9l2 3h2l2-3h4.6l-2-6H6.4Z" />;
    case "lightbulb":
      return <path d="M12 2a7 7 0 0 0-4 12.7V17h8v-2.3A7 7 0 0 0 12 2Zm-4 17h8v2H8v-2Z" />;
    case "menu":
      return <path d="M4 6h16v2H4V6Zm0 5h16v2H4v-2Zm0 5h16v2H4v-2Z" />;
    case "messages":
      return <path d="M4 4h16v11H8l-4 4V4Zm4 4h8v2H8V8Zm0 3h6v2H8v-2Z" />;
    case "money":
      return (
        <path d="M3 6h18v12H3V6Zm2 3a3 3 0 0 0 3-1h8a3 3 0 0 0 3 3v2a3 3 0 0 0-3 3H8a3 3 0 0 0-3-3V9Zm7 6a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" />
      );
    case "moon":
      return <path d="M20 15.5A8.5 8.5 0 0 1 8.5 4 9 9 0 1 0 20 15.5Z" />;
    case "openai":
      return <path d={OPENAI_ICON_PATH} />;
    case "search":
      return (
        <path d="M10 3a7 7 0 1 0 4.2 12.6l4.6 4.6 1.4-1.4-4.6-4.6A7 7 0 0 0 10 3Zm0 2a5 5 0 1 1 0 10 5 5 0 0 1 0-10Z" />
      );
    case "sessions":
      return <path d="M4 5h16v4H4V5Zm0 6h16v4H4v-4Zm0 6h16v2H4v-2Z" />;
    case "settings":
      return (
        <path d="m12 2 2 3.5 4 .5-2.5 3 1 4-4.5-1.8L7.5 13l1-4L6 6l4-.5L12 2Zm0 7a3 3 0 1 0 .01 0H12Z" />
      );
    case "sun":
      return (
        <path d="M11 1h2v4h-2V1Zm0 18h2v4h-2v-4ZM1 11h4v2H1v-2Zm18 0h4v2h-4v-2ZM4.2 2.8 7 5.6 5.6 7 2.8 4.2l1.4-1.4Zm14.2 14.2 2.8 2.8-1.4 1.4-2.8-2.8 1.4-1.4ZM19.8 2.8l1.4 1.4L18.4 7 17 5.6l2.8-2.8ZM5.6 17 7 18.4l-2.8 2.8-1.4-1.4L5.6 17ZM12 7a5 5 0 1 1 0 10 5 5 0 0 1 0-10Z" />
      );
    case "tools":
      return <path d="m21 7-5 5-2-2 5-5a6 6 0 0 0-8 7L3 20l1 1 8-8a6 6 0 0 0 9-6Z" />;
    case "upload":
      return <path d="M11 21h2v-9l3 3 1.4-1.4L12 8.2l-5.4 5.4L8 15l3-3v9ZM5 3h14v2H5V3Z" />;
    case "x":
      return <path d="m6.4 5 12.6 12.6-1.4 1.4L5 6.4 6.4 5Zm11.2 0L19 6.4 6.4 19 5 17.6 17.6 5Z" />;
  }
}

function recommendationIcon(row: Recommendation): IconName {
  const icon = row.icon ?? "";
  if (icon.includes("cpu")) {
    return "cpu";
  }
  if (icon.includes("document") || icon.includes("book")) {
    return "file";
  }
  if (icon.includes("wrench")) {
    return "tools";
  }
  if (icon.includes("chart")) {
    return "chart";
  }
  return "lightbulb";
}

function groupByCategory(rows: Recommendation[]): [string, Recommendation[]][] {
  const groups = new Map<string, Recommendation[]>();
  for (const row of rows) {
    const key = row.category ?? "Recommended";
    groups.set(key, [...(groups.get(key) ?? []), row]);
  }
  return [...groups.entries()];
}

function hasPromotion(row: Recommendation): boolean {
  return [row.memory_layer, row.promotion_target, row.trigger, row.success_metric].some(isPresent);
}

function handoffPrompt(row: Recommendation): string {
  return [row.prompt ?? row.action ?? row.suggestion, promotionText(row)]
    .filter(isPresent)
    .join("\n\n");
}

function promotionText(row: Recommendation): string {
  return [
    `# ${row.title}`,
    `Key: ${row.key}`,
    field("Layer", row.memory_layer),
    field("Promote to", row.promotion_target),
    field("Trigger", row.trigger),
    field("Evidence", row.evidence),
    field("Action", row.action),
    field("Done when", row.success_metric),
  ]
    .filter(isPresent)
    .join("\n");
}

function field(label: string, value: string | null): string | null {
  return isPresent(value) ? `${label}: ${value}` : null;
}

function modelBrandIcon(model: string, tone: BadgeTone): IconName | null {
  if (tone === "openai") {
    return "openai";
  }
  if (tone === "claude") {
    return model.toLowerCase().includes("anthropic") && !model.toLowerCase().includes("claude")
      ? "anthropic"
      : "claude";
  }
  return null;
}

function brandTone(model: string | null | undefined): BadgeTone {
  const normalized = (model ?? "").toLowerCase();
  if (
    normalized.includes("claude") ||
    normalized.includes("anthropic") ||
    normalized.includes("opus") ||
    normalized.includes("sonnet") ||
    normalized.includes("haiku")
  ) {
    return "claude";
  }
  if (
    normalized.includes("gpt") ||
    normalized.includes("openai") ||
    normalized.includes("codex") ||
    normalized.startsWith("o1") ||
    normalized.startsWith("o3")
  ) {
    return "openai";
  }
  return "neutral";
}

function toneName(tone: string | null | undefined): BadgeTone {
  return tone === "success" ||
    tone === "warning" ||
    tone === "danger" ||
    tone === "info" ||
    tone === "accent"
    ? tone
    : "neutral";
}

function fileTotal(row: FileRow): number {
  return row.reads + row.edits + row.writes + row.deletes;
}

function isPresent(value: string | null | undefined): value is string {
  return value != null && value.trim() !== "";
}

function firstLine(value: string, maxLength: number): string {
  const line = value.trim().split("\n", 1)[0] ?? "";
  return line.length > maxLength ? `${line.slice(0, maxLength - 1)}...` : line;
}

function formatInt(value: number): string {
  return Math.round(value).toLocaleString();
}

function compact(value: number): string {
  const abs = Math.abs(value);
  if (abs >= 1_000_000) {
    return `${(value / 1_000_000).toFixed(1)}M`;
  }
  if (abs >= 1_000) {
    return `${(value / 1_000).toFixed(1)}K`;
  }
  return formatInt(value);
}

function money(value: number): string {
  return `$${value.toFixed(2)}`;
}

function relativeTime(value: string | null | undefined): string {
  if (value == null || value === "") {
    return "-";
  }
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) {
    return value;
  }
  const deltaSeconds = Math.round((Date.now() - timestamp) / 1000);
  const abs = Math.abs(deltaSeconds);
  const units: [Intl.RelativeTimeFormatUnit, number][] = [
    ["year", 31_536_000],
    ["month", 2_592_000],
    ["day", 86_400],
    ["hour", 3_600],
    ["minute", 60],
  ];
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  for (const [unit, seconds] of units) {
    if (abs >= seconds) {
      return formatter.format(Math.round(-deltaSeconds / seconds), unit);
    }
  }
  return "just now";
}

function capitalize(value: string): string {
  return `${value.slice(0, 1).toUpperCase()}${value.slice(1)}`;
}

function latestSessionDay(sessions: SessionSummary[]): string | null {
  const latest = sessions.find((session) => session.started_at != null)?.started_at;
  if (latest == null) {
    return null;
  }
  const date = new Date(latest);
  if (Number.isNaN(date.getTime())) {
    return latest;
  }
  return date.toLocaleDateString(undefined, { month: "short", day: "2-digit" });
}

function InsightsView({
  rows,
  settingsInfo,
  onMarked,
}: {
  rows: Recommendation[];
  settingsInfo: SettingsInfo | null;
  onMarked: () => void;
}) {
  const [pending, setPending] = useState<string | null>(null);
  const signals = rows
    .filter((row) => row.kind === "signal")
    .slice()
    .sort((left, right) => right.score - left.score);
  const [hero, ...rest] = signals;
  const catalogGroups = groupByCategory(rows.filter((row) => row.kind === "catalog"));
  const completeRecommendation = (row: Recommendation) => {
    if (row.prompt != null && row.prompt.trim() !== "" && settingsInfo?.can_launch === true) {
      setPending(row.key);
      void getJson<{ ok: boolean }>("/api/launch/agent", {
        method: "POST",
        body: JSON.stringify({
          agent: settingsInfo.settings.agent,
          prompt: handoffPrompt(row),
          key: row.key,
        }),
      })
        .then(() => onMarked())
        .finally(() => setPending(null));
      return;
    }
    if (row.prompt != null && row.prompt.trim() !== "") {
      void navigator.clipboard?.writeText(handoffPrompt(row));
      return;
    }
    setPending(row.key);
    void getJson<{ ok: boolean }>("/api/recommendations/mark", {
      method: "POST",
      body: JSON.stringify({ key: row.key, source: "ui" }),
    })
      .then(onMarked)
      .finally(() => setPending(null));
  };

  return (
    <div className="view-stack insights-stack">
      <header className="page-heading">
        <h1>Insights</h1>
        <p>What could make your coding agents better, drawn from your archive.</p>
      </header>

      <section className="view-stack">
        <div className="section-title-row">
          <div>
            <h2>Promotion candidates</h2>
            <p>Data-backed lessons ranked by impact</p>
          </div>
          {signals.length > 0 ? <span>{formatInt(signals.length)} active</span> : null}
        </div>

        {signals.length === 0 ? (
          <EmptyState
            icon="lightbulb"
            message="Sync more sessions to surface patterns."
            title="No signals yet"
          />
        ) : null}

        {hero != null ? (
          <RecommendationHero
            pending={pending}
            row={hero}
            onComplete={completeRecommendation}
            canLaunch={settingsInfo?.can_launch === true}
          />
        ) : null}

        {rest.length > 0 ? (
          <div className="signal-list">
            {rest.map((row) => (
              <RecommendationRow
                key={row.key}
                pending={pending}
                row={row}
                onComplete={completeRecommendation}
                canLaunch={settingsInfo?.can_launch === true}
              />
            ))}
          </div>
        ) : null}
      </section>

      {catalogGroups.length > 0 ? (
        <section className="view-stack">
          <div className="section-title-row">
            <div>
              <h2>Recommended for coding agents</h2>
              <p>Set these up to make your agents faster and more consistent</p>
            </div>
          </div>
          {catalogGroups.map(([category, items], groupIndex) => (
            <div className="catalog-group" key={category}>
              <h3>{category}</h3>
              <div className="catalog-grid">
                {items.map((row, index) => (
                  <RecommendationCard
                    featured={groupIndex === 0 && index === 0}
                    key={row.key}
                    pending={pending}
                    row={row}
                    onComplete={completeRecommendation}
                    canLaunch={settingsInfo?.can_launch === true}
                  />
                ))}
              </div>
            </div>
          ))}
        </section>
      ) : null}
    </div>
  );
}

function ToolsView({ data }: { data: DashboardData }) {
  return (
    <div className="view-stack">
      <header className="page-heading inline-heading">
        <div>
          <h1>Tools &amp; MCP</h1>
          <p>Tool and MCP-server call volume, scoped to your archive.</p>
        </div>
        <span>{dateRange(data.dateBounds)}</span>
      </header>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>MCP servers</h2>
            <p>Model Context Protocol servers and their call volume</p>
          </div>
        </div>
        {data.mcp.length === 0 ? (
          <EmptyState
            icon="cpu"
            message="No MCP tool calls in this range."
            title="No MCP servers"
          />
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>Server</th>
                  <th className="numeric">Tools</th>
                  <th className="numeric">Calls</th>
                  <th className="numeric">Errors</th>
                </tr>
              </thead>
              <tbody>
                {data.mcp.map((row) => (
                  <tr key={row.mcp_server}>
                    <td className="mono icon-cell">
                      <Icon name="cpu" />
                      {row.mcp_server}
                    </td>
                    <td className="numeric muted">{formatInt(row.tools)}</td>
                    <td className="numeric">{formatInt(row.calls)}</td>
                    <td className="numeric">
                      {row.errors > 0 ? (
                        <Badge tone="danger">{formatInt(row.errors)}</Badge>
                      ) : (
                        <span className="faint">0</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>Tools</h2>
            <p>Built-in vs MCP, most-called first</p>
          </div>
        </div>
        <div className="table-scroll">
          <table>
            <thead>
              <tr>
                <th>Tool</th>
                <th>Kind</th>
                <th>Server</th>
                <th className="numeric">Calls</th>
                <th className="numeric">Errors</th>
              </tr>
            </thead>
            <tbody>
              {data.tools.length === 0 ? (
                <tr>
                  <td colSpan={5}>No tool calls.</td>
                </tr>
              ) : null}
              {data.tools.map((row) => (
                <tr key={`${row.tool_name}-${row.tool_kind}-${row.mcp_server ?? ""}`}>
                  <td className="mono">{row.tool_name}</td>
                  <td>
                    <Badge tone={row.tool_kind === "mcp" ? "accent" : "neutral"}>
                      {row.tool_kind === "mcp" ? "MCP" : "built-in"}
                    </Badge>
                  </td>
                  <td className="mono muted">
                    {row.mcp_server != null && row.mcp_server !== "" ? (
                      <span className="icon-cell">
                        <Icon name="cpu" />
                        {row.mcp_server}
                      </span>
                    ) : (
                      <span className="faint">-</span>
                    )}
                  </td>
                  <td className="numeric">{formatInt(row.calls)}</td>
                  <td className="numeric">
                    {row.errors > 0 ? (
                      <Badge tone="danger">{formatInt(row.errors)}</Badge>
                    ) : (
                      <span className="faint">0</span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}

function FilesView({ rows }: { rows: FileRow[] }) {
  const [group, setGroup] = useState<"path" | "ext">("path");
  const [op, setOp] = useState<"read" | "edit" | "write" | "delete" | null>(null);
  const [fileRows, setFileRows] = useState(rows);

  useEffect(() => {
    const opParam = op == null ? "" : `&op=${op}`;
    void getJson<FileRow[]>(`/api/files?group=${group}&limit=100${opParam}`).then(setFileRows);
  }, [group, op]);

  return (
    <div className="view-stack">
      <header className="page-heading inline-heading">
        <div>
          <h1>File hotspots</h1>
          <p>
            What agents touch most. Heavy re-reads with few edits are AGENTS.md / skill candidates;
            heavy edits are churn.
          </p>
        </div>
      </header>

      <div className="segment-row">
        <fieldset className="segmented-control">
          <legend>Group by</legend>
          <button aria-pressed={group === "path"} onClick={() => setGroup("path")} type="button">
            Files
          </button>
          <button aria-pressed={group === "ext"} onClick={() => setGroup("ext")} type="button">
            Languages
          </button>
        </fieldset>
        <fieldset className="segmented-control">
          <legend>Operation</legend>
          <button aria-pressed={op == null} onClick={() => setOp(null)} type="button">
            All ops
          </button>
          {(["read", "edit", "write", "delete"] as const).map((name) => (
            <button aria-pressed={op === name} key={name} onClick={() => setOp(name)} type="button">
              {capitalize(name)}
            </button>
          ))}
        </fieldset>
      </div>

      <section className="panel">
        <div className="panel-heading">
          <div>
            <h2>{group === "ext" ? "Languages" : "Hotspots"}</h2>
            <p>Per-operation counts from tool-call evidence, ordered by activity</p>
          </div>
        </div>
        {fileRows.length === 0 ? (
          <EmptyState
            icon="file"
            message="Hotspots appear once the archive has file activity."
            title="No file activity"
          />
        ) : (
          <div className="table-scroll">
            <table>
              <thead>
                <tr>
                  <th>{group === "ext" ? "Extension" : "File"}</th>
                  {group === "path" ? <th>Project</th> : null}
                  <th className="numeric">Reads</th>
                  <th className="numeric">Edits</th>
                  <th className="numeric">Writes</th>
                  <th className="numeric">Deletes</th>
                  <th className="numeric">Sessions</th>
                  <th className="numeric">Total</th>
                  <th className="numeric">Last touched</th>
                </tr>
              </thead>
              <tbody>
                {fileRows.map((row) => (
                  <tr key={`${group}-${row.project ?? ""}-${row.key}`}>
                    <td className="mono truncate-cell">
                      <a href={`/search?q=${encodeURIComponent(`"${row.key}"`)}`}>{row.key}</a>
                    </td>
                    {group === "path" ? (
                      <td className="muted" title={row.project ?? ""}>
                        {basename(row.project)}
                      </td>
                    ) : null}
                    <td className="numeric muted">{formatInt(row.reads)}</td>
                    <td className="numeric muted">{formatInt(row.edits)}</td>
                    <td className="numeric muted">{formatInt(row.writes)}</td>
                    <td className="numeric muted">{formatInt(row.deletes)}</td>
                    <td className="numeric muted">{formatInt(row.sessions)}</td>
                    <td className="numeric">{formatInt(fileTotal(row))}</td>
                    <td className="numeric muted">{relativeTime(row.last_touched_at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}

function SettingsView({
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
    <div className="settings-page">
      <header className="page-heading">
        <h1>Settings</h1>
        <p>
          How decant opens things on your machine. We start from what we detect and remember your
          choices.
        </p>
      </header>

      <section className="panel">
        <div className="settings-form">
          <SettingSelect
            help="The agent the Run button opens first across Insights."
            label="Preferred agent"
            options={settingsInfo?.options.agents ?? []}
            value={settings?.agent ?? "claude"}
            onChange={(agent) => save({ agent })}
          />
          <SettingSelect
            help="Where a session opens when you run an agent."
            label="Terminal"
            options={settingsInfo?.options.terminals ?? []}
            value={settings?.terminal ?? "terminal"}
            onChange={(terminal) => save({ terminal })}
          />
          <SettingSelect
            help="Which editor Open in editor uses for a session's project."
            label="Editor"
            options={settingsInfo?.options.ides ?? []}
            value={settings?.ide ?? "vscode"}
            onChange={(ide) => save({ ide })}
          />
        </div>
      </section>
      {settingsInfo?.can_launch !== true ? (
        <p className="settings-note">
          Opening terminals and editors works on macOS only. Your choices are still saved.
        </p>
      ) : null}
    </div>
  );
}

function SettingSelect({
  help,
  label,
  options,
  value,
  onChange,
}: {
  help: string;
  label: string;
  options: [string, string][];
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="setting-select">
      <span>
        <strong>{label}</strong>
        <small>{help}</small>
      </span>
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
  const [detail, setDetail] = useState<SessionDetailData | null>(null);

  useEffect(() => {
    if (Number.isFinite(id)) {
      void getJson<SessionDetailData>(`/api/sessions/${id}`).then(setDetail);
    }
  }, [id]);

  if (detail == null) {
    return <div className="notice">Loading session...</div>;
  }

  const messages = renderableMessages(detail.messages);
  const toc = threadToc(messages);
  const stats = threadStats(detail.summary, messages, toc);

  return (
    <div className="session-detail">
      <header className="thread-header">
        <div className="thread-header-inner">
          <h1>{detail.summary.title ?? "Untitled session"}</h1>
          <div className="thread-badges">
            <ToolBadge tool={detail.summary.tool} />
            <ModelBadge model={detail.summary.model} />
            {detail.summary.project_path != null ? (
              <span className="project-chip" title={detail.summary.project_path}>
                <Icon name="folder" />
                {detail.summary.project_path}
              </span>
            ) : null}
          </div>
          <div className="thread-stats">
            <span>
              <strong>{formatInt(stats.turns)}</strong> turns
            </span>
            <span>
              <strong>{formatInt(stats.replies)}</strong> replies
            </span>
            <span>
              <strong>{formatInt(stats.toolCalls)}</strong> tool calls
            </span>
            <span>
              <strong>{compact(stats.tokens)}</strong> tokens
            </span>
            <span>
              <strong>{money(detail.summary.estimated_cost_usd)}</strong>
            </span>
          </div>
        </div>
      </header>

      <a className="back-link" href="/" onClick={(event) => navigate(event, "/")}>
        <Icon name="arrowLeft" />
        Sessions
      </a>

      <div className="transcript-layout">
        <aside className="toc">
          <div className="toc-inner">
            <div className="toc-title">In this thread</div>
            {toc.length === 0 ? <p>No prompts to list</p> : null}
            {toc.map((item) => (
              <a href={`#turn-${item.index}`} key={item.index}>
                <span />
                <span>{item.label}</span>
                {item.tools > 0 ? <b>{item.tools}</b> : null}
              </a>
            ))}
          </div>
        </aside>

        <div className="transcript-column">
          {messages.map((message, index) => (
            <article className="turn" id={`turn-${index}`} key={messageKey(message)}>
              <Badge mono tone={roleTone(message.role)}>
                {message.role}
              </Badge>
              <div className="turn-body">
                {message.blocks.map((block) => (
                  <TranscriptBlock block={block} key={`${block.ordinal}-${block.block_type}`} />
                ))}
              </div>
            </article>
          ))}
        </div>
      </div>
    </div>
  );
}

type SessionDetailData = {
  summary: SessionSummary;
  messages: {
    role: string;
    timestamp: string | null;
    model: string | null;
    blocks: TranscriptBlockData[];
  }[];
};

type TranscriptBlockData = {
  ordinal: number;
  block_type: string;
  text: string | null;
  tool_name: string | null;
  tool_input: string | null;
  tool_result: string | null;
};

function TranscriptBlock({ block }: { block: TranscriptBlockData }) {
  if (block.block_type === "tool_use") {
    return (
      <div className="tool-call">
        <div>
          <Icon name="bolt" />
          <span>{block.tool_name ?? "tool_use"}</span>
          <small>tool call</small>
        </div>
        {isPresent(block.tool_input) ? (
          <details open={block.tool_input.length <= 240}>
            <summary>arguments</summary>
            <pre>{prettyJson(block.tool_input)}</pre>
          </details>
        ) : null}
      </div>
    );
  }
  if (block.block_type === "tool_result") {
    if (!isPresent(block.tool_result)) {
      return null;
    }
    return (
      <details className="tool-result">
        <summary>result</summary>
        <pre>{block.tool_result}</pre>
      </details>
    );
  }
  if (block.block_type === "thinking") {
    if (!isPresent(block.text)) {
      return null;
    }
    return (
      <details className="thinking-block">
        <summary>Thinking</summary>
        <p>{block.text}</p>
      </details>
    );
  }
  if (!isPresent(block.text)) {
    return null;
  }
  return <p className="text-block">{block.text}</p>;
}

function renderableMessages(
  messages: SessionDetailData["messages"],
): SessionDetailData["messages"] {
  return messages.filter((message) =>
    message.blocks.some((block) => {
      if (block.block_type === "text" || block.block_type === "thinking") {
        return isPresent(block.text);
      }
      return block.block_type === "tool_use" || block.block_type === "tool_result";
    }),
  );
}

function threadToc(
  messages: SessionDetailData["messages"],
): { index: number; label: string; tools: number }[] {
  return messages.flatMap((message, index) => {
    if (message.role !== "user") {
      return [];
    }
    const label =
      message.blocks.find((block) => block.block_type === "text" && isPresent(block.text))?.text ??
      "";
    if (label.trim() === "") {
      return [];
    }
    return [
      {
        index,
        label: firstLine(label, 70),
        tools: message.blocks.filter((block) => block.block_type === "tool_use").length,
      },
    ];
  });
}

function threadStats(
  summary: SessionSummary,
  messages: SessionDetailData["messages"],
  toc: { index: number; label: string; tools: number }[],
) {
  return {
    turns: toc.length,
    replies: messages.filter((message) => message.role === "assistant").length,
    toolCalls: messages.reduce(
      (sum, message) =>
        sum + message.blocks.filter((block) => block.block_type === "tool_use").length,
      0,
    ),
    tokens: summary.total_input_tokens + summary.total_output_tokens,
  };
}

function messageKey(message: SessionDetailData["messages"][number]): string {
  return [
    message.role,
    message.timestamp ?? "no-time",
    message.model ?? "no-model",
    message.blocks.map((block) => `${block.ordinal}:${block.block_type}`).join(","),
  ].join("|");
}

function roleTone(role: string): BadgeTone {
  return role === "assistant" ? "accent" : role === "tool" ? "info" : "neutral";
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
  const pathname = pathOnly(path);
  if (pathname.startsWith("/sessions/")) {
    return "Sessions";
  }
  if (pathname === "/settings") {
    return "Settings";
  }
  const match = navItems.find((item) => item.href === pathname);
  return match?.label ?? "Sessions";
}

function activeRouteKey(path: string): string {
  const pathname = pathOnly(path);
  if (pathname.startsWith("/sessions/")) {
    return "sessions";
  }
  if (pathname === "/settings") {
    return "settings";
  }
  return navItems.find((item) => item.href === pathname)?.key ?? "sessions";
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

function locationPath(): string {
  return `${window.location.pathname}${window.location.search}`;
}

function pathOnly(path: string): string {
  return path.split("?", 1)[0] ?? "/";
}

function navigate(
  event: MouseEvent<HTMLAnchorElement>,
  href: string,
  setPath?: (path: string) => void,
) {
  event.preventDefault();
  window.history.pushState(null, "", href);
  const next = locationPath();
  if (setPath != null) {
    setPath(next);
  } else {
    window.dispatchEvent(new PopStateEvent("popstate"));
  }
}

const root = document.getElementById("root");
if (root == null) {
  throw new Error("missing #root");
}
createRoot(root).render(<App />);
