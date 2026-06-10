//! Dashboard filter parsing + a parameterized SQL WHERE builder over the
//! `session` alias `s`, mirroring `web/lib/decant_web/filters.ex` +
//! `Decant.Archive.where_clause/1`:
//!
//! - `from` (inclusive) → `s.started_at >= from`
//! - `to`   (inclusive) → `s.started_at < (to + 1 day)` so the whole `to` day is included
//! - `tool` / `model`   → exact match
//! - `project`          → match root row and every worktree rolled up under it (path fallback for unresolved rows)
//!
//! Dates must be `YYYY-MM-DD`; anything else is a 400 (never a silent no-op).

use rusqlite::types::Value as SqlValue;
use serde_json::{json, Value};

use super::envelope::ApiError;

/// Parsed, validated dashboard filters. All fields optional.
#[derive(Debug, Default, Clone)]
pub struct Filters {
    pub from: Option<String>,
    pub to: Option<String>,
    pub tool: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
}

/// A built WHERE fragment plus its positional bind parameters. The fragment is
/// always a complete boolean expression (defaults to `1=1`) so callers can splice
/// it after `WHERE`. Bind params are appended to any the caller already has.
pub struct WhereClause {
    pub sql: String,
    pub params: Vec<SqlValue>,
}

impl Filters {
    /// Parse from raw query params. Empty strings are treated as absent. Dates
    /// are validated to `YYYY-MM-DD`.
    pub fn parse(
        from: Option<String>,
        to: Option<String>,
        tool: Option<String>,
        model: Option<String>,
        project: Option<String>,
    ) -> Result<Filters, ApiError> {
        Ok(Filters {
            from: parse_date(from, "from")?,
            to: parse_date(to, "to")?,
            tool: present(tool),
            model: present(model),
            project: present(project),
        })
    }

    /// Build the parameterized WHERE clause (on alias `s`). Inclusive `to` via
    /// the next-day upper bound, matching the web app.
    pub fn where_clause(&self) -> WhereClause {
        let mut clauses: Vec<&str> = Vec::new();
        let mut params: Vec<SqlValue> = Vec::new();

        if let Some(from) = &self.from {
            clauses.push("s.started_at >= ?");
            params.push(SqlValue::Text(from.clone()));
        }
        if let Some(to) = &self.to {
            clauses.push("s.started_at < ?");
            params.push(SqlValue::Text(day_after(to)));
        }
        if let Some(tool) = &self.tool {
            clauses.push("s.tool = ?");
            params.push(SqlValue::Text(tool.clone()));
        }
        if let Some(model) = &self.model {
            clauses.push("s.model = ?");
            params.push(SqlValue::Text(model.clone()));
        }
        if let Some(project) = &self.project {
            // A "project" value is a resolved root_path: match the root row and
            // every worktree that rolls up under it (plus a path fallback for any
            // not-yet-resolved row).
            clauses
                .push("s.project_id IN (SELECT id FROM project WHERE root_path = ? OR path = ?)");
            params.push(SqlValue::Text(project.clone()));
            params.push(SqlValue::Text(project.clone()));
        }

        let sql = if clauses.is_empty() {
            "1=1".to_string()
        } else {
            clauses.join(" AND ")
        };
        WhereClause { sql, params }
    }

    /// The applied filters as JSON for `meta.filters_applied` (only set keys).
    pub fn as_json(&self) -> Value {
        let mut m = serde_json::Map::new();
        if let Some(v) = &self.from {
            m.insert("from".into(), json!(v));
        }
        if let Some(v) = &self.to {
            m.insert("to".into(), json!(v));
        }
        if let Some(v) = &self.tool {
            m.insert("tool".into(), json!(v));
        }
        if let Some(v) = &self.model {
            m.insert("model".into(), json!(v));
        }
        if let Some(v) = &self.project {
            m.insert("project".into(), json!(v));
        }
        Value::Object(m)
    }
}

fn present(s: Option<String>) -> Option<String> {
    s.filter(|v| !v.is_empty())
}

/// Validate an optional `YYYY-MM-DD` date string.
fn parse_date(s: Option<String>, field: &str) -> Result<Option<String>, ApiError> {
    match present(s) {
        None => Ok(None),
        Some(v) => match chrono::NaiveDate::parse_from_str(&v, "%Y-%m-%d") {
            Ok(_) => Ok(Some(v)),
            Err(_) => Err(ApiError::invalid_filter(format!(
                "`{field}` must be an ISO date (YYYY-MM-DD), got {v:?}"
            ))
            .with_hint("example: 2026-06-01")),
        },
    }
}

/// The day after `date` (YYYY-MM-DD), used as an exclusive upper bound so `to`
/// is inclusive of the whole day. `date` is already validated by `parse_date`.
fn day_after(date: &str) -> String {
    match chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(d) => (d + chrono::Duration::days(1))
            .format("%Y-%m-%d")
            .to_string(),
        // Unreachable: only validated dates reach here. Fall back to the input so
        // a hypothetical bad value can't panic the request.
        Err(_) => date.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filters_match_all() {
        let f = Filters::default();
        let w = f.where_clause();
        assert_eq!(w.sql, "1=1");
        assert!(w.params.is_empty());
    }

    #[test]
    fn full_filters_build_clause_and_params() {
        let f = Filters::parse(
            Some("2026-05-01".into()),
            Some("2026-05-31".into()),
            Some("codex".into()),
            Some("gpt-5.5".into()),
            Some("/p".into()),
        )
        .unwrap();
        let w = f.where_clause();
        assert!(w.sql.contains("s.started_at >= ?"));
        assert!(w.sql.contains("s.started_at < ?"));
        assert!(w.sql.contains("s.tool = ?"));
        assert!(w.sql.contains("s.model = ?"));
        assert!(w.sql.contains("project WHERE root_path = ? OR path = ?"));
        assert_eq!(w.params.len(), 6);
    }

    #[test]
    fn to_is_inclusive_via_next_day_bound() {
        let f = Filters::parse(None, Some("2026-05-31".into()), None, None, None).unwrap();
        let w = f.where_clause();
        // The upper bound must be the *next* day so 2026-05-31 rows are included.
        match &w.params[0] {
            SqlValue::Text(s) => assert_eq!(s, "2026-06-01"),
            other => panic!("expected text bound, got {other:?}"),
        }
    }

    #[test]
    fn bad_date_is_invalid_filter() {
        let err = Filters::parse(Some("05/01/2026".into()), None, None, None, None).unwrap_err();
        assert_eq!(err.status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(err.code, "INVALID_FILTER");
    }

    #[test]
    fn empty_strings_are_absent() {
        let f = Filters::parse(
            Some("".into()),
            None,
            Some("".into()),
            None,
            Some("".into()),
        )
        .unwrap();
        assert!(f.from.is_none());
        assert!(f.tool.is_none());
        assert!(f.project.is_none());
        assert_eq!(f.where_clause().sql, "1=1");
    }

    #[test]
    fn as_json_only_includes_set_keys() {
        let f = Filters::parse(None, None, Some("codex".into()), None, None).unwrap();
        let j = f.as_json();
        assert_eq!(j, json!({"tool": "codex"}));
    }
}
