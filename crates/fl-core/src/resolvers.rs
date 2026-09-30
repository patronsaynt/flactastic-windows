//! `ArtistResolver` and `GenreResolver`.

use std::collections::{HashMap, HashSet};

use crate::model::Track;
use crate::text::{self, fold_diacritics_and_case, trim};

/// Explicit multi-value delimiters, in priority order. NUL and `;` are the
/// app's own storage delimiters; ` / ` is iTunes' (spaces required so
/// "AC/DC" survives).
const EXPLICIT_DELIMITERS: [&str; 3] = ["\u{0}", ";", " / "];

fn explicitly_separated(raw: &str) -> Option<Vec<String>> {
    for d in EXPLICIT_DELIMITERS {
        if raw.contains(d) {
            return Some(
                raw.split(d)
                    .map(|p| trim(p).to_owned())
                    .filter(|p| !p.is_empty())
                    .collect(),
            );
        }
    }
    None
}

/// Separator tokens, longest alternatives first so "feat." is not partially matched.
const SEPARATOR_PATTERNS: [&str; 12] = [
    " featuring ", " feat. ", " feat ", " ft. ", " ft ", " vs. ", " vs ", " with ", " & ", " x ", " X ", ", ",
];

#[derive(Debug, Clone, Default)]
pub struct ArtistResolver {
    /// Normalised key → preferred display casing observed in tags.
    pub canonical_by_key: HashMap<String, String>,
}

impl ArtistResolver {
    /// Pass 1 takes single-artist tags as display names (first wins); pass 2
    /// harvests names from multi-artist strings without overwriting pass 1.
    pub fn new(tracks: &[Track]) -> Self {
        let mut by_key: HashMap<String, String> = HashMap::new();
        let mut multi: Vec<String> = Vec::new();

        for track in tracks {
            for raw in [&track.album_artist, &track.artist] {
                let Some(raw) = raw.as_deref().filter(|s| !s.is_empty()) else { continue };
                let trimmed = trim(raw);
                if Self::contains_separator(trimmed) || explicitly_separated(trimmed).is_some() {
                    multi.push(trimmed.to_owned());
                } else {
                    by_key.entry(Self::key(trimmed)).or_insert_with(|| trimmed.to_owned());
                }
            }
        }

        // Swift iterates a Set (unordered); first-seen order keeps this deterministic.
        let mut seen = HashSet::new();
        for raw in multi.into_iter().filter(|r| seen.insert(r.clone())) {
            let pieces = explicitly_separated(&raw).unwrap_or_else(|| Self::split_on_separators(&raw));
            for piece in pieces {
                by_key.entry(Self::key(&piece)).or_insert(piece);
            }
        }

        ArtistResolver { canonical_by_key: by_key }
    }

    /// Lowercased, diacritic-folded, whitespace-collapsed.
    pub fn key(raw: &str) -> String {
        let folded = fold_diacritics_and_case(raw);
        folded.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub fn display_name(&self, key: &str) -> String {
        self.canonical_by_key.get(key).cloned().unwrap_or_else(|| key.to_owned())
    }

    /// Splits only when every fragment is a known canonical artist, unless the
    /// string uses an explicit delimiter.
    pub fn split(&self, raw: Option<&str>) -> Vec<String> {
        let Some(raw) = raw.filter(|s| !s.is_empty()) else { return vec![] };
        let trimmed = trim(raw);
        if trimmed.is_empty() {
            return vec![];
        }
        if let Some(explicit) = explicitly_separated(trimmed) {
            return explicit;
        }
        if self.canonical_by_key.contains_key(&Self::key(trimmed)) || !Self::contains_separator(trimmed) {
            return vec![trimmed.to_owned()];
        }
        let fragments = Self::split_on_separators(trimmed);
        if fragments.iter().all(|f| self.canonical_by_key.contains_key(&Self::key(f))) {
            fragments
        } else {
            vec![trimmed.to_owned()]
        }
    }

    pub fn explicitly_separated(raw: &str) -> Option<Vec<String>> {
        explicitly_separated(raw)
    }

    /// Lead credit using only the explicit delimiters: `"Deadmau5 ; Rob Swire"` → `"Deadmau5"`.
    pub fn primary_credit(raw: &str) -> String {
        match explicitly_separated(raw) {
            Some(pieces) if !pieces.is_empty() => pieces[0].clone(),
            _ => trim(raw).to_owned(),
        }
    }

    /// `" ; "`-joined storage form.
    pub fn join_explicit<S: AsRef<str>>(artists: &[S]) -> String {
        artists
            .iter()
            .map(|a| trim(a.as_ref()))
            .filter(|a| !a.is_empty())
            .collect::<Vec<_>>()
            .join(" ; ")
    }

    /// Explicit lists render as `"A, B, C"`; everything else passes through.
    pub fn display_string(raw: Option<&str>) -> Option<String> {
        let raw = raw.filter(|s| !s.is_empty())?;
        Some(match explicitly_separated(raw) {
            Some(p) => p.join(", "),
            None => raw.to_owned(),
        })
    }

    /// Canonical keys for a credit, deduped, order preserved.
    pub fn keys_for_credit(&self, raw: Option<&str>) -> Vec<String> {
        let mut seen = HashSet::new();
        self.split(raw)
            .into_iter()
            .map(|p| Self::key(&p))
            .filter(|k| seen.insert(k.clone()))
            .collect()
    }

    pub fn contains_separator(s: &str) -> bool {
        let lower = format!(" {s} ").to_lowercase();
        SEPARATOR_PATTERNS.iter().any(|t| lower.contains(&t.to_lowercase()))
    }

    pub fn split_on_separators(s: &str) -> Vec<String> {
        let mut fragments = vec![s.to_owned()];
        for token in SEPARATOR_PATTERNS {
            fragments = fragments.iter().flat_map(|p| text::split_ascii_ci(p, token)).collect();
        }
        fragments
            .into_iter()
            .map(|f| trim(&f).to_owned())
            .filter(|f| !f.is_empty())
            .collect()
    }
}

/// Packs a primary genre plus up to three secondaries into one GENRE string:
/// `"Primary ; Secondary One ; Secondary Two"`.
pub struct GenreResolver;

impl GenreResolver {
    pub const MAX_SECONDARY_COUNT: usize = 3;

    pub fn join<S: AsRef<str>>(primary: Option<&str>, secondary: &[S]) -> Option<String> {
        let clean_primary = primary.map(trim).filter(|s| !s.is_empty());
        let mut seen = HashSet::new();
        if let Some(p) = clean_primary {
            seen.insert(p.to_lowercase());
        }
        let mut clean_secondary: Vec<&str> = Vec::new();
        for raw in secondary {
            if clean_secondary.len() >= Self::MAX_SECONDARY_COUNT {
                break;
            }
            let t = trim(raw.as_ref());
            if t.is_empty() || !seen.insert(t.to_lowercase()) {
                continue;
            }
            clean_secondary.push(t);
        }
        let all: Vec<&str> = clean_primary.into_iter().chain(clean_secondary).collect();
        if all.is_empty() {
            None
        } else {
            Some(all.join(" ; "))
        }
    }

    pub fn split(raw: Option<&str>) -> (Option<String>, Vec<String>) {
        let Some(raw) = raw else { return (None, vec![]) };
        let trimmed = trim(raw);
        if trimmed.is_empty() {
            return (None, vec![]);
        }
        let Some(pieces) = explicitly_separated(trimmed) else {
            return (Some(trimmed.to_owned()), vec![]);
        };
        let Some(first) = pieces.first().cloned() else { return (None, vec![]) };
        let mut seen: HashSet<String> = [first.to_lowercase()].into();
        let mut secondary = Vec::new();
        for piece in pieces.into_iter().skip(1) {
            if secondary.len() >= Self::MAX_SECONDARY_COUNT {
                break;
            }
            if seen.insert(piece.to_lowercase()) {
                secondary.push(piece);
            }
        }
        (Some(first), secondary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // AlbumArtistRollUpTests

    #[test]
    fn primary_credit() {
        assert_eq!(ArtistResolver::primary_credit("Deadmau5 ; Rob Swire"), "Deadmau5");
        assert_eq!(ArtistResolver::primary_credit("Deadmau5 ; Wolfgang Gartner"), "Deadmau5");
        assert_eq!(ArtistResolver::primary_credit("Deadmau5"), "Deadmau5");
        assert_eq!(ArtistResolver::primary_credit("  Deadmau5  "), "Deadmau5");
        assert_eq!(ArtistResolver::primary_credit("Tyler, The Creator"), "Tyler, The Creator");
        assert_eq!(ArtistResolver::primary_credit("AC/DC"), "AC/DC");
        assert_eq!(ArtistResolver::primary_credit("Above & Beyond"), "Above & Beyond");
        assert_eq!(ArtistResolver::primary_credit("Deadmau5 / Rob Swire"), "Deadmau5");
    }

    #[test]
    fn split_only_known_fragments() {
        let mk = |a: &str| {
            let mut t = Track::new("/x".into(), "t".into(), crate::format::AudioFileFormat::Flac);
            t.artist = Some(a.into());
            t
        };
        let r = ArtistResolver::new(&[mk("Earth, Wind & Fire"), mk("Skrillex"), mk("Diplo")]);
        // Faithful to the Mac: pass 2 harvests "Earth"/"Wind"/"Fire" as known
        // names, so a comma name seen only as itself still splits.
        // Logged in docs/MAC-ISSUES.md.
        assert_eq!(r.split(Some("Earth, Wind & Fire")), vec!["Earth", "Wind", "Fire"]);
        assert_eq!(r.split(Some("Skrillex & Diplo")), vec!["Skrillex", "Diplo"]);
        assert_eq!(r.split(Some("Skrillex & Nobody")), vec!["Skrillex & Nobody"]);
        assert_eq!(r.split(Some("A ; B")), vec!["A", "B"]);
        assert_eq!(r.display_name("skrillex"), "Skrillex");
    }

    // GenreResolverTests

    #[test]
    fn genre_join() {
        let j = |p: Option<&str>, s: &[&str]| GenreResolver::join(p, s);
        assert_eq!(j(Some("Rock"), &["Pop", "Jazz"]).as_deref(), Some("Rock ; Pop ; Jazz"));
        assert_eq!(j(Some("Rock"), &[]).as_deref(), Some("Rock"));
        assert_eq!(j(Some("Rock"), &["Pop", "Jazz", "Soul", "Blues"]).as_deref(), Some("Rock ; Pop ; Jazz ; Soul"));
        assert_eq!(j(Some("Rock"), &["rock", "Pop"]).as_deref(), Some("Rock ; Pop"));
        assert_eq!(j(Some("Rock"), &["Pop", "pop", "Jazz"]).as_deref(), Some("Rock ; Pop ; Jazz"));
        assert_eq!(j(Some("  Rock "), &["  ", "Pop  "]).as_deref(), Some("Rock ; Pop"));
        assert_eq!(j(None, &[]), None);
        assert_eq!(j(Some("  "), &["  "]), None);
        assert_eq!(j(None, &["Pop"]).as_deref(), Some("Pop"));
    }

    #[test]
    fn genre_split() {
        assert_eq!(GenreResolver::split(Some("Rock ; Pop ; Jazz")), (Some("Rock".into()), vec!["Pop".into(), "Jazz".into()]));
        assert_eq!(GenreResolver::split(Some("Rock")), (Some("Rock".into()), vec![]));
        assert_eq!(GenreResolver::split(None), (None, vec![]));
        assert_eq!(GenreResolver::split(Some("")), (None, vec![]));
        assert_eq!(GenreResolver::split(Some("   ")), (None, vec![]));
        assert_eq!(
            GenreResolver::split(Some("Rock ; Pop ; Pop ; Jazz ; Soul ; Blues")),
            (Some("Rock".into()), vec!["Pop".into(), "Jazz".into(), "Soul".into()])
        );
        assert_eq!(GenreResolver::split(Some("Rock / Pop")), (Some("Rock".into()), vec!["Pop".into()]));
    }

    #[test]
    fn genre_round_trip() {
        for (p, s) in [("Rock", vec!["Pop", "Jazz"]), ("Electronic", vec![]), ("Hip-Hop", vec!["R&B", "Soul", "Funk"])] {
            let joined = GenreResolver::join(Some(p), &s);
            let (rp, rs) = GenreResolver::split(joined.as_deref());
            assert_eq!(rp.as_deref(), Some(p));
            assert_eq!(rs, s);
        }
    }
}
