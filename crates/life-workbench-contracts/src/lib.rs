//! Shared, fail-closed contracts for the LifeOS Workbench integration.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Versioned fixed Life tool catalog.
pub mod catalog;
/// Strict API and extension result envelopes plus `life://` resource references.
pub mod result;

/// Canonical input serialization failure.
#[derive(Debug, thiserror::Error)]
pub enum CanonicalJsonError {
    /// The input could not be represented as finite JSON.
    #[error("input is not valid finite JSON")]
    InvalidJson,
}

/// Serializes input as compact JSON with recursively sorted object keys.
///
/// Array order is preserved. Callers parse the wire input into [`Value`] first,
/// so JSON-invalid values such as non-finite floating-point numbers are rejected
/// before canonicalization.
pub fn canonical_json(input: &Value) -> Result<String, CanonicalJsonError> {
    let mut output = String::new();
    write_canonical(input, &mut output)?;
    Ok(output)
}

/// Returns `sha256:<lower-hex>` over the canonical JSON bytes.
pub fn normalized_input_hash(input: &Value) -> Result<String, CanonicalJsonError> {
    let canonical = canonical_json(input)?;
    Ok(format!(
        "sha256:{}",
        hex::encode(Sha256::digest(canonical.as_bytes()))
    ))
}

fn write_canonical(value: &Value, output: &mut String) -> Result<(), CanonicalJsonError> {
    match value {
        Value::Null => output.push_str("null"),
        Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
        Value::Number(value) => {
            if value.as_f64().is_some_and(|value| !value.is_finite()) {
                return Err(CanonicalJsonError::InvalidJson);
            }
            output.push_str(&value.to_string());
        }
        Value::String(value) => output
            .push_str(&serde_json::to_string(value).map_err(|_| CanonicalJsonError::InvalidJson)?),
        Value::Array(values) => {
            output.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                write_canonical(value, output)?;
            }
            output.push(']');
        }
        Value::Object(values) => {
            output.push('{');
            let mut entries = values.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    output.push(',');
                }
                output.push_str(
                    &serde_json::to_string(key).map_err(|_| CanonicalJsonError::InvalidJson)?,
                );
                output.push(':');
                write_canonical(value, output)?;
            }
            output.push('}');
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn canonical_json_sorts_nested_keys_and_preserves_array_order() {
        let input = json!({"z": 1, "a": {"two": 2, "one": 1}, "items": [3, 2, 1]});
        assert_eq!(
            canonical_json(&input).expect("canonical JSON"),
            r#"{"a":{"one":1,"two":2},"items":[3,2,1],"z":1}"#
        );
    }

    #[test]
    fn canonical_hash_is_stable_lower_hex_and_rejects_non_finite_numbers() {
        let left = json!({"b": [2, 1], "a": true});
        let right = json!({"a": true, "b": [2, 1]});
        let hash = normalized_input_hash(&left).expect("hash");
        assert_eq!(hash, normalized_input_hash(&right).expect("same hash"));
        assert_eq!(hash.len(), 71);
        assert!(hash
            .strip_prefix("sha256:")
            .expect("hash prefix")
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        assert!(serde_json::from_str::<Value>("NaN").is_err());
        assert!(serde_json::from_str::<Value>("Infinity").is_err());
    }
}
