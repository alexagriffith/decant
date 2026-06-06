use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    Codex,
}

impl Tool {
    pub fn as_str(self) -> &'static str {
        match self {
            Tool::ClaudeCode => "claude_code",
            Tool::Codex => "codex",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
    Tool,
    Other,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
            Role::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockType {
    Text,
    Thinking,
    ToolUse,
    ToolResult,
    WebSearch,
    Image,
    Other,
}

impl BlockType {
    pub fn as_str(self) -> &'static str {
        match self {
            BlockType::Text => "text",
            BlockType::Thinking => "thinking",
            BlockType::ToolUse => "tool_use",
            BlockType::ToolResult => "tool_result",
            BlockType::WebSearch => "web_search",
            BlockType::Image => "image",
            BlockType::Other => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Builtin,
    Mcp,
    Custom,
    WebSearch,
    ToolSearch,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolKind::Builtin => "builtin",
            ToolKind::Mcp => "mcp",
            ToolKind::Custom => "custom",
            ToolKind::WebSearch => "web_search",
            ToolKind::ToolSearch => "tool_search",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

#[derive(Debug, Clone)]
pub struct NormalizedBlock {
    pub ordinal: i64,
    pub block_type: BlockType,
    pub text: Option<String>,
    pub tool_name: Option<String>,
    pub tool_use_id: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_result: Option<String>,
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct NormalizedMessage {
    pub seq: i64,
    pub source_uuid: Option<String>,
    pub parent_source_uuid: Option<String>,
    pub role: Role,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub timestamp: Option<String>,
    pub usage: Option<TokenUsage>,
    pub raw: Value,
    pub blocks: Vec<NormalizedBlock>,
}

#[derive(Debug, Clone)]
pub struct NormalizedSession {
    pub tool: Tool,
    pub source_session_id: String,
    pub project_path: Option<String>,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub cli_version: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub is_archived: bool,
    pub raw_meta: Value,
    /// Session-level token totals (Codex sets these directly; Claude sums per-message).
    pub totals: TokenUsage,
    pub messages: Vec<NormalizedMessage>,
}

#[derive(Debug, Clone)]
pub struct Issue {
    pub line_no: usize,
    pub error: String,
    pub raw_line: String,
}

#[derive(Debug, Clone)]
pub struct ParsedSession {
    pub session: NormalizedSession,
    pub issues: Vec<Issue>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_strings_are_stable() {
        assert_eq!(Tool::ClaudeCode.as_str(), "claude_code");
        assert_eq!(Role::Tool.as_str(), "tool");
        assert_eq!(BlockType::ToolUse.as_str(), "tool_use");
        assert_eq!(ToolKind::Mcp.as_str(), "mcp");
    }
}
