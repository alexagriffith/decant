//! Identify a project's *root* repo and *worktree* identity from its path, so
//! analytics can roll worktree cost up under the repo it belongs to.
//!
//! Resolution is layered by confidence: git-authoritative (live dir) → in-tree
//! path string → external name-match → synthetic. The string logic here is pure
//! (no I/O) and unit-tested in isolation; `resolve_git_root` and the orchestrator
//! `resolve_worktree_roots` (added in later tasks) are the only parts that touch
//! the filesystem / DB.

use crate::Result;
use rusqlite::{params, Connection};
use std::path::Path;

/// Confidence tier of a root resolution; also the value stored in
/// `project.root_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    SelfRoot,
    Git,
    Intree,
    NameMatch,
    Synthetic,
}

impl RootSource {
    pub fn as_str(self) -> &'static str {
        match self {
            RootSource::SelfRoot => "self",
            RootSource::Git => "git",
            RootSource::Intree => "intree",
            RootSource::NameMatch => "namematch",
            RootSource::Synthetic => "synthetic",
        }
    }
}

/// The resolved root + worktree identity for one project path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub is_worktree: bool,
    pub root_path: String,
    pub worktree_label: Option<String>,
    pub worktree_tool: Option<String>,
    pub source: RootSource,
}

/// A known root project, used as a name-match target for external worktrees.
#[derive(Debug, Clone)]
pub struct KnownRoot {
    pub path: String,
    pub basename: String,
    pub sessions: i64,
    pub last_seen: Option<String>,
}

/// Split an absolute-ish path into non-empty segments, remembering the leading `/`.
fn segments(path: &str) -> (bool, Vec<&str>) {
    let abs = path.starts_with('/');
    (abs, path.split('/').filter(|s| !s.is_empty()).collect())
}

/// Re-join segments back into a path, restoring the leading `/` when `abs`.
fn join(abs: bool, parts: &[&str]) -> String {
    let body = parts.join("/");
    if abs {
        format!("/{body}")
    } else {
        body
    }
}

/// Last non-empty path segment (e.g. basename of a root path or a synthetic key).
pub fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .to_string()
}

/// In-tree worktree? Matches a `.worktrees` (tool `git`) or `.claude-worktrees`
/// (tool `claude`) segment and returns the root (everything before it) plus the
/// label (everything after it). No I/O.
pub fn classify_intree(path: &str) -> Option<Resolution> {
    let (abs, segs) = segments(path);
    let idx = segs
        .iter()
        .position(|s| *s == ".worktrees" || *s == ".claude-worktrees")?;
    if idx == 0 || idx + 1 >= segs.len() {
        return None; // need a root before and a label after
    }
    let tool = if segs[idx] == ".claude-worktrees" {
        "claude"
    } else {
        "git"
    };
    Some(Resolution {
        is_worktree: true,
        root_path: join(abs, &segs[..idx]),
        worktree_label: Some(segs[idx + 1..].join("/")),
        worktree_tool: Some(tool.to_string()),
        source: RootSource::Intree,
    })
}

/// External worktree container? Recognizes `.warp-worktrees`, `.t3-worktrees`
/// (parent segment), `conductor/workspaces` and Warp's current
/// `.warp/worktrees/<repo>` (two segments). Returns `(tool, leaf)` where `leaf`
/// is the directory under the container — for the nested Warp layout the repo
/// is explicit, so the leaf is the composite `<repo>-<dir>` that the
/// name-match and codename-strip paths already understand. No I/O.
pub fn external_container(path: &str) -> Option<(&'static str, String)> {
    let (_, segs) = segments(path);
    for (i, seg) in segs.iter().enumerate() {
        let tool = match *seg {
            ".warp-worktrees" => Some("warp"),
            ".t3-worktrees" => Some("t3"),
            "workspaces" if i > 0 && segs[i - 1] == "conductor" => Some("conductor"),
            "worktrees" if i > 0 && segs[i - 1] == ".warp" => Some("warp"),
            _ => None,
        };
        if let Some(tool) = tool {
            if *seg == "worktrees" {
                if let (Some(repo), Some(leaf)) = (segs.get(i + 1), segs.get(i + 2)) {
                    return Some((tool, format!("{repo}-{leaf}")));
                }
            } else if let Some(leaf) = segs.get(i + 1) {
                return Some((tool, leaf.to_string()));
            }
        }
    }
    None
}

/// Resolve an external worktree leaf: name-match against known roots, else a
/// best-effort synthetic repo key by stripping the per-tool codename. No I/O.
pub fn classify_external(
    tool: &str,
    leaf: &str,
    path: &str,
    known_roots: &[KnownRoot],
) -> Resolution {
    // Name-match: longest matching basename wins; tie-break sessions, recency, then
    // path so the winner is deterministic regardless of input order.
    let best = known_roots
        .iter()
        .filter(|r| !r.basename.is_empty())
        .filter(|r| leaf == r.basename || leaf.starts_with(&format!("{}-", r.basename)))
        .max_by(|a, b| {
            a.basename
                .len()
                .cmp(&b.basename.len())
                .then(a.sessions.cmp(&b.sessions))
                .then(a.last_seen.cmp(&b.last_seen))
                .then_with(|| a.path.cmp(&b.path))
        });
    if let Some(root) = best {
        let label = leaf
            .strip_prefix(&format!("{}-", root.basename))
            .map(str::to_string);
        return Resolution {
            is_worktree: true,
            root_path: root.path.clone(),
            worktree_label: label,
            worktree_tool: Some(tool.to_string()),
            source: RootSource::NameMatch,
        };
    }

    // Synthetic: strip the codename to a bare repo key.
    let key = strip_codename(tool, leaf);
    let (root_path, label) = if key.is_empty() {
        (path.to_string(), None) // unmerged — under-merge rather than mis-merge
    } else {
        let label = leaf.strip_prefix(&format!("{key}-")).map(str::to_string);
        (key, label)
    };
    Resolution {
        is_worktree: true,
        root_path,
        worktree_label: label,
        worktree_tool: Some(tool.to_string()),
        source: RootSource::Synthetic,
    }
}

/// Best-effort repo key from an external worktree leaf, per tool convention:
/// t3 = `<repo>-t3code-<hash>`, warp = `<repo>-<two-word-codename>`,
/// conductor = `<repo>-<one-word-codename>`.
fn strip_codename(tool: &str, leaf: &str) -> String {
    match tool {
        "t3" => match leaf.find("-t3code-") {
            Some(idx) => leaf[..idx].to_string(),
            None => leaf.to_string(),
        },
        "warp" => {
            let toks: Vec<&str> = leaf.split('-').collect();
            if toks.len() >= 3 {
                toks[..toks.len() - 2].join("-")
            } else {
                leaf.to_string()
            }
        }
        "conductor" => {
            let toks: Vec<&str> = leaf.split('-').collect();
            if toks.len() >= 2 {
                toks[..toks.len() - 1].join("-")
            } else {
                leaf.to_string()
            }
        }
        _ => leaf.to_string(),
    }
}

/// Authoritative resolution for a *live* worktree: if `<dir>/.git` is a regular
/// file pointing at `<root>/.git/worktrees/<name>`, return that root. Returns
/// `None` for a main checkout (`.git` is a directory), a missing dir, or a
/// non-worktree pointer (e.g. a submodule). Touches the filesystem.
pub fn resolve_git_root(dir: &Path) -> Option<Resolution> {
    let dotgit = dir.join(".git");
    if !dotgit.is_file() {
        return None; // .git dir = main checkout; absent = not a repo
    }
    let content = std::fs::read_to_string(&dotgit).ok()?;
    let target = content
        .lines()
        .find_map(|l| l.trim().strip_prefix("gitdir:"))?
        .trim();
    // git ≥ 2.48 can write relative pointers (worktree.useRelativePaths); a
    // relative root would be a junk grouping key, so fall back to the string
    // classifiers instead of locking it in as authoritative.
    if !Path::new(target).is_absolute() {
        return None;
    }
    let marker = "/.git/worktrees/";
    let idx = target.rfind(marker)?;
    let root_path = target[..idx].to_string();
    let name = target[idx + marker.len()..].trim_end_matches('/');
    if root_path.is_empty() || name.is_empty() {
        return None;
    }
    Some(Resolution {
        is_worktree: true,
        root_path,
        worktree_label: Some(name.to_string()),
        worktree_tool: Some(infer_tool(&dir.to_string_lossy()).to_string()),
        source: RootSource::Git,
    })
}

/// Resolve and persist root/worktree identity for every project. Idempotent;
/// operates on the `project` table plus cheap per-path filesystem stats. Rows
/// already locked at `root_source = 'git'` are left untouched (the worktree may
/// since have been deleted; we trust the earlier authoritative read).
pub fn resolve_worktree_roots(conn: &Connection) -> Result<()> {
    struct Proj {
        id: i64,
        path: String,
        is_worktree: i64,
        root_path: Option<String>,
        source: Option<String>,
        worktree_label: Option<String>,
        worktree_tool: Option<String>,
        sessions: i64,
        last_seen: Option<String>,
    }

    let mut stmt = conn.prepare(
        "SELECT p.id, p.path, p.is_worktree, p.root_path, p.root_source,
                p.worktree_label, p.worktree_tool,
                COUNT(s.id), MAX(s.started_at)
         FROM project p LEFT JOIN session s ON s.project_id = p.id
         GROUP BY p.id",
    )?;
    let projs: Vec<Proj> = stmt
        .query_map([], |r| {
            Ok(Proj {
                id: r.get(0)?,
                path: r.get(1)?,
                is_worktree: r.get(2)?,
                root_path: r.get(3)?,
                source: r.get(4)?,
                worktree_label: r.get(5)?,
                worktree_tool: r.get(6)?,
                sessions: r.get(7)?,
                last_seen: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Pass A: git / intree / self; defer externals. Accumulate known roots from
    // every non-deferred project's resolved root_path (so a root discovered only
    // via an in-tree/git worktree is still a name-match target).
    let mut writes: Vec<(i64, Resolution)> = Vec::new();
    let mut deferred: Vec<(i64, String, &'static str, String)> = Vec::new();
    let mut roots: std::collections::HashMap<String, (i64, Option<String>)> =
        std::collections::HashMap::new();
    let mut note_root = |rp: &str, sessions: i64, last: &Option<String>| {
        let e = roots.entry(rp.to_string()).or_insert((0, None));
        e.0 += sessions;
        if last > &e.1 {
            e.1 = last.clone();
        }
    };

    for p in &projs {
        // Locked authoritative result: contribute its root, never rewrite.
        if p.source.as_deref() == Some("git") {
            let rp = p.root_path.clone().unwrap_or_else(|| p.path.clone());
            note_root(&rp, p.sessions, &p.last_seen);
            continue;
        }
        if let Some(res) = resolve_git_root(Path::new(&p.path)) {
            note_root(&res.root_path, p.sessions, &p.last_seen);
            writes.push((p.id, res));
            continue;
        }
        if let Some(res) = classify_intree(&p.path) {
            note_root(&res.root_path, p.sessions, &p.last_seen);
            writes.push((p.id, res));
            continue;
        }
        if let Some((tool, leaf)) = external_container(&p.path) {
            deferred.push((p.id, p.path.clone(), tool, leaf));
            continue;
        }
        // Self root.
        note_root(&p.path, p.sessions, &p.last_seen);
        writes.push((
            p.id,
            Resolution {
                is_worktree: false,
                root_path: p.path.clone(),
                worktree_label: None,
                worktree_tool: None,
                source: RootSource::SelfRoot,
            },
        ));
    }

    let known: Vec<KnownRoot> = roots
        .into_iter()
        .map(|(path, (sessions, last_seen))| KnownRoot {
            basename: basename(&path),
            path,
            sessions,
            last_seen,
        })
        .collect();

    // Pass B: external worktrees, now that roots are known.
    for (id, path, tool, leaf) in &deferred {
        writes.push((*id, classify_external(tool, leaf, path, &known)));
    }

    // Write back. Skip no-op writes to keep this cheap on steady-state DBs.
    let tx = conn.unchecked_transaction()?;
    for (id, res) in &writes {
        let p = projs.iter().find(|p| p.id == *id).unwrap();
        let unchanged = p.is_worktree == res.is_worktree as i64
            && p.root_path.as_deref() == Some(res.root_path.as_str())
            && p.source.as_deref() == Some(res.source.as_str())
            && p.worktree_label == res.worktree_label
            && p.worktree_tool == res.worktree_tool;
        if unchanged {
            continue;
        }
        tx.execute(
            "UPDATE project
                SET is_worktree = ?2, root_path = ?3, worktree_label = ?4,
                    worktree_tool = ?5, root_source = ?6
              WHERE id = ?1",
            params![
                id,
                res.is_worktree as i64,
                res.root_path,
                res.worktree_label,
                res.worktree_tool,
                res.source.as_str(),
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Infer the worktree tool from the worktree's own path (container or in-tree
/// segment), defaulting to plain `git`.
pub fn infer_tool(path: &str) -> &'static str {
    if let Some((tool, _)) = external_container(path) {
        return tool;
    }
    let (_, segs) = segments(path);
    if segs.contains(&".claude-worktrees") {
        "claude"
    } else {
        "git"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, schema};

    #[test]
    fn intree_claude_worktree_recovers_root_and_label() {
        let r =
            classify_intree("/Users/onlydole/dosu/dosu/.claude-worktrees/teedole-ops-39").unwrap();
        assert!(r.is_worktree);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
        assert_eq!(r.worktree_label.as_deref(), Some("teedole-ops-39"));
        assert_eq!(r.worktree_tool.as_deref(), Some("claude"));
        assert_eq!(r.source, RootSource::Intree);
    }

    #[test]
    fn intree_plain_git_worktree_uses_git_tool() {
        let r = classify_intree("/Users/onlydole/oss/decant/.worktrees/feature-x").unwrap();
        assert_eq!(r.root_path, "/Users/onlydole/oss/decant");
        assert_eq!(r.worktree_label.as_deref(), Some("feature-x"));
        assert_eq!(r.worktree_tool.as_deref(), Some("git"));
        assert_eq!(r.source, RootSource::Intree);
    }

    #[test]
    fn plain_path_is_not_intree() {
        assert!(classify_intree("/Users/onlydole/oss/decant").is_none());
    }

    #[test]
    fn external_container_detects_warp_and_conductor() {
        assert_eq!(
            external_container("/Users/onlydole/.warp-worktrees/dosu-agate-spire"),
            Some(("warp", "dosu-agate-spire".to_string()))
        );
        assert_eq!(
            external_container("/Users/onlydole/conductor/workspaces/dosu-abuja"),
            Some(("conductor", "dosu-abuja".to_string()))
        );
        assert_eq!(external_container("/Users/onlydole/oss/decant"), None);
    }

    #[test]
    fn external_container_detects_t3() {
        assert_eq!(
            external_container("/Users/onlydole/.t3-worktrees/dosu-t3code-2d73eb17"),
            Some(("t3", "dosu-t3code-2d73eb17".to_string()))
        );
    }

    #[test]
    fn external_container_detects_nested_warp_layout() {
        // Warp's current layout: ~/.warp/worktrees/<repo>/<leaf>. The repo is
        // explicit, so the returned leaf is the composite `<repo>-<leaf>` the
        // name-match and codename-strip paths already understand.
        assert_eq!(
            external_container("/Users/onlydole/.warp/worktrees/astrocurious/joshua-ristra"),
            Some(("warp", "astrocurious-joshua-ristra".to_string()))
        );
        // The bare repo grouping dir is a container, not a worktree.
        assert_eq!(
            external_container("/Users/onlydole/.warp/worktrees/astrocurious"),
            None
        );
        // An unrelated `worktrees` segment is not Warp's container.
        assert_eq!(
            external_container("/Users/onlydole/oss/worktrees/decant"),
            None
        );
    }

    #[test]
    fn nested_warp_leaf_namematches_known_root() {
        let roots = vec![KnownRoot {
            path: "/Users/onlydole/oss/astrocurious".into(),
            basename: "astrocurious".into(),
            sessions: 10,
            last_seen: Some("2026-06-01".into()),
        }];
        let (tool, leaf) =
            external_container("/Users/onlydole/.warp/worktrees/astrocurious/joshua-ristra")
                .unwrap();
        let r = classify_external(tool, &leaf, "/x", &roots);
        assert_eq!(r.source, RootSource::NameMatch);
        assert_eq!(r.root_path, "/Users/onlydole/oss/astrocurious");
        assert_eq!(r.worktree_label.as_deref(), Some("joshua-ristra"));
        assert_eq!(r.worktree_tool.as_deref(), Some("warp"));
    }

    #[test]
    fn nested_warp_leaf_resolves_synthetically_without_a_known_root() {
        let (tool, leaf) =
            external_container("/Users/onlydole/.warp/worktrees/astrocurious/joshua-ristra")
                .unwrap();
        let r = classify_external(tool, &leaf, "/x", &[]);
        assert_eq!(r.source, RootSource::Synthetic);
        assert_eq!(r.root_path, "astrocurious", "two-word codename stripped");
        assert_eq!(r.worktree_label.as_deref(), Some("joshua-ristra"));
    }

    #[test]
    fn external_namematches_known_root() {
        let roots = vec![KnownRoot {
            path: "/Users/onlydole/dosu/dosu".into(),
            basename: "dosu".into(),
            sessions: 10,
            last_seen: Some("2026-06-01".into()),
        }];
        let r = classify_external(
            "warp",
            "dosu-agate-spire",
            "/Users/onlydole/.warp-worktrees/dosu-agate-spire",
            &roots,
        );
        assert_eq!(r.source, RootSource::NameMatch);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
        assert_eq!(r.worktree_label.as_deref(), Some("agate-spire"));
        assert_eq!(r.worktree_tool.as_deref(), Some("warp"));
        assert!(r.is_worktree);
    }

    #[test]
    fn external_namematch_longest_basename_wins() {
        let roots = vec![
            KnownRoot {
                path: "/u/dosu".into(),
                basename: "dosu".into(),
                sessions: 100,
                last_seen: Some("2026-06-01".into()),
            },
            KnownRoot {
                path: "/u/dosu-agate".into(),
                basename: "dosu-agate".into(),
                sessions: 1,
                last_seen: Some("2026-01-01".into()),
            },
        ];
        let r = classify_external("warp", "dosu-agate-spire", "/x", &roots);
        assert_eq!(
            r.root_path, "/u/dosu-agate",
            "longest basename beats sessions"
        );
        assert_eq!(r.worktree_label.as_deref(), Some("spire"));
    }

    #[test]
    fn external_namematch_tiebreaks_on_sessions() {
        let roots = vec![
            KnownRoot {
                path: "/Users/onlydole/dosu".into(),
                basename: "dosu".into(),
                sessions: 2,
                last_seen: Some("2026-06-01".into()),
            },
            KnownRoot {
                path: "/Users/onlydole/dosu/dosu".into(),
                basename: "dosu".into(),
                sessions: 40,
                last_seen: Some("2026-05-01".into()),
            },
        ];
        let r = classify_external("warp", "dosu-agate-spire", "/x", &roots);
        assert_eq!(
            r.root_path, "/Users/onlydole/dosu/dosu",
            "more sessions wins"
        );
    }

    #[test]
    fn external_namematch_tiebreaks_on_recency_then_path() {
        // Same basename + sessions: most-recent last_seen wins.
        let recency = vec![
            KnownRoot {
                path: "/u/a/dosu".into(),
                basename: "dosu".into(),
                sessions: 5,
                last_seen: Some("2026-01-01".into()),
            },
            KnownRoot {
                path: "/u/b/dosu".into(),
                basename: "dosu".into(),
                sessions: 5,
                last_seen: Some("2026-06-01".into()),
            },
        ];
        let r = classify_external("warp", "dosu-agate-spire", "/x", &recency);
        assert_eq!(r.root_path, "/u/b/dosu", "newer last_seen wins");

        // Full tie: deterministic regardless of input order (greatest path wins).
        let tie = |order: Vec<&str>| {
            let roots: Vec<KnownRoot> = order
                .into_iter()
                .map(|p| KnownRoot {
                    path: p.into(),
                    basename: "dosu".into(),
                    sessions: 5,
                    last_seen: Some("2026-06-01".into()),
                })
                .collect();
            classify_external("warp", "dosu-agate-spire", "/x", &roots).root_path
        };
        assert_eq!(
            tie(vec!["/u/a/dosu", "/u/b/dosu"]),
            tie(vec!["/u/b/dosu", "/u/a/dosu"])
        );
    }

    #[test]
    fn external_synthetic_strips_codename_per_tool() {
        assert_eq!(
            classify_external("warp", "dosu-agate-spire", "/x", &[]).root_path,
            "dosu"
        );
        assert_eq!(
            classify_external("t3", "dosu-t3code-2d73eb17", "/x", &[]).root_path,
            "dosu"
        );
        assert_eq!(
            classify_external("conductor", "dosu-abuja", "/x", &[]).root_path,
            "dosu"
        );
        let r = classify_external("warp", "dosu-agate-spire", "/x", &[]);
        assert_eq!(r.source, RootSource::Synthetic);
        assert_eq!(r.worktree_label.as_deref(), Some("agate-spire"));
    }

    #[test]
    fn root_source_as_str_all_arms() {
        assert_eq!(RootSource::SelfRoot.as_str(), "self");
        assert_eq!(RootSource::Git.as_str(), "git");
        assert_eq!(RootSource::Intree.as_str(), "intree");
        assert_eq!(RootSource::NameMatch.as_str(), "namematch");
        assert_eq!(RootSource::Synthetic.as_str(), "synthetic");
    }

    #[test]
    fn intree_relative_path_joins_without_leading_slash() {
        // A relative (non-absolute) in-tree path keeps its relative root via the
        // `join` non-abs branch.
        let r = classify_intree("repo/.worktrees/feature-y").unwrap();
        assert_eq!(r.root_path, "repo");
        assert_eq!(r.worktree_label.as_deref(), Some("feature-y"));
        assert_eq!(r.worktree_tool.as_deref(), Some("git"));
    }

    #[test]
    fn intree_needs_root_before_and_label_after() {
        // Container is the first segment (no root before it) → None.
        assert!(classify_intree("/.worktrees/feature").is_none());
        // Container is the last segment (no label after it) → None.
        assert!(classify_intree("/Users/x/repo/.worktrees").is_none());
    }

    #[test]
    fn external_container_none_when_leaf_missing() {
        // `.warp-worktrees` is the final segment, so there is no leaf to return.
        assert!(external_container("/Users/x/.warp-worktrees").is_none());
    }

    #[test]
    fn strip_codename_fallback_branches() {
        // t3 without the `-t3code-` marker is returned unchanged.
        assert_eq!(strip_codename("t3", "plainname"), "plainname");
        // warp with fewer than 3 tokens is returned unchanged.
        assert_eq!(strip_codename("warp", "one-two"), "one-two");
        // conductor with fewer than 2 tokens is returned unchanged.
        assert_eq!(strip_codename("conductor", "solo"), "solo");
        // An unknown tool returns the leaf unchanged (the `_` arm).
        assert_eq!(strip_codename("unknown-tool", "whatever"), "whatever");
    }

    #[test]
    fn classify_external_empty_key_falls_back_to_full_path() {
        // strip_codename can produce an empty key when the leaf is exactly the
        // stripped prefix (e.g. conductor leaf "-x" → tokens ["", "x"], len 2,
        // joined first token = ""). Then resolution keeps the full path.
        let r = classify_external("conductor", "-x", "/Users/x/conductor/workspaces/-x", &[]);
        assert_eq!(r.root_path, "/Users/x/conductor/workspaces/-x");
        assert!(r.worktree_label.is_none());
        assert_eq!(r.source, RootSource::Synthetic);
    }

    #[test]
    fn resolve_git_root_rejects_empty_root_or_name() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        // Pointer where the marker is at the very start → empty root_path → None.
        std::fs::write(wt.join(".git"), "gitdir: /.git/worktrees/wt\n").unwrap();
        assert!(resolve_git_root(&wt).is_none());
        // Pointer with an empty worktree name → None.
        std::fs::write(wt.join(".git"), "gitdir: /Users/x/repo/.git/worktrees/\n").unwrap();
        assert!(resolve_git_root(&wt).is_none());
    }

    #[test]
    fn infer_tool_defaults_and_recognizes_segments() {
        // External container tool wins.
        assert_eq!(infer_tool("/u/.warp-worktrees/dosu-x"), "warp");
        // In-tree claude segment → claude.
        assert_eq!(infer_tool("/u/repo/.claude-worktrees/x"), "claude");
        // Anything else defaults to plain git.
        assert_eq!(infer_tool("/u/repo/plain"), "git");
    }

    #[test]
    fn basename_handles_trailing_slash_and_empty() {
        assert_eq!(basename("/u/x/dosu/"), "dosu");
        assert_eq!(basename("dosu"), "dosu");
        assert_eq!(basename(""), "");
    }

    #[test]
    fn git_pointer_file_resolves_authoritative_root() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("agate-spire");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            wt.join(".git"),
            "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
        )
        .unwrap();
        let r = resolve_git_root(&wt).unwrap();
        assert!(r.is_worktree);
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
        assert_eq!(r.worktree_label.as_deref(), Some("agate-spire"));
        assert_eq!(r.worktree_tool.as_deref(), Some("git"));
        assert_eq!(r.source, RootSource::Git);
    }

    #[test]
    fn git_directory_is_main_checkout_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        assert!(resolve_git_root(tmp.path()).is_none());
    }

    #[test]
    fn missing_or_non_worktree_git_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve_git_root(tmp.path()).is_none()); // no .git at all
                                                         // a submodule-style pointer is not a worktree we roll up
        std::fs::write(tmp.path().join(".git"), "gitdir: ../.git/modules/foo\n").unwrap();
        assert!(resolve_git_root(tmp.path()).is_none());
    }

    #[test]
    fn git_pointer_in_external_container_infers_container_tool() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join(".warp-worktrees/dosu-agate-spire");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            wt.join(".git"),
            "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
        )
        .unwrap();
        let r = resolve_git_root(&wt).unwrap();
        assert_eq!(r.worktree_tool.as_deref(), Some("warp"));
        assert_eq!(r.root_path, "/Users/onlydole/dosu/dosu");
    }

    #[test]
    fn git_pointer_relative_or_unusual_forms() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("wt");
        std::fs::create_dir_all(&wt).unwrap();

        // Relative pointer (git worktree.useRelativePaths) → None, not junk.
        std::fs::write(wt.join(".git"), "gitdir: ../main/.git/worktrees/wt\n").unwrap();
        assert!(resolve_git_root(&wt).is_none());

        // No-space and CRLF forms still parse.
        std::fs::write(
            wt.join(".git"),
            "gitdir:/Users/x/repo/.git/worktrees/wt\r\n",
        )
        .unwrap();
        let r = resolve_git_root(&wt).unwrap();
        assert_eq!(r.root_path, "/Users/x/repo");
        assert_eq!(r.worktree_label.as_deref(), Some("wt"));
    }

    #[test]
    fn resolve_links_intree_and_external_worktrees_to_roots() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        // Synthetic, guaranteed-nonexistent paths so git resolution is skipped
        // and the string heuristics drive the result.
        conn.execute_batch(
            "INSERT INTO project(path, name) VALUES ('/home/x/dosu/dosu', 'dosu');
             INSERT INTO project(path, name) VALUES
               ('/home/x/dosu/dosu/.claude-worktrees/teedole-ops-39', 'teedole-ops-39');
             INSERT INTO project(path, name) VALUES
               ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');",
        )
        .unwrap();

        resolve_worktree_roots(&conn).unwrap();

        let row = |path: &str| -> (i64, String, Option<String>, Option<String>) {
            conn.query_row(
                "SELECT is_worktree, root_path, worktree_tool, root_source FROM project WHERE path = ?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap()
        };

        assert_eq!(
            row("/home/x/dosu/dosu"),
            (0, "/home/x/dosu/dosu".into(), None, Some("self".into()))
        );
        let (iw, rp, tool, src) = row("/home/x/dosu/dosu/.claude-worktrees/teedole-ops-39");
        assert_eq!(
            (iw, rp.as_str(), tool.as_deref(), src.as_deref()),
            (1, "/home/x/dosu/dosu", Some("claude"), Some("intree"))
        );
        let (iw2, rp2, tool2, src2) = row("/home/x/.warp-worktrees/dosu-agate-spire");
        assert_eq!(
            (iw2, rp2.as_str(), tool2.as_deref(), src2.as_deref()),
            (1, "/home/x/dosu/dosu", Some("warp"), Some("namematch"))
        );
    }

    #[test]
    fn resolve_preserves_git_locked_rows_and_uses_them_as_targets() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        // A locked authoritative row whose dir no longer exists, plus an
        // external sibling that should name-match against the locked row's root.
        conn.execute_batch(
            "INSERT INTO project(path, name, is_worktree, root_path, worktree_label, worktree_tool, root_source)
             VALUES ('/gone/wt', 'wt', 1, '/real/dosu', 'wt', 'git', 'git');
             INSERT INTO project(path, name) VALUES ('/home/x/.warp-worktrees/dosu-agate-spire', 'dosu-agate-spire');",
        )
        .unwrap();

        resolve_worktree_roots(&conn).unwrap();
        resolve_worktree_roots(&conn).unwrap(); // second run: steady state, zero writes

        let locked: (i64, String, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT is_worktree, root_path, worktree_label, root_source FROM project WHERE path = '/gone/wt'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            locked,
            (
                1,
                "/real/dosu".into(),
                Some("wt".into()),
                Some("git".into())
            ),
            "heuristics must never rewrite an authoritative row"
        );

        let (root, src): (String, String) = conn
            .query_row(
                "SELECT root_path, root_source FROM project WHERE path = '/home/x/.warp-worktrees/dosu-agate-spire'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            root, "/real/dosu",
            "locked row's root is a name-match target"
        );
        assert_eq!(src, "namematch");
    }

    #[test]
    fn resolve_worktree_roots_propagates_db_error() {
        // No `project` table -> the opening `prepare` `?` propagates.
        let bare = db::open_in_memory().unwrap();
        assert!(resolve_worktree_roots(&bare).is_err());
    }

    #[test]
    fn resolve_uses_on_disk_git_worktree_pointer() {
        // A project whose path is a real on-disk worktree (a `.git` *file*
        // pointing at an absolute `/.git/worktrees/<name>` root) takes the
        // authoritative `resolve_git_root` Some branch: note the root + record a
        // write.
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("agate-spire");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(
            wt.join(".git"),
            "gitdir: /Users/onlydole/dosu/dosu/.git/worktrees/agate-spire\n",
        )
        .unwrap();

        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO project(path, name) VALUES (?1, 'agate-spire')",
            [wt.to_string_lossy().to_string()],
        )
        .unwrap();

        resolve_worktree_roots(&conn).unwrap();

        let (is_wt, root, src): (i64, String, String) = conn
            .query_row(
                "SELECT is_worktree, root_path, root_source FROM project WHERE name = 'agate-spire'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(is_wt, 1);
        assert_eq!(root, "/Users/onlydole/dosu/dosu");
        assert_eq!(src, "git");
    }
}
