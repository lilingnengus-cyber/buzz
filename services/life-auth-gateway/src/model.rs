use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_id {
    ($name:ident, $docs:literal) => {
        #[doc = $docs]
        #[derive(
            Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Wraps an already validated UUID in its domain-specific type.
            pub const fn new(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the underlying UUID for persistence and wire encoding.
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }
    };
}

uuid_id!(
    LifeWorkbenchUserId,
    "Opaque identifier for a mapped Life Workbench user."
);
uuid_id!(
    IdentityBindingChallengeId,
    "Opaque identifier for a one-time Nostr identity-binding challenge."
);
uuid_id!(
    WorkbenchSessionId,
    "Opaque identifier for a Workbench OIDC session."
);
uuid_id!(
    AgentDelegationId,
    "Opaque identifier for a Life Agent delegation."
);
uuid_id!(
    EmbedCodeId,
    "Opaque identifier for a one-time Dock embed code."
);
uuid_id!(EmbedSessionId, "Opaque identifier for a Life Dock session.");
uuid_id!(
    WriteCommandConfirmationId,
    "Opaque identifier for an exact Life write confirmation."
);
