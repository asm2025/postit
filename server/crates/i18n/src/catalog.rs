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

fn load(locale: Locale, sources: &[&'static str]) -> Catalog {
    let langid: LanguageIdentifier = locale.tag().parse().unwrap_or_default();
    let mut bundle = FluentBundle::new_concurrent(vec![langid]);
    let mut ids = BTreeSet::new();
    if locale == Locale::Ar {
        bundle.set_formatter(Some(arabic_number));
    }
    for src in sources {
        // A partially parsed resource is still usable; the completeness test catches gaps.
        let resource = FluentResource::try_new((*src).to_owned()).unwrap_or_else(|(r, _)| r);
        for entry in resource.entries() {
            if let fluent_syntax::ast::Entry::Message(m) = entry {
                ids.insert(m.id.name.to_owned());
            }
        }
        let _ = bundle.add_resource(resource);
    }
    Catalog { bundle, ids }
}

static EN_US: LazyLock<Catalog> = LazyLock::new(|| load(Locale::EnUs, &[EN_US_MAIL]));
static AR: LazyLock<Catalog> = LazyLock::new(|| load(Locale::Ar, &[AR_MAIL]));

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
