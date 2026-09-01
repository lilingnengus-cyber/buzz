use ed25519_dalek::SigningKey;
use sha2::{Digest, Sha256};
use std::fmt;
use subtle::ConstantTimeEq as _;

/// A service credential retained only as a fixed-length SHA-256 digest.
#[derive(Clone)]
pub struct ServiceToken {
    digest: [u8; 32],
}

impl ServiceToken {
    /// Validates and hashes a service credential for constant-time matching.
    pub fn parse(name: &str, value: String) -> Result<Self, String> {
        if !(32..=512).contains(&value.len())
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(format!("{name} must contain between 32 and 512 safe bytes"));
        }
        Ok(Self {
            digest: Sha256::digest(value.as_bytes()).into(),
        })
    }

    /// Compares a presented credential through fixed-length digests.
    pub fn matches(&self, presented: &str) -> bool {
        let presented: [u8; 32] = Sha256::digest(presented.as_bytes()).into();
        bool::from(self.digest.ct_eq(&presented))
    }

    pub(crate) fn same_as(&self, other: &Self) -> bool {
        bool::from(self.digest.ct_eq(&other.digest))
    }
}

impl fmt::Debug for ServiceToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ServiceToken(<redacted>)")
    }
}

impl fmt::Display for ServiceToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

/// Validated Ed25519 signing material for Life call grants.
#[derive(Clone)]
pub struct SigningKeyMaterial {
    key: SigningKey,
}

impl SigningKeyMaterial {
    /// Parses exactly 32 bytes encoded as 64 lower-case hexadecimal characters.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(
                "LIFE_AUTH_ED25519_PRIVATE_KEY must be 64 lower-case hexadecimal characters".into(),
            );
        }
        let mut bytes = [0_u8; 32];
        hex::decode_to_slice(value, &mut bytes).map_err(|_| {
            "LIFE_AUTH_ED25519_PRIVATE_KEY must be 64 lower-case hexadecimal characters".to_string()
        })?;
        Ok(Self {
            key: SigningKey::from_bytes(&bytes),
        })
    }

    /// Confirms that a public verification key can be derived.
    pub fn ready(&self) -> bool {
        self.key.verifying_key().to_bytes().len() == 32
    }

    /// Returns the public verification-key bytes without exposing private key material.
    pub fn verifying_key_bytes(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }
}

impl fmt::Debug for SigningKeyMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SigningKeyMaterial(<redacted>)")
    }
}

impl fmt::Display for SigningKeyMaterial {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_token_comparison_is_exact_and_redacted() {
        let raw = "a".repeat(32);
        let token = ServiceToken::parse("TOKEN", raw.clone()).expect("valid token");
        assert!(token.matches(&raw));
        assert!(!token.matches(&format!("{}b", &raw[..31])));
        assert!(!format!("{token:?}").contains(&raw));
        assert!(!format!("{token}").contains(&raw));
    }

    #[test]
    fn signing_key_is_redacted_and_can_prove_readiness() {
        let raw = "11".repeat(32);
        let key = SigningKeyMaterial::parse(&raw).expect("valid key");
        assert!(key.ready());
        assert!(!format!("{key:?}").contains(&raw));
        assert!(!format!("{key}").contains(&raw));
    }
}
