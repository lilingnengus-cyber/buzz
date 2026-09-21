//! Exercise the production command module with a deterministic secret-store backend.
//! OS keychain persistence requires separate installed-app acceptance.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

mod app_state {
    pub fn keyring_service() -> &'static str {
        "credential-isolation-test"
    }
}
mod secret_store {
    use super::*;
    pub struct SecretStore(Mutex<HashMap<String, String>>);
    impl SecretStore {
        pub fn shared(_: &str) -> &'static Self {
            static STORE: OnceLock<SecretStore> = OnceLock::new();
            STORE.get_or_init(|| SecretStore(Mutex::new(HashMap::new())))
        }
        pub fn load(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().map_err(|_| "lock")?.get(key).cloned())
        }
        pub fn store(&self, key: &str, value: &str) -> Result<(), String> {
            self.0
                .lock()
                .map_err(|_| "lock")?
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        pub fn delete(&self, key: &str) -> Result<(), String> {
            self.0.lock().map_err(|_| "lock")?.remove(key);
            Ok(())
        }
    }
}
#[path = "../src/workbench_oidc_store.rs"]
mod commands;

#[test]
fn life_oidc_and_recovery_persist_without_overwriting_business_credentials() {
    use commands::*;
    let business = "buzz.oidc.user.user:https://issuer:business".to_string();
    let life = "buzz.life-workbench.oidc.user.user:https://issuer:life".to_string();
    let recovery = "buzz.life-workbench.session:https://gateway:pubkey".to_string();
    for (key, value) in [
        (&business, "business"),
        (&life, "life"),
        (&recovery, "recovery"),
    ] {
        workbench_oidc_user_save(key.clone(), value.into()).unwrap();
    }
    assert_eq!(
        workbench_oidc_user_load(business.clone())
            .unwrap()
            .as_deref(),
        Some("business")
    );
    assert_eq!(
        workbench_oidc_user_load(life.clone()).unwrap().as_deref(),
        Some("life")
    );
    assert_eq!(
        workbench_oidc_user_load(recovery.clone())
            .unwrap()
            .as_deref(),
        Some("recovery")
    );
    assert_eq!(
        workbench_oidc_user_keys().unwrap(),
        vec![business.clone(), life.clone()]
    );
    assert!(workbench_oidc_user_load(format!("{life}-other-client"))
        .unwrap()
        .is_none());
    workbench_oidc_user_delete(business).unwrap();
    assert_eq!(
        workbench_oidc_user_load(life.clone()).unwrap().as_deref(),
        Some("life")
    );
    workbench_oidc_user_delete(recovery.clone()).unwrap();
    assert!(workbench_oidc_user_load(recovery).unwrap().is_none());
    assert_eq!(
        workbench_oidc_user_load(life).unwrap().as_deref(),
        Some("life")
    );
}
