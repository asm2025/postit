//! Helpers shared by the integration tests that load the shipped `server/config/` files.

use std::path::PathBuf;

/// `server/config/`, the files baked into the image.
pub fn shipped_config_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config")
}

/// Every value a production deployment must supply on top of `production.toml`: the
/// vault's secrets plus the deployment-specific `POSTIT__…` settings that
/// `docker-compose.production.yml` sets. Removing any one of them must fail startup.
pub fn production_vars() -> Vec<(String, String)> {
    [
        ("POSTIT__DATABASE__USERNAME", "postgres"),
        ("POSTIT__DATABASE__PASSWORD", "db-password"),
        (
            "POSTIT__AUDIT__PSEUDONYM_KEY",
            "0123456789abcdef0123456789abcdef",
        ),
        ("POSTIT__AUTH__OIDC__ISSUER", "https://auth.example.com"),
        ("POSTIT__AUTH__OIDC__AUDIENCES", "[\"postit\"]"),
        ("POSTIT__MAIL__FROM_ADDRESS", "postit <noreply@example.com>"),
        ("POSTIT__MAIL__SMTP__HOST", "smtp.example.com"),
        ("POSTIT__SERVER__PUBLIC_URL", "https://api.example.com"),
        ("POSTIT__SERVER__WEB__ROOT", "/srv/web"),
        ("POSTIT__APP__PUBLIC_URL", "https://app.example.com"),
        (
            "POSTIT__CORS__ALLOWED_ORIGINS",
            "[\"https://app.example.com\"]",
        ),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}
