//! Fluent catalogs, one bundle per locale, embedded at compile time.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

use crate::Locale;

enum ArgValue {
    Str(String),
    Num(i64),
}

/// Named message arguments.
#[derive(Default)]
pub struct Args(Vec<(&'static str, ArgValue)>);

impl Args {
    /// An empty argument set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a string argument.
    #[must_use]
    pub fn str(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.0.push((name, ArgValue::Str(value.into())));
        self
    }

    /// Adds a number argument; it drives plural selection and is shown in the locale's digits.
    #[must_use]
    pub fn num(mut self, name: &'static str, value: i64) -> Self {
        self.0.push((name, ArgValue::Num(value)));
        self
    }

    fn to_fluent(&self) -> FluentArgs<'_> {
        let mut out = FluentArgs::new();
        for (name, value) in &self.0 {
            match value {
                ArgValue::Str(s) => out.set(*name, s.as_str()),
                #[allow(clippy::cast_precision_loss)]
                ArgValue::Num(n) => out.set(*name, *n as f64),
            }
        }
        out
    }
}

struct Catalog {
    bundle: FluentBundle<FluentResource>,
    ids: BTreeSet<String>,
}

const EN_US_MAIL: &str = include_str!("../locales/en-US/mail.ftl");
const AR_MAIL: &str = include_str!("../locales/ar/mail.ftl");
const EN_US_NOTIFY: &str = include_str!("../locales/en-US/notify.ftl");
const AR_NOTIFY: &str = include_str!("../locales/ar/notify.ftl");

/// Renders whole numbers with Arabic-Indic digits.
fn arabic_number<M>(value: &FluentValue<'_>, _memoizer: &M) -> Option<String> {
    let FluentValue::Number(n) = value else {
        return None;
    };
    if n.value.fract() != 0.0 {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    let digits = (n.value as i64).to_string();
    Some(
        digits
            .chars()
            .map(|c| match c.to_digit(10) {
                Some(d) => char::from_u32(0x0660 + d).unwrap_or(c),
                None => c,
            })
            .collect(),
    )
}

/// The embedded `.ftl` sources of a locale.
fn sources(locale: Locale) -> &'static [&'static str] {
    match locale {
        Locale::EnUs => &[EN_US_MAIL, EN_US_NOTIFY],
        Locale::Ar => &[AR_MAIL, AR_NOTIFY],
    }
}

/// Builds a locale's bundle, reporting any parse or duplicate-id error instead of hiding it.
fn try_load(locale: Locale) -> Result<Catalog, String> {
    let langid: LanguageIdentifier = locale.tag().parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    // Bidi isolation marks would end up in plain-text subjects and bodies; the HTML templates
    // isolate user values with `<bdi>` instead.
    bundle.set_use_isolating(false);
    let mut ids = BTreeSet::new();
    if locale == Locale::Ar {
        bundle.set_formatter(Some(arabic_number));
    }
    for src in sources(locale) {
        let resource = FluentResource::try_new((*src).to_owned()).map_err(|(_, errors)| {
            format!("{} catalog does not parse: {errors:?}", locale.tag())
        })?;
        for entry in resource.entries() {
            if let fluent_syntax::ast::Entry::Message(m) = entry {
                ids.insert(m.id.name.to_owned());
            }
        }
        bundle
            .add_resource(resource)
            .map_err(|errors| format!("{} catalog is invalid: {errors:?}", locale.tag()))?;
    }
    Ok(Catalog { bundle, ids })
}

/// The catalogs are embedded, so a failure here is a build defect that the unit tests catch; at
/// run time `t` degrades to the message id rather than failing a request.
fn load(locale: Locale) -> Catalog {
    try_load(locale).unwrap_or_else(|_| Catalog {
        bundle: FluentBundle::new_concurrent(Vec::new()),
        ids: BTreeSet::new(),
    })
}

static EN_US: LazyLock<Catalog> = LazyLock::new(|| load(Locale::EnUs));
static AR: LazyLock<Catalog> = LazyLock::new(|| load(Locale::Ar));

fn catalog(locale: Locale) -> &'static Catalog {
    match locale {
        Locale::EnUs => &EN_US,
        Locale::Ar => &AR,
    }
}

fn format(locale: Locale, id: &str, args: &Args) -> Option<String> {
    let cat = catalog(locale);
    let pattern = cat.bundle.get_message(id)?.value()?;
    let fluent_args = args.to_fluent();
    let mut errors = Vec::new();
    Some(
        cat.bundle
            .format_pattern(pattern, Some(&fluent_args), &mut errors)
            .into_owned(),
    )
}

/// Translates `id`, falling back to `en-US`, then to the id itself. Never panics.
#[must_use]
pub fn t(locale: Locale, id: &str, args: &Args) -> String {
    format(locale, id, args)
        .or_else(|| format(Locale::EnUs, id, args))
        .unwrap_or_else(|| id.to_owned())
}

/// Every message id defined for `locale`.
#[must_use]
pub fn message_ids(locale: Locale) -> BTreeSet<String> {
    catalog(locale).ids.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_parses_and_every_message_formats() {
        for locale in Locale::ALL {
            let cat = try_load(locale).unwrap_or_else(|e| unreachable!("{e}"));
            assert!(!cat.ids.is_empty(), "{}", locale.tag());
            let args = Args::new()
                .num("count", 3)
                .num("more", 3)
                .str("name", "Ada")
                .str("account", "Shop")
                .str("platform", "mastodon")
                .str("reason", "a reason");
            let fluent_args = args.to_fluent();
            for id in message_ids(locale) {
                let pattern = cat
                    .bundle
                    .get_message(&id)
                    .and_then(|m| m.value())
                    .unwrap_or_else(|| unreachable!("{}: {id} has no value", locale.tag()));
                let mut errors = Vec::new();
                let out = cat
                    .bundle
                    .format_pattern(pattern, Some(&fluent_args), &mut errors);
                assert!(errors.is_empty(), "{}: {id}: {errors:?}", locale.tag());
                assert!(!out.is_empty() && out != id, "{}: {id}", locale.tag());
            }
        }
    }

    #[test]
    fn output_carries_no_bidi_isolation_marks() {
        for locale in Locale::ALL {
            let out = t(locale, "pending-subject", &Args::new().num("count", 5));
            assert!(
                !out.chars().any(|c| ('\u{2066}'..='\u{2069}').contains(&c)),
                "{out:?}"
            );
        }
    }
}
