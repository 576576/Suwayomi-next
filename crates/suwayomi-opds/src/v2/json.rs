//! Media types, vocabulary constants, BCP-47 validation and JSON output for
//! the OPDS 2.0 layer.

use serde::Serialize;
use serde_json::{Map, Value};

/// OPDS 2.0 catalog feeds.
pub const MIME_OPDS_JSON: &str = "application/opds+json";
/// Readium Divina manifests (the Divina profile's own media type — not
/// `application/webpub+json`, which is the default RWPM profile).
pub const MIME_DIVINA_JSON: &str = "application/divina+json";
/// `metadata.conformsTo` for a Divina manifest.
pub const CONFORMS_TO_DIVINA: &str = "https://readium.org/webpub-manifest/profiles/divina";
/// JSON-LD context of a Readium manifest. OPDS 2.0 feeds must **not** carry
/// one: `feed.schema.json` routes unknown top-level keys to the subcollection
/// schema, and a `@context` string satisfies neither of its `anyOf` branches.
pub const CONTEXT_WEBPUB: &str = "http://readium.org/webpub-manifest/context.jsonld";

// Acquisition rels. `publication.schema.json#$defs.acquisition` enumerates
// these; bare `open-access` / `sample` / `download` are not in the enum.
pub const REL_ACQUISITION: &str = "http://opds-spec.org/acquisition";
pub const REL_ACQUISITION_OPEN_ACCESS: &str = "http://opds-spec.org/acquisition/open-access";

// Non-acquisition rels (free-form strings, but stay with the 1.2 vocabulary).
pub const REL_SELF: &str = "self";
pub const REL_START: &str = "start";
pub const REL_SEARCH: &str = "search";
pub const REL_SUBSECTION: &str = "subsection";
pub const REL_ALTERNATE: &str = "alternate";
pub const REL_NEXT: &str = "next";
pub const REL_PREVIOUS: &str = "previous";
pub const REL_FIRST: &str = "first";
pub const REL_LAST: &str = "last";

/// How many publications a feed page carries (mirrors `opdsItemsPerPage`'s
/// default while that setting is unwired — see §7 of the plan).
pub const ITEMS_PER_PAGE: usize = 50;

/// Mihon sources declare these two pseudo-languages for "works in every
/// language" / "other"; they pass the BCP-47 grammar but name no language, so
/// they must not be written into `metadata.language`.
const NON_LANGUAGE_TAGS: [&str; 2] = ["all", "other"];

fn all_alpha(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphabetic())
}

fn all_alnum(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn alpha_n(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && all_alpha(s)
}

fn alnum_n(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && all_alnum(s)
}

/// The BCP-47 grammar from `metadata.schema.json`'s `language` pattern,
/// hand-rolled so the crate needs no regex dependency.
///
/// Accepts exactly what the pattern accepts (the 26 grandfathered tags, then
/// `language ["-" script] ["-" region] *("-" variant) *("-" extension)
/// ["-" privateuse]`), minus the named capture groups the pattern carries.
fn is_bcp47(tag: &str) -> bool {
    const GRANDFATHERED: [&str; 26] = [
        "en-GB-oed",
        "i-ami",
        "i-bnn",
        "i-default",
        "i-enochian",
        "i-hak",
        "i-klingon",
        "i-lux",
        "i-mingo",
        "i-navajo",
        "i-pwn",
        "i-tao",
        "i-tay",
        "i-tsu",
        "sgn-BE-FR",
        "sgn-BE-NL",
        "sgn-CH-DE",
        "art-lojban",
        "cel-gaulish",
        "no-bok",
        "no-nyn",
        "zh-guoyu",
        "zh-hakka",
        "zh-min",
        "zh-min-nan",
        "zh-xiang",
    ];
    if GRANDFATHERED.iter().any(|g| g.eq_ignore_ascii_case(tag)) {
        return true;
    }

    let mut parts = tag.split('-').peekable();
    let Some(first) = parts.next() else {
        return false;
    };

    // `x-…` is a private-use tag on its own.
    if first.eq_ignore_ascii_case("x") {
        let rest: Vec<&str> = parts.collect();
        return !rest.is_empty() && rest.iter().all(|p| alnum_n(p, 1, 8));
    }

    // language: 2-3 alpha (plus up to three 3-alpha extlangs) | 4 alpha | 5-8 alpha
    if alpha_n(first, 2, 3) {
        let mut extlangs = 0;
        while extlangs < 3 {
            match parts.peek() {
                Some(p) if alpha_n(p, 3, 3) => {
                    parts.next();
                    extlangs += 1;
                }
                _ => break,
            }
        }
    } else if !(alpha_n(first, 4, 4) || alpha_n(first, 5, 8)) {
        return false;
    }

    // script: 4 alpha
    if let Some(p) = parts.peek()
        && alpha_n(p, 4, 4)
    {
        parts.next();
    }
    // region: 2 alpha | 3 digit
    if let Some(p) = parts.peek()
        && (alpha_n(p, 2, 2) || (p.len() == 3 && p.bytes().all(|b| b.is_ascii_digit())))
    {
        parts.next();
    }
    // variant: 5-8 alnum | 4 chars starting with a digit
    while let Some(p) = parts.peek() {
        let variant =
            alnum_n(p, 5, 8) || (p.len() == 4 && p.as_bytes().first().is_some_and(u8::is_ascii_digit) && all_alnum(p));
        if !variant {
            break;
        }
        parts.next();
    }
    // extension: single alnum (not `x`) followed by one or more 2-8 alnum subtags
    while let Some(p) = parts.peek() {
        if !(p.len() == 1 && all_alnum(p) && !p.eq_ignore_ascii_case("x")) {
            break;
        }
        parts.next();
        let mut subtags = 0;
        while let Some(q) = parts.peek()
            && alnum_n(q, 2, 8)
        {
            parts.next();
            subtags += 1;
        }
        if subtags == 0 {
            return false;
        }
    }
    // private use: `x` followed by one or more 1-8 alnum subtags
    if let Some(p) = parts.peek()
        && p.eq_ignore_ascii_case("x")
    {
        parts.next();
        let mut subtags = 0;
        while let Some(q) = parts.peek()
            && alnum_n(q, 1, 8)
        {
            parts.next();
            subtags += 1;
        }
        if subtags == 0 {
            return false;
        }
    }

    parts.next().is_none()
}

/// Returns the tag to write into `metadata.language`, or `None` when the raw
/// value is empty, a pseudo-language, or not a BCP-47 tag at all.
pub fn language_tag(raw: &str) -> Option<&str> {
    let tag = raw.trim();
    if tag.is_empty() || NON_LANGUAGE_TAGS.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
        return None;
    }
    is_bcp47(tag).then_some(tag)
}

/// Trims a value and drops it when nothing is left.
pub fn non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Builds a link `properties` object from static keys.
pub fn props(pairs: impl IntoIterator<Item = (&'static str, Value)>) -> Map<String, Value> {
    pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect()
}

/// Compact JSON. Serialisation of these models cannot fail (no non-string map
/// keys), but a failure must not be papered over with an empty body.
pub fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|err| {
        tracing::error!(error = %err, "OPDS 2.0 response failed to serialise");
        String::from("{}")
    })
}

#[cfg(test)]
mod tests {
    use super::{is_bcp47, language_tag};

    #[test]
    fn accepts_registered_and_private_tags() {
        for tag in ["en", "ja", "zh-Hans", "pt-BR", "zh-Hant-TW", "es-419", "en-x-custom", "x-private"] {
            assert!(is_bcp47(tag), "{tag} should be a valid BCP-47 tag");
        }
    }

    #[test]
    fn accepts_grandfathered_tags() {
        for tag in ["en-GB-oed", "i-klingon", "zh-min-nan", "sgn-BE-FR"] {
            assert!(is_bcp47(tag), "{tag} is grandfathered but valid");
        }
    }

    #[test]
    fn rejects_malformed_tags() {
        for tag in ["", "e", "-en", "en-", "en--US", "en_US", "1234", "en-12345-", "x-"] {
            assert!(!is_bcp47(tag), "{tag} should be rejected");
        }
    }

    #[test]
    fn pseudo_languages_are_dropped() {
        // `all` and `other` pass the grammar but name no language.
        assert!(is_bcp47("all"));
        assert!(is_bcp47("other"));
        assert_eq!(language_tag("all"), None);
        assert_eq!(language_tag("OTHER"), None);
        assert_eq!(language_tag("  "), None);
        assert_eq!(language_tag("en"), Some("en"));
        assert_eq!(language_tag(" pt-BR "), Some("pt-BR"));
    }
}
