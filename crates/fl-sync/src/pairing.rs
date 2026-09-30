//! Pairing sessions (`PairingSession.swift`).
//!
//! ```text
//! guest → hello(isPaired: false)
//! host  → pairCommit(SHA256(hostPub ‖ nonce))
//! guest → pairGuestKey(guestPub)
//! host  → pairReveal(hostPub, nonce)
//! guest → pairConfirm(HMAC(k, "guest" ‖ code))
//! host  → pairConfirm(HMAC(k, "host" ‖ code))
//! guest → pairResult(success)
//! host  → pairResult(success)        [ack; see exchange.rs]
//! ```

use fl_core::apple_json::AppleDate;
use fl_core::Uid;
use serde::{Deserialize, Serialize};

use crate::crypto::{self, Key, KeyPair, Role};
use crate::txt::sanitize_display_name;
use crate::protocol::DeviceKind;
use crate::wire::*;

/// A device this one has paired with. Never carries the key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedPeer {
    #[serde(rename = "deviceID")]
    pub device_id: Uid,
    pub display_name: String,
    pub kind: DeviceKind,
    pub paired_at: AppleDate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_at: Option<AppleDate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingIdentity {
    pub device_id: Uid,
    pub display_name: String,
    pub kind: DeviceKind,
}

#[derive(Debug)]
pub enum PairingStep {
    Send(WireMessage),
    SendAndFinish(WireMessage, PairedPeer, Key),
    Finish(PairedPeer, Key),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("That isn't a valid pairing code.")]
    MalformedCode,
    #[error("The other device sent something unexpected.")]
    UnexpectedMessage,
    #[error("This pairing attempt has already finished.")]
    SessionAlreadyFinished,
    #[error("The other device sent an invalid key.")]
    InvalidPublicKey,
    #[error("The other device changed its key mid-handshake.")]
    CommitmentMismatch,
    #[error("The codes didn't match.")]
    ConfirmationFailed,
    #[error("The other device rejected the pairing.")]
    PeerReportedFailure(Option<String>),
    #[error("The other device isn't showing a pairing code.")]
    NotAcceptingPairing,
    #[error("A device couldn't save the pairing key.")]
    StorageFailed,
}

impl PairingError {
    /// `pairResult.failureReason` marking a save failure on the wire.
    pub const STORAGE_FAILED_REASON: &'static str = "storage-failed";

    /// Every failure that could be an attacker probing reads the same.
    pub fn user_facing_message(&self) -> &'static str {
        match self {
            Self::MalformedCode => "That isn't a valid pairing code.",
            Self::UnexpectedMessage | Self::SessionAlreadyFinished => "Pairing didn't complete. Try again.",
            Self::InvalidPublicKey | Self::CommitmentMismatch | Self::ConfirmationFailed | Self::PeerReportedFailure(_) => {
                "Pairing failed. Check the code and try again."
            }
            Self::NotAcceptingPairing => {
                "That device isn't showing a pairing code. Show one on it, then try again."
            }
            Self::StorageFailed => {
                "One of the devices couldn't save the pairing securely, so neither kept it. Try again."
            }
        }
    }
}

fn peer_from(p: &PairConfirm) -> PairedPeer {
    PairedPeer {
        device_id: p.device_id,
        display_name: sanitize_display_name(Some(&p.display_name), p.device_kind.display_name()),
        kind: p.device_kind,
        paired_at: AppleDate::now(),
        last_synced_at: None,
    }
}

// MARK: - Host

enum HostState {
    Fresh,
    AwaitingGuestKey,
    AwaitingGuestConfirm { guest_pub: Vec<u8> },
    AwaitingResult { peer: PairedPeer, key: Key },
    Finished,
}

/// The side that displays the code.
pub struct HostPairingSession {
    pub code: String,
    identity: PairingIdentity,
    kp: KeyPair,
    nonce: Vec<u8>,
    commitment: Vec<u8>,
    state: HostState,
}

impl HostPairingSession {
    pub fn new(code: String, identity: PairingIdentity) -> Self {
        let kp = KeyPair::generate();
        let nonce = crypto::random_nonce();
        let commitment = crypto::commitment(&kp.public_bytes(), &nonce);
        HostPairingSession { code, identity, kp, nonce, commitment, state: HostState::Fresh }
    }

    /// Sent before the guest reveals anything — the whole security argument.
    pub fn begin(&mut self) -> WireMessage {
        self.state = HostState::AwaitingGuestKey;
        WireMessage::PairCommit(PairCommit { commitment: self.commitment.clone() })
    }

    fn transcript(&self, guest_pub: &[u8]) -> Vec<u8> {
        crypto::transcript(&self.commitment, &self.kp.public_bytes(), &self.nonce, guest_pub)
    }

    pub fn receive(&mut self, msg: WireMessage) -> Result<PairingStep, PairingError> {
        let state = std::mem::replace(&mut self.state, HostState::Finished);
        match (state, msg) {
            (HostState::AwaitingGuestKey, WireMessage::PairGuestKey(p)) => {
                crypto::parse_public_key(&p.public_key).map_err(|_| PairingError::InvalidPublicKey)?;
                self.state = HostState::AwaitingGuestConfirm { guest_pub: p.public_key };
                Ok(PairingStep::Send(WireMessage::PairReveal(PairReveal {
                    public_key: self.kp.public_bytes(),
                    nonce: self.nonce.clone(),
                })))
            }
            (HostState::AwaitingGuestConfirm { guest_pub }, WireMessage::PairConfirm(p)) => {
                let t = self.transcript(&guest_pub);
                let key = crypto::derive_key(&self.kp, &guest_pub, &t).map_err(|_| PairingError::InvalidPublicKey)?;
                if !crypto::verify_confirmation(&p.mac, &key, &self.code, Role::Guest) {
                    return Err(PairingError::ConfirmationFailed);
                }
                let peer = peer_from(&p);
                let long_term = crypto::derive_long_term_key(&key, &t);
                self.state = HostState::AwaitingResult { peer, key: long_term };
                Ok(PairingStep::Send(WireMessage::PairConfirm(PairConfirm {
                    mac: crypto::confirmation_mac(&key, &self.code, Role::Host),
                    device_id: self.identity.device_id,
                    display_name: self.identity.display_name.clone(),
                    device_kind: self.identity.kind,
                })))
            }
            (HostState::AwaitingResult { peer, key }, WireMessage::PairResult(r)) => {
                if !r.success {
                    return Err(PairingError::PeerReportedFailure(r.failure_reason));
                }
                Ok(PairingStep::Finish(peer, key))
            }
            (HostState::Finished, _) => Err(PairingError::SessionAlreadyFinished),
            _ => Err(PairingError::UnexpectedMessage),
        }
    }
}

// MARK: - Guest

enum GuestState {
    AwaitingCommit,
    AwaitingReveal { commitment: Vec<u8> },
    AwaitingHostConfirm { key: Key, long_term: Key },
    Finished,
}

/// The side that types the code.
pub struct GuestPairingSession {
    code: String,
    identity: PairingIdentity,
    kp: KeyPair,
    state: GuestState,
}

impl GuestPairingSession {
    /// Fails on a malformed code before touching the network.
    pub fn new(typed_code: &str, identity: PairingIdentity) -> Result<Self, PairingError> {
        let code = crypto::normalize_typed_code(typed_code);
        if !crypto::is_well_formed_code(&code) {
            return Err(PairingError::MalformedCode);
        }
        Ok(GuestPairingSession { code, identity, kp: KeyPair::generate(), state: GuestState::AwaitingCommit })
    }

    pub fn receive(&mut self, msg: WireMessage) -> Result<PairingStep, PairingError> {
        let state = std::mem::replace(&mut self.state, GuestState::Finished);
        match (state, msg) {
            (GuestState::AwaitingCommit, WireMessage::PairCommit(p)) => {
                self.state = GuestState::AwaitingReveal { commitment: p.commitment };
                Ok(PairingStep::Send(WireMessage::PairGuestKey(PairGuestKey { public_key: self.kp.public_bytes() })))
            }
            (GuestState::AwaitingReveal { commitment }, WireMessage::PairReveal(p)) => {
                if !crypto::verify_commitment(&commitment, &p.public_key, &p.nonce) {
                    return Err(PairingError::CommitmentMismatch);
                }
                let t = crypto::transcript(&commitment, &p.public_key, &p.nonce, &self.kp.public_bytes());
                let key = crypto::derive_key(&self.kp, &p.public_key, &t).map_err(|_| PairingError::InvalidPublicKey)?;
                let mac = crypto::confirmation_mac(&key, &self.code, Role::Guest);
                let long_term = crypto::derive_long_term_key(&key, &t);
                self.state = GuestState::AwaitingHostConfirm { key, long_term };
                Ok(PairingStep::Send(WireMessage::PairConfirm(PairConfirm {
                    mac,
                    device_id: self.identity.device_id,
                    display_name: self.identity.display_name.clone(),
                    device_kind: self.identity.kind,
                })))
            }
            (GuestState::AwaitingHostConfirm { key, long_term }, WireMessage::PairConfirm(p)) => {
                if !crypto::verify_confirmation(&p.mac, &key, &self.code, Role::Host) {
                    return Err(PairingError::ConfirmationFailed);
                }
                Ok(PairingStep::SendAndFinish(
                    WireMessage::PairResult(PairResult { success: true, failure_reason: None }),
                    peer_from(&p),
                    long_term,
                ))
            }
            (GuestState::Finished, _) => Err(PairingError::SessionAlreadyFinished),
            _ => Err(PairingError::UnexpectedMessage),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(name: &str) -> PairingIdentity {
        PairingIdentity { device_id: Uid::new_v4(), display_name: name.into(), kind: DeviceKind::Other }
    }

    fn run(host_code: &str, typed: &str) -> Result<(PairedPeer, Key, PairedPeer, Key), PairingError> {
        let (hi, gi) = (ident("Host"), ident("Guest"));
        let mut host = HostPairingSession::new(host_code.into(), hi.clone());
        let mut guest = GuestPairingSession::new(typed, gi.clone())?;
        let mut to_guest = host.begin();
        loop {
            match guest.receive(to_guest)? {
                PairingStep::Send(m) => match host.receive(m)? {
                    PairingStep::Send(m) => to_guest = m,
                    _ => unreachable!(),
                },
                PairingStep::SendAndFinish(m, host_peer, gk) => match host.receive(m)? {
                    PairingStep::Finish(guest_peer, hk) => return Ok((host_peer, gk, guest_peer, hk)),
                    _ => unreachable!(),
                },
                PairingStep::Finish(..) => unreachable!(),
            }
        }
    }

    #[test]
    fn happy_path_agrees_on_key_and_identities() {
        let (host_peer, gk, guest_peer, hk) = run("12345678", "1234 5678").unwrap();
        assert_eq!(gk, hk);
        assert_eq!(host_peer.display_name, "Host");
        assert_eq!(guest_peer.display_name, "Guest");
    }

    #[test]
    fn wrong_code_fails_on_host() {
        assert_eq!(run("12345678", "87654321").unwrap_err(), PairingError::ConfirmationFailed);
    }

    #[test]
    fn malformed_code_fails_locally() {
        assert_eq!(GuestPairingSession::new("12", ident("g")).err(), Some(PairingError::MalformedCode));
    }

    #[test]
    fn tampered_reveal_fails_commitment() {
        let mut host = HostPairingSession::new("12345678".into(), ident("h"));
        let mut guest = GuestPairingSession::new("12345678", ident("g")).unwrap();
        let PairingStep::Send(gk) = guest.receive(host.begin()).unwrap() else { panic!() };
        let PairingStep::Send(WireMessage::PairReveal(mut r)) = host.receive(gk).unwrap() else { panic!() };
        r.nonce[0] ^= 1;
        assert_eq!(guest.receive(WireMessage::PairReveal(r)).unwrap_err(), PairingError::CommitmentMismatch);
    }

    #[test]
    fn out_of_order_is_unexpected_then_finished() {
        let mut host = HostPairingSession::new("12345678".into(), ident("h"));
        host.begin();
        let err = host.receive(WireMessage::PairResult(PairResult { success: true, failure_reason: None }));
        assert_eq!(err.unwrap_err(), PairingError::UnexpectedMessage);
        let err = host.receive(WireMessage::PairResult(PairResult { success: true, failure_reason: None }));
        assert_eq!(err.unwrap_err(), PairingError::SessionAlreadyFinished);
    }
}
