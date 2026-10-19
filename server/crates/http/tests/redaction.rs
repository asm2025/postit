//! Own test binary: the log capture installs a thread-default subscriber, and tracing's
//! callsite-interest cache is process-wide, so it must not share a process with other tests.

use std::time::Duration;

use postit_config::HttpSettings;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use postit_http::{build_client, execute_traced};

#[tokio::test]
async fn failed_request_never_leaks_query_secrets_into_the_error_or_logs() {
    let (logs, _guard) = LogCapture::install();

    // Bind then drop a listener so the port is closed and the connection is refused.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_or_else(|err| unreachable!("reserving a port: {err}"), |a| a.port());

    let client = build_client(&HttpSettings {
        connect_timeout: Duration::from_secs(2),
        request_timeout: Duration::from_secs(2),
        user_agent: "postit-http-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test client: {err}"));
    let request = client
        .get(format!(
            "http://127.0.0.1:{port}/cb?token=tok-secret-1&code=code-secret-2&page=3"
        ))
        .build()
        .unwrap_or_else(|err| unreachable!("building test request: {err}"));

    let Err(err) = execute_traced(&client, request).await else {
        unreachable!("a closed port unexpectedly answered");
    };
    assert!(err.is_retryable(), "connection refused is transient: {err}");

    let logs = logs.text();
    assert!(logs.contains("http request failed"), "{logs}");
    assert_no_leak(
        &["tok-secret-1", "code-secret-2"],
        &[
            ("error", &err.to_string()),
            ("debug", &format!("{err:?}")),
            ("logs", &logs),
        ],
    );
}

// URL patterns: none of these log, so they are safe in this binary next to the capture test.
// Each builds a local `Redactor`; only `redact_secrets` reads the process-wide one, which
// this binary never installs (so it is `Redactor::with_defaults()`).

mod url_patterns {
    use postit_http::{Redactor, SensitiveUrl, redact_secrets};
    use url::Url;

    fn parse(raw: &str) -> Url {
        Url::parse(raw).unwrap_or_else(|err| unreachable!("test url {raw}: {err}"))
    }

    #[test]
    fn telegram_bot_token_paths_redacted_by_default() {
        for raw in [
            "https://api.telegram.org/bot123456:AAH-SECRET/sendMessage",
            "https://api.telegram.org/file/bot123456:AAH-SECRET/photos/file_1.jpg",
        ] {
            assert_eq!(
                Redactor::with_defaults().redact_url(&parse(raw)),
                "https://api.telegram.org/[redacted]",
                "{raw}"
            );
        }
        let text = redact_secrets("GET https://api.telegram.org/bot123:AAH-SECRET/getMe failed");
        assert!(!text.contains("AAH-SECRET"), "{text}");
    }

    #[test]
    fn telegram_host_is_exact() {
        for raw in [
            "https://evil-api.telegram.org.example/bot123:AAH-SECRET/getMe",
            "https://sub.api.telegram.org/bot123:AAH-SECRET/getMe",
            "https://api.telegram.org/other/bot123:AAH-SECRET",
        ] {
            let text = Redactor::with_defaults().redact_url(&parse(raw));
            assert!(text.contains("AAH-SECRET"), "{raw} -> {text}");
        }
    }

    #[test]
    fn slack_webhook_path_redacted() {
        let url = parse("https://hooks.slack.com/services/T0/B0/SECRET");
        assert_eq!(
            Redactor::with_defaults().redact_url(&url),
            "https://hooks.slack.com/[redacted]"
        );
    }

    #[test]
    fn mixed_case_and_trailing_dot_host_redacted() {
        let url = parse("https://HOOKS.Slack.com./services/x/y/SECRET");
        let text = Redactor::with_defaults().redact_url(&url);
        assert!(!text.contains("SECRET"), "{text}");
        assert!(text.ends_with("/[redacted]"), "{text}");
    }

    #[test]
    fn discord_webhook_redacted_but_other_discord_paths_not() {
        let redactor = Redactor::with_defaults();
        for raw in [
            "https://discord.com/api/webhooks/1/tok",
            "https://discordapp.com/api/webhooks/1/tok",
        ] {
            let text = redactor.redact_url(&parse(raw));
            assert!(!text.contains("tok"), "{text}");
        }
        let other = "https://discord.com/api/v10/channels/1";
        assert_eq!(redactor.redact_url(&parse(other)), other);
    }

    #[test]
    fn discord_versioned_and_subdomain_webhooks_redacted() {
        let redactor = Redactor::with_defaults();
        for raw in [
            "https://discord.com/api/v10/webhooks/1/tok",
            "https://discord.com/api/v9/webhooks/2/tok/messages/@original",
            "https://canary.discord.com/api/webhooks/3/tok",
            "https://ptb.discord.com/api/v10/webhooks/4/tok",
            "https://discordapp.com/api/v8/webhooks/5/tok",
            "https://Canary.DiscordApp.com./api/v10/webhooks/6/tok",
        ] {
            let text = redactor.redact_url(&parse(raw));
            assert!(!text.contains("tok"), "{raw} -> {text}");
            assert!(text.ends_with("/[redacted]"), "{raw} -> {text}");
        }
        for raw in [
            "https://discord.com/api/v10/channels/1",
            "https://canary.discord.com/api/v10/channels/1",
            "https://discord.com/api/v/webhooks/1/tok",
            "https://discord.com/api/vx10/webhooks/1/tok",
            "https://notdiscord.com/api/webhooks/1/tok",
            "https://notdiscord.com/api/v10/webhooks/1/tok",
        ] {
            assert_eq!(redactor.redact_url(&parse(raw)), raw);
        }
    }

    #[test]
    fn version_placeholder_needs_digits() {
        let mut redactor = Redactor::default();
        redactor.add(SensitiveUrl {
            host: "api.example".to_string(),
            host_suffix: false,
            path_prefix: Some("/v{n}/hooks/".to_string()),
        });
        assert!(
            !redactor
                .redact_url(&parse("https://api.example/v123/hooks/tok"))
                .contains("tok")
        );
        let other = "https://api.example/v/hooks/tok";
        assert_eq!(redactor.redact_url(&parse(other)), other);
    }

    #[test]
    fn host_suffix_matches_subdomains_only_when_asked() {
        let mut redactor = Redactor::default();
        redactor.add(SensitiveUrl {
            host: "push.example".to_string(),
            host_suffix: true,
            path_prefix: None,
        });
        let sub = redactor.redact_url(&parse("https://eu.push.example/p/SECRET"));
        assert_eq!(sub, "https://eu.push.example/[redacted]");
        let other = "https://notpush.example/p/x";
        assert_eq!(redactor.redact_url(&parse(other)), other);
    }

    #[test]
    fn registered_media_prefix_redacted() {
        let mut redactor = Redactor::default();
        redactor.add(SensitiveUrl {
            host: "postit.local".to_string(),
            host_suffix: false,
            path_prefix: Some("/public/media/".to_string()),
        });
        let text = redactor.redact_url(&parse(
            "https://postit.local:44310/public/media/abc?exp=1&sig=zz",
        ));
        assert!(!text.contains("abc"), "{text}");
        assert!(!text.contains("zz"), "{text}");
        let api = "https://postit.local:44310/api/v1/posts";
        assert_eq!(redactor.redact_url(&parse(api)), api);
    }

    #[test]
    fn new_query_params_redacted() {
        let url = parse(
            "https://idp.example/token?client_secret=a1&refresh_token=b2&id_token=c3&exp=d4&state=e5&page=2",
        );
        let text = Redactor::with_defaults().redact_url(&url);
        for secret in ["a1", "b2", "c3", "d4", "e5"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
        assert!(text.contains("page=2"), "{text}");
    }

    #[test]
    fn redact_url_drops_userinfo() {
        let text = Redactor::with_defaults().redact_url(&parse("https://u:pw-1@h.example/a"));
        assert!(!text.contains("pw-1"), "{text}");
    }

    #[test]
    fn redact_secrets_applies_sensitive_urls_in_free_text() {
        let text = redact_secrets(
            "posting to https://hooks.slack.com/services/T/B/SECRET1 failed (see https://discord.com/api/webhooks/9/SECRET2), ok https://h.example/a",
        );
        assert!(!text.contains("SECRET1"), "{text}");
        assert!(!text.contains("SECRET2"), "{text}");
        assert!(
            text.contains("https://hooks.slack.com/[redacted] failed"),
            "{text}"
        );
        assert!(text.contains("[redacted])"), "{text}");
        assert!(text.contains("ok https://h.example/a"), "{text}");
    }

    #[test]
    fn local_redactor_text_uses_its_own_patterns() {
        let mut redactor = Redactor::with_defaults();
        redactor.add(SensitiveUrl {
            host: "media.example".to_string(),
            host_suffix: false,
            path_prefix: Some("/public/".to_string()),
        });
        let text = redactor.redact_text("GET https://Media.Example./public/x/SECRET3 -> 500");
        assert!(!text.contains("SECRET3"), "{text}");
    }
}
