//! The sender-domain check: SPF, DKIM, and DMARC for the domain of the account's from
//! address, read from `TXT` records through a [`TxtLookup`].
//!
//! - **SPF** (`<domain>`): one `v=spf1` record ending in `-all` or `~all`, or delegating with
//!   `redirect=`, is `spf_ok`. `+all`, `?all`, a bare `all`, no `all` at all (neutral), or
//!   two SPF records (a permanent error) is `spf_weak`; none is `spf_missing`.
//! - **DKIM** (`<selector>._domainkey.<domain>`): only with a selector, which cannot be
//!   discovered; without one it is `dkim_unknown`. A key with a non-empty `p=` is `dkim_ok`;
//!   no key, or a revoked one (`p=` empty), is `dkim_missing`.
//! - **DMARC** (`_dmarc.<domain>`, then each parent domain down to two labels, where `sp=`
//!   comes before `p=`): `p=quarantine` or `p=reject` is `dmarc_ok`; `p=none`, no policy, or
//!   two records is `dmarc_weak`; none anywhere is `dmarc_missing`.
//!
//! A lookup that fails or outlasts [`CHECK_TIMEOUT`] makes that record `<record>_unknown`,
//! never an error. Findings are fixed codes: no domain, selector, or record text reaches a
//! finding, an error, or a log.

use std::sync::OnceLock;
use std::time::Duration;

use async_trait::async_trait;
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use postit_core::{DomainFinding, DomainRecord, DomainStatus};
use tokio::time::Instant;

/// The longest a whole check may take; a lookup still running then is `unknown`.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// The most DMARC names looked up: the domain and its parents.
const MAX_DMARC_NAMES: usize = 8;

/// A `TXT` lookup that did not answer. It names no domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LookupError {
    /// The resolver failed or the name could not be looked up.
    #[error("the DNS lookup failed")]
    Failed,
    /// The lookup took too long.
    #[error("the DNS lookup timed out")]
    TimedOut,
}

/// Looks up `TXT` records.
#[async_trait]
pub trait TxtLookup: Send + Sync {
    /// The `TXT` records at `name` (no trailing dot), each record's strings joined. A name
    /// with no records, or that does not exist, is an empty list.
    ///
    /// # Errors
    /// [`LookupError`] when the lookup did not answer.
    async fn txt(&self, name: &str) -> Result<Vec<String>, LookupError>;
}

/// [`TxtLookup`] through `hickory-resolver` with the system's resolver configuration, built
/// on first use and kept once built. While that configuration cannot be read, lookups fail,
/// and the next lookup tries to build the resolver again.
pub struct HickoryLookup {
    resolver: OnceLock<TokioResolver>,
    build: fn() -> Option<TokioResolver>,
}

impl Default for HickoryLookup {
    fn default() -> Self {
        Self {
            resolver: OnceLock::new(),
            build: system_resolver,
        }
    }
}

fn system_resolver() -> Option<TokioResolver> {
    TokioResolver::builder_tokio()
        .ok()
        .and_then(|builder| builder.build().ok())
}

impl HickoryLookup {
    /// A lookup over the system resolver.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The resolver, built now when no earlier build succeeded. Two lookups racing on the
    /// first build may both build; one of the two is kept.
    fn resolver(&self) -> Option<&TokioResolver> {
        if let Some(resolver) = self.resolver.get() {
            return Some(resolver);
        }
        let built = (self.build)()?;
        Some(self.resolver.get_or_init(|| built))
    }
}

#[async_trait]
impl TxtLookup for HickoryLookup {
    async fn txt(&self, name: &str) -> Result<Vec<String>, LookupError> {
        let resolver = self.resolver().ok_or(LookupError::Failed)?;
        // A trailing dot makes the name absolute, so no search domain is appended.
        match resolver.txt_lookup(format!("{name}.")).await {
            Ok(lookup) => Ok(lookup
                .answers()
                .iter()
                .filter_map(|record| match &record.data {
                    RData::TXT(txt) => Some(
                        txt.txt_data
                            .iter()
                            .map(|part| String::from_utf8_lossy(part))
                            .collect::<String>(),
                    ),
                    _ => None,
                })
                .collect()),
            Err(err) if err.is_no_records_found() => Ok(Vec::new()),
            Err(_) => Err(LookupError::Failed),
        }
    }
}

fn finding(record: DomainRecord, status: DomainStatus, code: &str) -> DomainFinding {
    DomainFinding {
        record,
        status,
        code: code.to_owned(),
    }
}

fn unknown(record: DomainRecord) -> DomainFinding {
    let code = match record {
        DomainRecord::Spf => "spf_unknown",
        DomainRecord::Dkim => "dkim_unknown",
        DomainRecord::Dmarc => "dmarc_unknown",
    };
    finding(record, DomainStatus::Unknown, code)
}

/// Checks `domain` (the from address's domain) and, when given, the DKIM key under
/// `selector`, a valid DNS label. Every lookup ends by [`CHECK_TIMEOUT`].
pub(crate) async fn check(
    lookup: &dyn TxtLookup,
    domain: &str,
    selector: Option<&str>,
) -> Vec<DomainFinding> {
    let domain = domain.trim().trim_end_matches('.').to_lowercase();
    if domain.is_empty() || domain.starts_with('[') {
        // An address literal has no DNS records to check.
        return vec![
            unknown(DomainRecord::Spf),
            unknown(DomainRecord::Dkim),
            unknown(DomainRecord::Dmarc),
        ];
    }
    let deadline = Instant::now() + CHECK_TIMEOUT;
    let (spf, dkim, dmarc) = tokio::join!(
        spf(lookup, &domain, deadline),
        dkim(lookup, &domain, selector, deadline),
        dmarc(lookup, &domain, deadline),
    );
    vec![spf, dkim, dmarc]
}

async fn txt(
    lookup: &dyn TxtLookup,
    name: &str,
    deadline: Instant,
) -> Result<Vec<String>, LookupError> {
    tokio::time::timeout_at(deadline, lookup.txt(name))
        .await
        .unwrap_or(Err(LookupError::TimedOut))
}

async fn spf(lookup: &dyn TxtLookup, domain: &str, deadline: Instant) -> DomainFinding {
    let Ok(records) = txt(lookup, domain, deadline).await else {
        return unknown(DomainRecord::Spf);
    };
    let found: Vec<&str> = records
        .iter()
        .map(|r| r.trim())
        .filter(|r| is_spf(r))
        .collect();
    let (status, code) = match found.as_slice() {
        [] => (DomainStatus::Missing, "spf_missing"),
        [record] if spf_is_strict(record) => (DomainStatus::Ok, "spf_ok"),
        _ => (DomainStatus::Weak, "spf_weak"),
    };
    finding(DomainRecord::Spf, status, code)
}

/// Whether `record` is an SPF record: `v=spf1` alone or followed by a space.
fn is_spf(record: &str) -> bool {
    let mut terms = record.split_ascii_whitespace();
    terms
        .next()
        .is_some_and(|version| version.eq_ignore_ascii_case("v=spf1"))
}

/// Whether the SPF record fails or soft-fails mail it does not list. The first `all` decides;
/// with none, a `redirect=` hands the decision to another record, and without one the result
/// is neutral.
fn spf_is_strict(record: &str) -> bool {
    let terms: Vec<String> = record
        .split_ascii_whitespace()
        .skip(1)
        .map(str::to_ascii_lowercase)
        .collect();
    if let Some(all) = terms
        .iter()
        .find(|t| matches!(t.as_str(), "all" | "+all" | "-all" | "~all" | "?all"))
    {
        return matches!(all.as_str(), "-all" | "~all");
    }
    terms.iter().any(|t| t.starts_with("redirect="))
}

/// The `tag=value` pairs of a DKIM or DMARC record, tags lowercased, values trimmed.
fn tags(record: &str) -> Vec<(String, String)> {
    record
        .split(';')
        .filter_map(|part| {
            let (tag, value) = part.split_once('=')?;
            Some((tag.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect()
}

fn tag<'a>(tags: &'a [(String, String)], name: &str) -> Option<&'a str> {
    tags.iter()
        .find(|(tag, _)| tag == name)
        .map(|(_, value)| value.as_str())
}

async fn dkim(
    lookup: &dyn TxtLookup,
    domain: &str,
    selector: Option<&str>,
    deadline: Instant,
) -> DomainFinding {
    let Some(selector) = selector else {
        return unknown(DomainRecord::Dkim);
    };
    let name = format!("{}._domainkey.{domain}", selector.to_ascii_lowercase());
    let Ok(records) = txt(lookup, &name, deadline).await else {
        return unknown(DomainRecord::Dkim);
    };
    if records.iter().any(|r| dkim_has_key(r)) {
        finding(DomainRecord::Dkim, DomainStatus::Ok, "dkim_ok")
    } else {
        finding(DomainRecord::Dkim, DomainStatus::Missing, "dkim_missing")
    }
}

/// Whether `record` is a DKIM key record with a key: `v=DKIM1`, when present, first, and a
/// non-empty `p=` (an empty one revokes the key).
fn dkim_has_key(record: &str) -> bool {
    let tags = tags(record);
    if let Some(position) = tags.iter().position(|(tag, _)| tag == "v")
        && (position != 0 || !tags[position].1.eq_ignore_ascii_case("DKIM1"))
    {
        return false;
    }
    tag(&tags, "p").is_some_and(|key| key.chars().any(|c| !c.is_whitespace()))
}

/// The domain and its parents, down to two labels, at most [`MAX_DMARC_NAMES`].
fn dmarc_domains(domain: &str) -> Vec<&str> {
    let mut out = vec![domain];
    let mut rest = domain;
    while let Some((_, parent)) = rest.split_once('.') {
        if !parent.contains('.') || out.len() == MAX_DMARC_NAMES {
            break;
        }
        out.push(parent);
        rest = parent;
    }
    out
}

async fn dmarc(lookup: &dyn TxtLookup, domain: &str, deadline: Instant) -> DomainFinding {
    for (level, name) in dmarc_domains(domain).into_iter().enumerate() {
        let Ok(records) = txt(lookup, &format!("_dmarc.{name}"), deadline).await else {
            return unknown(DomainRecord::Dmarc);
        };
        let found: Vec<Vec<(String, String)>> = records
            .iter()
            .map(|r| tags(r))
            .filter(|tags| {
                tags.first()
                    .is_some_and(|(tag, value)| tag == "v" && value.eq_ignore_ascii_case("DMARC1"))
            })
            .collect();
        let policy = match found.as_slice() {
            [] => continue,
            [tags] if level > 0 => tag(tags, "sp").or_else(|| tag(tags, "p")),
            [tags] => tag(tags, "p"),
            // More than one record: receivers apply none.
            _ => None,
        };
        return match policy.map(str::to_ascii_lowercase).as_deref() {
            Some("quarantine" | "reject") => {
                finding(DomainRecord::Dmarc, DomainStatus::Ok, "dmarc_ok")
            }
            _ => finding(DomainRecord::Dmarc, DomainStatus::Weak, "dmarc_weak"),
        };
    }
    finding(DomainRecord::Dmarc, DomainStatus::Missing, "dmarc_missing")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    static FAILED_BUILDS: AtomicUsize = AtomicUsize::new(0);

    fn unreadable_config() -> Option<TokioResolver> {
        FAILED_BUILDS.fetch_add(1, Ordering::SeqCst);
        None
    }

    #[tokio::test]
    async fn a_failed_resolver_build_is_tried_again() {
        let lookup = HickoryLookup {
            resolver: OnceLock::new(),
            build: unreadable_config,
        };
        assert_eq!(lookup.txt("bakery.example").await, Err(LookupError::Failed));
        assert_eq!(lookup.txt("bakery.example").await, Err(LookupError::Failed));
        assert_eq!(FAILED_BUILDS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn dmarc_walks_up_to_two_labels() {
        assert_eq!(
            dmarc_domains("a.mail.bakery.example"),
            vec![
                "a.mail.bakery.example",
                "mail.bakery.example",
                "bakery.example"
            ]
        );
        assert_eq!(dmarc_domains("bakery.example"), vec!["bakery.example"]);
        assert_eq!(dmarc_domains("localhost"), vec!["localhost"]);
        let deep = "a.b.c.d.e.f.g.h.i.j.example";
        assert_eq!(dmarc_domains(deep).len(), MAX_DMARC_NAMES);
    }

    #[test]
    fn spf_version_must_be_a_whole_term() {
        assert!(is_spf("v=spf1"));
        assert!(is_spf("V=SPF1 -all"));
        assert!(!is_spf("v=spf10 -all"));
        assert!(!is_spf("x v=spf1 -all"));
    }

    #[test]
    fn dkim_version_must_come_first() {
        assert!(dkim_has_key("p=abc"));
        assert!(dkim_has_key("v=DKIM1; p=ab c"));
        assert!(!dkim_has_key("k=rsa; v=DKIM1; p=abc"));
        assert!(!dkim_has_key("v=DKIM2; p=abc"));
        assert!(!dkim_has_key("v=DKIM1; p= "));
        assert!(!dkim_has_key("v=DKIM1"));
    }
}
