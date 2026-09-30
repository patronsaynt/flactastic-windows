//! TLS-PSK contexts (`SyncTLS.swift`), on OpenSSL 3.
//!
//! The Mac appends only `TLS_AES_128_GCM_SHA256` with a TLS 1.2 minimum, so
//! sessions are TLS 1.3 with an external PSK. OpenSSL's PSK client/server
//! callbacks drive TLS 1.3 external PSKs for SHA-256 suites; the same
//! callbacks also serve a TLS 1.2 `PSK-AES128-GCM-SHA256` fallback.
//! `ALLOW_NO_DHE_KEX` accepts either `psk_ke` or `psk_dhe_ke`, whichever the
//! peer offers.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use fl_core::Uid;
use foreign_types::ForeignTypeRef;
use openssl::ssl::{SslContext, SslContextBuilder, SslMethod, SslOptions, SslRef, SslVersion};
use sha2::{Digest, Sha256};

use crate::crypto::Key;

pub const PAIRING_IDENTITY: &str = "flactastic-pairing-v1";

pub fn paired_identity(device_id: Uid) -> String {
    format!("flactastic-peer-v1:{}", device_id.uuid_string())
}

/// `SHA256("flactastic-public-pairing-key-v1")` — public by design; it only
/// buys encryption for the pairing handshake, not authentication.
pub fn pairing_key() -> Key {
    Key(Sha256::digest(b"flactastic-public-pairing-key-v1").into())
}

fn base(method: SslMethod) -> Result<SslContextBuilder, openssl::error::ErrorStack> {
    let mut b = SslContextBuilder::new(method)?;
    b.set_min_proto_version(Some(SslVersion::TLS1_2))?;
    b.set_ciphersuites("TLS_AES_128_GCM_SHA256")?;
    b.set_cipher_list("PSK-AES128-GCM-SHA256")?;
    // SSL_OP_ALLOW_NO_DHE_KEX (not named by rust-openssl).
    b.set_options(SslOptions::from_bits_retain(0x0000_0400) | SslOptions::NO_TICKET);
    Ok(b)
}

fn copy_psk(out: &mut [u8], key: &Key) -> Result<usize, openssl::error::ErrorStack> {
    if out.len() < 32 {
        return Ok(0);
    }
    out[..32].copy_from_slice(key.as_bytes());
    Ok(32)
}

/// Client context presenting one identity/key.
pub fn client_context(identity: String, key: Key) -> Result<SslContext, openssl::error::ErrorStack> {
    let mut b = base(SslMethod::tls_client())?;
    b.set_psk_client_callback(move |_ssl, _hint, id_out, psk_out| {
        let id = identity.as_bytes();
        if id.len() + 1 > id_out.len() {
            return Ok(0);
        }
        id_out[..id.len()].copy_from_slice(id);
        id_out[id.len()] = 0;
        copy_psk(psk_out, &key)
    });
    Ok(b.build())
}

/// Dialling an already-paired peer: identity is *our* device ID.
pub fn paired_client(key: Key, local_device_id: Uid) -> Result<SslContext, openssl::error::ErrorStack> {
    client_context(paired_identity(local_device_id), key)
}

/// First contact.
pub fn pairing_client() -> Result<SslContext, openssl::error::ErrorStack> {
    client_context(PAIRING_IDENTITY.to_owned(), pairing_key())
}

/// The listener's live PSK table: one key per paired peer, plus the public
/// pairing key while a code is on screen. Removing a peer's key makes its
/// handshake fail outright.
#[derive(Default)]
pub struct ListenerKeys {
    pub paired: HashMap<Uid, Key>,
    pub allow_pairing: bool,
}

impl ListenerKeys {
    fn lookup(&self, identity: &[u8]) -> Option<Key> {
        let id = std::str::from_utf8(identity).ok()?;
        if id == PAIRING_IDENTITY {
            return self.allow_pairing.then(pairing_key);
        }
        let uuid = id.strip_prefix("flactastic-peer-v1:")?;
        self.paired.get(&Uid::parse(uuid)?).cloned()
    }
}

pub type SharedListenerKeys = Arc<RwLock<ListenerKeys>>;

pub fn server_context(keys: SharedListenerKeys) -> Result<SslContext, openssl::error::ErrorStack> {
    let mut b = base(SslMethod::tls_server())?;
    b.set_psk_server_callback(move |_ssl, identity, psk_out| {
        let key = identity.and_then(|id| keys.read().ok()?.lookup(id));
        match key {
            Some(k) => copy_psk(psk_out, &k),
            None => Ok(0),
        }
    });
    Ok(b.build())
}

/// What a finished handshake negotiated — recorded for the interop spike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Negotiated {
    pub version: String,
    pub cipher: String,
    /// `true` when an (EC)DHE group was used (`psk_dhe_ke`), `false` for `psk_ke`.
    pub dhe: bool,
    pub group: Option<String>,
}

pub fn negotiated(ssl: &SslRef) -> Negotiated {
    const SSL_CTRL_GET_NEGOTIATED_GROUP: std::ffi::c_int = 134;
    let nid = unsafe { openssl_sys::SSL_ctrl(ssl.as_ptr(), SSL_CTRL_GET_NEGOTIATED_GROUP, 0, std::ptr::null_mut()) } as i64;
    // Groups OpenSSL has no NID for come back as TLSEXT_nid_unknown | group id.
    const NID_UNKNOWN: i64 = 0x0100_0000;
    let group = (nid > 0).then(|| match nid {
        n if n & NID_UNKNOWN != 0 => match n & 0xFFFF {
            0x11EC => "X25519MLKEM768".to_owned(),
            0x11EB => "SecP256r1MLKEM768".to_owned(),
            id => format!("group 0x{id:04X}"),
        },
        n => openssl::nid::Nid::from_raw(n as i32).short_name().map(str::to_owned).unwrap_or_else(|_| n.to_string()),
    });
    Negotiated {
        version: ssl.version_str().to_owned(),
        cipher: ssl.current_cipher().map(|c| c.standard_name().unwrap_or(c.name()).to_owned()).unwrap_or_default(),
        dhe: group.is_some(),
        group,
    }
}

/// RFC 5705 / 8446 exporter with no context (`use_context = 0`), as
/// `sec_protocol_metadata_create_secret` computes it.
pub fn exporter(ssl: &SslRef) -> Result<[u8; 32], openssl::error::ErrorStack> {
    let mut out = [0u8; 32];
    ssl.export_keying_material(&mut out, crate::crypto::EXPORTER_LABEL, None)?;
    Ok(out)
}
