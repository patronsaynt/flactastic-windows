//! Organizer: profiles, name templates, move planning, execution and the
//! preview tree.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::apple_json::Uid;
use crate::format::AudioFileFormat;
use crate::import_copy;
use crate::model::{relative_path, Track};
use crate::text::{self, trim, trim_ws};

// MARK: - Profile

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupingField {
    AlbumArtist,
    Artist,
    Album,
    Genre,
    Year,
    Decade,
    Format,
    FirstLetterOfArtist,
}

impl GroupingField {
    pub const ALL: [GroupingField; 8] = [
        Self::AlbumArtist,
        Self::Artist,
        Self::Album,
        Self::Genre,
        Self::Year,
        Self::Decade,
        Self::Format,
        Self::FirstLetterOfArtist,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            Self::AlbumArtist => "Album Artist",
            Self::Artist => "Artist",
            Self::Album => "Album",
            Self::Genre => "Genre",
            Self::Year => "Year",
            Self::Decade => "Decade",
            Self::Format => "Format",
            Self::FirstLetterOfArtist => "Artist Initial",
        }
    }

    pub fn default_template(self) -> &'static str {
        match self {
            Self::AlbumArtist => "{albumArtist}",
            Self::Artist => "{artist}",
            Self::Album => "{album}",
            Self::Genre => "{genre}",
            Self::Year => "{year}",
            Self::Decade => "{decade}s",
            Self::Format => "{format}",
            Self::FirstLetterOfArtist => "{artistInitial}",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HierarchyLevel {
    pub id: Uid,
    pub group_by: GroupingField,
    pub name: String,
    pub name_template: String,
}

impl<'de> Deserialize<'de> for HierarchyLevel {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            id: Uid,
            group_by: GroupingField,
            #[serde(default)]
            name: Option<String>,
            name_template: String,
        }
        let r = Raw::deserialize(d)?;
        Ok(HierarchyLevel {
            id: r.id,
            group_by: r.group_by,
            name: r.name.unwrap_or_else(|| r.group_by.display_name().to_owned()),
            name_template: r.name_template,
        })
    }
}

impl HierarchyLevel {
    pub fn new(group_by: GroupingField, name: Option<&str>, template: Option<&str>) -> Self {
        HierarchyLevel {
            id: Uid::new_v4(),
            group_by,
            name: name.unwrap_or(group_by.display_name()).to_owned(),
            name_template: template.unwrap_or(group_by.default_template()).to_owned(),
        }
    }

    /// Non-empty label; the fallback folder name when a template renders empty.
    pub fn display_label(&self) -> String {
        let t = trim_ws(&self.name);
        if t.is_empty() {
            self.group_by.display_name().to_owned()
        } else {
            t.to_owned()
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizerProfile {
    pub id: Uid,
    pub name: String,
    pub levels: Vec<HierarchyLevel>,
    pub file_template: String,
    /// Reduce "A & B" / "A feat. B" to "A" for bucketing and rendering.
    #[serde(default = "default_true")]
    pub use_primary_artist_only: bool,
    /// Delete source folders left without audio after a move.
    #[serde(default = "default_true")]
    pub delete_empty_originals: bool,
}

impl OrganizerProfile {
    pub fn new(name: &str, levels: Vec<HierarchyLevel>) -> Self {
        OrganizerProfile {
            id: Uid::new_v4(),
            name: name.to_owned(),
            levels,
            file_template: "{track} - {title}".into(),
            use_primary_artist_only: true,
            delete_empty_originals: true,
        }
    }

    pub fn new_level() -> HierarchyLevel {
        HierarchyLevel::new(GroupingField::Genre, Some("New level"), Some("{genre}"))
    }

    pub fn default_profile() -> Self {
        Self::new(
            "Artist / Album",
            vec![
                HierarchyLevel::new(GroupingField::AlbumArtist, Some("Album Artist"), None),
                HierarchyLevel::new(GroupingField::Album, Some("Album"), Some("{album} ({year})")),
            ],
        )
    }

    pub fn presets() -> Vec<Self> {
        vec![
            Self::default_profile(),
            Self::new(
                "Genre / Artist / Album",
                vec![
                    HierarchyLevel::new(GroupingField::Genre, Some("Genre"), None),
                    HierarchyLevel::new(GroupingField::AlbumArtist, Some("Album Artist"), None),
                    HierarchyLevel::new(GroupingField::Album, Some("Album"), None),
                ],
            ),
            Self::new(
                "Year / Album",
                vec![
                    HierarchyLevel::new(GroupingField::Year, Some("Year"), None),
                    HierarchyLevel::new(GroupingField::Album, Some("Album"), Some("{albumArtist} - {album}")),
                ],
            ),
        ]
    }

    /// "<name> Copy" with fresh level IDs.
    pub fn duplicate(&self) -> Self {
        OrganizerProfile {
            id: Uid::new_v4(),
            name: format!("{} Copy", self.name),
            levels: self.levels.iter().map(|l| HierarchyLevel { id: Uid::new_v4(), ..l.clone() }).collect(),
            ..self.clone()
        }
    }
}

// MARK: - Template

pub struct Token {
    pub key: &'static str,
    pub description: &'static str,
}

pub const ALL_TOKENS: [Token; 11] = [
    Token { key: "artist", description: "Track artist" },
    Token { key: "albumArtist", description: "Album artist" },
    Token { key: "album", description: "Album title" },
    Token { key: "title", description: "Track title" },
    Token { key: "track", description: "Track number, zero-padded" },
    Token { key: "year", description: "Release year" },
    Token { key: "decade", description: "Release decade (e.g. 1990)" },
    Token { key: "genre", description: "Genre" },
    Token { key: "format", description: "File format (FLAC, MP3, …)" },
    Token { key: "ext", description: "File extension (lowercased)" },
    Token { key: "artistInitial", description: "First letter of artist" },
];

/// Renders `{token}` placeholders, then sanitises for the filesystem.
pub fn render(template: &str, track: &Track, fallback: &str, primary_artist_only: bool) -> String {
    let mut out = template.to_owned();
    for t in &ALL_TOKENS {
        out = out.replace(&format!("{{{}}}", t.key), &token_value(t.key, track, primary_artist_only));
    }
    import_copy::sanitize(&out, fallback)
}

/// First credited artist of "A & B", "A; B", "A feat. B", "A, B", "A / B".
pub fn primary_artist(raw: &str) -> String {
    const SEPS: [&str; 13] =
        [";", " feat.", " feat ", " ft.", " ft ", " featuring ", " & ", " and ", " with ", " vs.", " vs ", "/", ","];
    let mut s = raw.to_owned();
    for sep in SEPS {
        if let Some((start, _)) = text::find_ascii_ci(&s, sep) {
            s.truncate(start);
        }
    }
    trim(&s).to_owned()
}

fn token_value(key: &str, t: &Track, primary_only: bool) -> String {
    let prim = |s: String| if primary_only { primary_artist(&s) } else { s };
    match key {
        "artist" => prim(t.artist.clone().unwrap_or_else(|| "Unknown Artist".into())),
        "albumArtist" => {
            prim(t.album_artist.clone().or_else(|| t.artist.clone()).unwrap_or_else(|| "Unknown Artist".into()))
        }
        "album" => t.album.clone().unwrap_or_else(|| "Unknown Album".into()),
        "title" => {
            if t.title.is_empty() {
                "Untitled".into()
            } else {
                t.title.clone()
            }
        }
        "track" => t.track_number.map(text::pad2).unwrap_or_else(|| "00".into()),
        "year" => t.year.map(|y| y.to_string()).unwrap_or_default(),
        "decade" => t.year.map(|y| ((y / 10) * 10).to_string()).unwrap_or_default(),
        "genre" => t.genre.clone().unwrap_or_default(),
        "format" => t.file_format.raw_value().to_uppercase(),
        "ext" => t.extension().to_lowercase(),
        "artistInitial" => {
            let mut name = t.album_artist.clone().or_else(|| t.artist.clone()).unwrap_or_default();
            if primary_only {
                name = primary_artist(&name);
            }
            trim_ws(&name).chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "#".into())
        }
        _ => String::new(),
    }
}

// MARK: - Planner

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpStatus {
    Move,
    Unchanged,
    Conflict(String),
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub id: Uid,
    pub track: Track,
    pub source: PathBuf,
    pub destination: PathBuf,
    pub status: OpStatus,
}

impl Operation {
    pub fn destination_folder(&self) -> &Path {
        self.destination.parent().unwrap_or(Path::new(""))
    }
}

/// Lexically normalises `.`/`..` (`standardizedFileURL`).
pub fn standardize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn path_key_ci(p: &Path) -> String {
    p.to_string_lossy().to_lowercase()
}

pub fn plan(tracks: &[Track], profile: &OrganizerProfile, root: &Path) -> Vec<Operation> {
    let prelim: Vec<(&Track, PathBuf)> = tracks
        .iter()
        .map(|t| {
            let mut folder = root.to_path_buf();
            for level in &profile.levels {
                folder.push(render(&level.name_template, t, &level.display_label(), profile.use_primary_artist_only));
            }
            // Extension casing is preserved (a case-only rename is a no-op on
            // case-insensitive filesystems and would re-plan forever).
            let ext = t.extension();
            let name = render(&profile.file_template, t, "Untitled", profile.use_primary_artist_only);
            let file = if ext.is_empty() { name } else { format!("{name}.{ext}") };
            (t, standardize(&folder.join(file)))
        })
        .collect();

    let mut collisions: HashMap<&PathBuf, usize> = HashMap::new();
    for (_, d) in &prelim {
        *collisions.entry(d).or_default() += 1;
    }

    let mut ops: Vec<Operation> = prelim
        .iter()
        .map(|(t, dest)| {
            let source = standardize(&t.path);
            let status = if path_key_ci(&source) == path_key_ci(dest) {
                OpStatus::Unchanged
            } else if collisions[dest] > 1 {
                OpStatus::Conflict("Multiple tracks resolve to this destination".into())
            } else {
                OpStatus::Move
            };
            Operation { id: t.id, track: (*t).clone(), source, destination: dest.clone(), status }
        })
        .collect();

    ops.sort_by(|a, b| text::standard_compare(&a.destination.to_string_lossy(), &b.destination.to_string_lossy()));
    ops
}

// MARK: - Executor

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Moving,
    CleaningUp,
    Validating,
}

#[derive(Debug, Default)]
pub struct ExecResult {
    pub moved: Vec<(Uid, PathBuf)>,
    pub skipped: usize,
    pub failed: Vec<(PathBuf, String)>,
    /// Recorded as moved but missing afterwards.
    pub lost: Vec<PathBuf>,
}

/// Runs the moves sequentially (deterministic `(n)` suffixes), prunes
/// audio-free source folders, then validates.
pub fn execute(ops: &[Operation], root: &Path, delete_empty_originals: bool, mut progress: impl FnMut(Phase, usize, usize)) -> ExecResult {
    let mut r = ExecResult::default();
    let mut parents: HashSet<PathBuf> = HashSet::new();
    let total = ops.len();
    progress(Phase::Moving, 0, total);

    for (i, op) in ops.iter().enumerate() {
        if op.status == OpStatus::Unchanged {
            r.skipped += 1;
            progress(Phase::Moving, i + 1, total);
            continue;
        }
        let res = (|| -> std::io::Result<PathBuf> {
            let folder = op.destination_folder();
            std::fs::create_dir_all(folder)?;
            let dest = if op.destination.exists() {
                let name = op.destination.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                import_copy::unique_destination(&name, folder)
            } else {
                op.destination.clone()
            };
            import_copy::move_file(&op.source, &dest)?;
            Ok(dest)
        })();
        match res {
            Ok(dest) => {
                if let Some(p) = op.source.parent() {
                    parents.insert(p.to_path_buf());
                }
                r.moved.push((op.track.id, dest));
            }
            Err(e) => r.failed.push((op.source.clone(), e.to_string())),
        }
        progress(Phase::Moving, i + 1, total);
    }

    if delete_empty_originals {
        prune_audio_free_dirs(&parents, root, &mut progress);
    }

    let n = r.moved.len();
    progress(Phase::Validating, 0, n);
    for (i, (_, p)) in r.moved.iter().enumerate() {
        if !p.exists() {
            r.lost.push(p.clone());
        }
        progress(Phase::Validating, i + 1, n);
    }
    r
}

fn prune_audio_free_dirs(leaves: &HashSet<PathBuf>, root: &Path, progress: &mut impl FnMut(Phase, usize, usize)) {
    let root = standardize(root);
    let mut ordered: Vec<PathBuf> = leaves.iter().map(|p| standardize(p)).collect();
    ordered.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let total = ordered.len();
    progress(Phase::CleaningUp, 0, total);
    let mut visited: HashSet<PathBuf> = HashSet::new();
    for (i, leaf) in ordered.iter().enumerate() {
        let mut dir = leaf.clone();
        while dir != root && dir.starts_with(&root) && !visited.contains(&dir) {
            visited.insert(dir.clone());
            if dir.exists() {
                if directory_contains_audio(&dir) {
                    break;
                }
                let _ = std::fs::remove_dir_all(&dir);
            }
            if !dir.pop() {
                break;
            }
        }
        progress(Phase::CleaningUp, i + 1, total);
    }
}

/// Any audio file anywhere below `dir` (hidden entries skipped).
pub fn directory_contains_audio(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    for e in rd.flatten() {
        let name = e.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let p = e.path();
        match e.file_type() {
            Ok(ft) if ft.is_dir() => {
                if directory_contains_audio(&p) {
                    return true;
                }
            }
            Ok(_) => {
                if AudioFileFormat::classify(&p).is_some() {
                    return true;
                }
            }
            Err(_) => {}
        }
    }
    false
}

/// Old → new relative paths for `TrackIdStore::rename_paths`.
pub fn relative_path_map(ops: &[Operation], result: &ExecResult, root: &Path) -> HashMap<String, String> {
    let by_id: HashMap<Uid, &Operation> =
        ops.iter().filter(|o| o.status == OpStatus::Move).map(|o| (o.track.id, o)).collect();
    let mut map = HashMap::new();
    for (id, new_path) in &result.moved {
        let Some(op) = by_id.get(id) else { continue };
        if let (Some(old), Some(new)) = (relative_path(&op.source, root), relative_path(new_path, root)) {
            map.insert(old, new);
        }
    }
    map
}

// MARK: - Preview tree

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRow {
    pub id: String,
    pub depth: usize,
    pub label: String,
    pub is_folder: bool,
    pub badge: Option<String>,
    pub is_conflict: bool,
    pub is_unchanged: bool,
}

/// Flattens a sorted plan into folder/file rows (first `limit` rows) and
/// returns how many tracks were left out.
pub fn preview_rows(ops: &[Operation], root: &Path, limit: usize) -> (Vec<PreviewRow>, usize) {
    let root_parts: Vec<String> = standardize(root).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let mut rows = Vec::new();
    let mut prev: Vec<String> = Vec::new();
    let mut shown = 0;
    for op in ops {
        if rows.len() >= limit {
            break;
        }
        let mut parts: Vec<String> =
            standardize(&op.destination).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if parts.len() >= root_parts.len() && parts[..root_parts.len()] == root_parts[..] {
            parts.drain(..root_parts.len());
        }
        let Some(file) = parts.pop() else { continue };
        let shared = parts.iter().zip(&prev).take_while(|(a, b)| a == b).count();
        for depth in shared..parts.len() {
            rows.push(PreviewRow {
                id: format!("folder/{}", parts[..=depth].join("/")),
                depth,
                label: parts[depth].clone(),
                is_folder: true,
                badge: None,
                is_conflict: false,
                is_unchanged: false,
            });
        }
        let depth = parts.len();
        prev = parts;
        rows.push(PreviewRow {
            id: format!("file/{}", op.id),
            depth,
            label: file,
            is_folder: false,
            badge: Some(op.track.file_format.display_name().to_owned()),
            is_conflict: matches!(op.status, OpStatus::Conflict(_)),
            is_unchanged: op.status == OpStatus::Unchanged,
        });
        shown += 1;
    }
    (rows, ops.len().saturating_sub(shown))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(root: &Path, rel: &str, artist: &str, album: &str, n: i64, title: &str) -> Track {
        let p = crate::model::join_relative(root, rel);
        let mut t = Track::make_from_path(&p).unwrap();
        t.artist = Some(artist.into());
        t.album = Some(album.into());
        t.track_number = Some(n);
        t.title = title.into();
        t.year = Some(1997);
        t
    }

    #[test]
    fn primary_artist_splits() {
        assert_eq!(primary_artist("A feat. B"), "A");
        assert_eq!(primary_artist("A & B"), "A");
        assert_eq!(primary_artist("AC/DC"), "AC");
        assert_eq!(primary_artist("Solo"), "Solo");
    }

    #[test]
    fn plan_execute_and_prune() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = track(root, "inbox/x/one.FLAC", "Band feat. Guest", "LP", 1, "One");
        let b = track(root, "inbox/x/two.flac", "Band", "LP", 2, "Two");
        let c = track(root, "Band/LP (1997)/03 - Three.flac", "Band", "LP", 3, "Three");
        for t in [&a, &b, &c] {
            std::fs::create_dir_all(t.path.parent().unwrap()).unwrap();
            std::fs::write(&t.path, b"x").unwrap();
        }
        std::fs::write(root.join("inbox/x/cover.jpg"), b"x").unwrap();

        let profile = OrganizerProfile::default_profile();
        let ops = plan(&[a.clone(), b.clone(), c.clone()], &profile, root);
        let dest_a = ops.iter().find(|o| o.id == a.id).unwrap();
        assert_eq!(dest_a.destination, standardize(&root.join("Band").join("LP (1997)").join("01 - One.FLAC")));
        assert_eq!(ops.iter().find(|o| o.id == c.id).unwrap().status, OpStatus::Unchanged);

        let (rows, hidden) = preview_rows(&ops, root, 300);
        assert_eq!(hidden, 0);
        assert_eq!(rows[0].label, "Band");
        assert!(rows[1].is_folder);
        assert_eq!(rows.iter().filter(|r| !r.is_folder).count(), 3);

        let res = execute(&ops, root, true, |_, _, _| {});
        assert_eq!(res.moved.len(), 2);
        assert_eq!(res.skipped, 1);
        assert!(res.lost.is_empty() && res.failed.is_empty());
        assert!(!root.join("inbox").exists(), "audio-free source tree is pruned");
        let map = relative_path_map(&ops, &res, root);
        assert_eq!(map["inbox/x/two.flac"], "Band/LP (1997)/02 - Two.flac");
    }

    #[test]
    fn conflicts_detected() {
        let dir = tempfile::tempdir().unwrap();
        let a = track(dir.path(), "a.flac", "X", "Y", 1, "Same");
        let b = track(dir.path(), "b.flac", "X", "Y", 1, "Same");
        let ops = plan(&[a, b], &OrganizerProfile::default_profile(), dir.path());
        assert!(ops.iter().all(|o| matches!(o.status, OpStatus::Conflict(_))));
    }

    #[test]
    fn profile_decodes_old_level_without_name() {
        let json = r#"[{"id":"E621E1F8-C36C-495A-93FC-0C247A3E6E5F","name":"P","levels":[{"id":"E621E1F8-C36C-495A-93FC-0C247A3E6E50","groupBy":"decade","nameTemplate":"{decade}s"}],"fileTemplate":"{title}"}]"#;
        let p: Vec<OrganizerProfile> = serde_json::from_str(json).unwrap();
        assert_eq!(p[0].levels[0].name, "Decade");
        assert!(p[0].use_primary_artist_only && p[0].delete_empty_originals);
    }
}
