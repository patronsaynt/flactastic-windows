//! Message plumbing for pairing and the paired handshake
//! (`PairingExchange.swift`, the hello half of `SyncSession.swift`).

use std::future::Future;
use std::time::Duration;

use fl_core::Uid;

use crate::connection::{ConnectionError, SyncConnection, DEFAULT_RECEIVE_TIMEOUT};
use crate::crypto::{self, Key};
use crate::pairing::*;
use crate::protocol::VERSION;
use crate::wire::*;

pub const STEP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    #[error(transparent)]
    Pairing(#[from] PairingError),
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

/// What a dialler wants, read from its opening `hello`.
#[derive(Debug)]
pub enum IncomingRequest {
    Pairing(Hello),
    Sync(Hello),
}

impl IncomingRequest {
    pub async fn read(conn: &SyncConnection) -> Result<IncomingRequest, ConnectionError> {
        match conn.receive_message(STEP_TIMEOUT).await? {
            WireMessage::Hello(h) if h.is_paired => Ok(IncomingRequest::Sync(h)),
            WireMessage::Hello(h) => Ok(IncomingRequest::Pairing(h)),
            _ => {
                let _ = conn
                    .send(&WireMessage::ProtocolError(ProtocolFailure::new(FailureCode::UnexpectedMessage, "Expected hello.")))
                    .await;
                Err(ConnectionError::ProtocolViolation("The first message was not hello.".into()))
            }
        }
    }
}

fn hello(identity: &PairingIdentity, is_paired: bool, proof: Option<Vec<u8>>) -> Hello {
    Hello {
        version: VERSION,
        device_id: identity.device_id,
        display_name: identity.display_name.clone(),
        device_kind: identity.kind,
        is_paired,
        proof,
    }
}

// MARK: - Guest

/// Pairs with a device showing a code, over an already-handshaken
/// pairing-PSK connection. Saves only after the host confirms it saved too.
pub async fn run_guest<P, PF, R, RF>(
    conn: &SyncConnection,
    mut session: GuestPairingSession,
    identity: &PairingIdentity,
    persist: P,
    rollback: R,
) -> Result<PairedPeer, ExchangeError>
where
    P: FnOnce(PairedPeer, Key) -> PF,
    PF: Future<Output = Result<(), String>>,
    R: FnOnce(Uid) -> RF,
    RF: Future<Output = ()>,
{
    conn.send(&WireMessage::Hello(hello(identity, false, None))).await?;
    loop {
        let msg = conn.receive_message(STEP_TIMEOUT).await?;
        match &msg {
            WireMessage::ProtocolError(f) => {
                return Err(if f.code == FailureCode::PairingClosed {
                    PairingError::NotAcceptingPairing
                } else {
                    PairingError::PeerReportedFailure(Some(f.message.clone()))
                }
                .into())
            }
            WireMessage::PairResult(r) if !r.success => {
                return Err(PairingError::PeerReportedFailure(r.failure_reason.clone()).into())
            }
            _ => {}
        }
        // On failure the caller drops the connection; the host sees it close
        // and burns the code, as with the Mac guest.
        match session.receive(msg)? {
            PairingStep::Send(m) => conn.send(&m).await?,
            PairingStep::SendAndFinish(m, peer, key) => {
                commit_as_guest(conn, peer.clone(), key, Some(m), persist, rollback).await?;
                return Ok(peer);
            }
            PairingStep::Finish(peer, key) => {
                commit_as_guest(conn, peer.clone(), key, None, persist, rollback).await?;
                return Ok(peer);
            }
        }
    }
}

async fn commit_as_guest<P, PF, R, RF>(
    conn: &SyncConnection,
    peer: PairedPeer,
    key: Key,
    success: Option<WireMessage>,
    persist: P,
    rollback: R,
) -> Result<(), ExchangeError>
where
    P: FnOnce(PairedPeer, Key) -> PF,
    PF: Future<Output = Result<(), String>>,
    R: FnOnce(Uid) -> RF,
    RF: Future<Output = ()>,
{
    let id = peer.device_id;
    if persist(peer, key).await.is_err() {
        let _ = conn
            .send(&WireMessage::PairResult(PairResult {
                success: false,
                failure_reason: Some(PairingError::STORAGE_FAILED_REASON.into()),
            }))
            .await;
        return Err(PairingError::StorageFailed.into());
    }
    let ack = async {
        conn.send(&success.unwrap_or(WireMessage::PairResult(PairResult { success: true, failure_reason: None })))
            .await?;
        match conn.receive_message(STEP_TIMEOUT).await? {
            WireMessage::PairResult(r) if r.success => Ok(()),
            WireMessage::PairResult(r) if r.failure_reason.as_deref() == Some(PairingError::STORAGE_FAILED_REASON) => {
                Err(PairingError::StorageFailed.into())
            }
            WireMessage::PairResult(r) => Err(PairingError::PeerReportedFailure(r.failure_reason).into()),
            _ => Err(ExchangeError::from(PairingError::UnexpectedMessage)),
        }
    }
    .await;
    if ack.is_err() {
        // The host never confirmed it saved; don't keep a one-sided pairing.
        rollback(id).await;
    }
    ack
}

// MARK: - Host

/// Answers a guest's `hello(isPaired: false)`. `code` is the code on screen
/// when the connection arrived. Every failure except `NotAcceptingPairing`
/// must burn the code (`PairingGatekeeper::record_failure`).
pub async fn run_host<P, PF>(
    conn: &SyncConnection,
    opening: &Hello,
    code: Option<String>,
    identity: &PairingIdentity,
    persist: P,
) -> Result<PairedPeer, ExchangeError>
where
    P: FnOnce(PairedPeer, Key) -> PF,
    PF: Future<Output = Result<(), String>>,
{
    let Some(code) = code else {
        let _ = conn.send(&WireMessage::ProtocolError(ProtocolFailure::new(FailureCode::PairingClosed, "Not pairing."))).await;
        return Err(PairingError::NotAcceptingPairing.into());
    };
    let mut session = HostPairingSession::new(code, identity.clone());
    let result: Result<PairedPeer, ExchangeError> = async {
        if !VERSION.is_compatible(&opening.version) {
            let _ = conn
                .send(&WireMessage::ProtocolError(ProtocolFailure::new(
                    FailureCode::IncompatibleVersion,
                    format!("Version {VERSION} required."),
                )))
                .await;
            return Err(PairingError::UnexpectedMessage.into());
        }
        conn.send(&session.begin()).await?;
        loop {
            let msg = conn.receive_message(STEP_TIMEOUT).await?;
            let (peer, key) = match session.receive(msg)? {
                PairingStep::Send(m) => {
                    conn.send(&m).await?;
                    continue;
                }
                PairingStep::SendAndFinish(m, peer, key) => {
                    conn.send(&m).await?;
                    (peer, key)
                }
                PairingStep::Finish(peer, key) => (peer, key),
            };
            persist(peer.clone(), key).await.map_err(|_| PairingError::StorageFailed)?;
            conn.send(&WireMessage::PairResult(PairResult { success: true, failure_reason: None })).await?;
            return Ok(peer);
        }
    }
    .await;
    if let Err(e) = &result {
        let reason = if matches!(e, ExchangeError::Pairing(PairingError::StorageFailed)) {
            PairingError::STORAGE_FAILED_REASON
        } else {
            "Pairing failed."
        };
        let _ = conn.send(&WireMessage::PairResult(PairResult { success: false, failure_reason: Some(reason.into()) })).await;
    }
    result
}

// MARK: - Paired handshake

#[derive(Debug, thiserror::Error)]
pub enum HelloError {
    #[error("The other device couldn't confirm it's paired with this one. Forget it and pair again.")]
    NotAuthenticated,
    #[error("The other device refused: {0}")]
    RefusedByPeer(String),
    #[error("The other device speaks sync version {0}; this one speaks {VERSION}. Update both to the same release.")]
    IncompatibleVersion(crate::protocol::Version),
    #[error("The other device sent something unexpected ({0}).")]
    UnexpectedMessage(String),
    #[error("The other device connected but didn't answer.")]
    PeerSilent,
    #[error(transparent)]
    Connection(#[from] ConnectionError),
}

/// Initiator: `hello(proof)` → `helloAck`. Returns the peer's ack.
pub async fn initiator_hello(conn: &SyncConnection, identity: &PairingIdentity, paired_key: &Key) -> Result<Hello, HelloError> {
    let proof = crypto::hello_proof(paired_key, conn.exporter_secret(), identity.device_id);
    conn.send(&WireMessage::Hello(hello(identity, true, Some(proof)))).await?;
    let ack = match conn.receive_message(DEFAULT_RECEIVE_TIMEOUT).await {
        Err(ConnectionError::TimedOut) => return Err(HelloError::PeerSilent),
        r => r?,
    };
    let ack = match ack {
        WireMessage::ProtocolError(f) => return Err(HelloError::RefusedByPeer(f.message)),
        WireMessage::HelloAck(a) => a,
        other => return Err(HelloError::UnexpectedMessage(format!("expected helloAck, got {}", other.tag()))),
    };
    if !VERSION.is_compatible(&ack.version) {
        let _ = conn
            .send(&WireMessage::ProtocolError(ProtocolFailure::new(
                FailureCode::IncompatibleVersion,
                format!("Version {VERSION} required."),
            )))
            .await;
        return Err(HelloError::IncompatibleVersion(ack.version));
    }
    Ok(ack)
}

/// Responder: authenticate before anything else (including the version
/// check), then `helloAck`. `authenticate` returns the key held for a peer.
pub async fn responder_hello(
    conn: &SyncConnection,
    hello_msg: &Hello,
    identity: &PairingIdentity,
    authenticate: impl FnOnce(Uid) -> Option<Key>,
) -> Result<(), HelloError> {
    let ok = authenticate(hello_msg.device_id).is_some_and(|key| {
        crypto::verify_hello_proof(hello_msg.proof.as_deref(), &key, conn.exporter_secret(), hello_msg.device_id)
    });
    if !ok {
        let _ = conn.send(&WireMessage::ProtocolError(ProtocolFailure::new(FailureCode::NotPaired, "Not paired with this device."))).await;
        return Err(HelloError::NotAuthenticated);
    }
    if !VERSION.is_compatible(&hello_msg.version) {
        let _ = conn
            .send(&WireMessage::ProtocolError(ProtocolFailure::new(
                FailureCode::IncompatibleVersion,
                format!("Version {VERSION} required."),
            )))
            .await;
        return Err(HelloError::IncompatibleVersion(hello_msg.version));
    }
    conn.send(&WireMessage::HelloAck(hello(identity, true, None))).await?;
    Ok(())
}
