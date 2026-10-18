//! The pure part of the validation pipeline: everything checkable without calling an agent.

use chrono::{DateTime, Duration, Utc};

use super::{measure, pick_format};
use crate::post::kind_violations;
use crate::{
    AgentDescriptor, FormatId, FormatSpec, MediaAsset, MediaAssetId, MediaAssetState, MediaRules,
    OptionsSchema, SuggestedFix, Variant, Violation, ViolationCode,
};

/// How far in the past a scheduled time may be before it is rejected, to absorb clock skew
/// between a client and the server.
const SCHEDULE_GRACE_SECS: i64 = 60;

/// Two aspect ratios closer than this are the same ratio.
const ASPECT_EPSILON: f64 = 1e-3;

/// When a target is meant to publish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// As soon as possible.
    Now,
    /// At a given time.
    At {
        /// When to publish.
        at: DateTime<Utc>,
        /// Whether the platform should hold the post itself. Honored only by agents that
        /// support native scheduling; others are scheduled by postit.
        native: bool,
    },
}

/// Everything [`validate_target`] needs about one target.
#[derive(Debug, Clone, Copy)]
pub struct ValidationInput<'a> {
    /// The agent's descriptor.
    pub descriptor: &'a AgentDescriptor,
    /// The options schema to check against: the account-specific one when the caller has it,
    /// else the descriptor's.
    pub options_schema: &'a OptionsSchema,
    /// The resolved content for this target.
    pub variant: &'a Variant,
    /// The variant's media, found by asset ID. IDs with no asset here count as missing.
    pub media: &'a [MediaAsset],
    /// When the target publishes.
    pub schedule: Schedule,
    /// The current time.
    pub now: DateTime<Utc>,
}

/// Runs every check that needs no agent and returns all violations found, in pipeline order:
/// kind, schedule, format pick, format rules, options. An empty vector means the target is
/// valid as far as postit can tell. The publishing engine appends the agent's own result.
#[must_use]
pub fn validate_target(input: &ValidationInput<'_>) -> Vec<Violation> {
    let variant = input.variant;
    let mut out = kind_violations(variant.kind, variant.reply_to.as_ref());
    check_schedule(input, &mut out);

    let resolved: Vec<&MediaAsset> = variant
        .media
        .iter()
        .filter_map(|item| find(input.media, item.asset))
        .collect();
    match pick_format(input.descriptor, variant, &resolved) {
        Ok(format) => check_format(input, format, &mut out),
        Err(violation) => out.push(violation),
    }

    out.extend(input.options_schema.validate(&variant.options));
    out
}

fn find(media: &[MediaAsset], id: MediaAssetId) -> Option<&MediaAsset> {
    media.iter().find(|m| m.id == id)
}

fn check_schedule(input: &ValidationInput<'_>, out: &mut Vec<Violation>) {
    let Schedule::At { at, native } = input.schedule else {
        return;
    };
    let lead = at - input.now;
    if lead < -Duration::seconds(SCHEDULE_GRACE_SECS) {
        out.push(Violation::new(ViolationCode::ScheduleInPast).at("schedule"));
        return;
    }
    if !native || !input.descriptor.supports_native_schedule() {
        return;
    }
    let Some(window) = input.descriptor.native_schedule else {
        return;
    };
    let min = Duration::seconds(i64::try_from(window.min_lead_secs).unwrap_or(i64::MAX));
    let max = Duration::seconds(i64::try_from(window.max_lead_secs).unwrap_or(i64::MAX));
    if lead < min || lead > max {
        out.push(
            Violation::new(ViolationCode::NativeScheduleOutsideWindow)
                .at("schedule")
                .param("min_ms", window.min_lead_secs.saturating_mul(1000))
                .param("max_ms", window.max_lead_secs.saturating_mul(1000)),
        );
    }
}

fn check_format(input: &ValidationInput<'_>, format: &FormatSpec, out: &mut Vec<Violation>) {
    let variant = input.variant;
    check_text(variant, format, out);

    for (i, item) in variant.media.iter().enumerate() {
        let path = format!("media[{i}]");
        match find(input.media, item.asset) {
            Some(asset) => check_asset(asset, &format.media, &path, out),
            None => out.push(missing(item.asset, &path)),
        }
    }
    if let Some(email) = &variant.email {
        for (i, block) in email.blocks.iter().enumerate() {
            let crate::EmailBlock::Image { asset, .. } = block else {
                continue;
            };
            let path = format!("email.blocks[{i}]");
            match find(input.media, *asset) {
                Some(found) => check_asset(found, &format.media, &path, out),
                None => out.push(missing(*asset, &path)),
            }
        }
    }
    let found = variant
        .media
        .iter()
        .filter(|item| find(input.media, item.asset).is_some())
        .count();
    // Items that could not be found are already reported as missing; counting them again
    // would only add noise.
    check_count(found, found < variant.media.len(), &format.media, out);

    if format.id == FormatId::HtmlEmail {
        match &variant.email {
            None => out.push(Violation::new(ViolationCode::EmailDocumentRequired).at("email")),
            Some(email) if email.subject.trim().is_empty() => {
                out.push(Violation::new(ViolationCode::EmailSubjectRequired).at("email.subject"));
            }
            Some(_) => {}
        }
    }
}

fn check_text(variant: &Variant, format: &FormatSpec, out: &mut Vec<Violation>) {
    let rules = &format.text;
    if rules.required && variant.text.trim().is_empty() {
        out.push(Violation::new(ViolationCode::TextRequired).at("text"));
        return;
    }
    let actual = measure(&variant.text, rules.measure);
    if actual > rules.max {
        out.push(
            Violation::new(ViolationCode::TextTooLong)
                .at("text")
                .param("max", rules.max)
                .param("actual", actual)
                .param(
                    "measure",
                    serde_json::to_value(rules.measure).unwrap_or_default(),
                )
                .fix(SuggestedFix::ShortenText { max: rules.max }),
        );
    }
}

fn check_count(actual: usize, some_missing: bool, rules: &MediaRules, out: &mut Vec<Violation>) {
    let actual = u32::try_from(actual).unwrap_or(u32::MAX);
    if actual == 0 && rules.min_count > 0 {
        if !some_missing {
            out.push(Violation::new(ViolationCode::MediaRequired).at("media"));
        }
    } else if (actual < rules.min_count && !some_missing) || actual > rules.max_count {
        let mut v = Violation::new(ViolationCode::MediaCount)
            .at("media")
            .param("min", rules.min_count)
            .param("max", rules.max_count)
            .param("actual", actual);
        if actual > rules.max_count {
            v = v.fix(SuggestedFix::RemoveMedia { index: None });
        }
        out.push(v);
    }
}

fn missing(id: MediaAssetId, path: &str) -> Violation {
    Violation::new(ViolationCode::MediaMissing)
        .at(path)
        .param("id", id.to_string())
}

/// Index of the media item a violation at `path` refers to, for `media[i]` paths.
fn media_index(path: &str) -> Option<usize> {
    path.strip_prefix("media[")?.strip_suffix(']')?.parse().ok()
}

fn check_asset(asset: &MediaAsset, rules: &MediaRules, path: &str, out: &mut Vec<Violation>) {
    if asset.state != MediaAssetState::Ready {
        out.push(
            Violation::new(ViolationCode::MediaNotReady)
                .at(path)
                .param("id", asset.id.to_string()),
        );
        return;
    }
    if !rules.kinds.contains(&asset.kind) {
        let mut v = Violation::new(ViolationCode::MediaKindNotAllowed)
            .at(path)
            .param("kind", serde_json::to_value(asset.kind).unwrap_or_default())
            .param(
                "allowed",
                serde_json::to_value(&rules.kinds).unwrap_or_default(),
            );
        if let Some(index) = media_index(path) {
            v = v.fix(SuggestedFix::RemoveMedia { index: Some(index) });
        }
        out.push(v);
        return;
    }
    if !rules.mime_types.is_empty()
        && !rules
            .mime_types
            .iter()
            .any(|mime| mime.eq_ignore_ascii_case(&asset.mime))
    {
        out.push(
            Violation::new(ViolationCode::MediaTypeNotAllowed)
                .at(path)
                .param("content_type", asset.mime.as_str())
                .param("allowed", rules.mime_types.clone()),
        );
    }
    if let Some(max) = rules.max_bytes
        && asset.bytes > max
    {
        out.push(
            Violation::new(ViolationCode::MediaTooLarge)
                .at(path)
                .param("max_bytes", max)
                .param("actual_bytes", asset.bytes),
        );
    }
    check_dimensions(asset, rules, path, out);
    check_aspect(asset, rules, path, out);
    check_duration(asset, rules, path, out);
}

fn check_dimensions(asset: &MediaAsset, rules: &MediaRules, path: &str, out: &mut Vec<Violation>) {
    let (Some(width), Some(height)) = (asset.width, asset.height) else {
        return;
    };
    let bad = rules.min_width.is_some_and(|m| width < m)
        || rules.max_width.is_some_and(|m| width > m)
        || rules.min_height.is_some_and(|m| height < m)
        || rules.max_height.is_some_and(|m| height > m);
    if !bad {
        return;
    }
    let mut v = Violation::new(ViolationCode::MediaDimensions).at(path);
    for (key, bound) in [
        ("min_width", rules.min_width),
        ("max_width", rules.max_width),
        ("min_height", rules.min_height),
        ("max_height", rules.max_height),
    ] {
        if let Some(bound) = bound {
            v = v.param(key, bound);
        }
    }
    out.push(v.param("width", width).param("height", height));
}

fn check_aspect(asset: &MediaAsset, rules: &MediaRules, path: &str, out: &mut Vec<Violation>) {
    let (Some(actual), Some(width), Some(height)) = (asset.aspect(), asset.width, asset.height)
    else {
        return;
    };
    let Some(first) = rules.aspect_ratios.first() else {
        return;
    };
    let allowed = rules.aspect_ratios.iter().any(|r| {
        actual >= r.min.as_f64() - ASPECT_EPSILON && actual <= r.max.as_f64() + ASPECT_EPSILON
    });
    if allowed {
        return;
    }
    let min = rules
        .aspect_ratios
        .iter()
        .map(|r| r.min)
        .reduce(|a, b| if a.as_f64() <= b.as_f64() { a } else { b })
        .unwrap_or(first.min);
    let max = rules
        .aspect_ratios
        .iter()
        .map(|r| r.max)
        .reduce(|a, b| if a.as_f64() >= b.as_f64() { a } else { b })
        .unwrap_or(first.max);
    let mut v = Violation::new(ViolationCode::MediaAspectRatio)
        .at(path)
        .param("min", min.to_string())
        .param("max", max.to_string())
        .param("actual", format!("{width}:{height}"));
    v = v.fix(SuggestedFix::Fit {
        aspect: first.preferred,
    });
    out.push(v);
}

fn check_duration(asset: &MediaAsset, rules: &MediaRules, path: &str, out: &mut Vec<Violation>) {
    let Some(actual) = asset.duration_ms else {
        return;
    };
    let too_short = rules.min_duration_ms.is_some_and(|m| actual < m);
    let too_long = rules.max_duration_ms.is_some_and(|m| actual > m);
    if !too_short && !too_long {
        return;
    }
    let mut v = Violation::new(ViolationCode::MediaDuration).at(path);
    if let Some(min) = rules.min_duration_ms {
        v = v.param("min_ms", min);
    }
    if let Some(max) = rules.max_duration_ms {
        v = v.param("max_ms", max);
    }
    v = v.param("actual_ms", actual);
    if let (true, Some(max)) = (too_long, rules.max_duration_ms) {
        v = v.fix(SuggestedFix::Trim { max_ms: max });
    }
    out.push(v);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::TimeZone;
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::{
        AccountId, AccountRef, AgentKind, AspectRange, AspectRatio, Capability, ConnectMethod,
        EmailBlock, EmailDocument, FormatSelector, MediaItem, MediaKind, NativeScheduleWindow,
        PlatformId, PostKind, ReplyTo, TextMeasure, TextRules, UserId,
    };

    const SCHEMA: &str = r#"{
        "type":"object",
        "properties":{
            "format":{"type":"string","enum":["short_video","image","other"],
                "x-postit-enum-labels":[{"key":"a","en":"A"},{"key":"c","en":"C"},{"key":"b","en":"B"}],
                "x-postit-label":{"key":"k","en":"E"}},
            "privacy":{"type":"string","enum":["public","private"],"default":"public",
                "x-postit-enum-labels":[{"key":"a","en":"A"},{"key":"b","en":"B"}],
                "x-postit-label":{"key":"k","en":"E"}}
        },
        "additionalProperties":false
    }"#;

    fn ratio(w: u32, h: u32) -> AspectRatio {
        AspectRatio::new(w, h).unwrap()
    }

    fn rules(max_count: u32, kinds: &[MediaKind]) -> MediaRules {
        MediaRules {
            max_count,
            kinds: kinds.to_vec(),
            ..MediaRules::none()
        }
    }

    fn text_format() -> FormatSpec {
        FormatSpec {
            id: FormatId::Text,
            text: TextRules {
                measure: TextMeasure::Graphemes,
                max: 500,
                required: false,
            },
            media: rules(4, &[MediaKind::Image]),
            selected_by: None,
        }
    }

    fn short_video_format() -> FormatSpec {
        FormatSpec {
            id: FormatId::ShortVideo,
            text: TextRules {
                measure: TextMeasure::Graphemes,
                max: 150,
                required: false,
            },
            media: MediaRules {
                min_count: 1,
                max_count: 1,
                kinds: vec![MediaKind::Video],
                aspect_ratios: vec![AspectRange {
                    min: ratio(1, 2),
                    max: ratio(3, 5),
                    preferred: ratio(9, 16),
                }],
                max_duration_ms: Some(60_000),
                ..MediaRules::none()
            },
            selected_by: Some(FormatSelector {
                option: "format".to_owned(),
                value: "short_video".to_owned(),
            }),
        }
    }

    fn image_format() -> FormatSpec {
        FormatSpec {
            id: FormatId::Image,
            text: TextRules {
                measure: TextMeasure::Graphemes,
                max: 500,
                required: false,
            },
            media: MediaRules {
                min_count: 1,
                max_count: 1,
                kinds: vec![MediaKind::Image],
                mime_types: vec!["image/png".to_owned(), "image/jpeg".to_owned()],
                max_bytes: Some(1_000_000),
                min_width: Some(100),
                ..MediaRules::none()
            },
            selected_by: None,
        }
    }

    fn email_format() -> FormatSpec {
        FormatSpec {
            id: FormatId::HtmlEmail,
            text: TextRules {
                measure: TextMeasure::Graphemes,
                max: 10_000,
                required: false,
            },
            media: rules(10, &[MediaKind::Image]),
            selected_by: None,
        }
    }

    fn descriptor(formats: Vec<FormatSpec>) -> AgentDescriptor {
        AgentDescriptor {
            platform: PlatformId::new("demo").unwrap(),
            display_name: "Demo".into(),
            icon_png: &[1],
            brand_color: "#112233".into(),
            kind: AgentKind::Broadcast,
            connect: ConnectMethod::Credentials(OptionsSchema::empty()),
            formats,
            capabilities: BTreeSet::new(),
            config_schema_version: 1,
            native_schedule: None,
            quota_buckets: Vec::new(),
            options_schema: OptionsSchema::from_json_str(SCHEMA).unwrap(),
        }
    }

    fn standard() -> AgentDescriptor {
        descriptor(vec![text_format(), short_video_format(), image_format()])
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap()
    }

    fn asset(n: u128, kind: MediaKind, w: u32, h: u32) -> MediaAsset {
        MediaAsset {
            id: MediaAssetId::from_uuid(Uuid::from_u128(n)),
            owner_id: UserId::from_uuid(Uuid::from_u128(1)),
            kind,
            mime: match kind {
                MediaKind::Image => "image/png".to_owned(),
                MediaKind::Video => "video/mp4".to_owned(),
            },
            bytes: 1000,
            width: Some(w),
            height: Some(h),
            duration_ms: (kind == MediaKind::Video).then_some(30_000),
            sha256: None,
            state: MediaAssetState::Ready,
            derived_from: None,
        }
    }

    fn variant(media: &[&MediaAsset]) -> Variant {
        Variant {
            target: AccountRef {
                platform: PlatformId::new("demo").unwrap(),
                account: AccountId::from_uuid(Uuid::from_u128(2)),
            },
            kind: PostKind::Post,
            text: "hello".to_owned(),
            media: media
                .iter()
                .map(|m| MediaItem {
                    asset: m.id,
                    alt_text: None,
                })
                .collect(),
            link: None,
            tags: Vec::new(),
            disclose_ai_generated: false,
            reply_to: None,
            email: None,
            options: serde_json::Map::new(),
        }
    }

    fn run(
        d: &AgentDescriptor,
        v: &Variant,
        media: &[MediaAsset],
        schedule: Schedule,
    ) -> Vec<Violation> {
        validate_target(&ValidationInput {
            descriptor: d,
            options_schema: &d.options_schema,
            variant: v,
            media,
            schedule,
            now: now(),
        })
    }

    fn codes(violations: &[Violation]) -> Vec<ViolationCode> {
        violations.iter().map(|v| v.code).collect()
    }

    fn pick(d: &AgentDescriptor, v: &Variant, media: &[&MediaAsset]) -> FormatId {
        pick_format(d, v, media).unwrap().id
    }

    #[test]
    fn pick_text_without_media() {
        assert_eq!(pick(&standard(), &variant(&[]), &[]), FormatId::Text);
    }

    #[test]
    fn pick_short_video_for_vertical_clip() {
        let clip = asset(10, MediaKind::Video, 1080, 1920);
        assert_eq!(
            pick(&standard(), &variant(&[&clip]), &[&clip]),
            FormatId::ShortVideo
        );
    }

    #[test]
    fn pick_falls_back_to_text_with_images() {
        let d = descriptor(vec![text_format()]);
        let a = asset(10, MediaKind::Image, 800, 600);
        let b = asset(11, MediaKind::Image, 800, 600);
        assert_eq!(pick(&d, &variant(&[&a, &b]), &[&a, &b]), FormatId::Text);
        assert_eq!(pick(&d, &variant(&[&a]), &[&a]), FormatId::Text);
    }

    #[test]
    fn selector_wins_over_inference() {
        let mut v = variant(&[]);
        v.options.insert("format".into(), json!("short_video"));
        assert_eq!(pick(&standard(), &v, &[]), FormatId::ShortVideo);
    }

    #[test]
    fn unsupported_format_reports_inferred() {
        let clip = asset(10, MediaKind::Video, 1920, 1080);
        let err = pick_format(&standard(), &variant(&[&clip]), &[&clip]).unwrap_err();
        assert_eq!(err.code, ViolationCode::FormatUnsupported);
        assert_eq!(err.params["inferred"], json!("video"));
    }

    #[test]
    fn collects_every_violation_at_once() {
        let clip = asset(10, MediaKind::Video, 1920, 1080);
        let mut v = variant(&[&clip]);
        v.text = "x".repeat(200);
        v.options.insert("format".into(), json!("short_video"));
        v.options.insert("privacy".into(), json!("secret"));
        let found = codes(&run(&standard(), &v, &[clip], Schedule::Now));
        assert_eq!(
            found,
            vec![
                ViolationCode::TextTooLong,
                ViolationCode::MediaAspectRatio,
                ViolationCode::OptionNotAllowed
            ]
        );
    }

    #[test]
    fn aspect_violation_suggests_fit() {
        let clip = asset(10, MediaKind::Video, 1920, 1080);
        let mut v = variant(&[&clip]);
        v.options.insert("format".into(), json!("short_video"));
        let found = run(&standard(), &v, &[clip], Schedule::Now);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path.as_deref(), Some("media[0]"));
        assert_eq!(
            found[0].fix,
            Some(SuggestedFix::Fit {
                aspect: ratio(9, 16)
            })
        );
    }

    #[test]
    fn long_video_suggests_trim() {
        let mut clip = asset(10, MediaKind::Video, 1080, 1920);
        clip.duration_ms = Some(90_000);
        let mut v = variant(&[&clip]);
        v.options.insert("format".into(), json!("short_video"));
        let found = run(&standard(), &v, &[clip], Schedule::Now);
        assert_eq!(codes(&found), vec![ViolationCode::MediaDuration]);
        assert_eq!(found[0].fix, Some(SuggestedFix::Trim { max_ms: 60_000 }));
        assert_eq!(found[0].params["actual_ms"], json!(90_000));
    }

    #[test]
    fn schedule_59s_past_ok_61s_past_rejected() {
        let d = standard();
        let v = variant(&[]);
        let at = |secs: i64| Schedule::At {
            at: now() - Duration::seconds(secs),
            native: false,
        };
        assert!(run(&d, &v, &[], at(59)).is_empty());
        assert!(run(&d, &v, &[], at(60)).is_empty());
        assert_eq!(
            codes(&run(&d, &v, &[], at(61))),
            vec![ViolationCode::ScheduleInPast]
        );
    }

    fn native() -> AgentDescriptor {
        let mut d = standard();
        d.capabilities.insert(Capability::NativeScheduler);
        d.native_schedule = Some(NativeScheduleWindow {
            min_lead_secs: 600,
            max_lead_secs: 3600,
        });
        d
    }

    #[test]
    fn native_window_edges_inclusive() {
        let d = native();
        let v = variant(&[]);
        let at = |secs: i64| Schedule::At {
            at: now() + Duration::seconds(secs),
            native: true,
        };
        assert!(run(&d, &v, &[], at(600)).is_empty());
        assert!(run(&d, &v, &[], at(3600)).is_empty());
        let early = run(&d, &v, &[], at(599));
        assert_eq!(
            codes(&early),
            vec![ViolationCode::NativeScheduleOutsideWindow]
        );
        assert_eq!(early[0].params["min_ms"], json!(600_000));
        assert_eq!(
            codes(&run(&d, &v, &[], at(3601))),
            vec![ViolationCode::NativeScheduleOutsideWindow]
        );
    }

    #[test]
    fn native_on_unsupported_agent_uses_postit_scheduling() {
        let v = variant(&[]);
        let schedule = Schedule::At {
            at: now() + Duration::seconds(5),
            native: true,
        };
        assert!(run(&standard(), &v, &[], schedule).is_empty());
    }

    #[test]
    fn deleted_media_is_missing() {
        let a = asset(10, MediaKind::Image, 800, 600);
        let b = asset(11, MediaKind::Image, 800, 600);
        let mut v = variant(&[&a, &b]);
        v.text = "x".repeat(600);
        let found = run(&standard(), &v, &[a], Schedule::Now);
        assert_eq!(
            codes(&found),
            vec![ViolationCode::TextTooLong, ViolationCode::MediaMissing]
        );
        assert_eq!(found[1].path.as_deref(), Some("media[1]"));
        assert_eq!(found[1].params["id"], json!(b.id.to_string()));
    }

    #[test]
    fn missing_email_image_is_missing() {
        let d = descriptor(vec![email_format()]);
        let img = asset(10, MediaKind::Image, 800, 600);
        let mut v = variant(&[]);
        v.email = Some(EmailDocument {
            subject: "Hi".to_owned(),
            preheader: None,
            blocks: vec![
                EmailBlock::Text {
                    text: "hello".to_owned(),
                },
                EmailBlock::Image {
                    asset: img.id,
                    alt: "pic".to_owned(),
                    link: None,
                },
            ],
        });
        let found = run(&d, &v, &[], Schedule::Now);
        assert_eq!(codes(&found), vec![ViolationCode::MediaMissing]);
        assert_eq!(found[0].path.as_deref(), Some("email.blocks[1]"));
    }

    #[test]
    fn html_email_requires_document_and_subject() {
        let d = descriptor(vec![email_format()]);
        let mut v = variant(&[]);
        v.options.clear();
        // No document: the content infers Text, which this agent does not declare.
        let found = run(&d, &v, &[], Schedule::Now);
        assert_eq!(codes(&found), vec![ViolationCode::FormatUnsupported]);

        let mut selected = d.clone();
        selected.formats[0].selected_by = Some(FormatSelector {
            option: "format".to_owned(),
            value: "short_video".to_owned(),
        });
        v.options.insert("format".into(), json!("short_video"));
        assert_eq!(
            codes(&run(&selected, &v, &[], Schedule::Now)),
            vec![ViolationCode::EmailDocumentRequired]
        );

        v.email = Some(EmailDocument {
            subject: "  ".to_owned(),
            preheader: None,
            blocks: Vec::new(),
        });
        assert_eq!(
            codes(&run(&selected, &v, &[], Schedule::Now)),
            vec![ViolationCode::EmailSubjectRequired]
        );
    }

    #[test]
    fn comment_without_reply_to() {
        let mut v = variant(&[]);
        v.kind = PostKind::Comment;
        assert_eq!(
            codes(&run(&standard(), &v, &[], Schedule::Now)),
            vec![ViolationCode::ReplyToRequired]
        );
        v.reply_to = Some(ReplyTo {
            remote_post_id: "1".to_owned(),
            remote_comment_id: None,
            url: None,
        });
        assert!(run(&standard(), &v, &[], Schedule::Now).is_empty());
    }

    #[test]
    fn media_rules_flag_each_property() {
        let d = descriptor(vec![image_format()]);
        let mut a = asset(10, MediaKind::Image, 50, 600);
        a.mime = "image/gif".to_owned();
        a.bytes = 2_000_000;
        let found = run(&d, &variant(&[&a]), std::slice::from_ref(&a), Schedule::Now);
        assert_eq!(
            codes(&found),
            vec![
                ViolationCode::MediaTypeNotAllowed,
                ViolationCode::MediaTooLarge,
                ViolationCode::MediaDimensions
            ]
        );
        assert_eq!(found[2].params["min_width"], json!(100));
        assert!(!found[2].params.contains_key("max_width"));

        let mut d = d;
        d.formats[0].selected_by = Some(FormatSelector {
            option: "format".to_owned(),
            value: "image".to_owned(),
        });
        let mut empty = variant(&[]);
        empty.options.insert("format".into(), json!("image"));
        let none = run(&d, &empty, &[], Schedule::Now);
        assert_eq!(codes(&none), vec![ViolationCode::MediaRequired]);

        let mut pending = asset(11, MediaKind::Image, 800, 600);
        pending.state = MediaAssetState::Uploading;
        let found = run(
            &d,
            &variant(&[&pending]),
            std::slice::from_ref(&pending),
            Schedule::Now,
        );
        assert_eq!(codes(&found), vec![ViolationCode::MediaNotReady]);
    }

    #[test]
    fn too_many_media_suggests_removal() {
        let mut carousel = text_format();
        carousel.id = FormatId::Carousel;
        let d = descriptor(vec![carousel]);
        let imgs: Vec<MediaAsset> = (10..16)
            .map(|n| asset(n, MediaKind::Image, 800, 600))
            .collect();
        let refs: Vec<&MediaAsset> = imgs.iter().collect();
        let found = run(&d, &variant(&refs), &imgs, Schedule::Now);
        assert_eq!(codes(&found), vec![ViolationCode::MediaCount]);
        assert_eq!(
            found[0].fix,
            Some(SuggestedFix::RemoveMedia { index: None })
        );
    }

    #[test]
    fn missing_item_is_not_also_counted() {
        let carousel = FormatSpec {
            id: FormatId::Carousel,
            media: MediaRules {
                min_count: 2,
                ..rules(4, &[MediaKind::Image])
            },
            ..text_format()
        };
        let a = asset(10, MediaKind::Image, 800, 600);
        let b = asset(11, MediaKind::Image, 800, 600);
        let found = run(
            &descriptor(vec![carousel]),
            &variant(&[&a, &b]),
            &[a],
            Schedule::Now,
        );
        assert_eq!(codes(&found), vec![ViolationCode::MediaMissing]);
    }

    #[test]
    fn mime_type_matches_ignoring_case() {
        let mut a = asset(10, MediaKind::Image, 800, 600);
        a.mime = "image/PNG".to_owned();
        let found = run(
            &standard(),
            &variant(&[&a]),
            std::slice::from_ref(&a),
            Schedule::Now,
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
