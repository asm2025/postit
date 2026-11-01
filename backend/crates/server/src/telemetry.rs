use postit_config::Environment;
use tracing_subscriber::EnvFilter;

/// Crates whose logs name what they look up: `hickory-resolver` logs the queried name, which
/// for the email agent's sender-domain check is an owner's sending domain.
const SILENCED: &[&str] = &[
    "hickory_resolver=off",
    "hickory_net=off",
    "hickory_proto=off",
];

/// Pretty `debug` in development, JSON `info` elsewhere; `RUST_LOG` overrides the level.
/// The [`SILENCED`] crates are off at any level, also when `RUST_LOG` names one of them
/// (`hickory_resolver=debug`), since their directives are added last and replace a directive
/// for the same target. Only a `RUST_LOG` directive for one of their modules, which is more
/// specific (`hickory_proto::udp=debug`), turns that module back on: an operator's explicit
/// choice while debugging.
pub fn init(env: Environment) {
    let default = if env == Environment::Development {
        "debug"
    } else {
        "info"
    };
    let filter =
        silenced(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default)));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = if env == Environment::Development {
        builder.pretty().try_init()
    } else {
        builder.json().try_init()
    };
}

/// `filter` with the [`SILENCED`] directives added.
fn silenced(filter: EnvFilter) -> EnvFilter {
    SILENCED
        .iter()
        .fold(filter, |filter, directive| match directive.parse() {
            Ok(directive) => filter.add_directive(directive),
            Err(_) => filter,
        })
}

#[cfg(test)]
mod tests {
    use postit_http::testkit::leak::LogCapture;
    use tracing_subscriber::EnvFilter;

    use super::{SILENCED, silenced};

    #[test]
    fn silenced_directives_parse() {
        for directive in SILENCED {
            assert!(
                directive
                    .parse::<tracing_subscriber::filter::Directive>()
                    .is_ok(),
                "{directive}"
            );
        }
    }

    /// What a subscriber with `rust_log` and the silenced crates writes for one event from
    /// each silenced crate and one from postit.
    fn logged(rust_log: &str) -> String {
        let capture = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(silenced(EnvFilter::new(rust_log)))
            .with_ansi(false)
            .with_writer(capture.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(target: "hickory_resolver::lookup", "resolver bakery.example");
            tracing::error!(target: "hickory_net", "net bakery.example");
            tracing::error!(target: "hickory_proto::udp", "proto bakery.example");
            tracing::info!(target: "postit_server", "kept");
        });
        capture.text()
    }

    #[test]
    fn silenced_crates_log_nothing_at_any_level() {
        for rust_log in [
            "info",
            "trace",
            "hickory_resolver=trace,hickory_net=trace,hickory_proto=trace,info",
        ] {
            let text = logged(rust_log);
            assert!(text.contains("kept"), "{rust_log}: {text}");
            assert!(!text.contains("bakery.example"), "{rust_log}: {text}");
        }
    }
}
