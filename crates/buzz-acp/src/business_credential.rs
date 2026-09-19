//! Local credential loading for desktop-managed Business agents.
use std::{fs::File, io::Read, path::Path};

pub(super) fn load(value: Option<String>, path: Option<String>) -> Result<String, String> {
    let credential = match (value, path) {
        (Some(_), Some(_)) => {
            return Err("Configure only one Business service credential source".into())
        }
        (Some(value), None) => value,
        (None, Some(path)) => read_file(Path::new(&path))?,
        (None, None) => return Err("Business service credential is required".into()),
    };
    if credential.len() < 32 || credential.len() > 4096 || credential.chars().any(char::is_control)
    {
        return Err(
            "Business service credential must be 32–4096 bytes without control characters".into(),
        );
    }
    Ok(credential)
}

fn read_file(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("Business credential file path must be absolute".into());
    }
    let file = File::open(path).map_err(|_| "Business credential file could not be opened")?;
    let metadata = file
        .metadata()
        .map_err(|_| "Business credential file metadata unavailable")?;
    if !metadata.is_file() || metadata.len() > 4097 {
        return Err("Business credential file must be a small regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("Business credential file must be accessible only to its owner".into());
        }
    }
    let mut contents = String::new();
    file.take(4098)
        .read_to_string(&mut contents)
        .map_err(|_| "Business credential file could not be read")?;
    Ok(contents.strip_suffix('\n').unwrap_or(&contents).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_ambiguous_and_invalid_credentials() {
        assert!(load(None, None).is_err());
        assert!(load(Some("x".repeat(32)), Some("/unused".into())).is_err());
        assert!(load(Some("short".into()), None).is_err());
        assert!(load(Some(format!("{}\n", "x".repeat(32))), None).is_err());
        assert!(load(None, Some("relative".into())).is_err());
        assert_eq!(load(Some("x".repeat(32)), None).unwrap(), "x".repeat(32));
    }

    #[cfg(unix)]
    #[test]
    fn reads_private_file_and_rejects_shared_permissions() {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let path =
            std::env::temp_dir().join(format!("business-credential-test-{}", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        writeln!(file, "{}", "t".repeat(43)).unwrap();
        assert_eq!(
            load(None, Some(path.display().to_string())).unwrap(),
            "t".repeat(43)
        );
        file.set_permissions(std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(load(None, Some(path.display().to_string())).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
