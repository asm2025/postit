use postit_config::Environment;
use tracing_subscriber::EnvFilter;

/// Pretty `debug` in development, JSON `info` elsewhere; `RUST_LOG` overrides the level.
pub fn init(env: Environment) {
    let default = if env == Environment::Development {
        "debug"
    } else {
        "info"
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    let _ = if env == Environment::Development {
        builder.pretty().try_init()
    } else {
        builder.json().try_init()
    };
}
