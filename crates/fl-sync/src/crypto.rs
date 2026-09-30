//! `PairingCrypto` and `ChannelBinding`.

use fl_core::Uid;
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::protocol::{PAIRING_CODE_DIGITS, PAIRING_KDF_INFO, SESSION_KDF_INFO};

type HmacSha256 = Hmac<Sha256>;

/// 32-byte symmetric key (`SymmetricKey`). Zeroed on drop.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(pub [u8; 32]);

impl Key {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn from_slice(b: &[u8]) -> Option<Key> {
        Some(Key(b.try_into().ok()?))
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // Volatile writes so the compiler can't elide the wipe.
        for b in self.0.iter_mut() {
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(…)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Host,
    Guest,
}

impl Role {
    pub fn raw_value(self) -> &'static str {
        match self {
            Role::Host => "host",
            Role::Guest => "guest",
        }
    }
}

pub const NONCE_BYTES: usize = 32;

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("OS RNG available");
    b
}

/// Uniform digits by rejection sampling (bytes ≥ 250 are discarded).
pub fn generate_code(digits: usize) -> String {
    let mut code = String::with_capacity(digits);
    while code.len() < digits {
        let [b] = random_bytes::<1>();
        if b < 250 {
            code.push(char::from(b'0' + b % 10));
        }
    }
    code
}

pub fn random_nonce() -> Vec<u8> {
    random_bytes::<NONCE_BYTES>().to_vec()
}

/// `SHA256(publicKey ‖ nonce)`.
pub fn commitment(public_key: &[u8], nonce: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(public_key);
    h.update(nonce);
    h.finalize().to_vec()
}

pub fn verify_commitment(c: &[u8], public_key: &[u8], nonce: &[u8]) -> bool {
    constant_time_equals(c, &commitment(public_key, nonce))
}

/// Each field length-prefixed (u32 BE), in a fixed order.
pub fn transcript(commitment: &[u8], host_pub: &[u8], host_nonce: &[u8], guest_pub: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for f in [commitment, host_pub, host_nonce, guest_pub] {
        out.extend_from_slice(&(f.len() as u32).to_be_bytes());
        out.extend_from_slice(f);
    }
    out
}

/// An X25519 key pair for one pairing attempt.
pub struct KeyPair {
    secret: StaticSecret,
    public: PublicKey,
}

impl KeyPair {
    pub fn generate() -> Self {
        let secret = StaticSecret::from(random_bytes::<32>());
        let public = PublicKey::from(&secret);
        KeyPair { secret, public }
    }

    /// Raw 32-byte public key (CryptoKit `rawRepresentation`).
    pub fn public_bytes(&self) -> Vec<u8> {
        self.public.as_bytes().to_vec()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid public key")]
pub struct InvalidPublicKey;

/// Parses a raw X25519 public key (CryptoKit accepts any 32 bytes).
pub fn parse_public_key(raw: &[u8]) -> Result<PublicKey, InvalidPublicKey> {
    let b: [u8; 32] = raw.try_into().map_err(|_| InvalidPublicKey)?;
    Ok(PublicKey::from(b))
}

/// X25519 → HKDF-SHA256(salt = transcript, info = `flactastic-pair-v1`), 32 bytes.
/// A non-contributory (low-order) result is rejected, as CryptoKit does.
pub fn derive_key(kp: &KeyPair, peer_public: &[u8], transcript: &[u8]) -> Result<Key, InvalidPublicKey> {
    let peer = parse_public_key(peer_public)?;
    let shared = kp.secret.diffie_hellman(&peer);
    if !shared.was_contributory() {
        return Err(InvalidPublicKey);
    }
    Ok(hkdf32(shared.as_bytes(), transcript, PAIRING_KDF_INFO))
}

fn hkdf32(ikm: &[u8], salt: &[u8], info: &str) -> Key {
    let hk = Hkdf::<Sha256>::new(Some(salt), ikm);
    let mut out = [0u8; 32];
    hk.expand(info.as_bytes(), &mut out).expect("32 bytes is a valid HKDF length");
    Key(out)
}

/// `HMAC(key, role ‖ 0x00 ‖ code)`.
pub fn confirmation_mac(key: &Key, code: &str, role: Role) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    m.update(role.raw_value().as_bytes());
    m.update(&[0]);
    m.update(code.as_bytes());
    m.finalize().into_bytes().to_vec()
}

pub fn verify_confirmation(mac: &[u8], key: &Key, code: &str, role: Role) -> bool {
    constant_time_equals(mac, &confirmation_mac(key, code, role))
}

/// Long-term TLS PSK: HKDF(ikm = pairing key, salt = transcript, info = `flactastic-session-v1`).
pub fn derive_long_term_key(pairing_key: &Key, transcript: &[u8]) -> Key {
    hkdf32(pairing_key.as_bytes(), transcript, SESSION_KDF_INFO)
}

pub fn constant_time_equals(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Keeps only digits (drops spaces and dashes people type).
pub fn normalize_typed_code(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub fn is_well_formed_code(code: &str) -> bool {
    code.len() == PAIRING_CODE_DIGITS && code.bytes().all(|b| b.is_ascii_digit())
}

// MARK: - Channel binding

pub const EXPORTER_LABEL: &str = "EXPORTER-flactastic-channel-v1";
pub const EXPORTER_LENGTH: usize = 32;

/// `HMAC(longTermKey, "flactastic-hello-v1" ‖ 0x00 ‖ deviceID (16 raw bytes) ‖ exporter)`.
pub fn hello_proof(key: &Key, exporter: &[u8], device_id: Uid) -> Vec<u8> {
    let mut m = HmacSha256::new_from_slice(key.as_bytes()).unwrap();
    m.update(b"flactastic-hello-v1");
    m.update(&[0]);
    m.update(device_id.as_bytes());
    m.update(exporter);
    m.finalize().into_bytes().to_vec()
}

pub fn verify_hello_proof(proof: Option<&[u8]>, key: &Key, exporter: &[u8], device_id: Uid) -> bool {
    proof.is_some_and(|p| constant_time_equals(p, &hello_proof(key, exporter, device_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_digits() {
        let c = generate_code(8);
        assert!(is_well_formed_code(&c));
        assert_eq!(normalize_typed_code("1234-5678 "), "12345678");
        assert!(!is_well_formed_code("1234567"));
    }

    #[test]
    fn both_sides_derive_the_same_keys() {
        let host = KeyPair::generate();
        let guest = KeyPair::generate();
        let nonce = random_nonce();
        let c = commitment(&host.public_bytes(), &nonce);
        assert!(verify_commitment(&c, &host.public_bytes(), &nonce));
        let t = transcript(&c, &host.public_bytes(), &nonce, &guest.public_bytes());
        let kh = derive_key(&host, &guest.public_bytes(), &t).unwrap();
        let kg = derive_key(&guest, &host.public_bytes(), &t).unwrap();
        assert_eq!(kh, kg);
        let mac = confirmation_mac(&kg, "12345678", Role::Guest);
        assert!(verify_confirmation(&mac, &kh, "12345678", Role::Guest));
        assert!(!verify_confirmation(&mac, &kh, "12345678", Role::Host), "role separates MACs");
        assert_eq!(derive_long_term_key(&kh, &t), derive_long_term_key(&kg, &t));
        assert_ne!(derive_long_term_key(&kh, &t), kh);
    }

    #[test]
    fn low_order_point_rejected() {
        let kp = KeyPair::generate();
        assert_eq!(derive_key(&kp, &[0u8; 32], b"t"), Err(InvalidPublicKey));
        assert_eq!(derive_key(&kp, &[1u8; 31], b"t"), Err(InvalidPublicKey));
    }

    #[test]
    fn transcript_is_length_prefixed() {
        assert_eq!(transcript(b"a", b"", b"bc", b"d"), [0, 0, 0, 1, b'a', 0, 0, 0, 0, 0, 0, 0, 2, b'b', b'c', 0, 0, 0, 1, b'd']);
    }
}
