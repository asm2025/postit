//! `postit-config`: layered settings (TOML, environment, secrets file) and their validation.
#![warn(missing_docs)]

mod duration;
mod environment;
mod error;
mod loader;
mod platforms;
mod secret;
mod settings;
mod settings_publishing;

pub use environment::Environment;
pub use error::ConfigError;
pub use loader::{load, load_with};
pub use platforms::PlatformTable;
pub use secret::RedactedSecret;
pub use settings::{
    AccountsSettings, AppSettings, AuditSettings, AuthSettings, BootstrapSettings, CorsSettings,
    DEVELOPMENT_KEYS, DatabaseSettings, EmailSettings, HttpSettings, JobHistoryRetention,
    JobSchedules, JobsSettings, MailSettings, MailTransport, MediaSettings, OidcClaimNames,
    OidcSettings, OpsSettings, PushSettings, RateBucket, RateLimitSettings, RetentionSettings,
    ServerSettings, Settings, SmtpSettings, SmtpTls, TlsSettings, UserinfoMode, VaultSettings,
    WebSettings,
};
pub use settings_publishing::{
    AiSettings, InboxSettings, LegalSettings, LimitsSettings, OAuthSettings, PublishingSettings,
    RecurrenceSettings,
};
