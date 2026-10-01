//! `SyncSession`: one run from handshake to summary.
//!
//! ```text
//! initiator → hello                 responder → helloAck
//! initiator → syncRequest(direction, filter, manifest)
//! responder → syncRequest(inverted, filter, manifest)
//! both compute the plan; the initiator shows it to the user
//! initiator → planDecision(planHash, approved, selection?)
//! responder → checks the hash against its own plan
//! files flow in the agreed direction, then playlists; sender → syncComplete
//! ```

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fl_core::{AudioFileFormat, Playlist, Uid};

use crate::connection::{ConnectionError, SyncConnection, DEFAULT_RECEIVE_TIMEOUT};
use crate::crypto::Key;
use crate::exchange::{initiator_hello, responder_hello, HelloError};
use crate::manifest::{plan as diff_plan, LibraryManifest, SyncFilter, SyncPlan, SyncSelection};
use crate::pairing::PairingIdentity;
use crate::path_sanitizer::destination_for_incoming_track;
use crate::protocol::{Direction, MANIFEST_WAIT_TIMEOUT, PLAN_REVIEW_TIMEOUT};
use crate::transfer::{self, ReceivedFile, TransferError, PART_FILE_MAX_AGE};
use crate::wire::*;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub tracks_transferred: i64,
    pub playlists_transferred: i64,
    pub bytes_transferred: i64,
    pub skipped: i64,
    /// Per-file failures; one bad file doesn't abort the run.
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub completed_files: i64,
    pub total_files: i64,
    pub bytes_transferred: i64,
    pub total_bytes: i64,
    pub current_file_name: Option<String>,
}

impl Progress {
    pub fn fraction(&self) -> f64 {
        if self.total_bytes <= 0 {
            return if self.total_files == 0 { 1.0 } else { 0.0 };
        }
        (self.bytes_transferred as f64 / self.total_bytes as f64).min(1.0)
    }
}

/// What this device brings to a run.
pub struct LocalContext {
    pub root: PathBuf,
    pub manifest: LibraryManifest,
    pub filter: SyncFilter,
    /// Full bodies, for when this side sends.
    pub playlists: Vec<Playlist>,
}

/// Files and playlists a receiving run landed, for the caller to register
/// (track IDs, playlist store, rescan).
#[derive(Debug, Clone, Default)]
pub struct Landed {
    pub files: Vec<ReceivedFile>,
    pub playlists: Vec<Playlist>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("The other device couldn't confirm it's paired with this one. Forget it and pair again.")]
    NotAuthenticated,
    #[error("The other device refused: {0}")]
    RefusedByPeer(String),
    #[error("Couldn't reach {0}. Make sure FLACtastic's Sync screen is open on it, and that your firewall allows FLACtastic on private networks.")]
    Unreachable(String),
    #[error("The other device connected but didn't answer. If it's a Mac, look for a password or permission prompt on its screen, then try again.")]
    PeerSilent,
    #[error("The other device speaks sync version {0}; this one speaks {v}. Update both to the same release.", v = crate::protocol::VERSION)]
    IncompatibleVersion(crate::protocol::Version),
    #[error("The other device sent something unexpected ({0}).")]
    UnexpectedMessage(String),
    #[error("Sync was cancelled.")]
    Declined,
    #[error("The library changed while you were reviewing — nothing was transferred. Try again.")]
    PlanChanged,
    #[error("No music folder is open.")]
    NoLibraryRoot,
    #[error("This sync needs {} but only {} is free.", bytes(*needed), bytes(*available))]
    InsufficientStorage { needed: i64, available: i64 },
    #[error("This sync would transfer {}, which is over the limit set for this device.", bytes(*.0))]
    TransferCapExceeded(i64),
    #[error("Sync was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Prepare(String),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

impl From<HelloError> for SessionError {
    fn from(e: HelloError) -> Self {
        match e {
            HelloError::NotAuthenticated => Self::NotAuthenticated,
            HelloError::RefusedByPeer(m) => Self::RefusedByPeer(m),
            HelloError::IncompatibleVersion(v) => Self::IncompatibleVersion(v),
            HelloError::UnexpectedMessage(m) => Self::UnexpectedMessage(m),
            HelloError::PeerSilent => Self::PeerSilent,
            HelloError::Connection(c) => Self::Connection(c),
        }
    }
}

/// `ByteCountFormatter` (`.file` style: decimal units).
pub fn bytes(n: i64) -> String {
    let n = n as f64;
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if n < 1000.0 {
        return format!("{} bytes", n as i64);
    }
    let mut v = n;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v < 10.0 {
        format!("{v:.1} {}", UNITS[i])
    } else {
        format!("{v:.0} {}", UNITS[i])
    }
}

/// The plan from the receiver's point of view; both sides compute it from
/// the same inputs, so both get the same `planHash`.
pub fn plan(
    direction: Direction,
    local_manifest: &LibraryManifest,
    local_filter: &SyncFilter,
    peer_manifest: &LibraryManifest,
    peer_filter: &SyncFilter,
    local_is_initiator: bool,
) -> SyncPlan {
    let initiator_is_receiver = direction == Direction::Pull;
    let we_receive = local_is_initiator == initiator_is_receiver;
    let (incoming, local_side, receiver_filter) = if we_receive {
        (peer_manifest, local_manifest, local_filter)
    } else {
        (local_manifest, peer_manifest, peer_filter)
    };
    diff_plan(incoming, local_side, direction, receiver_filter)
}

/// Refuses a run that can't finish: the per-device cap, then free space with
/// 500 MB headroom.
pub fn check_capacity(plan: &SyncPlan, filter: &SyncFilter, root: &Path) -> Result<(), SessionError> {
    let needed = plan.total_transfer_bytes();
    if filter.exceeds_total_cap(needed) {
        return Err(SessionError::TransferCapExceeded(needed));
    }
    let Some(available) = crate::fs_space::available_bytes(root) else { return Ok(()) };
    const HEADROOM: i64 = 500 * 1024 * 1024;
    if needed + HEADROOM > available as i64 {
        return Err(SessionError::InsufficientStorage { needed, available: available as i64 });
    }
    Ok(())
}

async fn send_error(conn: &SyncConnection, code: FailureCode, msg: impl Into<String>) {
    let _ = conn.send(&WireMessage::ProtocolError(ProtocolFailure::new(code, msg))).await;
}

// MARK: - Initiator

/// Runs the side the user is at. `approve` sees the plan and returns what
/// the user ticked (`everything()` if unchanged) or `None` to decline;
/// nothing moves before it returns.
#[allow(clippy::too_many_arguments)]
pub async fn run_initiator<A, AF>(
    conn: &SyncConnection,
    direction: Direction,
    identity: &PairingIdentity,
    paired_key: &Key,
    local: LocalContext,
    approve: A,
    progress: &(dyn Fn(&Progress) + Sync),
    cancelled: &AtomicBool,
) -> Result<(Summary, Landed), SessionError>
where
    A: FnOnce(SyncPlan) -> AF,
    AF: Future<Output = Option<SyncSelection>>,
{
    initiator_hello(conn, identity, paired_key).await?;
    conn.send(&WireMessage::SyncRequest(SyncRequest {
        direction,
        filter: local.filter.clone(),
        manifest: local.manifest.clone(),
    }))
    .await?;
    // Generous: the responder builds (hashes) its manifest only now.
    let peer = match conn.receive_message(MANIFEST_WAIT_TIMEOUT).await? {
        WireMessage::ProtocolError(f) => return Err(SessionError::RefusedByPeer(f.message)),
        WireMessage::SyncRequest(r) => r,
        other => return Err(SessionError::UnexpectedMessage(format!("expected the peer's manifest, got {}", other.tag()))),
    };
    let full = plan(direction, &local.manifest, &local.filter, &peer.manifest, &peer.filter, true);

    let Some(selection) = approve(full.clone()).await else {
        let _ = conn
            .send(&WireMessage::PlanDecision(PlanDecision { plan_hash: full.plan_hash(), approved: false, selection: None }))
            .await;
        return Err(SessionError::Declined);
    };
    let selected = full.restricted(&selection);
    // Capacity is checked against the selection: choosing what fits is the
    // point of choosing.
    if direction == Direction::Pull {
        if let Err(e) = check_capacity(&selected, &local.filter, &local.root) {
            let _ = conn
                .send(&WireMessage::PlanDecision(PlanDecision { plan_hash: full.plan_hash(), approved: false, selection: None }))
                .await;
            return Err(e);
        }
    }
    // The hash is always the full plan's; the selection narrows it identically.
    conn.send(&WireMessage::PlanDecision(PlanDecision {
        plan_hash: full.plan_hash(),
        approved: true,
        selection: (!selection.is_everything()).then_some(selection),
    }))
    .await?;

    match direction {
        Direction::Push => send_payload(&selected, conn, &local, progress, cancelled).await.map(|s| (s, Landed::default())),
        Direction::Pull => receive_payload(&selected, conn, &local, progress, cancelled).await,
    }
}

// MARK: - Responder

/// Runs the side that accepted the connection, given its opening `hello`.
/// `prepare` (which hashes the library) runs only after `helloAck` is out.
#[allow(clippy::too_many_arguments)]
pub async fn run_responder<P, PF>(
    conn: &SyncConnection,
    opening: &Hello,
    identity: &PairingIdentity,
    authenticate: impl FnOnce(Uid) -> Option<Key>,
    prepare: P,
    progress: &(dyn Fn(&Progress) + Sync),
    cancelled: &AtomicBool,
) -> Result<(Summary, Landed), SessionError>
where
    P: FnOnce() -> PF,
    PF: Future<Output = Result<LocalContext, String>>,
{
    responder_hello(conn, opening, identity, authenticate).await?;
    let request = match conn.receive_message(DEFAULT_RECEIVE_TIMEOUT).await? {
        WireMessage::SyncRequest(r) => r,
        other => return Err(SessionError::UnexpectedMessage(format!("expected syncRequest, got {}", other.tag()))),
    };
    let local = match prepare().await {
        Ok(l) => l,
        Err(e) => {
            send_error(conn, FailureCode::InternalFailure, e.clone()).await;
            return Err(SessionError::Prepare(e));
        }
    };
    let ours = request.direction.inverted();
    conn.send(&WireMessage::SyncRequest(SyncRequest {
        direction: ours,
        filter: local.filter.clone(),
        manifest: local.manifest.clone(),
    }))
    .await?;
    let full = plan(request.direction, &local.manifest, &local.filter, &request.manifest, &request.filter, false);

    // Someone is ticking through a checklist on the other device.
    let decision = match conn.receive_message(PLAN_REVIEW_TIMEOUT).await? {
        WireMessage::PlanDecision(d) => d,
        other => return Err(SessionError::UnexpectedMessage(format!("expected planDecision, got {}", other.tag()))),
    };
    if !decision.approved {
        return Err(SessionError::Declined);
    }
    if decision.plan_hash != full.plan_hash() {
        send_error(conn, FailureCode::PlanStale, "The plan changed.").await;
        return Err(SessionError::PlanChanged);
    }
    // Narrowed only after the hash check, and only ever narrowed.
    let selected = full.restricted(&decision.selection.unwrap_or_default());
    if ours == Direction::Pull {
        check_capacity(&selected, &local.filter, &local.root)?;
    }
    match ours {
        Direction::Push => send_payload(&selected, conn, &local, progress, cancelled).await.map(|s| (s, Landed::default())),
        Direction::Pull => receive_payload(&selected, conn, &local, progress, cancelled).await,
    }
}

// MARK: - Payload

fn file_name(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned()
}

async fn send_payload(
    plan: &SyncPlan,
    conn: &SyncConnection,
    local: &LocalContext,
    progress: &(dyn Fn(&Progress) + Sync),
    cancelled: &AtomicBool,
) -> Result<Summary, SessionError> {
    let mut summary = Summary::default();
    let queue = plan.all_incoming_tracks();
    let mut state = Progress { total_files: queue.len() as i64, total_bytes: plan.total_transfer_bytes(), ..Default::default() };
    // The last file actually sent, in case the receiver later reports its
    // checksum failed (MAC-ISSUES #6).
    let mut last_sent: Option<(String, i64)> = None;

    for entry in queue {
        if cancelled.load(Ordering::Relaxed) {
            return Err(SessionError::Cancelled);
        }
        state.current_file_name = Some(file_name(&entry.relative_path));
        progress(&state);
        let source = fl_core::model::join_relative(&local.root, &entry.relative_path);
        let base = state.bytes_transferred;
        let result = transfer::send(entry, &source, conn, cancelled, |sent| {
            let mut s = state.clone();
            s.bytes_transferred = base + sent;
            progress(&s);
        })
        .await;
        match result {
            Ok(o) => {
                if o.previous_failed {
                    if let Some((title, bytes)) = last_sent.take() {
                        summary.tracks_transferred -= 1;
                        summary.bytes_transferred -= bytes;
                        summary.failures.push(format!("{title}: {}", TransferError::HashMismatch));
                    }
                }
                if o.sent == 0 {
                    summary.skipped += 1;
                    last_sent = None;
                } else {
                    summary.tracks_transferred += 1;
                    last_sent = Some((entry.title.clone(), o.sent));
                }
                summary.bytes_transferred += o.sent;
            }
            Err(e) if e.is_fatal() => {
                return Err(match e {
                    TransferError::Cancelled => SessionError::Cancelled,
                    TransferError::Connection(c) => SessionError::Connection(c),
                    other => SessionError::UnexpectedMessage(other.to_string()),
                })
            }
            Err(e) => {
                summary.failures.push(format!("{}: {e}", entry.title));
                last_sent = None;
            }
        }
        state.completed_files += 1;
        state.bytes_transferred += entry.file_size;
        progress(&state);
    }

    let wanted: HashSet<Uid> =
        plan.new_playlists.iter().map(|p| p.id).chain(plan.playlist_conflicts.iter().map(|c| c.incoming.id)).collect();
    let playlists: Vec<WirePlaylist> = local.playlists.iter().filter(|p| wanted.contains(&p.id)).map(WirePlaylist::from).collect();
    if !playlists.is_empty() {
        summary.playlists_transferred = playlists.len() as i64;
        conn.send(&WireMessage::Playlists(PlaylistPayload { playlists })).await?;
    }
    conn.send(&WireMessage::SyncComplete(Completion {
        tracks_transferred: summary.tracks_transferred,
        playlists_transferred: summary.playlists_transferred,
        bytes_transferred: summary.bytes_transferred,
    }))
    .await?;
    // A Mac receiver reports the last file's checksum failure after its
    // fileEnd; catch it if it's already on the wire.
    if let Some((title, bytes)) = last_sent {
        if let Ok(WireMessage::ProtocolError(f)) = conn.receive_message(std::time::Duration::from_millis(750)).await {
            if f.code == FailureCode::HashMismatch {
                summary.tracks_transferred -= 1;
                summary.bytes_transferred -= bytes;
                summary.failures.push(format!("{title}: {}", TransferError::HashMismatch));
            }
        }
    }
    Ok(summary)
}

async fn receive_payload(
    plan: &SyncPlan,
    conn: &SyncConnection,
    local: &LocalContext,
    progress: &(dyn Fn(&Progress) + Sync),
    cancelled: &AtomicBool,
) -> Result<(Summary, Landed), SessionError> {
    let mut summary = Summary::default();
    let mut landed = Landed::default();
    let mut state = Progress {
        total_files: plan.all_incoming_tracks().len() as i64,
        total_bytes: plan.total_transfer_bytes(),
        ..Default::default()
    };
    let root = local.root.clone();
    // Only files the approved plan covers are accepted.
    let approved: HashSet<Uid> = plan.all_incoming_tracks().iter().map(|t| t.track_id).collect();
    let approved_playlists: HashSet<Uid> =
        plan.new_playlists.iter().map(|p| p.id).chain(plan.playlist_conflicts.iter().map(|c| c.incoming.id)).collect();

    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(SessionError::Cancelled);
        }
        match conn.receive_message(DEFAULT_RECEIVE_TIMEOUT).await? {
            WireMessage::FileStart(start) => {
                let ext = start.relative_path.rsplit('.').next().unwrap_or("");
                let format = AudioFileFormat::classify_ext(ext).unwrap_or(AudioFileFormat::Flac);
                if !approved.contains(&start.track_id) || !local.filter.allows(format, None, start.file_size) {
                    conn.send(&WireMessage::FileAccept(FileAccept { track_id: start.track_id, resume_offset: 0, skip: true }))
                        .await?;
                    summary.skipped += 1;
                    continue;
                }
                let base = state.bytes_transferred;
                let result = transfer::receive(
                    &start,
                    conn,
                    &root,
                    |rel| destination_for_incoming_track(rel, &root),
                    |_, _| false,
                    cancelled,
                    |written| {
                        let mut s = state.clone();
                        s.bytes_transferred = base + written;
                        progress(&s);
                    },
                )
                .await;
                match result {
                    Ok(Some(f)) => {
                        summary.tracks_transferred += 1;
                        summary.bytes_transferred += f.bytes_written;
                        landed.files.push(f);
                    }
                    Ok(None) => summary.skipped += 1,
                    Err(e) if e.is_fatal() => {
                        return Err(match e {
                            TransferError::Cancelled => SessionError::Cancelled,
                            TransferError::Connection(c) => SessionError::Connection(c),
                            other => SessionError::UnexpectedMessage(other.to_string()),
                        })
                    }
                    Err(e) => summary.failures.push(format!("{}: {e}", file_name(&start.relative_path))),
                }
                state.completed_files += 1;
                state.bytes_transferred = base + start.file_size;
                state.current_file_name = Some(file_name(&start.relative_path));
                progress(&state);
            }
            WireMessage::Playlists(payload) => {
                // Held to the plan like tracks: an unticked playlist must not
                // arrive just because the filter allows it.
                let allowed: Vec<Playlist> = payload
                    .playlists
                    .into_iter()
                    .filter(|p| approved_playlists.contains(&p.id) && local.filter.allows_playlist(p.id))
                    .map(Playlist::from)
                    .collect();
                summary.playlists_transferred = allowed.len() as i64;
                landed.playlists = allowed;
            }
            WireMessage::SyncComplete(_) => break,
            WireMessage::Cancel(c) => return Err(SessionError::UnexpectedMessage(c.reason)),
            WireMessage::ProtocolError(f) => return Err(SessionError::UnexpectedMessage(f.message)),
            other => return Err(SessionError::UnexpectedMessage(format!("{} during transfer", other.tag()))),
        }
    }
    transfer::sweep_abandoned_part_files(&root, PART_FILE_MAX_AGE);
    Ok((summary, landed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counts_read_like_foundation() {
        assert_eq!(bytes(512), "512 bytes");
        assert_eq!(bytes(1_500_000), "1.5 MB");
        assert_eq!(bytes(42_000_000_000), "42 GB");
    }
}
