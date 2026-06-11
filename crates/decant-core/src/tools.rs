use crate::model::ToolKind;

/// Classify a logged tool name into (kind, mcp_server, base_name).
/// MCP convention: `mcp__<server>__<base>` (base may itself contain `__`).
pub fn classify_tool(name: &str) -> (ToolKind, Option<String>, String) {
    if let Some(rest) = name.strip_prefix("mcp__") {
        if let Some((server, base)) = rest.split_once("__") {
            return (ToolKind::Mcp, Some(server.to_string()), base.to_string());
        }
        return (ToolKind::Mcp, None, rest.to_string());
    }
    (ToolKind::Builtin, None, name.to_string())
}

/// First `max` chars of a string, with an ellipsis if truncated.
pub fn preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tool() {
        let (kind, server, base) = classify_tool("Bash");
        assert_eq!(kind, ToolKind::Builtin);
        assert_eq!(server, None);
        assert_eq!(base, "Bash");
    }

    #[test]
    fn simple_mcp_tool() {
        let (kind, server, base) = classify_tool("mcp__claude_ai_Linear__create_issue");
        assert_eq!(kind, ToolKind::Mcp);
        assert_eq!(server.as_deref(), Some("claude_ai_Linear"));
        assert_eq!(base, "create_issue");
    }

    #[test]
    fn nested_gateway_mcp_tool() {
        let (kind, server, base) = classify_tool("mcp__codex_apps__hubspot__create_deal");
        assert_eq!(kind, ToolKind::Mcp);
        assert_eq!(server.as_deref(), Some("codex_apps"));
        assert_eq!(base, "hubspot__create_deal");
    }

    #[test]
    fn mcp_prefix_without_server_separator() {
        // `mcp__<base>` with no second `__`: classified MCP but no server name.
        let (kind, server, base) = classify_tool("mcp__lonely");
        assert_eq!(kind, ToolKind::Mcp);
        assert_eq!(server, None);
        assert_eq!(base, "lonely");
    }

    #[test]
    fn preview_truncates() {
        assert_eq!(preview("abcdef", 3), "abc…");
        assert_eq!(preview("ab", 3), "ab");
    }
}
