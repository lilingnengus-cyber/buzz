//! Strict Life Workbench responses and trusted resource references.

use serde::{
    de::{Error as _, Unexpected},
    Deserialize, Deserializer, Serialize, Serializer,
};
use uuid::Uuid;

/// Literal JSON `true` used by successful API envelopes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SuccessMarker;

impl Serialize for SuccessMarker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for SuccessMarker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Ok(Self)
        } else {
            Err(D::Error::invalid_value(
                Unexpected::Bool(false),
                &"the literal true",
            ))
        }
    }
}

/// Literal JSON `false` used by failed API envelopes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FailureMarker;

impl Serialize for FailureMarker {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(false)
    }
}

/// Literal JSON string `life` used by extension result envelopes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LifeExtensionId;

impl Serialize for LifeExtensionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("life")
    }
}

impl<'de> Deserialize<'de> for LifeExtensionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value == "life" {
            Ok(Self)
        } else {
            Err(D::Error::invalid_value(
                Unexpected::Str(&value),
                &"the literal life",
            ))
        }
    }
}

impl<'de> Deserialize<'de> for FailureMarker {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if bool::deserialize(deserializer)? {
            Err(D::Error::invalid_value(
                Unexpected::Bool(true),
                &"the literal false",
            ))
        } else {
            Ok(Self)
        }
    }
}

/// Fixed `life` URI scheme marker.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceScheme {
    /// A resource owned by the isolated Life extension.
    Life,
}

/// Resource kinds accepted by the `life://` protocol.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceType {
    /// Life dashboard.
    Dashboard,
    /// Domain.
    Domain,
    /// Goal.
    Goal,
    /// Project.
    Project,
    /// Action.
    Action,
    /// Calendar date.
    Calendar,
    /// Journal entry.
    Journal,
    /// Knowledge item.
    Knowledge,
    /// Review.
    Review,
    /// AI execution.
    AiExecution,
    /// Server-created draft.
    Draft,
}

impl ResourceType {
    fn uri_segment(self) -> &'static str {
        match self {
            Self::Dashboard => "dashboard",
            Self::Domain => "domain",
            Self::Goal => "goal",
            Self::Project => "project",
            Self::Action => "action",
            Self::Calendar => "calendar",
            Self::Journal => "journal",
            Self::Knowledge => "knowledge",
            Self::Review => "review",
            Self::AiExecution => "ai-execution",
            Self::Draft => "draft",
        }
    }
}

/// Validated resource-reference construction error.
#[derive(Debug, thiserror::Error)]
pub enum ResourceRefError {
    /// The resource kind, identifier, version, or title is invalid.
    #[error("Life resource reference is invalid")]
    Invalid,
}

/// A trusted Life resource with a deterministic `life://` location.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifeResourceRef {
    scheme: ResourceScheme,
    #[serde(rename = "type")]
    resource_type: ResourceType,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RawResourceRef {
    scheme: ResourceScheme,
    #[serde(rename = "type")]
    resource_type: ResourceType,
    id: Option<String>,
    version: Option<u64>,
    title: Option<String>,
}

impl<'de> Deserialize<'de> for LifeResourceRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawResourceRef::deserialize(deserializer)?;
        if raw.scheme != ResourceScheme::Life {
            return Err(D::Error::custom("resource scheme must be life"));
        }
        Self::new(raw.resource_type, raw.id, raw.version, raw.title)
            .map_err(|_| D::Error::custom("invalid Life resource reference"))
    }
}

impl LifeResourceRef {
    /// Validates and creates a resource reference.
    pub fn new(
        resource_type: ResourceType,
        id: Option<String>,
        version: Option<u64>,
        title: Option<String>,
    ) -> Result<Self, ResourceRefError> {
        let id_valid = match (resource_type, id.as_deref()) {
            (ResourceType::Dashboard, None) => true,
            (ResourceType::Dashboard, Some(_)) | (_, None) => false,
            (_, Some(id)) => safe_id(id),
        };
        if !id_valid
            || version == Some(0)
            || title.as_deref().is_some_and(|title| !safe_text(title, 256))
        {
            return Err(ResourceRefError::Invalid);
        }
        Ok(Self {
            scheme: ResourceScheme::Life,
            resource_type,
            id,
            version,
            title,
        })
    }

    /// Returns the resource kind.
    pub const fn resource_type(&self) -> ResourceType {
        self.resource_type
    }

    /// Returns the opaque resource identifier, absent only for the dashboard.
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Returns the trusted optimistic version when the service supplied one.
    pub const fn version(&self) -> Option<u64> {
        self.version
    }

    /// Returns the bounded display title when supplied.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Builds the deterministic `life://` URI for this validated reference.
    pub fn life_uri(&self) -> String {
        self.id.as_ref().map_or_else(
            || format!("life://{}", self.resource_type.uri_segment()),
            |id| format!("life://{}/{id}", self.resource_type.uri_segment()),
        )
    }
}

/// Stable Workbench error codes safe to return across the MCP boundary.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Input failed strict validation.
    ValidationFailed,
    /// Tool is not in the fixed catalog.
    UnknownTool,
    /// No active Life identity binding exists.
    BindingRequired,
    /// Current principal is inactive.
    PrincipalInactive,
    /// Current scope does not authorize the resource.
    ScopeDenied,
    /// Operation requires a direct-message context.
    DmRequired,
    /// Operation requires a new exact confirmation.
    ConfirmationRequired,
    /// Optimistic resource version is stale.
    VersionConflict,
    /// Write command has already been consumed.
    CommandConsumed,
    /// Write command has expired.
    CommandExpired,
    /// A bounded rate limit was reached.
    RateLimited,
    /// Life Gateway is unavailable.
    GatewayUnavailable,
    /// LifeOS Workbench API is unavailable.
    LifeApiUnavailable,
    /// A timed-out write has an unknown outcome and must not be retried blindly.
    WriteOutcomeUnknown,
    /// Internal failure with details withheld.
    InternalError,
}

/// Safe error detail returned to the MCP.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErrorDetail {
    /// Stable machine-readable code.
    pub code: ErrorCode,
    /// Sanitized user-facing message.
    pub message: String,
    /// Whether the caller may follow the documented retry policy.
    pub retryable: bool,
}

/// Successful Workbench API envelope.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkbenchSuccess<T> {
    /// Literal success marker.
    pub ok: SuccessMarker,
    /// Strict route-specific response data.
    pub data: T,
    /// Trusted resource references supplied by LifeOS.
    pub resource_refs: Vec<LifeResourceRef>,
    /// Domain audit identifier created with the operation.
    pub audit_id: Uuid,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// Failed Workbench API envelope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkbenchFailure {
    /// Literal failure marker.
    pub ok: FailureMarker,
    /// Sanitized stable error.
    pub error: ErrorDetail,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
}

/// Strict success-or-failure response returned by a LifeOS Workbench route.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WorkbenchResult<T> {
    /// A server-confirmed success.
    Success(WorkbenchSuccess<T>),
    /// A sanitized stable failure.
    Failure(WorkbenchFailure),
}

/// Final extension status accepted by Pacioli.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionStatus {
    /// Server confirmed that the operation succeeded.
    Succeeded,
    /// Server confirmed that the operation failed.
    Failed,
}

/// Sanitized result emitted by the Life MCP and accepted by the host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LifeExtensionResult {
    /// Must be exactly `life`.
    pub extension_id: LifeExtensionId,
    /// Stable semantic operation name.
    pub operation: String,
    /// Server-confirmed outcome.
    pub status: ExtensionStatus,
    /// Sanitized server-provided summary.
    pub summary: String,
    /// Trusted resource references supplied by LifeOS.
    pub resource_refs: Vec<LifeResourceRef>,
    /// Distributed trace identifier.
    pub trace_id: Uuid,
    /// Domain audit identifier when one was created.
    pub audit_id: Option<Uuid>,
}

impl LifeExtensionResult {
    /// Enforces the fixed extension id and bounded, non-empty display text.
    pub fn validate(&self) -> Result<(), ResourceRefError> {
        if !safe_operation(&self.operation)
            || !safe_text(&self.summary, 2_000)
            || self.resource_refs.len() > 100
            || (self.status == ExtensionStatus::Succeeded && self.audit_id.is_none())
        {
            return Err(ResourceRefError::Invalid);
        }
        Ok(())
    }
}

fn safe_id(value: &str) -> bool {
    (1..=128).contains(&value.chars().count())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b':')
        })
}

fn safe_operation(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || matches!(byte, b'.' | b'_'))
}

fn safe_text(value: &str, max: usize) -> bool {
    let length = value.chars().count();
    (1..=max).contains(&length) && value.trim() == value && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resource_refs_build_only_fixed_safe_life_uris() {
        let action = LifeResourceRef::new(
            ResourceType::Action,
            Some("action_123".into()),
            Some(8),
            Some("Complete interface design".into()),
        )
        .expect("action ref");
        assert_eq!(action.life_uri(), "life://action/action_123");
        let execution = LifeResourceRef::new(
            ResourceType::AiExecution,
            Some("run-1".into()),
            Some(1),
            None,
        )
        .expect("execution ref");
        assert_eq!(execution.life_uri(), "life://ai-execution/run-1");
        assert!(
            LifeResourceRef::new(ResourceType::Action, Some("../other".into()), Some(1), None,)
                .is_err()
        );
        assert!(LifeResourceRef::new(ResourceType::Dashboard, None, None, None).is_ok());
    }

    #[test]
    fn result_envelopes_reject_wrong_markers_unknown_fields_and_unsafe_refs() {
        let trace_id = Uuid::new_v4();
        let audit_id = Uuid::new_v4();
        let success = json!({
            "ok": true,
            "data": {"version": 8},
            "resourceRefs": [{
                "scheme": "life", "type": "action", "id": "action-1", "version": 8
            }],
            "auditId": audit_id,
            "traceId": trace_id
        });
        assert!(serde_json::from_value::<WorkbenchSuccess<serde_json::Value>>(success).is_ok());

        let wrong_marker = json!({
            "ok": false, "data": {}, "resourceRefs": [],
            "auditId": audit_id, "traceId": trace_id
        });
        assert!(
            serde_json::from_value::<WorkbenchSuccess<serde_json::Value>>(wrong_marker).is_err()
        );
        let unknown = json!({
            "ok": false,
            "error": {"code": "scope_denied", "message": "denied", "retryable": false},
            "traceId": trace_id,
            "rawError": "database detail"
        });
        assert!(serde_json::from_value::<WorkbenchFailure>(unknown).is_err());
        let unsafe_ref = json!({
            "ok": true,
            "data": {},
            "resourceRefs": [{"scheme": "life", "type": "action", "id": "../escape"}],
            "auditId": audit_id,
            "traceId": trace_id
        });
        assert!(serde_json::from_value::<WorkbenchSuccess<serde_json::Value>>(unsafe_ref).is_err());
    }

    #[test]
    fn extension_results_require_the_life_domain_and_bounded_summary() {
        let valid = LifeExtensionResult {
            extension_id: LifeExtensionId,
            operation: "action.status.update".into(),
            status: ExtensionStatus::Succeeded,
            summary: "Action completed".into(),
            resource_refs: vec![],
            trace_id: Uuid::new_v4(),
            audit_id: Some(Uuid::new_v4()),
        };
        assert!(valid.validate().is_ok());
        let mut encoded = serde_json::to_value(valid).expect("serialize extension result");
        encoded["extensionId"] = json!("business");
        assert!(serde_json::from_value::<LifeExtensionResult>(encoded).is_err());
        let missing_audit = LifeExtensionResult {
            extension_id: LifeExtensionId,
            operation: "action.status.update".into(),
            status: ExtensionStatus::Succeeded,
            summary: "Action completed".into(),
            resource_refs: vec![],
            trace_id: Uuid::new_v4(),
            audit_id: None,
        };
        assert!(missing_audit.validate().is_err());
    }
}
