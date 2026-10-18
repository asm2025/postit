//! The set of agents the server was built with.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::Agent;
use crate::{DescriptorError, PlatformId};

/// Why an agent could not be registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// Another agent already serves this platform.
    #[error("platform {0} is already registered")]
    Duplicate(PlatformId),
    /// The agent's descriptor failed validation.
    #[error("descriptor for platform {platform} is invalid: {source}")]
    InvalidDescriptor {
        /// The platform the agent declared.
        platform: PlatformId,
        /// What is wrong with the descriptor.
        source: DescriptorError,
    },
}

/// Agents by platform ID. The composition root builds it once at startup; nothing else adds
/// platforms.
#[derive(Default)]
pub struct AgentRegistry {
    agents: BTreeMap<PlatformId, Arc<dyn Agent>>,
}

impl AgentRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an agent after validating its descriptor.
    ///
    /// # Errors
    /// [`RegistryError::InvalidDescriptor`] when the descriptor fails validation and
    /// [`RegistryError::Duplicate`] when its platform is taken.
    pub fn register(&mut self, agent: Arc<dyn Agent>) -> Result<(), RegistryError> {
        let descriptor = agent.descriptor();
        let platform = descriptor.platform.clone();
        descriptor
            .validate()
            .map_err(|source| RegistryError::InvalidDescriptor {
                platform: platform.clone(),
                source,
            })?;
        if self.agents.contains_key(&platform) {
            return Err(RegistryError::Duplicate(platform));
        }
        self.agents.insert(platform, agent);
        Ok(())
    }

    /// The agent for `id`.
    #[must_use]
    pub fn get(&self, id: &PlatformId) -> Option<&Arc<dyn Agent>> {
        self.agents.get(id)
    }

    /// Every agent, ordered by platform ID.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Agent>> {
        self.agents.values()
    }

    /// How many agents are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Whether no agent is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::agent::test_support::{FakeAgent, fake_descriptor};

    fn id(s: &str) -> PlatformId {
        PlatformId::new(s).unwrap()
    }

    #[test]
    fn registers_and_looks_up() {
        let mut registry = AgentRegistry::new();
        assert!(registry.is_empty());
        registry.register(Arc::new(FakeAgent::new("demo"))).unwrap();
        assert_eq!(registry.len(), 1);
        let agent = registry.get(&id("demo")).unwrap();
        assert_eq!(agent.descriptor().platform, id("demo"));
        assert!(registry.get(&id("other")).is_none());
    }

    #[test]
    fn rejects_duplicate_platform() {
        let mut registry = AgentRegistry::new();
        registry.register(Arc::new(FakeAgent::new("demo"))).unwrap();
        let err = registry
            .register(Arc::new(FakeAgent::new("demo")))
            .unwrap_err();
        assert_eq!(err, RegistryError::Duplicate(id("demo")));
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn rejects_invalid_descriptor() {
        let mut descriptor = fake_descriptor("demo");
        descriptor.formats.clear();
        let mut registry = AgentRegistry::new();
        let err = registry
            .register(Arc::new(FakeAgent::with_descriptor(descriptor)))
            .unwrap_err();
        assert_eq!(
            err,
            RegistryError::InvalidDescriptor {
                platform: id("demo"),
                source: DescriptorError::NoFormats,
            }
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn iterates_in_platform_order() {
        let mut registry = AgentRegistry::new();
        registry
            .register(Arc::new(FakeAgent::new("telegram")))
            .unwrap();
        registry
            .register(Arc::new(FakeAgent::new("email")))
            .unwrap();
        let order: Vec<&str> = registry
            .iter()
            .map(|agent| agent.descriptor().platform.as_str())
            .collect();
        assert_eq!(order, ["email", "telegram"]);
    }
}
