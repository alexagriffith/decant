import { Database } from "bun:sqlite";
import { afterEach, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { openDb } from "../src/db.ts";
import {
  basename,
  classifyExternal,
  classifyIntree,
  externalContainer,
  inferTool,
  type KnownRoot,
  resolveGitRoot,
  resolveWorktreeRoots,
} from "../src/worktree.ts";

const tempRoots: string[] = [];

afterEach(() => {
  for (const dir of tempRoots.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

function tempDir(prefix: string): string {
  const dir = mkdtempSync(join(tmpdir(), prefix));
  tempRoots.push(dir);
  return dir;
}

function tempDb(): Database {
  return openDb(join(tempDir("decant-worktree-db-"), "test.db"));
}

function pathRow(db: Database, path: string): [number, string, string | null, string | null] {
  const row = db
    .query("SELECT is_worktree, root_path, worktree_tool, root_source FROM project WHERE path = ?1")
    .get(path) as {
    is_worktree: number;
    root_path: string;
    worktree_tool: string | null;
    root_source: string | null;
  };
  return [row.is_worktree, row.root_path, row.worktree_tool, row.root_source];
}

describe("worktree pure classifiers", () => {
  test("intree claude worktree recovers root and label", () => {
    const result = classifyIntree("/Users/onlydole/dosu/dosu/.claude-worktrees/teedole-ops-39");
    expect(result).toMatchObject({
      isWorktree: true,
      rootPath: "/Users/onlydole/dosu/dosu",
      worktreeLabel: "teedole-ops-39",
      worktreeTool: "claude",
      source: "intree",
    });
  });

  test("intree plain git worktree uses git tool", () => {
    const result = classifyIntree("/Users/onlydole/oss/decant/.worktrees/feature-x");
    expect(result).toMatchObject({
      rootPath: "/Users/onlydole/oss/decant",
      worktreeLabel: "feature-x",
      worktreeTool: "git",
      source: "intree",
    });
  });

  test("plain path is not intree", () => {
    expect(classifyIntree("/Users/onlydole/oss/decant")).toBeNull();
  });

  test("external container detects warp conductor and t3", () => {
    expect(externalContainer("/Users/onlydole/.warp-worktrees/dosu-agate-spire")).toEqual({
      tool: "warp",
      leaf: "dosu-agate-spire",
    });
    expect(externalContainer("/Users/onlydole/conductor/workspaces/dosu-abuja")).toEqual({
      tool: "conductor",
      leaf: "dosu-abuja",
    });
    expect(externalContainer("/Users/onlydole/.t3-worktrees/dosu-t3code-2d73eb17")).toEqual({
      tool: "t3",
      leaf: "dosu-t3code-2d73eb17",
    });
    expect(externalContainer("/Users/onlydole/oss/decant")).toBeNull();
  });

  test("external container detects nested warp layout", () => {
    expect(externalContainer("/Users/onlydole/.warp/worktrees/astrocurious/joshua-ristra")).toEqual(
      {
        tool: "warp",
        leaf: "astrocurious-joshua-ristra",
      },
    );
    expect(externalContainer("/Users/onlydole/.warp/worktrees/astrocurious")).toBeNull();
    expect(externalContainer("/Users/onlydole/oss/worktrees/decant")).toBeNull();
  });

  test("nested warp leaf name-matches known root", () => {
    const roots: KnownRoot[] = [
      {
        path: "/Users/onlydole/oss/astrocurious",
        basename: "astrocurious",
        sessions: 10,
        lastSeen: "2026-06-01",
      },
    ];
    const container = externalContainer(
      "/Users/onlydole/.warp/worktrees/astrocurious/joshua-ristra",
    );
    expect(container).not.toBeNull();
    const result = classifyExternal(container?.tool ?? "", container?.leaf ?? "", "/x", roots);
    expect(result).toMatchObject({
      source: "namematch",
      rootPath: "/Users/onlydole/oss/astrocurious",
      worktreeLabel: "joshua-ristra",
      worktreeTool: "warp",
    });
  });

  test("nested warp leaf resolves synthetically without a known root", () => {
    const container = externalContainer(
      "/Users/onlydole/.warp/worktrees/astrocurious/joshua-ristra",
    );
    expect(container).not.toBeNull();
    const result = classifyExternal(container?.tool ?? "", container?.leaf ?? "", "/x", []);
    expect(result).toMatchObject({
      source: "synthetic",
      rootPath: "astrocurious",
      worktreeLabel: "joshua-ristra",
    });
  });

  test("external name-matches known root", () => {
    const roots: KnownRoot[] = [
      {
        path: "/Users/onlydole/dosu/dosu",
        basename: "dosu",
        sessions: 10,
        lastSeen: "2026-06-01",
      },
    ];
    const result = classifyExternal(
      "warp",
      "dosu-agate-spire",
      "/Users/onlydole/.warp-worktrees/dosu-agate-spire",
      roots,
    );
    expect(result).toMatchObject({
      isWorktree: true,
      source: "namematch",
      rootPath: "/Users/onlydole/dosu/dosu",
      worktreeLabel: "agate-spire",
      worktreeTool: "warp",
    });
  });

  test("external name-match longest basename wins", () => {
    const roots: KnownRoot[] = [
      { path: "/u/dosu", basename: "dosu", sessions: 100, lastSeen: "2026-06-01" },
      { path: "/u/dosu-agate", basename: "dosu-agate", sessions: 1, lastSeen: "2026-01-01" },
    ];
    const result = classifyExternal("warp", "dosu-agate-spire", "/x", roots);
    expect(result.rootPath).toBe("/u/dosu-agate");
    expect(result.worktreeLabel).toBe("spire");
  });

  test("external name-match tie-breaks on sessions recency then path", () => {
    const sessions: KnownRoot[] = [
      { path: "/Users/onlydole/dosu", basename: "dosu", sessions: 2, lastSeen: "2026-06-01" },
      {
        path: "/Users/onlydole/dosu/dosu",
        basename: "dosu",
        sessions: 40,
        lastSeen: "2026-05-01",
      },
    ];
    expect(classifyExternal("warp", "dosu-agate-spire", "/x", sessions).rootPath).toBe(
      "/Users/onlydole/dosu/dosu",
    );

    const recency: KnownRoot[] = [
      { path: "/u/a/dosu", basename: "dosu", sessions: 5, lastSeen: "2026-01-01" },
      { path: "/u/b/dosu", basename: "dosu", sessions: 5, lastSeen: "2026-06-01" },
    ];
    expect(classifyExternal("warp", "dosu-agate-spire", "/x", recency).rootPath).toBe("/u/b/dosu");

    const tie = (order: string[]) =>
      classifyExternal(
        "warp",
        "dosu-agate-spire",
        "/x",
        order.map((path) => ({
          path,
          basename: "dosu",
          sessions: 5,
          lastSeen: "2026-06-01",
        })),
      ).rootPath;
    expect(tie(["/u/a/dosu", "/u/b/dosu"])).toBe(tie(["/u/b/dosu", "/u/a/dosu"]));
  });

  test("external synthetic strips codename per tool", () => {
    expect(classifyExternal("warp", "dosu-agate-spire", "/x", []).rootPath).toBe("dosu");
    expect(classifyExternal("t3", "dosu-t3code-2d73eb17", "/x", []).rootPath).toBe("dosu");
    expect(classifyExternal("conductor", "dosu-abuja", "/x", []).rootPath).toBe("dosu");
    const result = classifyExternal("warp", "dosu-agate-spire", "/x", []);
    expect(result.source).toBe("synthetic");
    expect(result.worktreeLabel).toBe("agate-spire");
  });

  test("source strings and path edge cases", () => {
    expect(classifyIntree("repo/.worktrees/feature-y")).toMatchObject({
      rootPath: "repo",
      worktreeLabel: "feature-y",
      worktreeTool: "git",
    });
    expect(classifyIntree("/.worktrees/feature")).toBeNull();
    expect(classifyIntree("/Users/x/repo/.worktrees")).toBeNull();
    expect(externalContainer("/Users/x/.warp-worktrees")).toBeNull();
    expect(classifyExternal("t3", "plainname", "/x", []).rootPath).toBe("plainname");
    expect(classifyExternal("warp", "one-two", "/x", []).rootPath).toBe("one-two");
    expect(classifyExternal("conductor", "solo", "/x", []).rootPath).toBe("solo");
    expect(classifyExternal("unknown-tool", "whatever", "/x", []).rootPath).toBe("whatever");
    const empty = classifyExternal("conductor", "-x", "/Users/x/conductor/workspaces/-x", []);
    expect(empty.rootPath).toBe("/Users/x/conductor/workspaces/-x");
    expect(empty.worktreeLabel).toBeNull();
    expect(basename("/u/x/dosu/")).toBe("dosu");
    expect(basename("dosu")).toBe("dosu");
    expect(basename("")).toBe("");
  });
});

describe("git worktree pointer resolution", () => {
  test("git pointer file resolves authoritative root", () => {
    const root = tempDir("decant-worktree-git-");
    const wt = join(root, "agate-spire");
    mkdirSync(wt, { recursive: true });
    writeFileSync(
      wt.concat("/.git"),
      "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
    );
    const result = resolveGitRoot(wt);
    expect(result).toMatchObject({
      isWorktree: true,
      rootPath: "/Users/onlydole/dosu/dosu",
      worktreeLabel: "agate-spire",
      worktreeTool: "git",
      source: "git",
    });
  });

  test("git directory missing or non-worktree returns null", () => {
    const main = tempDir("decant-main-git-");
    mkdirSync(join(main, ".git"), { recursive: true });
    expect(resolveGitRoot(main)).toBeNull();

    const missing = tempDir("decant-missing-git-");
    expect(resolveGitRoot(missing)).toBeNull();
    writeFileSync(join(missing, ".git"), "gitdir: ../.git/modules/foo\n");
    expect(resolveGitRoot(missing)).toBeNull();
  });

  test("git pointer in external container infers container tool", () => {
    const root = tempDir("decant-warp-git-");
    const wt = join(root, ".warp-worktrees", "dosu-agate-spire");
    mkdirSync(wt, { recursive: true });
    writeFileSync(
      join(wt, ".git"),
      "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
    );
    expect(resolveGitRoot(wt)).toMatchObject({
      worktreeTool: "warp",
      rootPath: "/Users/onlydole/dosu/dosu",
    });
  });

  test("git pointer unusual forms", () => {
    const root = tempDir("decant-pointer-");
    const wt = join(root, "wt");
    mkdirSync(wt, { recursive: true });

    writeFileSync(join(wt, ".git"), "gitdir: /.git/worktrees/wt\n");
    expect(resolveGitRoot(wt)).toBeNull();

    writeFileSync(join(wt, ".git"), "gitdir: /Users/x/repo/.git/worktrees/\n");
    expect(resolveGitRoot(wt)).toBeNull();

    writeFileSync(join(wt, ".git"), "gitdir: ../main/.git/worktrees/wt\n");
    expect(resolveGitRoot(wt)).toBeNull();

    writeFileSync(join(wt, ".git"), "gitdir:/Users/x/repo/.git/worktrees/wt\r\n");
    expect(resolveGitRoot(wt)).toMatchObject({
      rootPath: "/Users/x/repo",
      worktreeLabel: "wt",
    });
  });

  test("infer tool defaults and recognizes segments", () => {
    expect(inferTool("/u/.warp-worktrees/dosu-x")).toBe("warp");
    expect(inferTool("/u/repo/.claude-worktrees/x")).toBe("claude");
    expect(inferTool("/u/repo/plain")).toBe("git");
  });
});

describe("resolveWorktreeRoots", () => {
  test("links intree and external worktrees to roots", () => {
    const db = tempDb();
    db.exec(`
      INSERT INTO project(path, name) VALUES ('/home/x/dosu/dosu', 'dosu');
      INSERT INTO project(path, name) VALUES
        ('/home/x/dosu/dosu/.claude-worktrees/teedole-ops-39', 'teedole-ops-39');
      INSERT INTO project(path, name) VALUES
        ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');
    `);
    resolveWorktreeRoots(db);

    expect(pathRow(db, "/home/x/dosu/dosu")).toEqual([0, "/home/x/dosu/dosu", null, "self"]);
    expect(pathRow(db, "/home/x/dosu/dosu/.claude-worktrees/teedole-ops-39")).toEqual([
      1,
      "/home/x/dosu/dosu",
      "claude",
      "intree",
    ]);
    expect(pathRow(db, "/home/x/.warp-worktrees/dosu-agate-spire")).toEqual([
      1,
      "/home/x/dosu/dosu",
      "warp",
      "namematch",
    ]);
    db.close();
  });

  test("preserves git locked rows and uses them as targets", () => {
    const db = tempDb();
    db.exec(`
      INSERT INTO project(path, name, is_worktree, root_path, worktree_label, worktree_tool, root_source)
      VALUES ('/gone/wt', 'wt', 1, '/real/dosu', 'wt', 'git', 'git');
      INSERT INTO project(path, name)
      VALUES ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');
    `);
    resolveWorktreeRoots(db);
    resolveWorktreeRoots(db);

    const locked = db
      .query(
        "SELECT is_worktree, root_path, worktree_label, root_source FROM project WHERE path = '/gone/wt'",
      )
      .get() as {
      is_worktree: number;
      root_path: string;
      worktree_label: string | null;
      root_source: string | null;
    };
    expect(locked).toEqual({
      is_worktree: 1,
      root_path: "/real/dosu",
      worktree_label: "wt",
      root_source: "git",
    });

    const external = db
      .query(
        "SELECT root_path, root_source FROM project WHERE path = '/home/x/.warp-worktrees/dosu-agate-spire'",
      )
      .get() as { root_path: string; root_source: string };
    expect(external).toEqual({ root_path: "/real/dosu", root_source: "namematch" });
    db.close();
  });

  test("propagates db error", () => {
    const db = new Database(":memory:");
    expect(() => resolveWorktreeRoots(db)).toThrow();
    db.close();
  });

  test("uses on-disk git worktree pointer", () => {
    const root = tempDir("decant-real-git-");
    const wt = join(root, "agate-spire");
    mkdirSync(wt, { recursive: true });
    writeFileSync(
      wt.concat("/.git"),
      "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
    );

    const db = tempDb();
    db.query("INSERT INTO project(path, name) VALUES (?1, 'agate-spire')").run(wt);
    resolveWorktreeRoots(db);

    const result = db
      .query("SELECT is_worktree, root_path, root_source FROM project WHERE name = 'agate-spire'")
      .get() as { is_worktree: number; root_path: string; root_source: string };
    expect(result).toEqual({
      is_worktree: 1,
      root_path: "/Users/onlydole/dosu/dosu",
      root_source: "git",
    });
    db.close();
  });
});
