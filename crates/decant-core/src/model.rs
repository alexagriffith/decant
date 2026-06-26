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
    /// Output tokens spent on internal reasoning ("planning"). A *sub-component*
    /// of `output`, not additive to it — informational/analytics only, never
    /// priced separately. Codex reports it via `reasoning_output_tokens`; Claude
    /// redacts its thinking text and reports no count, so this stays 0 there.
    pub reasoning: i64,
}

/// Provenance of a session's reasoning-token figure, so consumers can tell an
/// exact count from an estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReasoningSource {
    /// The tool reported an exact count (Codex `reasoning_output_tokens`); read
    /// it from [`TokenUsage::reasoning`].
    Reported,
    /// No exact count; estimated by subtracting visible output from the turn's
    /// total (Claude). Read [`NormalizedSession::est_reasoning_tokens`]; treat as
    /// approximate (±soft).
    Inferred,
    /// No exact count and nothing to estimate from (no reasoning/thinking).
    #[default]
    None,
}

impl ReasoningSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningSource::Reported => "reported",
            ReasoningSource::Inferred => "inferred",
            ReasoningSource::None => "none",
        }
    }
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
    /// Estimated reasoning tokens when the tool reports no exact count: per Claude
    /// turn, `max(0, turn_output − est(visible text + tool args))`, summed. Always
    /// `<= totals.output`. 0 (and `reasoning_source = None`) when there's nothing
    /// to estimate; 0 for Codex (which reports the exact count in `totals.reasoning`).
    pub est_reasoning_tokens: i64,
    /// Whether the reasoning figure is exact (`Reported`), estimated (`Inferred`),
    /// or unavailable (`None`). See [`ReasoningSource`].
    pub reasoning_source: ReasoningSource,
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

    #[test]
    fn all_enum_string_arms_are_stable() {
        // Cover every remaining as_str arm so the wire strings stay pinned.
        assert_eq!(Tool::Codex.as_str(), "codex");

        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");
        assert_eq!(Role::System.as_str(), "system");
        assert_eq!(Role::Other.as_str(), "other");

        assert_eq!(BlockType::Text.as_str(), "text");
        assert_eq!(BlockType::Thinking.as_str(), "thinking");
        assert_eq!(BlockType::ToolResult.as_str(), "tool_result");
        assert_eq!(BlockType::WebSearch.as_str(), "web_search");
        assert_eq!(BlockType::Image.as_str(), "image");
        assert_eq!(BlockType::Other.as_str(), "other");

        assert_eq!(ToolKind::Builtin.as_str(), "builtin");
        assert_eq!(ToolKind::Custom.as_str(), "custom");
        assert_eq!(ToolKind::WebSearch.as_str(), "web_search");
        assert_eq!(ToolKind::ToolSearch.as_str(), "tool_search");

        assert_eq!(ReasoningSource::Reported.as_str(), "reported");
        assert_eq!(ReasoningSource::Inferred.as_str(), "inferred");
        assert_eq!(ReasoningSource::None.as_str(), "none");
        assert_eq!(ReasoningSource::default(), ReasoningSource::None);
    }
}
