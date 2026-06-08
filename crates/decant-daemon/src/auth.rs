use rand::RngCore;
use std::io;
use std::path::Path;

/// Read the bearer token from `path`, creating a fresh 32-byte (hex) token
/// with 0600 permissions if it does not yet exist.
pub fn load_or_create(path: &Path) -> io::Result<String> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let trimmed = existing.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    std::fs::write(path, &token)?;
    set_0600(path)?;
    Ok(token)
}

#[cfg(unix)]
fn set_0600(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_0600(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Constant-time comparison of a presented token against the expected one.
pub fn token_matches(expected: &str, presented: &str) -> bool {
    let a = expected.as_bytes();
    let b = presented.as_bytes();
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reuses_token() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.token");

        let first = load_or_create(&path).unwrap();
        assert_eq!(first.len(), 64); // 32 bytes hex-encoded

        let second = load_or_create(&path).unwrap();
        assert_eq!(first, second); // stable across calls

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
