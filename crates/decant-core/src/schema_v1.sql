CREATE TABLE IF NOT EXISTS project (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  name TEXT,
  first_seen_at TEXT,
  last_seen_at TEXT,
  is_worktree INTEGER NOT NULL DEFAULT 0,
  root_path TEXT,
  worktree_label TEXT,
  worktree_tool TEXT,
  root_source TEXT
);

CREATE TABLE IF NOT EXISTS session (
  id INTEGER PRIMARY KEY,
  tool TEXT NOT NULL,
  source_session_id TEXT NOT NULL,
  project_id INTEGER REFERENCES project(id),
  title TEXT,
  cwd TEXT,
  git_branch TEXT,
  model TEXT,
  cli_version TEXT,
  started_at TEXT,
  ended_at TEXT,
  message_count INTEGER NOT NULL DEFAULT 0,
  total_input_tokens INTEGER NOT NULL DEFAULT 0,
  total_output_tokens INTEGER NOT NULL DEFAULT 0,
  total_cache_read_tokens INTEGER NOT NULL DEFAULT 0,
  total_cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
  estimated_cost_usd REAL NOT NULL DEFAULT 0,
  is_archived INTEGER NOT NULL DEFAULT 0,
  source_path TEXT,
  raw_meta TEXT,
  ingested_at TEXT,
  source_mtime INTEGER,
  source_size INTEGER,
  source_hash TEXT,
  turn_count INTEGER NOT NULL DEFAULT 0,
  error_count INTEGER NOT NULL DEFAULT 0,
  interruption_count INTEGER NOT NULL DEFAULT 0,
  compaction_count INTEGER NOT NULL DEFAULT 0,
  sidechain_message_count INTEGER NOT NULL DEFAULT 0,
  agent_spawn_count INTEGER NOT NULL DEFAULT 0,
  skill_count INTEGER NOT NULL DEFAULT 0,
  command_count INTEGER NOT NULL DEFAULT 0,
  thinking_block_count INTEGER NOT NULL DEFAULT 0,
  thinking_chars INTEGER NOT NULL DEFAULT 0,
  active_seconds INTEGER NOT NULL DEFAULT 0,
  outcome TEXT,
  work_type TEXT,
  UNIQUE(tool, source_session_id)
);

CREATE TABLE IF NOT EXISTS message (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  source_uuid TEXT,
  parent_source_uuid TEXT,
  parent_id INTEGER REFERENCES message(id),
  role TEXT,
  model TEXT,
  stop_reason TEXT,
  timestamp TEXT,
  input_tokens INTEGER,
  output_tokens INTEGER,
  cache_read_tokens INTEGER,
  cache_creation_tokens INTEGER,
  raw TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS block (
  id INTEGER PRIMARY KEY,
  message_id INTEGER NOT NULL REFERENCES message(id) ON DELETE CASCADE,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  type TEXT,
  text TEXT,
  tool_name TEXT,
  tool_use_id TEXT,
  tool_input TEXT,
  tool_result TEXT
);

CREATE TABLE IF NOT EXISTS tool_call (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  message_id INTEGER REFERENCES message(id) ON DELETE CASCADE,
  call_block_id INTEGER REFERENCES block(id),
  result_block_id INTEGER REFERENCES block(id),
  tool_kind TEXT,
  tool_name TEXT,
  mcp_server TEXT,
  tool_base_name TEXT,
  tool_use_id TEXT,
  input TEXT,
  is_error INTEGER,
  output_preview TEXT,
  output_bytes INTEGER,
  duration_ms INTEGER,
  ordinal INTEGER,
  timestamp TEXT
);

-- File-level evidence extracted deterministically from tool calls at ingest
-- (Claude Read/Edit/Write/NotebookEdit file_path; Codex apply_patch headers).
-- rel_path is the project-relative aggregation key; NULL when underivable.
CREATE TABLE IF NOT EXISTS file_ref (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  message_id INTEGER REFERENCES message(id) ON DELETE CASCADE,
  path TEXT NOT NULL,
  rel_path TEXT,
  ext TEXT,
  operation TEXT NOT NULL,
  timestamp TEXT
);

CREATE TABLE IF NOT EXISTS ingest_source (
  path TEXT PRIMARY KEY,
  tool TEXT,
  size INTEGER,
  mtime INTEGER,
  hash TEXT,
  session_id INTEGER REFERENCES session(id) ON DELETE SET NULL,
  line_count INTEGER,
  status TEXT,
  error TEXT,
  last_ingested_at TEXT
);

CREATE TABLE IF NOT EXISTS ingest_issue (
  id INTEGER PRIMARY KEY,
  source_path TEXT,
  line_no INTEGER,
  error TEXT,
  raw_line TEXT,
  created_at TEXT
);

CREATE TABLE IF NOT EXISTS model_pricing (
  model TEXT PRIMARY KEY,
  input_per_mtok REAL,
  output_per_mtok REAL,
  cache_read_per_mtok REAL,
  cache_write_per_mtok REAL,
  source TEXT,
  updated_at TEXT
);

-- Recommendations: data-derived signals + the evergreen catalog, materialized
-- with state by `recommendations::regenerate` at each sync. Rust-owns this
-- table (the daemon writes; the web app is a read-only HTTP client). Stable
-- `key`s let state (status/source/note/implemented_at) survive re-sync.
CREATE TABLE IF NOT EXISTS recommendation (
  key            TEXT PRIMARY KEY,
  kind           TEXT NOT NULL,      -- 'signal' | 'catalog'
  category       TEXT,
  title          TEXT NOT NULL,
  detail         TEXT,
  suggestion     TEXT,
  prompt         TEXT,
  url            TEXT,
  link_label     TEXT,
  icon           TEXT,
  tone           TEXT,
  score          REAL,
  status         TEXT NOT NULL DEFAULT 'open',   -- 'open' | 'implemented'
  status_source  TEXT,               -- 'agent' | 'activity' | 'manual'
  note           TEXT,
  first_seen_at  TEXT,
  updated_at     TEXT,
  implemented_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_session_project ON session(project_id);
CREATE INDEX IF NOT EXISTS idx_session_tool ON session(tool);
CREATE INDEX IF NOT EXISTS idx_session_started ON session(started_at);
CREATE INDEX IF NOT EXISTS idx_session_model ON session(model);
CREATE INDEX IF NOT EXISTS idx_message_session ON message(session_id, seq);
CREATE INDEX IF NOT EXISTS idx_block_session ON block(session_id);
CREATE INDEX IF NOT EXISTS idx_block_message ON block(message_id, ordinal);
CREATE INDEX IF NOT EXISTS idx_block_type ON block(type);
CREATE INDEX IF NOT EXISTS idx_block_tool ON block(tool_name);
CREATE INDEX IF NOT EXISTS idx_toolcall_session ON tool_call(session_id);
CREATE INDEX IF NOT EXISTS idx_toolcall_kind ON tool_call(tool_kind);
CREATE INDEX IF NOT EXISTS idx_toolcall_server ON tool_call(mcp_server);
CREATE INDEX IF NOT EXISTS idx_toolcall_name ON tool_call(tool_name);
CREATE INDEX IF NOT EXISTS idx_fileref_session ON file_ref(session_id);
CREATE INDEX IF NOT EXISTS idx_fileref_path ON file_ref(rel_path, operation);

CREATE VIRTUAL TABLE IF NOT EXISTS block_fts USING fts5(
  text, tool_name, tool_input,
  content='block', content_rowid='id'
);

CREATE TRIGGER IF NOT EXISTS block_ai AFTER INSERT ON block BEGIN
  INSERT INTO block_fts(rowid, text, tool_name, tool_input)
  VALUES (new.id, new.text, new.tool_name, new.tool_input);
END;
CREATE TRIGGER IF NOT EXISTS block_ad AFTER DELETE ON block BEGIN
  INSERT INTO block_fts(block_fts, rowid, text, tool_name, tool_input)
  VALUES ('delete', old.id, old.text, old.tool_name, old.tool_input);
END;
CREATE TRIGGER IF NOT EXISTS block_au AFTER UPDATE ON block BEGIN
  INSERT INTO block_fts(block_fts, rowid, text, tool_name, tool_input)
  VALUES ('delete', old.id, old.text, old.tool_name, old.tool_input);
  INSERT INTO block_fts(rowid, text, tool_name, tool_input)
  VALUES (new.id, new.text, new.tool_name, new.tool_input);
END;
