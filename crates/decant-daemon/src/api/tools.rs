//! `GET /api/v1/tools/usage` and `GET /api/v1/tools/mcp-usage`.

use axum::extract::{Query, State};
use serde::Deserialize;

use super::envelope::{ApiError, Envelope, Meta};
use super::filters::Filters;
use super::query;
use super::with_read_conn;
use crate::http::AppState;

/// Query params for tool usage: filters + `errors_only` + `limit`.
#[derive(Debug, Deserialize)]
pub struct ToolUsageParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub limit: Option<i64>,
    /// Presence (any value) or `true` keeps only tools with >0 errors.
    pub errors_only: Option<String>,
}

fn truthy(v: &Option<String>) -> bool {
    match v.as_deref() {
        Some("" | "true" | "1" | "yes") => true,
        Some(_) => false,
        None => false,
    }
}

/// `GET /api/v1/tools/usage` — per-tool calls/errors/error_rate.
pub async fn usage(
    State(state): State<AppState>,
    Query(params): Query<ToolUsageParams>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    let errors_only = truthy(&params.errors_only);
    let filters = Filters::parse(
        params.from,
        params.to,
        params.tool,
        params.model,
        params.project,
    )?;
    let limit = query::clamp_limit(params.limit);
    let filters_json = filters.as_json();

    let rows = with_read_conn(&state.read_pool, move |conn| {
        query::tool_usage(conn, &filters, errors_only, limit)
    })
    .await?;
    Ok(Envelope::with_meta(
        serde_json::json!(rows),
        Meta::now().with_filters(filters_json),
    ))
}

/// Query params for MCP usage: filters + `limit`.
#[derive(Debug, Deserialize)]
pub struct McpUsageParams {
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub limit: Option<i64>,
}

/// `GET /api/v1/tools/mcp-usage` — per-MCP-server rollup.
pub async fn mcp_usage(
    State(state): State<AppState>,
    Query(params): Query<McpUsageParams>,
) -> Result<Envelope<serde_json::Value>, ApiError> {
    let filters = Filters::parse(
        params.from,
        params.to,
        params.tool,
        params.model,
        params.project,
    )?;
    let limit = query::clamp_limit(params.limit);
    let filters_json = filters.as_json();

    let rows = with_read_conn(&state.read_pool, move |conn| {
        query::mcp_usage(conn, &filters, limit)
    })
    .await?;
    Ok(Envelope::with_meta(
        serde_json::json!(rows),
        Meta::now().with_filters(filters_json),
    ))
}
