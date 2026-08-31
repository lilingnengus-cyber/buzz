use std::sync::Arc;

use crate::turn_observer::{TurnApplicability, TurnExtension, VerifiedTurnContext};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RegistryError {
    InvalidContext(String),
    DuplicateId(&'static str),
    Classification {
        extension_id: &'static str,
        reason: String,
    },
    Ambiguous {
        extension_ids: Vec<&'static str>,
        reason: String,
    },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidContext(reason) => {
                write!(formatter, "invalid verified turn context: {reason}")
            }
            Self::DuplicateId(id) => write!(formatter, "duplicate turn extension id: {id}"),
            Self::Classification {
                extension_id,
                reason,
            } => write!(
                formatter,
                "turn extension {extension_id} could not classify the turn: {reason}"
            ),
            Self::Ambiguous {
                extension_ids,
                reason,
            } => write!(
                formatter,
                "turn matches multiple product extensions ({}): {reason}",
                extension_ids.join(", ")
            ),
        }
    }
}

pub(crate) struct TurnExtensionRegistry {
    extensions: Vec<Arc<dyn TurnExtension>>,
}

impl TurnExtensionRegistry {
    pub(crate) fn new(mut extensions: Vec<Arc<dyn TurnExtension>>) -> Result<Self, RegistryError> {
        extensions.sort_by_key(|extension| extension.id());
        for pair in extensions.windows(2) {
            if pair[0].id() == pair[1].id() {
                return Err(RegistryError::DuplicateId(pair[0].id()));
            }
        }
        Ok(Self { extensions })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.extensions.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn ids(&self) -> Vec<&'static str> {
        self.extensions
            .iter()
            .map(|extension| extension.id())
            .collect()
    }

    pub(crate) fn select(
        &self,
        context: &VerifiedTurnContext<'_>,
    ) -> Result<Option<Arc<dyn TurnExtension>>, RegistryError> {
        context.validate().map_err(RegistryError::InvalidContext)?;
        let mut applicable = Vec::new();
        for extension in &self.extensions {
            match extension.classify_turn(context).map_err(|reason| {
                RegistryError::Classification {
                    extension_id: extension.id(),
                    reason,
                }
            })? {
                TurnApplicability::NotApplicable => {}
                TurnApplicability::Applicable { priority, reason } => {
                    applicable.push((priority, reason, Arc::clone(extension)));
                }
                TurnApplicability::Ambiguous { reason } => {
                    return Err(RegistryError::Ambiguous {
                        extension_ids: vec![extension.id()],
                        reason: reason.to_string(),
                    });
                }
            }
        }
        let Some(highest_priority) = applicable.iter().map(|item| item.0).max() else {
            return Ok(None);
        };
        let mut highest = applicable
            .into_iter()
            .filter(|item| item.0 == highest_priority)
            .collect::<Vec<_>>();
        if highest.len() > 1 {
            return Err(RegistryError::Ambiguous {
                extension_ids: highest.iter().map(|item| item.2.id()).collect::<Vec<_>>(),
                reason: "equal-priority extension match".into(),
            });
        }
        Ok(highest.pop().map(|item| item.2))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_observer::{TurnExtensionAccess, TurnExtensionFuture, VerifiedConversation};

    struct TestExtension {
        id: &'static str,
        applicability: Result<TurnApplicability, &'static str>,
    }

    impl TurnExtension for TestExtension {
        fn id(&self) -> &'static str {
            self.id
        }

        fn classify_turn(
            &self,
            _context: &VerifiedTurnContext<'_>,
        ) -> Result<TurnApplicability, String> {
            self.applicability.map_err(str::to_string)
        }

        fn begin_turn<'a>(
            &'a self,
            _context: VerifiedTurnContext<'a>,
        ) -> TurnExtensionFuture<'a, Result<Option<Box<dyn TurnExtensionAccess>>, String>> {
            Box::pin(async { Ok(None) })
        }
    }

    fn extension(
        id: &'static str,
        applicability: Result<TurnApplicability, &'static str>,
    ) -> Arc<dyn TurnExtension> {
        Arc::new(TestExtension { id, applicability })
    }

    fn context() -> VerifiedTurnContext<'static> {
        VerifiedTurnContext {
            source_event: None,
            source_event_id: None,
            source_pubkey: None,
            community_id: "community",
            conversation: VerifiedConversation::Heartbeat,
            agent_id: "agent",
            agent_turn_id: "turn",
            trace_id: "trace",
        }
    }

    #[test]
    fn selects_zero_or_one_extension() {
        let empty = TurnExtensionRegistry::new(vec![]).expect("empty registry");
        assert!(empty.select(&context()).expect("selection").is_none());

        let registry = TurnExtensionRegistry::new(vec![extension(
            "business",
            Ok(TurnApplicability::Applicable {
                priority: 100,
                reason: "explicit business resource",
            }),
        )])
        .expect("registry");
        assert_eq!(
            registry
                .select(&context())
                .expect("selection")
                .map(|selected| selected.id()),
            Some("business")
        );
    }

    #[test]
    fn highest_priority_wins_and_equal_priority_fails_closed() {
        let registry = TurnExtensionRegistry::new(vec![
            extension(
                "business",
                Ok(TurnApplicability::Applicable {
                    priority: 200,
                    reason: "explicit resource",
                }),
            ),
            extension(
                "life",
                Ok(TurnApplicability::Applicable {
                    priority: 100,
                    reason: "weaker intent",
                }),
            ),
        ])
        .expect("registry");
        assert_eq!(
            registry
                .select(&context())
                .expect("selection")
                .unwrap()
                .id(),
            "business"
        );

        let tied = TurnExtensionRegistry::new(vec![
            extension(
                "business",
                Ok(TurnApplicability::Applicable {
                    priority: 100,
                    reason: "match",
                }),
            ),
            extension(
                "life",
                Ok(TurnApplicability::Applicable {
                    priority: 100,
                    reason: "match",
                }),
            ),
        ])
        .expect("registry");
        assert!(matches!(
            tied.select(&context()),
            Err(RegistryError::Ambiguous { .. })
        ));
    }

    #[test]
    fn ambiguous_or_failed_classifier_fails_closed() {
        let ambiguous = TurnExtensionRegistry::new(vec![extension(
            "life",
            Ok(TurnApplicability::Ambiguous {
                reason: "domain unclear",
            }),
        )])
        .expect("registry");
        assert!(matches!(
            ambiguous.select(&context()),
            Err(RegistryError::Ambiguous { .. })
        ));

        let failed = TurnExtensionRegistry::new(vec![extension("life", Err("classifier failed"))])
            .expect("registry");
        assert!(matches!(
            failed.select(&context()),
            Err(RegistryError::Classification { .. })
        ));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        assert!(matches!(
            TurnExtensionRegistry::new(vec![
                extension("life", Ok(TurnApplicability::NotApplicable)),
                extension("life", Ok(TurnApplicability::NotApplicable)),
            ]),
            Err(RegistryError::DuplicateId("life"))
        ));
    }
}
