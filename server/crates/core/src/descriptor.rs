//! The agent descriptor: everything the rest of the system learns about a platform.

use std::collections::BTreeSet;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};

use crate::oauth::OAuthProviderSpec;
use crate::{FormatId, FormatSpec, OptionsSchema, PlatformId};

/// Whether an agent publishes to an audience or sends to individuals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    /// One post reaches the account's audience.
    Broadcast,
    /// Messages go to individual recipients.
    Direct,
}

/// An optional ability beyond publishing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Can post comments and replies.
    Commenter,
    /// Can read comments on the account's posts.
    InboxReader,
    /// Can read post and account metrics.
    MetricsReader,
    /// Can send to individual recipients.
    DirectSender,
    /// Can hand a scheduled post to the platform to publish later.
    NativeScheduler,
}

/// How far ahead a platform accepts a natively scheduled post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct NativeScheduleWindow {
    /// Shortest lead time, in seconds.
    pub min_lead_secs: u64,
    /// Longest lead time, in seconds.
    pub max_lead_secs: u64,
}

/// What a quota bucket is shared by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum QuotaScope {
    /// Shared by every account on the deployment's developer app.
    App,
    /// Counted per connected account.
    Account,
}

/// How often a quota bucket resets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum QuotaPeriod {
    /// Resets at midnight in the bucket's time zone.
    Day,
    /// Resets at the start of the month in the bucket's time zone.
    Month,
}

/// What each operation costs against a bucket. 0 means the operation does not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QuotaCosts {
    /// Cost of one publish.
    pub publish: u32,
    /// Cost of one comment.
    pub comment: u32,
    /// Cost of one inbox poll.
    pub inbox_poll: u32,
    /// Cost of one metrics read.
    pub metrics: u32,
}

impl QuotaCosts {
    fn is_zero(&self) -> bool {
        self.publish == 0 && self.comment == 0 && self.inbox_poll == 0 && self.metrics == 0
    }
}

/// A calendar-aligned quota the platform enforces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct QuotaBucket {
    /// Bucket name, unique within the agent.
    pub id: String,
    /// What the bucket is shared by.
    pub scope: QuotaScope,
    /// How often it resets.
    pub period: QuotaPeriod,
    /// Allowance per period when the deployment sets no override.
    pub default_limit: u32,
    /// IANA time zone the period is aligned to. Stored, not resolved, here.
    pub reset_time_zone: String,
    /// What each operation costs.
    pub costs: QuotaCosts,
}

/// How an account is connected.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // built once per agent at startup; boxing buys nothing
pub enum ConnectMethod {
    /// OAuth authorization code flow.
    OAuth(OAuthProviderSpec),
    /// The user enters credentials described by this schema.
    Credentials(OptionsSchema),
}

impl Serialize for ConnectMethod {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::OAuth(_) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("method", "oauth")?;
                map.end()
            }
            Self::Credentials(schema) => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("method", "credentials")?;
                map.serialize_entry("schema", schema)?;
                map.end()
            }
        }
    }
}

#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for ConnectMethod {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::Type;
        let method = utoipa::openapi::ObjectBuilder::new()
            .schema_type(Type::String)
            .enum_values(Some(["oauth", "credentials"]));
        utoipa::openapi::ObjectBuilder::new()
            .schema_type(Type::Object)
            .property("method", method)
            .required("method")
            .property("schema", <OptionsSchema as utoipa::PartialSchema>::schema())
            .description(Some(
                "How to connect: `oauth`, or `credentials` with the schema of fields to collect.",
            ))
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for ConnectMethod {}

/// Why a descriptor is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DescriptorError {
    /// The agent declares no formats.
    #[error("the agent declares no formats")]
    NoFormats,
    /// A format is declared twice.
    #[error("format {0:?} is declared more than once")]
    DuplicateFormat(FormatId),
    /// The brand color is not `#` and six hex digits.
    #[error("brand color {0:?} is not #RRGGBB")]
    InvalidBrandColor(String),
    /// The icon has no bytes.
    #[error("the icon is empty")]
    EmptyIcon,
    /// A direct agent does not declare the `direct_sender` capability.
    #[error("a direct agent must declare the direct_sender capability")]
    DirectWithoutCapability,
    /// A native schedule window is declared without the `native_scheduler` capability.
    #[error("a native schedule window requires the native_scheduler capability")]
    NativeScheduleWithoutCapability,
    /// The native schedule window's minimum lead is longer than its maximum.
    #[error("the native schedule window's minimum lead exceeds its maximum")]
    InvalidNativeScheduleWindow,
    /// The `native_scheduler` capability is declared without a native schedule window.
    #[error("the native_scheduler capability requires a native schedule window")]
    NativeSchedulerWithoutWindow,
    /// A format selector's value is not a value its option accepts.
    #[error("format selector value is not accepted by option {0:?}")]
    InvalidSelectorValue(String),
    /// A format selector names an option the options schema does not have.
    #[error("format selector names unknown option {0:?}")]
    UnknownSelectorOption(String),
    /// A quota bucket ID is used twice.
    #[error("quota bucket {0:?} is declared more than once")]
    DuplicateQuotaBucket(String),
    /// A quota bucket has a zero limit, no time zone, or no costs.
    #[error("quota bucket {0:?} is invalid")]
    InvalidQuotaBucket(String),
}

/// Everything the system knows about a platform, declared by its agent.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentDescriptor {
    /// Platform ID.
    pub platform: PlatformId,
    /// Name shown to people.
    pub display_name: String,
    /// PNG icon, served by `GET /platforms/{id}/icon` rather than in this document.
    #[serde(skip)]
    #[cfg_attr(feature = "openapi", schema(ignore))]
    pub icon_png: &'static [u8],
    /// Brand color as `#RRGGBB`.
    pub brand_color: String,
    /// Broadcast or direct.
    pub kind: AgentKind,
    /// How accounts are connected.
    pub connect: ConnectMethod,
    /// Formats the agent can publish.
    pub formats: Vec<FormatSpec>,
    /// Optional abilities.
    pub capabilities: BTreeSet<Capability>,
    /// Version of the agent's stored configuration shape.
    pub config_schema_version: u32,
    /// Native scheduling window, when the platform schedules posts itself.
    pub native_schedule: Option<NativeScheduleWindow>,
    /// Quotas the platform enforces.
    pub quota_buckets: Vec<QuotaBucket>,
    /// Per-post options the agent understands.
    pub options_schema: OptionsSchema,
}

impl AgentDescriptor {
    /// Checks the descriptor is internally consistent. Agents call this in their tests and the
    /// registry calls it at startup.
    ///
    /// # Errors
    /// Returns the first [`DescriptorError`] found.
    pub fn validate(&self) -> Result<(), DescriptorError> {
        if self.formats.is_empty() {
            return Err(DescriptorError::NoFormats);
        }
        let mut seen = BTreeSet::new();
        for format in &self.formats {
            if !seen.insert(format.id) {
                return Err(DescriptorError::DuplicateFormat(format.id));
            }
        }
        if !is_hex_color(&self.brand_color) {
            return Err(DescriptorError::InvalidBrandColor(self.brand_color.clone()));
        }
        if self.icon_png.is_empty() {
            return Err(DescriptorError::EmptyIcon);
        }
        if self.kind == AgentKind::Direct && !self.capabilities.contains(&Capability::DirectSender)
        {
            return Err(DescriptorError::DirectWithoutCapability);
        }
        if self.native_schedule.is_some()
            && !self.capabilities.contains(&Capability::NativeScheduler)
        {
            return Err(DescriptorError::NativeScheduleWithoutCapability);
        }
        match self.native_schedule {
            Some(window) if window.min_lead_secs > window.max_lead_secs => {
                return Err(DescriptorError::InvalidNativeScheduleWindow);
            }
            None if self.capabilities.contains(&Capability::NativeScheduler) => {
                return Err(DescriptorError::NativeSchedulerWithoutWindow);
            }
            _ => {}
        }
        for selector in self.formats.iter().filter_map(|f| f.selected_by.as_ref()) {
            let Some(field) = self
                .options_schema
                .fields()
                .iter()
                .find(|f| f.name == selector.option)
            else {
                return Err(DescriptorError::UnknownSelectorOption(
                    selector.option.clone(),
                ));
            };
            if !field.accepts(&serde_json::Value::String(selector.value.clone())) {
                return Err(DescriptorError::InvalidSelectorValue(
                    selector.option.clone(),
                ));
            }
        }
        let mut buckets = BTreeSet::new();
        for bucket in &self.quota_buckets {
            if !buckets.insert(bucket.id.as_str()) {
                return Err(DescriptorError::DuplicateQuotaBucket(bucket.id.clone()));
            }
            if bucket.default_limit == 0
                || bucket.reset_time_zone.is_empty()
                || bucket.costs.is_zero()
            {
                return Err(DescriptorError::InvalidQuotaBucket(bucket.id.clone()));
            }
        }
        Ok(())
    }

    /// Whether the platform can schedule posts itself.
    #[must_use]
    pub fn supports_native_schedule(&self) -> bool {
        self.native_schedule.is_some() && self.capabilities.contains(&Capability::NativeScheduler)
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use url::Url;

    use super::*;
    use crate::oauth::{ClientAuth, Pkce, RefreshPolicy};
    use crate::{AspectRange, FormatSelector, MediaRules, SafeZone, TextRules};
    use crate::{AspectRatio, MediaKind, Message, TextMeasure};

    static ICON: &[u8] = &[0x89, b'P', b'N', b'G'];

    fn text_format() -> FormatSpec {
        FormatSpec {
            id: FormatId::Text,
            text: TextRules {
                measure: TextMeasure::Graphemes,
                max: 500,
                required: true,
            },
            media: MediaRules::none(),
            selected_by: None,
        }
    }

    fn descriptor() -> AgentDescriptor {
        AgentDescriptor {
            platform: PlatformId::new("demo").unwrap(),
            display_name: "Demo".into(),
            icon_png: ICON,
            brand_color: "#1A2B3C".into(),
            kind: AgentKind::Broadcast,
            connect: ConnectMethod::Credentials(OptionsSchema::empty()),
            formats: vec![text_format()],
            capabilities: BTreeSet::new(),
            config_schema_version: 1,
            native_schedule: None,
            quota_buckets: Vec::new(),
            options_schema: OptionsSchema::empty(),
        }
    }

    fn bucket(id: &str) -> QuotaBucket {
        QuotaBucket {
            id: id.into(),
            scope: QuotaScope::Account,
            period: QuotaPeriod::Day,
            default_limit: 50,
            reset_time_zone: "UTC".into(),
            costs: QuotaCosts {
                publish: 1,
                comment: 0,
                inbox_poll: 0,
                metrics: 0,
            },
        }
    }

    #[test]
    fn descriptor_validate_accepts_minimal() {
        descriptor().validate().unwrap();
    }

    #[test]
    fn descriptor_validate_rejects() {
        let mut cases: Vec<(AgentDescriptor, DescriptorError)> = Vec::new();

        let mut d = descriptor();
        d.formats.clear();
        cases.push((d, DescriptorError::NoFormats));

        let mut d = descriptor();
        d.formats.push(text_format());
        cases.push((d, DescriptorError::DuplicateFormat(FormatId::Text)));

        for bad in ["1A2B3C", "#1A2B3", "#1A2B3G", "#1A2B3C0"] {
            let mut d = descriptor();
            d.brand_color = bad.into();
            cases.push((d, DescriptorError::InvalidBrandColor(bad.into())));
        }

        let mut d = descriptor();
        d.icon_png = &[];
        cases.push((d, DescriptorError::EmptyIcon));

        let mut d = descriptor();
        d.kind = AgentKind::Direct;
        cases.push((d, DescriptorError::DirectWithoutCapability));

        let mut d = descriptor();
        d.native_schedule = Some(NativeScheduleWindow {
            min_lead_secs: 600,
            max_lead_secs: 86_400,
        });
        cases.push((d, DescriptorError::NativeScheduleWithoutCapability));

        let mut d = descriptor();
        d.formats[0].selected_by = Some(FormatSelector {
            option: "missing".into(),
            value: "x".into(),
        });
        cases.push((d, DescriptorError::UnknownSelectorOption("missing".into())));

        let mut d = descriptor();
        d.quota_buckets = vec![bucket("a"), bucket("a")];
        cases.push((d, DescriptorError::DuplicateQuotaBucket("a".into())));

        let mut zero_limit = bucket("z");
        zero_limit.default_limit = 0;
        let mut no_zone = bucket("n");
        no_zone.reset_time_zone.clear();
        let mut free = bucket("f");
        free.costs.publish = 0;
        for b in [zero_limit, no_zone, free] {
            let mut d = descriptor();
            let id = b.id.clone();
            d.quota_buckets = vec![b];
            cases.push((d, DescriptorError::InvalidQuotaBucket(id)));
        }

        for (descriptor, want) in cases {
            assert_eq!(descriptor.validate(), Err(want.clone()), "{want:?}");
        }
    }

    #[test]
    fn selector_naming_a_schema_option_is_accepted() {
        let mut d = descriptor();
        d.options_schema = OptionsSchema::from_json_str(
            r#"{"type":"object","properties":{"kind":{"type":"string","x-postit-label":{"key":"k","en":"Kind"}}}}"#,
        )
        .unwrap();
        d.formats[0].selected_by = Some(FormatSelector {
            option: "kind".into(),
            value: "x".into(),
        });
        d.validate().unwrap();
    }

    #[test]
    fn native_schedule_needs_window_and_capability() {
        let mut d = descriptor();
        assert!(!d.supports_native_schedule());
        d.capabilities.insert(Capability::NativeScheduler);
        assert!(!d.supports_native_schedule());
        d.native_schedule = Some(NativeScheduleWindow {
            min_lead_secs: 600,
            max_lead_secs: 86_400,
        });
        assert!(d.supports_native_schedule());
    }

    #[test]
    fn connect_method_hides_oauth_details() {
        let spec = OAuthProviderSpec::new(
            Url::parse("https://a.example/authorize").unwrap(),
            Url::parse("https://a.example/token").unwrap(),
        );
        assert_eq!(
            serde_json::to_value(ConnectMethod::OAuth(spec)).unwrap(),
            json!({"method": "oauth"})
        );
        let value =
            serde_json::to_value(ConnectMethod::Credentials(OptionsSchema::empty())).unwrap();
        assert_eq!(value["method"], "credentials");
        assert_eq!(value["schema"]["type"], "object");
    }

    #[test]
    fn descriptor_serialization_skips_the_icon() {
        let value = serde_json::to_value(descriptor()).unwrap();
        assert!(value.get("icon_png").is_none());
        assert_eq!(value["platform"], "demo");
        assert_eq!(value["kind"], "broadcast");
    }

    #[test]
    fn oauth_spec_defaults() {
        let auth = Url::parse("https://a.example/authorize").unwrap();
        let token = Url::parse("https://a.example/token").unwrap();
        let spec = OAuthProviderSpec::new(auth.clone(), token.clone());
        assert_eq!(spec.authorize_url, auth);
        assert_eq!(spec.token_url, token);
        assert_eq!(spec.revoke_url, None);
        assert!(spec.scopes.is_empty());
        assert_eq!(spec.scope_separator, " ");
        assert_eq!(spec.pkce, Pkce::S256);
        assert_eq!(spec.client_auth, ClientAuth::Basic);
        assert!(spec.extra_authorize_params.is_empty());
        assert_eq!(
            spec.refresh,
            RefreshPolicy {
                issued: true,
                rotates: false
            }
        );
        assert_eq!(spec.default_access_ttl, None);
    }

    #[test]
    fn oauth_spec_builders_chain() {
        let spec = OAuthProviderSpec::new(
            Url::parse("https://a.example/authorize").unwrap(),
            Url::parse("https://a.example/token").unwrap(),
        )
        .with_revoke_url(Url::parse("https://a.example/revoke").unwrap())
        .with_scopes(["a", "b"])
        .with_scope_separator(",")
        .with_pkce(Pkce::None)
        .with_client_auth(ClientAuth::PostBody)
        .with_extra_authorize_param("access_type", "offline")
        .with_refresh(RefreshPolicy {
            issued: true,
            rotates: true,
        })
        .with_default_access_ttl(Duration::from_secs(3600));
        assert_eq!(spec.scopes, ["a", "b"]);
        assert_eq!(spec.scope_separator, ",");
        assert_eq!(spec.pkce, Pkce::None);
        assert_eq!(spec.client_auth, ClientAuth::PostBody);
        assert_eq!(
            spec.extra_authorize_params,
            [("access_type".to_owned(), "offline".to_owned())]
        );
        assert!(spec.refresh.rotates);
        assert_eq!(spec.default_access_ttl, Some(Duration::from_secs(3600)));
        assert!(spec.revoke_url.is_some());
    }

    #[test]
    fn format_spec_round_trips() {
        let spec = FormatSpec {
            id: FormatId::ShortVideo,
            text: TextRules {
                measure: TextMeasure::Utf16Units,
                max: 2200,
                required: false,
            },
            media: MediaRules {
                min_count: 1,
                max_count: 1,
                kinds: vec![MediaKind::Video],
                mime_types: vec!["video/mp4".into()],
                max_bytes: Some(287_000_000),
                aspect_ratios: vec![AspectRange {
                    min: "1:2".parse::<AspectRatio>().unwrap(),
                    max: "3:5".parse().unwrap(),
                    preferred: "9:16".parse().unwrap(),
                }],
                max_duration_ms: Some(60_000),
                safe_zones: vec![SafeZone {
                    x: 0.0,
                    y: 0.7,
                    width: 1.0,
                    height: 0.3,
                    label: Message::new_static("demo.safe.captions", "Keep captions above this"),
                }],
                ..MediaRules::none()
            },
            selected_by: Some(FormatSelector {
                option: "kind".into(),
                value: "short".into(),
            }),
        };
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["id"], "short_video");
        assert_eq!(json["media"]["aspect_ratios"][0]["preferred"], "9:16");
        let back: FormatSpec = serde_json::from_value(json).unwrap();
        assert_eq!(back, spec);
    }

    #[test]
    fn rejects_inconsistent_native_schedule_and_selectors() {
        let mut d = descriptor();
        d.capabilities.insert(Capability::NativeScheduler);
        d.native_schedule = Some(NativeScheduleWindow {
            min_lead_secs: 600,
            max_lead_secs: 60,
        });
        assert_eq!(
            d.validate(),
            Err(DescriptorError::InvalidNativeScheduleWindow)
        );

        let mut d = descriptor();
        d.capabilities.insert(Capability::NativeScheduler);
        assert_eq!(
            d.validate(),
            Err(DescriptorError::NativeSchedulerWithoutWindow)
        );

        let mut d = descriptor();
        d.options_schema = OptionsSchema::from_json_str(
            r#"{"type":"object","properties":{"kind":{"type":"string","enum":["reel","post"],
                "x-postit-enum-labels":[{"key":"k","en":"R"},{"key":"k","en":"P"}],
                "x-postit-label":{"key":"k","en":"Kind"}}}}"#,
        )
        .unwrap();
        d.formats[0].selected_by = Some(FormatSelector {
            option: "kind".into(),
            value: "story".into(),
        });
        assert_eq!(
            d.validate(),
            Err(DescriptorError::InvalidSelectorValue("kind".into()))
        );
        d.formats[0].selected_by = Some(FormatSelector {
            option: "kind".into(),
            value: "reel".into(),
        });
        d.validate().unwrap();
    }
}
