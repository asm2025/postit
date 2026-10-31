use postit_config::Environment;
use tracing_subscriber::EnvFilter;

/// Crates whose logs name what they look up: `hickory-resolver` logs the queried name, which
/// for the email agent's sender-domain check is an owner's sending domain. Always off.
const SILENCED: &[&str] = &[
    "hickory_resolver=off",
    "hickory_net=off",
    "hickory_proto=off",
];

/// Pretty `debug` in development, JSON `info` elsewhere; `RUST_LOG` overrides the level. The
/// [`SILENCED`] crates stay off either way.
pub fn init(env: Environment) {
    let default = if env == Environment::Development {
        "debug"
    } else {
        "info"
    };
    let filter = SILENCED.iter().fold(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default)),
        |filter, directive| match directive.parse() {
            Ok(directive) => filter.add_directive(directive),
            Err(_) => filter,
        },
    );
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = if env == Environment::Development {
        builder.pretty().try_init()
    } else {
        builder.json().try_init()
    };
}

#[cfg(test)]
mod tests {
    use super::SILENCED;

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
}
