//! String helpers standing in for the Foundation APIs the macOS code leans on:
//! `localizedStandardCompare`, `localizedCaseInsensitiveCompare`,
//! `folding([.diacriticInsensitive, .caseInsensitive])` and
//! `trimmingCharacters(in: .whitespacesAndNewlines)`.

use std::cmp::Ordering;
use std::sync::OnceLock;

use icu_collator::{Collator, CollatorOptions, Numeric, Strength};
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

fn root_locale() -> icu_locid::Locale {
    // The Mac collates with the user's current locale; the root collation is
    // the stable common base and agrees with it for Latin/Cyrillic/Greek text.
    icu_locid::Locale::UND
}

fn standard_collator() -> &'static Collator {
    static C: OnceLock<Collator> = OnceLock::new();
    C.get_or_init(|| {
        let mut o = CollatorOptions::new();
        o.strength = Some(Strength::Tertiary);
        o.numeric = Some(Numeric::On);
        Collator::try_new(&root_locale().into(), o).expect("root collation data is compiled in")
    })
}

fn case_insensitive_collator() -> &'static Collator {
    static C: OnceLock<Collator> = OnceLock::new();
    C.get_or_init(|| {
        let mut o = CollatorOptions::new();
        o.strength = Some(Strength::Secondary);
        Collator::try_new(&root_locale().into(), o).expect("root collation data is compiled in")
    })
}

/// `localizedStandardCompare` — Finder ordering: case-insensitive first,
/// numeric runs compared by value ("Track 2" < "Track 10").
/// Ties at every collation level fall back to code-point order so the
/// result is total (Foundation's `.forcedOrdering`).
pub fn standard_compare(a: &str, b: &str) -> Ordering {
    let c = standard_collator();
    // Tertiary strength distinguishes case; Finder only uses case as a late
    // tiebreak, so compare case-insensitively first.
    match case_insensitive_numeric(a, b) {
        Ordering::Equal => {}
        o => return o,
    }
    match c.compare(a, b) {
        Ordering::Equal => a.cmp(b),
        o => o,
    }
}

fn case_insensitive_numeric(a: &str, b: &str) -> Ordering {
    static C: OnceLock<Collator> = OnceLock::new();
    let c = C.get_or_init(|| {
        let mut o = CollatorOptions::new();
        o.strength = Some(Strength::Secondary);
        o.numeric = Some(Numeric::On);
        Collator::try_new(&root_locale().into(), o).expect("root collation data is compiled in")
    });
    c.compare(a, b)
}

/// `localizedCaseInsensitiveCompare`.
pub fn case_insensitive_compare(a: &str, b: &str) -> Ordering {
    case_insensitive_collator().compare(a, b)
}

/// `a.localizedStandardCompare(b) == .orderedAscending`.
pub fn standard_less(a: &str, b: &str) -> bool {
    standard_compare(a, b) == Ordering::Less
}

/// `folding(options: [.diacriticInsensitive, .caseInsensitive], locale: nil)`.
pub fn fold_diacritics_and_case(s: &str) -> String {
    let stripped: String = s.nfd().filter(|c| !is_combining_mark(*c)).collect();
    let lowered = stripped.to_lowercase();
    // Foundation's case folding maps ß → ss; mirror the common special cases.
    lowered.replace('ß', "ss").nfc().collect()
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace())
}

/// `trimmingCharacters(in: .whitespaces)` — spaces/tabs only, not newlines.
pub fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() && !matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{0B}' | '\u{0C}'))
}

/// Case-insensitive search for an ASCII `needle`, returning the byte range of
/// the first match in `hay` (`range(of:options:.caseInsensitive)`).
pub fn find_ascii_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return if n.is_empty() { Some((0, 0)) } else { None };
    }
    'outer: for start in 0..=h.len() - n.len() {
        if !hay.is_char_boundary(start) {
            continue;
        }
        for (i, nb) in n.iter().enumerate() {
            if !h[start + i].eq_ignore_ascii_case(nb) {
                continue 'outer;
            }
        }
        if hay.is_char_boundary(start + n.len()) {
            return Some((start, start + n.len()));
        }
    }
    None
}

/// Case-insensitive split on an ASCII separator.
pub fn split_ascii_ci(s: &str, sep: &str) -> Vec<String> {
    if sep.is_empty() {
        return vec![s.to_owned()];
    }
    let mut out = Vec::new();
    let mut rest = s;
    while let Some((a, b)) = find_ascii_ci(rest, sep) {
        out.push(rest[..a].to_owned());
        rest = &rest[b..];
    }
    out.push(rest.to_owned());
    out
}

/// `String(format: "%02d", n)`.
pub fn pad2(n: i64) -> String {
    format!("{n:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_compare_is_numeric_and_case_insensitive() {
        assert_eq!(standard_compare("Track 2", "Track 10"), Ordering::Less);
        assert_eq!(standard_compare("abc", "ABD"), Ordering::Less);
        assert_eq!(standard_compare("Émile", "Zed"), Ordering::Less);
        assert_ne!(standard_compare("abc", "ABC"), Ordering::Equal);
    }

    #[test]
    fn folding() {
        assert_eq!(fold_diacritics_and_case("Beyoncé"), "beyonce");
        assert_eq!(fold_diacritics_and_case("SIGUR RÓS"), "sigur ros");
    }

    #[test]
    fn ci_split() {
        assert_eq!(split_ascii_ci("A FEAT. B feat. C", " feat. "), vec!["A", "B", "C"]);
    }
}
