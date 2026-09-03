//! Low-sensitivity security-audit vocabulary.

use uuid::Uuid;

/// Fixed audit outcome, suitable for storage and low-cardinality metrics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditOutcome {
    /// The guarded operation completed.
    Success,
    /// Policy denied the guarded operation.
    Denied,
    /// A dependency or invariant failed closed.
    Failure,
}

impl AuditOutcome {
    /// Returns the stable database/metric value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Denied => "denied",
            Self::Failure => "failure",
        }
    }
}

/// Audit correlation fields that may be logged without personal content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuditCorrelation {
    /// Distributed trace identifier.
    pub trace_id: Uuid,
    /// Fixed event type from code, never caller text.
    pub event_type: &'static str,
    /// Fixed outcome.
    pub outcome: AuditOutcome,
    /// Optional stable machine reason from code.
    pub reason_code: Option<&'static str>,
}

impl AuditCorrelation {
    /// Emits a structured, content-free audit edge.
    pub fn emit(&self) {
        tracing::info!(
            trace_id = %self.trace_id,
            event_type = self.event_type,
            outcome = self.outcome.as_str(),
            reason_code = self.reason_code.unwrap_or("none"),
            "life security decision"
        );
    }
}
