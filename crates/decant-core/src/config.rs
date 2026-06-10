use directories::BaseDirs;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub db_path: PathBuf,
    pub claude_dir: PathBuf,
    pub codex_dir: PathBuf,
}

impl Config {
    /// Resolve with precedence: explicit override > env > platform default.
    pub fn resolve(
        db_override: Option<PathBuf>,
        claude_override: Option<PathBuf>,
        codex_override: Option<PathBuf>,
    ) -> Config {
        let home = BaseDirs::new()
            .map(|b| b.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        // Same default as the daemon's config: one archive for all surfaces.
        let db_path = db_override
            .or_else(|| std::env::var_os("DECANT_DB").map(PathBuf::from))
            .unwrap_or_else(|| home.join(".decant/decant.db"));
        let claude_dir = claude_override
            .or_else(|| std::env::var_os("DECANT_CLAUDE_DIR").map(PathBuf::from))
            .unwrap_or_else(|| home.join(".claude/projects"));
        let codex_dir = codex_override
            .or_else(|| std::env::var_os("DECANT_CODEX_DIR").map(PathBuf::from))
            .unwrap_or_else(|| home.join(".codex"));

        Config {
            db_path,
            claude_dir,
            codex_dir,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_override_wins() {
        let c = Config::resolve(Some(PathBuf::from("/tmp/x.db")), None, None);
        assert_eq!(c.db_path, PathBuf::from("/tmp/x.db"));
    }

    #[test]
    fn defaults_point_into_home() {
        let c = Config::resolve(None, None, None);
        assert!(c.claude_dir.ends_with(".claude/projects"));
        assert!(c.db_path.ends_with(".decant/decant.db"));
    }
}
