//! The compiled-in agents and the production guard for the mock agent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
use postit_config::Environment;
use postit_core::PlatformId;
use postit_server::compose::{agent_registry, refuse_mock_in_production};

#[test]
fn registry_has_both_mocks() {
    let registry = agent_registry().unwrap();
    for id in ["mock", "mock_direct"] {
        let id = PlatformId::new(id).unwrap();
        assert!(registry.get(&id).is_some(), "{id} is registered");
    }
}

#[test]
fn production_refuses_mock() {
    let err = refuse_mock_in_production(Environment::Production).unwrap_err();
    assert!(err.to_string().contains("mock-agent"), "{err}");
    assert!(refuse_mock_in_production(Environment::Development).is_ok());
    assert!(refuse_mock_in_production(Environment::Qa).is_ok());
}
