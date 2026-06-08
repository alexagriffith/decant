use std::path::PathBuf;

/// Daemon configuration, resolved from environment with sensible defaults.
#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub config_dir: PathBuf,
    pub db_path: PathBuf,
}

impl Config {
    /// Resolve from process environment.
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var("DECANT_DAEMON_PORT").ok(),
            std::env::var("DECANT_CONFIG_DIR").ok(),
            std::env::var("DECANT_DB").ok(),
        )
    }

    /// Resolve from explicit optional values (testable).
    pub fn from_values(
        port: Option<String>,
        config_dir: Option<String>,
        db: Option<String>,
    ) -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let config_dir = config_dir
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".decant"));
        let db_path = db
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&home).join(".decant").join("decant.db"));
        let port = port.and_then(|p| p.parse().ok()).unwrap_or(4577);
        Self {
            port,
            config_dir,
            db_path,
        }
    }

    pub fn bind_addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn token_path(&self) -> PathBuf {
        self.config_dir.join("daemon.token")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.config_dir.join("daemon.lock")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_env_absent() {
        let c = Config::from_values(None, None, None);
        assert_eq!(c.port, 4577);
        assert!(c.config_dir.ends_with(".decant"));
        assert!(c.bind_addr().starts_with("127.0.0.1:"));
    }

    #[test]
    fn port_override_parses() {
        let c = Config::from_values(Some("9000".into()), None, None);
        assert_eq!(c.port, 9000);
    }
}
