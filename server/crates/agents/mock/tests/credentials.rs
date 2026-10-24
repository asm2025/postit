//! Pasted credentials pick the mock's connect results; a `MockScript` overrides them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{TOKEN, account, client, context, fields};
use postit_agent_kit::testkit::TestContext;
use postit_agent_mock::{InfoScript, MockAgent, VerifyScript};
use postit_core::{Agent, AgentError, DiscoveredAccount, RetryClass};
use serde_json::Value;

async fn verify(
    agent: &MockAgent,
    account_name: &str,
    token: &str,
) -> Result<Vec<String>, AgentError> {
    let ctx = TestContext::new();
    agent
        .verify_credentials(
            &ctx.check_context(Value::Null),
            &fields(account_name, token),
        )
        .await
        .map(|found| found.accounts.into_iter().map(|a| a.remote_id).collect())
}

#[tokio::test]
async fn keywords_pick_results() {
    let agent = MockAgent::broadcast();
    assert_eq!(
        verify(&agent, "shop", "invalid").await,
        Err(AgentError::InvalidCredentials {
            field: Some("token".to_owned())
        })
    );
    assert_eq!(
        verify(&agent, "shop", "rate_limited").await,
        Err(AgentError::RateLimited {
            retry_after: Some(Duration::from_secs(30))
        })
    );
    assert_eq!(
        verify(&agent, "shop", "expired").await,
        Ok(vec!["mock-account-shop".to_owned()])
    );
    assert_eq!(
        verify(&agent, "shop", TOKEN).await,
        Ok(vec!["mock-account-shop".to_owned()])
    );
}

#[tokio::test]
async fn keywords_ignore_case_and_space() {
    let agent = MockAgent::broadcast();
    assert_eq!(
        verify(&agent, "shop", " Invalid ").await,
        Err(AgentError::InvalidCredentials {
            field: Some("token".to_owned())
        })
    );
    assert!(matches!(
        verify(&agent, "shop", "\tRATE_LIMITED\n").await,
        Err(AgentError::RateLimited { .. })
    ));
    let ctx = context(" Expired ");
    let info = client(&agent, &ctx).account_info(&account("mock")).await;
    assert_eq!(info.unwrap_err().retry_class(), RetryClass::NeedsReconnect);
}

#[tokio::test]
async fn account_names_trimmed_deduped() {
    let agent = MockAgent::broadcast();
    let ctx = TestContext::new();
    let found = agent
        .verify_credentials(
            &ctx.check_context(Value::Null),
            &fields("shop,, shop ,blog", TOKEN),
        )
        .await
        .unwrap();
    let names: Vec<(&str, &str)> = found
        .accounts
        .iter()
        .map(|a| (a.remote_id.as_str(), a.display_name.as_str()))
        .collect();
    assert_eq!(
        names,
        [("mock-account-shop", "shop"), ("mock-account-blog", "blog")]
    );
    assert_eq!(
        verify(&agent, " , ,", TOKEN).await,
        Err(AgentError::InvalidCredentials {
            field: Some("account_name".to_owned())
        })
    );
}

#[tokio::test]
async fn expired_token_reports_needs_reconnect() {
    let agent = MockAgent::broadcast();
    let ctx = context("expired");
    let err = client(&agent, &ctx)
        .account_info(&account("mock"))
        .await
        .unwrap_err();
    assert!(matches!(err, AgentError::Authentication { .. }), "{err:?}");
    assert_eq!(err.retry_class(), RetryClass::NeedsReconnect);

    let ctx = context(TOKEN);
    let info = client(&agent, &ctx)
        .account_info(&account("mock"))
        .await
        .unwrap();
    assert_eq!(info.remote_id, "mock-account-shop");
    assert_eq!(info.status, postit_core::AccountStatus::Active);
    assert_eq!(agent.script().seen_remote_ids(), ["mock-account-shop"; 2]);
}

#[tokio::test]
async fn script_overrides_keywords() {
    let agent = MockAgent::broadcast();
    let script = agent.script();
    let only = DiscoveredAccount {
        remote_id: "scripted".to_owned(),
        display_name: "Scripted".to_owned(),
        handle: None,
        avatar_url: None,
    };
    script.set_verify(VerifyScript::Accounts(vec![only]));
    assert_eq!(
        verify(&agent, "shop", "invalid").await,
        Ok(vec!["scripted".to_owned()])
    );
    script.set_verify(VerifyScript::Invalid(Some("account_name".to_owned())));
    assert_eq!(
        verify(&agent, "shop", TOKEN).await,
        Err(AgentError::InvalidCredentials {
            field: Some("account_name".to_owned())
        })
    );
    script.set_verify(VerifyScript::RateLimited);
    assert!(matches!(
        verify(&agent, "shop", TOKEN).await,
        Err(AgentError::RateLimited { .. })
    ));
    script.set_verify(VerifyScript::Fail);
    assert_eq!(
        verify(&agent, "shop", TOKEN)
            .await
            .unwrap_err()
            .retry_class(),
        RetryClass::Retryable
    );

    script.set_account_info(InfoScript::AuthFailed);
    let ctx = context(TOKEN);
    let mock = client(&agent, &ctx);
    assert!(matches!(
        mock.account_info(&account("mock")).await,
        Err(AgentError::Authentication { .. })
    ));
    script.set_account_info(InfoScript::Ok);
    let ctx = context("expired");
    assert!(
        client(&agent, &ctx)
            .account_info(&account("mock"))
            .await
            .is_ok()
    );

    assert_eq!(script.revoke_calls(), 0);
    mock.revoke().await.unwrap();
    script.fail_revoke(true);
    assert!(mock.revoke().await.is_err());
    assert_eq!(script.revoke_calls(), 2);
}

#[tokio::test(start_paused = true)]
async fn by_fields_delays() {
    let agent = MockAgent::broadcast();
    agent
        .script()
        .set_verify(VerifyScript::ByFields(Arc::new(|fields| {
            let slow = fields
                .get_plain("account_name")
                .and_then(Value::as_str)
                .is_some_and(|name| name == "slow");
            if slow {
                (Duration::from_secs(5), VerifyScript::Fail)
            } else {
                (Duration::ZERO, VerifyScript::Invalid(None))
            }
        })));
    let started = tokio::time::Instant::now();
    assert_eq!(
        verify(&agent, "fast", TOKEN).await,
        Err(AgentError::InvalidCredentials { field: None })
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    let started = tokio::time::Instant::now();
    assert!(matches!(
        verify(&agent, "slow", TOKEN).await,
        Err(AgentError::Remote { .. })
    ));
    assert!(started.elapsed() >= Duration::from_secs(5));
}

#[tokio::test]
async fn oauth_mode_does_not_verify_credentials() {
    let provider = url::Url::parse("http://127.0.0.1:9/").unwrap();
    let agent = MockAgent::oauth(&provider);
    assert_eq!(agent.descriptor().platform.as_str(), "mock_oauth");
    let postit_core::ConnectMethod::OAuth(spec) = &agent.descriptor().connect else {
        panic!("OAuth connect method expected");
    };
    assert_eq!(spec.token_url.as_str(), "http://127.0.0.1:9/token");
    assert_eq!(spec.authorize_url.as_str(), "http://127.0.0.1:9/authorize");
    assert_eq!(
        spec.revoke_url.as_ref().map(url::Url::as_str),
        Some("http://127.0.0.1:9/revoke")
    );
    assert!(matches!(
        verify(&agent, "shop", TOKEN).await,
        Err(AgentError::Unsupported { .. })
    ));
}
