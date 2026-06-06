use crate::query::SessionDetail;
use std::fmt::Write;

/// Render a full session transcript to Markdown. UI-agnostic (no IO), so the CLI
/// and the future web/macOS apps can all reuse it.
pub fn to_markdown(detail: &SessionDetail) -> String {
    let s = &detail.summary;
    let mut out = String::new();
    let title = s.title.clone().unwrap_or_else(|| s.source_session_id.clone());
    let _ = writeln!(out, "# {title}\n");
    let _ = writeln!(
        out,
        "- **tool:** {}\n- **model:** {}\n- **messages:** {}\n- **est. cost:** ${:.2}\n- **started:** {}\n",
        s.tool,
        s.model.clone().unwrap_or_default(),
        s.message_count,
        s.estimated_cost_usd,
        s.started_at.clone().unwrap_or_default(),
    );

    for m in &detail.messages {
        let _ = writeln!(out, "## {}\n", m.role.to_uppercase());
        for b in &m.blocks {
            match b.block_type.as_str() {
                "text" => {
                    if let Some(t) = &b.text {
                        let _ = writeln!(out, "{t}\n");
                    }
                }
                "thinking" => {
                    if let Some(t) = &b.text {
                        let _ = writeln!(out, "> _thinking:_ {t}\n");
                    }
                }
                "tool_use" => {
                    let _ = writeln!(
                        out,
                        "**\u{2192} {}**\n\n```json\n{}\n```\n",
                        b.tool_name.clone().unwrap_or_default(),
                        b.tool_input.clone().unwrap_or_default(),
                    );
                }
                "tool_result" => {
                    let _ = writeln!(out, "```\n{}\n```\n", b.tool_result.clone().unwrap_or_default());
                }
                other => {
                    let _ = writeln!(out, "_[{other}]_\n");
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, ingest, query, schema, sources};

    #[test]
    fn renders_markdown_transcript() {
        let conn = db::open_in_memory().unwrap();
        schema::migrate(&conn).unwrap();
        let content = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/claude/sample.jsonl")).unwrap();
        let parsed = sources::claude::parse_session("sess-claude-1", &content);
        let tx = conn.unchecked_transaction().unwrap();
        let id = ingest::upsert_session(&tx, &parsed, "/x.jsonl", 1, 2, "h").unwrap();
        tx.commit().unwrap();

        let detail = query::get_session(&conn, id).unwrap().unwrap();
        let md = to_markdown(&detail);
        assert!(md.starts_with("# Fix the failing auth test"));
        assert!(md.contains("## USER"));
        assert!(md.contains("Read"));
        assert!(md.contains("_thinking:_"));
    }
}
