//! `FileTransfer`: one file's bytes across a `SyncConnection`, in both roles.
//!
//! Incoming files land in `<root>/.flactastic/incoming/<ID>.part`, are
//! verified against the sender's SHA-256 and only then moved into place, so
//! the scanner never sees a half-written track. The `.part` length is the
//! resume record.
//!
//! MAC-ISSUES #6: a Mac receiver reports a checksum mismatch *after* the
//! sender's `fileEnd`, where the sender reads it as the answer to its next
//! `fileStart`. As a receiver this port records the failure without sending
//! that message; as a sender it recognises a stray `hashMismatch` in place of
//! `fileAccept`, charges it to the previous file and reads on. See
//! SYNC-INTEROP.md.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use fl_core::Uid;

use crate::connection::{ConnectionError, SyncConnection, DEFAULT_RECEIVE_TIMEOUT};
use crate::frame::FrameType;
use crate::manifest::{digests_match, hex_digest_of_file, TrackManifestEntry};
use crate::path_sanitizer::Rejection;
use crate::protocol::{FILE_CHUNK_BYTES, MAX_FILE_BYTES};
use crate::wire::*;

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("The transferred file didn't match its checksum and was discarded.")]
    HashMismatch,
    #[error("Expected {declared} bytes but received {received}.")]
    SizeMismatch { declared: i64, received: i64 },
    #[error("The other device offered a {0}-byte file, which is over the limit.")]
    DeclaredSizeTooLarge(i64),
    #[error("Couldn't read {0}.")]
    SourceUnreadable(String),
    #[error("Couldn't write the incoming file: {0}")]
    CannotWrite(String),
    #[error("The other device refused the transfer: {0}")]
    RejectedByPeer(String),
    #[error("{0}")]
    Path(#[from] Rejection),
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

impl TransferError {
    /// Errors after which the stream position is unknown: the run must stop.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Connection(_) | Self::Cancelled | Self::SizeMismatch { .. })
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SendOutcome {
    /// Bytes actually sent (0 when the receiver already had the file).
    pub sent: i64,
    /// The receiver reported the *previous* file's checksum failed (Mac
    /// ordering, MAC-ISSUES #6).
    pub previous_failed: bool,
}

fn name_of(p: &str) -> String {
    p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned()
}

/// Offers one file and streams it if the receiver wants it.
pub async fn send(
    entry: &TrackManifestEntry,
    source: &Path,
    conn: &SyncConnection,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(i64),
) -> Result<SendOutcome, TransferError> {
    conn.send(&WireMessage::FileStart(FileStart {
        track_id: entry.track_id,
        relative_path: entry.relative_path.clone(),
        file_size: entry.file_size,
        content_hash: entry.content_hash.clone(),
        tag_fingerprint: entry.tag_fingerprint.clone(),
    }))
    .await?;

    let mut outcome = SendOutcome::default();
    let accept = loop {
        match conn.receive_message(DEFAULT_RECEIVE_TIMEOUT).await? {
            WireMessage::FileAccept(a) => break a,
            WireMessage::ProtocolError(f) if f.code == FailureCode::HashMismatch && !outcome.previous_failed => {
                outcome.previous_failed = true;
            }
            WireMessage::ProtocolError(f) => return Err(TransferError::RejectedByPeer(f.message)),
            _ => return Err(ConnectionError::ProtocolViolation("Expected fileAccept.".into()).into()),
        }
    };
    if accept.track_id != entry.track_id {
        return Err(ConnectionError::ProtocolViolation("fileAccept for the wrong file.".into()).into());
    }
    if accept.skip {
        return Ok(outcome);
    }

    let mut f = std::fs::File::open(source).map_err(|_| TransferError::SourceUnreadable(name_of(&entry.relative_path)))?;
    if accept.resume_offset > 0 {
        f.seek(SeekFrom::Start(accept.resume_offset as u64))
            .map_err(|_| TransferError::SourceUnreadable(name_of(&entry.relative_path)))?;
    }
    let mut buf = vec![0u8; FILE_CHUNK_BYTES];
    let mut sent = accept.resume_offset;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(TransferError::Cancelled);
        }
        let n = read_full(&mut f, &mut buf).map_err(|_| TransferError::SourceUnreadable(name_of(&entry.relative_path)))?;
        if n == 0 {
            break;
        }
        conn.send_chunk(&buf[..n]).await?;
        sent += n as i64;
        progress(sent);
    }
    conn.send(&WireMessage::FileEnd(FileEnd { track_id: entry.track_id })).await?;
    outcome.sent = sent - accept.resume_offset;
    Ok(outcome)
}

/// Fills `buf` unless the file ends first (chunks are full-size, like the Mac's).
fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match f.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[derive(Debug, Clone)]
pub struct ReceivedFile {
    pub track_id: Uid,
    pub destination: PathBuf,
    pub relative_path: String,
    pub bytes_written: i64,
}

fn incoming_dir(root: &Path) -> PathBuf {
    root.join(".flactastic").join("incoming")
}

async fn refuse(conn: &SyncConnection, code: FailureCode, msg: &str) -> Result<(), TransferError> {
    conn.send(&WireMessage::ProtocolError(ProtocolFailure::new(code, msg))).await?;
    Ok(())
}

/// Accepts one file announced by `start`. `placement` maps the sender's
/// untrusted path to a local destination (built on `PathSanitizer`).
pub async fn receive(
    start: &FileStart,
    conn: &SyncConnection,
    root: &Path,
    placement: impl FnOnce(&str) -> Result<PathBuf, Rejection>,
    already_have: impl FnOnce(Uid, &str) -> bool,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(i64),
) -> Result<Option<ReceivedFile>, TransferError> {
    if start.file_size < 0 || start.file_size > MAX_FILE_BYTES {
        refuse(conn, FailureCode::SizeExceeded, "File too large.").await?;
        return Err(TransferError::DeclaredSizeTooLarge(start.file_size));
    }
    let destination = match placement(&start.relative_path) {
        Ok(d) => d,
        Err(e) => {
            refuse(conn, FailureCode::InvalidPath, "Rejected path.").await?;
            return Err(e.into());
        }
    };
    if already_have(start.track_id, &start.relative_path) {
        conn.send(&WireMessage::FileAccept(FileAccept { track_id: start.track_id, resume_offset: 0, skip: true })).await?;
        return Ok(None);
    }

    let dir = incoming_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| TransferError::CannotWrite(e.to_string()))?;
    let part = dir.join(format!("{}.part", start.track_id));
    // A `.part` longer than declared belongs to another version of the file.
    let held = std::fs::metadata(&part).map(|m| m.len() as i64).unwrap_or(0);
    let resume_offset = if held <= start.file_size { held } else { 0 };
    conn.send(&WireMessage::FileAccept(FileAccept { track_id: start.track_id, resume_offset, skip: false })).await?;

    let written = write_chunks(conn, &part, resume_offset, start.file_size, start.track_id, cancelled, &mut progress).await?;
    if written != start.file_size {
        let _ = std::fs::remove_file(&part);
        return Err(TransferError::SizeMismatch { declared: start.file_size, received: written });
    }

    // Verify before anything enters the library; a bad file is deleted, not
    // kept for resume (resuming on wrong bytes would repeat the failure).
    let part2 = part.clone();
    let actual = tokio::task::spawn_blocking(move || hex_digest_of_file(&part2, &|| false))
        .await
        .map_err(|e| TransferError::CannotWrite(e.to_string()))?
        .map_err(|e| TransferError::CannotWrite(e.to_string()))?;
    if !digests_match(&actual, &start.content_hash) {
        let _ = std::fs::remove_file(&part);
        // No protocolError here: the sender has moved on (MAC-ISSUES #6).
        return Err(TransferError::HashMismatch);
    }

    install(&part, &destination).map_err(|e| TransferError::CannotWrite(e.to_string()))?;
    Ok(Some(ReceivedFile {
        track_id: start.track_id,
        destination,
        relative_path: start.relative_path.clone(),
        bytes_written: written,
    }))
}

async fn write_chunks(
    conn: &SyncConnection,
    part: &Path,
    offset: i64,
    declared: i64,
    track_id: Uid,
    cancelled: &AtomicBool,
    progress: &mut impl FnMut(i64),
) -> Result<i64, TransferError> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(part)
        .map_err(|e| TransferError::CannotWrite(e.to_string()))?;
    f.set_len(offset as u64).map_err(|e| TransferError::CannotWrite(e.to_string()))?;
    f.seek(SeekFrom::End(0)).map_err(|e| TransferError::CannotWrite(e.to_string()))?;
    let mut written = offset;
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err(TransferError::Cancelled);
        }
        let frame = conn.receive_frame(DEFAULT_RECEIVE_TIMEOUT).await?;
        match frame.ty {
            FrameType::FileChunk => {
                written += frame.payload.len() as i64;
                // Stop as soon as a peer exceeds its declaration.
                if written > declared {
                    return Err(TransferError::SizeMismatch { declared, received: written });
                }
                f.write_all(&frame.payload).map_err(|e| TransferError::CannotWrite(e.to_string()))?;
                progress(written);
            }
            FrameType::Control => {
                let msg = WireMessage::decoded(&frame.payload)
                    .map_err(|_| ConnectionError::ProtocolViolation("Unreadable control message.".into()))?;
                match msg {
                    WireMessage::FileEnd(e) if e.track_id == track_id => {
                        f.flush().map_err(|e| TransferError::CannotWrite(e.to_string()))?;
                        return Ok(written);
                    }
                    WireMessage::FileEnd(_) => {
                        return Err(ConnectionError::ProtocolViolation("fileEnd for the wrong file.".into()).into())
                    }
                    WireMessage::Cancel(c) => return Err(TransferError::RejectedByPeer(c.reason)),
                    WireMessage::ProtocolError(p) => return Err(TransferError::RejectedByPeer(p.message)),
                    _ => return Err(ConnectionError::ProtocolViolation("Unexpected message mid-transfer.".into()).into()),
                }
            }
        }
    }
}

/// Moves a verified `.part` into the library; replacing an existing track
/// is a single rename, so readers see the old file or the new one.
fn install(part: &Path, destination: &Path) -> std::io::Result<()> {
    if let Some(dir) = destination.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::rename(part, destination) {
        Ok(()) => Ok(()),
        // Different volume (the library root spans a mount point).
        Err(_) => {
            std::fs::copy(part, destination)?;
            std::fs::remove_file(part)
        }
    }
}

/// Deletes `.part` files older than `age` (default a week) at the end of a run.
pub fn sweep_abandoned_part_files(root: &Path, age: Duration) {
    let Ok(rd) = std::fs::read_dir(incoming_dir(root)) else { return };
    let cutoff = SystemTime::now().checked_sub(age).unwrap_or(SystemTime::UNIX_EPOCH);
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "part")
            && e.metadata().and_then(|m| m.modified()).is_ok_and(|m| m < cutoff)
        {
            let _ = std::fs::remove_file(p);
        }
    }
}

pub const PART_FILE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
