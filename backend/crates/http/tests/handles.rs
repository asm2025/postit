#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::{
    AddressPolicy, PublicClient, StandardClient, agent_handles, build_client, build_public_client,
};

fn settings() -> HttpSettings {
    HttpSettings {
        connect_timeout: Duration::from_secs(1),
        request_timeout: Duration::from_secs(1),
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::<PathBuf>::new(),
    }
}

#[test]
fn handles_downcast_to_their_clients() {
    let standard = build_client(&settings()).unwrap();
    let public = build_public_client(&settings(), AddressPolicy::strict()).unwrap();

    let handles = agent_handles(standard, public);

    assert!(
        handles
            .default
            .as_any()
            .downcast_ref::<StandardClient>()
            .is_some()
    );
    assert!(
        handles
            .user_hosts
            .as_any()
            .downcast_ref::<PublicClient>()
            .is_some()
    );
}
