use serde::{Deserialize, Serialize};

const KEYRING_KEY: &str = "workbench_oidc_user_v1";
const LIFE_OIDC_KEY: &str = "life_oidc_user_v1";
const LIFE_SESSION_KEY: &str = "life_workbench_session_v1";

fn storage_key(key: &str) -> Result<&'static str, String> {
    if key.is_empty() || key.len() > MAX_KEY_LENGTH {
        return Err("invalid Workbench credential key".into());
    }
    if key.starts_with("buzz.oidc.user.user:") {
        Ok(KEYRING_KEY)
    } else if key.starts_with("buzz.life-workbench.oidc.user.user:") {
        Ok(LIFE_OIDC_KEY)
    } else if key.starts_with("buzz.life-workbench.session:") {
        Ok(LIFE_SESSION_KEY)
    } else {
        Err("invalid Workbench credential key".into())
    }
}
const MAX_KEY_LENGTH: usize = 512;
const MAX_VALUE_LENGTH: usize = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct StoredUser {
    key: String,
    value: String,
}

fn validate_key(key: &str) -> Result<(), String> {
    storage_key(key).map(|_| ())
}

fn store() -> &'static crate::secret_store::SecretStore {
    crate::secret_store::SecretStore::shared(crate::app_state::keyring_service())
}

#[tauri::command]
pub fn workbench_oidc_user_load(key: String) -> Result<Option<String>, String> {
    validate_key(&key)?;
    let Some(raw) = store().load(storage_key(&key)?)? else {
        return Ok(None);
    };
    let saved: StoredUser = serde_json::from_str(&raw)
        .map_err(|_| "stored Workbench OIDC user is invalid".to_string())?;
    Ok((saved.key == key).then_some(saved.value))
}

#[tauri::command]
pub fn workbench_oidc_user_save(key: String, value: String) -> Result<(), String> {
    validate_key(&key)?;
    if value.is_empty() || value.len() > MAX_VALUE_LENGTH {
        return Err("invalid Workbench OIDC user value".into());
    }
    let saved_key = key.clone();
    let raw = serde_json::to_string(&StoredUser { key, value })
        .map_err(|_| "could not encode Workbench OIDC user".to_string())?;
    store().store(storage_key(&saved_key)?, &raw)
}

#[tauri::command]
pub fn workbench_oidc_user_delete(key: String) -> Result<(), String> {
    validate_key(&key)?;
    if workbench_oidc_user_load(key.clone())?.is_some() {
        store().delete(storage_key(&key)?)?;
    }
    Ok(())
}

#[tauri::command]
pub fn workbench_oidc_user_keys() -> Result<Vec<String>, String> {
    let mut keys = Vec::new();
    for slot in [KEYRING_KEY, LIFE_OIDC_KEY] {
        if let Some(raw) = store().load(slot)? {
            let saved: StoredUser = serde_json::from_str(&raw)
                .map_err(|_| "stored Workbench OIDC user is invalid".to_string())?;
            validate_key(&saved.key)?;
            keys.push(saved.key);
        }
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::{storage_key, validate_key};

    #[test]
    fn accepts_only_oidc_user_keys() {
        assert!(validate_key("buzz.oidc.user.user:https://issuer:client").is_ok());
        assert!(validate_key("buzz.life-workbench.oidc.user.user:https://issuer:client").is_ok());
        assert!(validate_key("buzz.life-workbench.session:https://gateway:subject").is_ok());
        assert_ne!(
            storage_key("buzz.oidc.user.user:x").unwrap(),
            storage_key("buzz.life-workbench.oidc.user.user:x").unwrap()
        );
        assert_ne!(
            storage_key("buzz.life-workbench.session:x").unwrap(),
            storage_key("buzz.life-workbench.oidc.user.user:x").unwrap()
        );
        assert!(validate_key("user:https://issuer:client").is_err());
        assert!(validate_key("state:opaque").is_err());
        assert!(validate_key("").is_err());
    }
}
