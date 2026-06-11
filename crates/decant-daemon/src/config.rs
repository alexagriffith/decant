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

    #[test]
    fn invalid_port_falls_back_to_default() {
        // A non-numeric port string parses to None -> default.
        let c = Config::from_values(Some("not-a-port".into()), None, None);
        assert_eq!(c.port, 4577);
    }

    #[test]
    fn config_dir_and_db_overrides_drive_paths() {
        let c = Config::from_values(None, Some("/tmp/cfg".into()), Some("/tmp/x.db".into()));
        assert_eq!(c.config_dir, std::path::PathBuf::from("/tmp/cfg"));
        assert_eq!(c.db_path, std::path::PathBuf::from("/tmp/x.db"));
        assert_eq!(
            c.token_path(),
            std::path::PathBuf::from("/tmp/cfg/daemon.token")
        );
        assert_eq!(
            c.lock_path(),
            std::path::PathBuf::from("/tmp/cfg/daemon.lock")
        );
    }

    // Guards the env-var manipulation below: `from_env` reads process-wide vars,
    // so serialize the one test that mutates them.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Set or clear an env var, returning a guard that restores the prior value.
    struct EnvVarGuard {
        key: &'static str,
        prev: Option<String>,
    }
    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }
    }
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn from_env_reads_process_environment() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _p = EnvVarGuard::set("DECANT_DAEMON_PORT", "5123");
        let _c = EnvVarGuard::set("DECANT_CONFIG_DIR", "/tmp/from-env-cfg");
        let _d = EnvVarGuard::set("DECANT_DB", "/tmp/from-env.db");

        let cfg = Config::from_env();
        assert_eq!(cfg.port, 5123);
        assert_eq!(
            cfg.config_dir,
            std::path::PathBuf::from("/tmp/from-env-cfg")
        );
        assert_eq!(cfg.db_path, std::path::PathBuf::from("/tmp/from-env.db"));
    }

    #[test]
    fn env_var_guard_restores_a_previous_value() {
        let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Seed a pre-existing value, then guard the same key: on drop the guard
        // must restore the original (the `Some(v) => set_var` arm).
        std::env::set_var("DECANT_DAEMON_PORT", "original");
        {
            let _g = EnvVarGuard::set("DECANT_DAEMON_PORT", "overridden");
            assert_eq!(std::env::var("DECANT_DAEMON_PORT").unwrap(), "overridden");
        }
        assert_eq!(std::env::var("DECANT_DAEMON_PORT").unwrap(), "original");
        std::env::remove_var("DECANT_DAEMON_PORT");
    }
}
