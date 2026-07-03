#!/usr/bin/env bun
import { mkdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { Command, InvalidArgumentError } from "commander";
import { type Config, type ConfigOverrides, resolveConfig } from "./config.ts";
import { openDb } from "./db.ts";
import type { Operation } from "./enrich.ts";
import { toMarkdown } from "./export.ts";
import { sync as ingestSync } from "./ingest.ts";
import { getSession, listProjects, listSessions, search } from "./query.ts";
import {
  byDimension,
  fileHotspots,
  mcpUsage,
  parseDimension,
  parseFileGroup,
  toolUsage,
  totals,
} from "./stats.ts";

export interface CliResult {
  code: number;
  stdout: string;
  stderr: string;
}

export interface CliRunOptions {
  env?: Record<string, string | undefined>;
  homeDir?: string | null;
}

interface GlobalOptions {
  db?: string;
  json?: boolean;
  format?: string;
  quiet?: boolean;
  sync?: boolean;
}

interface Io {
  stdout: string;
  stderr: string;
  writeOut(value: string): void;
  writeErr(value: string): void;
}

interface Archive {
  db: ReturnType<typeof openDb>;
  config: Config;
}

type CliAction = () => number | undefined;

interface DbInfo {
  path: string;
  size_bytes: number;
  schema_version: number;
  sessions: number;
  messages: number;
  tool_calls: number;
}

export async function runCli(argv: string[], options: CliRunOptions = {}): Promise<CliResult> {
  const io: Io = {
    stdout: "",
    stderr: "",
    writeOut(value) {
      this.stdout += value;
    },
    writeErr(value) {
      this.stderr += value;
    },
  };
  let code = 0;
  const setCode = (value: number): void => {
    if (code === 0 || value !== 0) {
      code = value;
    }
  };
  const run = (action: CliAction): void => {
    try {
      setCode(action() ?? 0);
    } catch (error) {
      io.writeErr(`error: ${error instanceof Error ? error.message : String(error)}\n`);
      setCode(1);
    }
  };

  const program = new Command();
  program
    .name("decant")
    .description("extract, browse, and search Claude Code and Codex sessions")
    .exitOverride()
    .configureOutput({
      writeOut: (value) => io.writeOut(value),
      writeErr: (value) => io.writeErr(value),
    })
    .option("--db <path>", "path to the decant SQLite database")
    .option("--json", "emit machine-readable JSON")
    .option("--format <format>", "output format")
    .option("-q, --quiet", "suppress non-essential output")
    .option("--no-color", "disable ANSI color")
    .option("--no-sync", "skip sync-on-read for read commands");

  const globals = (): GlobalOptions => program.opts<GlobalOptions>();
  const resolve = (overrides: Partial<ConfigOverrides> = {}): Config =>
    resolveConfig({
      dbPath: globals().db,
      env: options.env,
      homeDir: options.homeDir,
      ...overrides,
    });
  const output = (value: unknown, renderHuman: () => string): void => {
    if (isJson(globals())) {
      io.writeOut(`${JSON.stringify(value, null, 2)}\n`);
    } else if (!globals().quiet) {
      io.writeOut(renderHuman());
    }
  };
  const readArchive = (overrides: Partial<ConfigOverrides> = {}): Archive => {
    const archive = openArchive(resolve(overrides));
    if (shouldSync(globals(), options.env)) {
      ingestSync(archive.db, archive.config);
    }
    return archive;
  };

  const runSync = (commandOptions: { claudeDir?: string; codexDir?: string }): number => {
    const archive = openArchive(
      resolve({ claudeDir: commandOptions.claudeDir, codexDir: commandOptions.codexDir }),
    );
    try {
      const report = ingestSync(archive.db, archive.config);
      const jsonReport = {
        scanned: report.scanned,
        ingested: report.ingested,
        skipped: report.skipped,
        issues: report.issues,
        failed: report.failed,
      };
      if (isJson(globals())) {
        io.writeOut(`${JSON.stringify(jsonReport, null, 2)}\n`);
      } else if (!globals().quiet) {
        io.writeErr(
          `synced: ${report.scanned} scanned, ${report.ingested} ingested, ` +
            `${report.skipped} skipped, ${report.issues} issues, ${report.failed} failed\n`,
        );
      }
      return report.issues > 0 ? 3 : 0;
    } finally {
      archive.db.close();
    }
  };

  program
    .command("sync")
    .description("scan session directories and upsert new or changed sessions")
    .option("--claude-dir <dir>", "override the Claude projects directory")
    .option("--codex-dir <dir>", "override the Codex home directory")
    .action((commandOptions: { claudeDir?: string; codexDir?: string }) =>
      run(() => runSync(commandOptions)),
    );

  const addLs = (command: Command): void => {
    command
      .description("list sessions")
      .option("--tool <tool>", "only this tool")
      .option("--limit <n>", "max rows", parseInteger, 50)
      .action((commandOptions: { tool?: string; limit?: number }) =>
        run(() => {
          const archive = readArchive();
          try {
            const rows = listSessions(archive.db, {
              tool: commandOptions.tool,
              limit: commandOptions.limit,
            });
            output(rows, () =>
              globals().quiet
                ? `${rows.map((row) => row.id).join("\n")}${rows.length > 0 ? "\n" : ""}`
                : `${rows.map((row) => `${row.id}\t${row.tool}\t${row.title ?? ""}`).join("\n")}\n`,
            );
          } finally {
            archive.db.close();
          }
        }),
      );
  };

  const addShow = (command: Command): void => {
    command
      .description("render a full transcript")
      .argument("<id>", "session id", parseInteger)
      .action((id: number) =>
        run(() => {
          const archive = readArchive();
          try {
            const detail = getSession(archive.db, id);
            if (detail == null) {
              io.writeErr(`error: no session with id ${id}\n`);
              return 1;
            }
            output(detail, () => toMarkdown(detail));
          } finally {
            archive.db.close();
          }
        }),
      );
  };

  const session = program.command("session").description("inspect sessions");
  addLs(session.command("ls"));
  addShow(session.command("show"));
  addLs(program.command("ls"));
  addShow(program.command("show"));

  const project = program.command("project").description("inspect projects");
  project
    .command("ls")
    .description("list projects with session counts and cost")
    .action(() =>
      run(() => {
        const archive = readArchive();
        try {
          const rows = listProjects(archive.db);
          output(rows, () =>
            rows
              .map(
                (row) =>
                  `${row.id}\t${row.path}\t${row.sessions}\t` +
                  `${row.estimated_cost_usd.toFixed(2)}\t${row.last_seen_at ?? ""}`,
              )
              .join("\n")
              .concat(rows.length > 0 ? "\n" : ""),
          );
        } finally {
          archive.db.close();
        }
      }),
    );

  const dbCommand = program.command("db").description("inspect and maintain the archive");
  dbCommand
    .command("info")
    .description("show DB path, size, schema version, and row counts")
    .action(() =>
      run(() => {
        const archive = openArchive(resolve());
        try {
          const row = dbInfo(archive);
          output(
            row,
            () =>
              `path:       ${row.path}\n` +
              `size_bytes: ${row.size_bytes}\n` +
              `schema:     v${row.schema_version}\n` +
              `sessions:   ${row.sessions}\n` +
              `messages:   ${row.messages}\n` +
              `tool_calls: ${row.tool_calls}\n`,
          );
        } finally {
          archive.db.close();
        }
      }),
    );
  dbCommand
    .command("migrate")
    .description("apply schema migrations explicitly")
    .action(() =>
      run(() => {
        const archive = openArchive(resolve());
        try {
          io.writeErr(`schema up to date at ${archive.config.dbPath}\n`);
        } finally {
          archive.db.close();
        }
      }),
    );
  dbCommand
    .command("vacuum")
    .description("reclaim free space")
    .action(() =>
      run(() => {
        const archive = openArchive(resolve());
        try {
          archive.db.exec("VACUUM;");
          io.writeErr(`vacuumed ${archive.config.dbPath}\n`);
        } finally {
          archive.db.close();
        }
      }),
    );

  program
    .command("completion")
    .description("generate a shell completion script")
    .argument("<shell>", "bash | zsh | fish | powershell | elvish")
    .action((shell: string) =>
      run(() => {
        const script = renderCompletion(shell);
        if (script == null) {
          io.writeErr(
            `error: unknown completion shell ${JSON.stringify(shell)} ` +
              "(expected: bash | zsh | fish | powershell | elvish)\n",
          );
          return 2;
        }
        io.writeOut(script);
      }),
    );

  program
    .command("search")
    .description("full-text search across all sessions")
    .argument("<query>", "FTS query")
    .option("--limit <n>", "max rows", parseInteger, 30)
    .action((query: string, commandOptions: { limit?: number }) =>
      run(() => {
        const archive = readArchive();
        try {
          const rows = search(archive.db, query, commandOptions.limit ?? 30);
          output(rows, () => rows.map((row) => `${row.session_id}\t${row.snippet}`).join("\n"));
        } finally {
          archive.db.close();
        }
      }),
    );

  program
    .command("stats")
    .description("usage and cost rollups")
    .option("--by <dimension>", "tool | model | project | day")
    .action((commandOptions: { by?: string }) =>
      run(() => {
        const archive = readArchive();
        try {
          if (commandOptions.by != null) {
            const dimension = parseDimension(commandOptions.by);
            if (dimension == null) {
              io.writeErr(
                `error: unknown --by value ${JSON.stringify(commandOptions.by)} ` +
                  "(expected: tool | model | project | day)\n",
              );
              return 2;
            }
            const rows = byDimension(archive.db, dimension);
            output(rows, () => rows.map((row) => `${row.key}\t${row.sessions}`).join("\n"));
          } else {
            const row = totals(archive.db);
            output(row, () => `sessions:   ${row.sessions}\nmessages:   ${row.messages}\n`);
          }
        } finally {
          archive.db.close();
        }
      }),
    );

  program
    .command("files")
    .description("file hotspots")
    .option("--group <group>", "path | ext", "path")
    .option("--op <op>", "read | edit | write | delete")
    .option("--limit <n>", "max rows", parseInteger, 25)
    .action((commandOptions: { group: string; op?: string; limit?: number }) =>
      run(() => {
        const group = parseFileGroup(commandOptions.group);
        if (group == null) {
          io.writeErr(
            `error: unknown --group value ${JSON.stringify(commandOptions.group)} ` +
              "(expected: path | ext)\n",
          );
          return 2;
        }
        const op = commandOptions.op == null ? null : parseOperation(commandOptions.op);
        if (commandOptions.op != null && op == null) {
          io.writeErr(
            `error: unknown --op value ${JSON.stringify(commandOptions.op)} ` +
              "(expected: read | edit | write | delete)\n",
          );
          return 2;
        }
        const archive = readArchive();
        try {
          const rows = fileHotspots(archive.db, group, op, commandOptions.limit ?? 25);
          output(rows, () => rows.map((row) => `${row.key}\t${row.sessions}`).join("\n"));
        } finally {
          archive.db.close();
        }
      }),
    );

  const addToolStats = (command: Command): void => {
    command
      .description("tool usage stats")
      .option("--errors-only", "only tools with at least one error")
      .option("--limit <n>", "max rows", parseInteger, 50)
      .action((commandOptions: { errorsOnly?: boolean; limit?: number }) =>
        run(() => {
          const archive = readArchive();
          try {
            const rows = toolUsage(
              archive.db,
              commandOptions.errorsOnly === true,
              commandOptions.limit ?? 50,
            );
            output(rows, () => rows.map((row) => `${row.tool_name}\t${row.calls}`).join("\n"));
          } finally {
            archive.db.close();
          }
        }),
      );
  };
  const tool = program.command("tool").description("tool usage");
  addToolStats(tool.command("ls"));
  addToolStats(tool.command("stats"));

  const addMcpStats = (command: Command): void => {
    command
      .description("MCP server usage")
      .option("--limit <n>", "max rows", parseInteger, 50)
      .action((commandOptions: { limit?: number }) =>
        run(() => {
          const archive = readArchive();
          try {
            const rows = mcpUsage(archive.db, commandOptions.limit ?? 50);
            output(rows, () => rows.map((row) => `${row.mcp_server}\t${row.calls}`).join("\n"));
          } finally {
            archive.db.close();
          }
        }),
      );
  };
  const mcp = program.command("mcp").description("MCP server usage");
  addMcpStats(mcp.command("ls"));
  addMcpStats(mcp.command("stats"));

  program
    .command("export")
    .description("export a session to Markdown or JSON")
    .argument("[id]", "session id", optionalInteger)
    .option("--all", "export every session")
    .option("--out <dir>", "output directory")
    .action((id: number | undefined, commandOptions: { all?: boolean; out?: string }) =>
      run(() => {
        const archive = readArchive();
        try {
          const ext = isJson(globals()) ? "json" : "md";
          const render = (sessionId: number): string | null => {
            const detail = getSession(archive.db, sessionId);
            if (detail == null) {
              return null;
            }
            return isJson(globals()) ? JSON.stringify(detail, null, 2) : toMarkdown(detail);
          };

          if (commandOptions.all === true) {
            if (commandOptions.out == null) {
              io.writeErr("error: --all requires --out <dir>\n");
              return 2;
            }
            mkdirSync(commandOptions.out, { recursive: true });
            let count = 0;
            for (const session of listSessions(archive.db, { limit: Number.MAX_SAFE_INTEGER })) {
              const content = render(session.id);
              if (content != null) {
                writeFileSync(join(commandOptions.out, `${session.id}.${ext}`), content);
                count += 1;
              }
            }
            io.writeErr(`exported ${count} sessions to ${commandOptions.out}\n`);
            return 0;
          }

          if (id == null) {
            io.writeErr("error: provide a session id, or --all --out <dir>\n");
            return 2;
          }
          const content = render(id);
          if (content == null) {
            io.writeErr(`error: no session with id ${id}\n`);
            return 1;
          }
          if (commandOptions.out != null) {
            mkdirSync(commandOptions.out, { recursive: true });
            const path = join(commandOptions.out, `${id}.${ext}`);
            writeFileSync(path, content);
            io.writeErr(`wrote ${path}\n`);
          } else {
            io.writeOut(`${content}\n`);
          }
          return 0;
        } finally {
          archive.db.close();
        }
      }),
    );

  try {
    await program.parseAsync(argv, { from: "user" });
  } catch (error) {
    if (typeof error === "object" && error !== null && "exitCode" in error) {
      setCode(Number((error as { exitCode: number }).exitCode));
    } else {
      io.writeErr(`error: ${error instanceof Error ? error.message : String(error)}\n`);
      setCode(1);
    }
  }
  return { code, stdout: io.stdout, stderr: io.stderr };
}

function openArchive(config: Config): Archive {
  mkdirSync(dirname(config.dbPath), { recursive: true });
  return { db: openDb(config.dbPath), config };
}

function isJson(options: GlobalOptions): boolean {
  return options.json === true || options.format === "json";
}

function shouldSync(
  options: GlobalOptions,
  env: Record<string, string | undefined> | undefined,
): boolean {
  return options.sync !== false && (env ?? process.env).DECANT_NO_SYNC == null;
}

function parseInteger(value: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed)) {
    throw new InvalidArgumentError("expected an integer");
  }
  return parsed;
}

function optionalInteger(value: string | undefined): number | undefined {
  return value == null ? undefined : parseInteger(value);
}

function parseOperation(value: string): Operation | null {
  return value === "read" || value === "edit" || value === "write" || value === "delete"
    ? value
    : null;
}

function dbInfo(archive: Archive): DbInfo {
  const version =
    (
      archive.db.query("SELECT COALESCE(MAX(version), 0) AS v FROM schema_migrations").get() as {
        v: number;
      }
    ).v ?? 0;
  const counts = archive.db
    .query(
      `SELECT (SELECT COUNT(*) FROM session) AS sessions,
              (SELECT COUNT(*) FROM message) AS messages,
              (SELECT COUNT(*) FROM tool_call) AS tool_calls`,
    )
    .get() as Pick<DbInfo, "sessions" | "messages" | "tool_calls">;
  return {
    path: archive.config.dbPath,
    size_bytes: statSync(archive.config.dbPath, { throwIfNoEntry: false })?.size ?? 0,
    schema_version: version,
    ...counts,
  };
}

const completionWords = [
  "sync",
  "session",
  "ls",
  "show",
  "project",
  "db",
  "search",
  "stats",
  "files",
  "tool",
  "mcp",
  "export",
  "completion",
];

function renderCompletion(shell: string): string | null {
  const words = completionWords.join(" ");
  switch (shell) {
    case "bash":
      return `_decant_complete() {
  local cur="\${COMP_WORDS[COMP_CWORD]}"
  COMPREPLY=( $(compgen -W "${words}" -- "$cur") )
}
complete -F _decant_complete decant
`;
    case "zsh":
      return `#compdef decant
_arguments '1:command:(${words})' '*::arg:->args'
`;
    case "fish":
      return `${completionWords.map((word) => `complete -c decant -f -a '${word}'`).join("\n")}\n`;
    case "powershell":
      return `Register-ArgumentCompleter -Native -CommandName decant -ScriptBlock {
  param($wordToComplete)
  "${words}".Split(" ") | Where-Object { $_ -like "$wordToComplete*" }
}
`;
    case "elvish":
      return `set edit:completion:arg-completer[decant] = {|@words|
  put ${completionWords.map((word) => JSON.stringify(word)).join(" ")}
}
`;
    default:
      return null;
  }
}

if (import.meta.main) {
  const result = await runCli(process.argv.slice(2));
  process.stdout.write(result.stdout);
  process.stderr.write(result.stderr);
  process.exit(result.code);
}
