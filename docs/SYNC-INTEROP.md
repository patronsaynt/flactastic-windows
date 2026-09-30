# Sync interop: code-vs-spec deltas and conformance

The Mac **code** is authoritative for anything on the wire. Each rule below
has a test in `crates/fl-sync` or `crates/fl-core`.

## Byte-exact rules

| Rule | Source (Mac) | Desktop | Test |
|---|---|---|---|
| Frame: `[type u8][len u32 BE][payload]`, control ≤ 8 MiB, chunk ≤ 1 MiB, reject on header alone | `FrameCodec.swift` | `fl_sync::frame` | frame tests (Phase 5 port of `SyncFramingTests`) |
| Wire JSON `{"t","d"}`, Foundation `.sortedKeys` order (`[.numeric, .caseInsensitive, .forcedOrdering]` — **not** bytewise), `/` → `\/`, nil optionals omitted | `WireMessage.swift:271-313` | `fl_core::apple_json::to_vec_sorted` | `hello_bytes_match_foundation`, `sorted_keys_follow_foundation` |
| Dates on the wire: ISO-8601 UTC, exactly 3 fraction digits; a date without a fractional part is rejected | `WireMessage.makeDecoder` | `fl_sync::wire::iso_date` | `dates_need_fractional_seconds` |
| Dates in sidecars: Double seconds since 2001-01-01 | Foundation default | `fl_core::apple_json::AppleDate` | `apple_date_epoch` |
| UUIDs uppercase; decode accepts any case | Foundation | `fl_core::Uid` | `uid_round_trips_uppercase` |
| `Hello.version` = `{"major":1,"minor":0}` | `SyncProtocol.version` | `fl_sync::protocol::VERSION` | `hello_bytes_match_foundation` |
| PSK identity `flactastic-peer-v1:<UPPERCASE UUID>` / `flactastic-pairing-v1` | `SyncTLS.swift` | `fl_sync::tls` | loopback |
| Pairing PSK = `SHA256("flactastic-public-pairing-key-v1")` | `SyncTLS.pairingKey` | `tls::pairing_key` | loopback |
| Exporter: label `EXPORTER-flactastic-channel-v1`, **no context**, 32 bytes | `SyncConnection.exporterSecret` | `tls::exporter` (`use_context = 0`) | loopback |
| Proof = HMAC-SHA256(key, `"flactastic-hello-v1" ‖ 0x00 ‖ 16 raw UUID bytes ‖ exporter`) | `ChannelBinding.swift` | `crypto::hello_proof` | loopback |
| Pairing: commitment, length-prefixed transcript, HKDF infos, role MACs | `PairingCrypto.swift` | `fl_sync::crypto` | `crypto::tests`, `pairing::tests` |
| `deviceKind` in `Hello`/`PairConfirm` is a closed enum on the Mac: sending anything but `mac/iPhone/iPad/other` makes the Mac reject the message | `SyncDeviceKind` | Desktop sends `other` everywhere | — |
| TXT `k` unknown → `other` on decode (safe), `v` must be `major.minor` digits | `TXTRecordCodec.swift` | `fl_sync::txt` | `round_trip_and_unknown_kind` |

`planHash` group ordering, playlist hash, `tagFingerprint` and the rest of
the manifest rules land with Phase 5.

## TLS (Spike A)

The Mac appends only `TLS_AES_128_GCM_SHA256` (0x1301, a TLS 1.3 suite) with a
TLS 1.2 minimum. The desktop:

- offers TLS 1.3 with `TLS_AES_128_GCM_SHA256`, plus `PSK-AES128-GCM-SHA256`
  as a TLS 1.2 fallback in case Network.framework negotiates 1.2;
- uses OpenSSL's PSK client/server callbacks, which drive TLS 1.3 external PSKs
  for SHA-256 suites;
- sets `SSL_OP_ALLOW_NO_DHE_KEX`, so either `psk_ke` or `psk_dhe_ke` from
  Apple is accepted;
- disables session tickets (`SSL_OP_NO_TICKET`).

Desktop↔desktop (loopback tests) negotiates TLS 1.3, `TLS_AES_128_GCM_SHA256`,
`psk_dhe_ke` over `X25519MLKEM768` (OpenSSL 3.5 default groups).

### Still to confirm against a real Mac

- [ ] Mac as listener, Windows dials: pairing (`pair`) and `hello`.
- [ ] Windows as listener, Mac dials: pairing (`host-pair`) and `listen`.
- [ ] Record the negotiated version, cipher and group/`psk_ke` from the output
      and fill in the table below.
- [ ] Repeat with an iPhone.

| Pair | Direction | Version | Cipher | Key exchange | Result |
|---|---|---|---|---|---|
| Windows ↔ Mac | Win dials | | | | |
| Windows ↔ Mac | Mac dials | | | | |

### Running the conformance harness

```
cargo build -p fl-conformance
target\debug\fl-conformance browse
```

1. On the Mac, open **Sync** and choose to show a pairing code. On Windows:
   `fl-conformance pair <mac-device-id> <code>`.
2. `fl-conformance hello <mac-device-id>` — expects `helloAck`.
3. `fl-conformance host-pair` prints a code; on the Mac, pair with
   "<PC> (conformance)" and type it.
4. `fl-conformance listen`, then start a sync from the Mac. The harness answers
   `helloAck`, saves the Mac's `syncRequest` (its manifest — useful as golden
   data) to `%APPDATA%\FLACtastic\captured-syncRequest.json`, then cancels.

Windows Firewall will ask whether to allow `fl-conformance.exe` on private
networks the first time it listens; that's needed for steps 3–4.

For a packet capture, Wireshark on port 5353 (mDNS) and the advertised TCP
port shows the ClientHello (`supported_versions`, `psk_key_exchange_modes`,
`key_share`) in clear.

## Desktop fixes to Mac behaviour (local-only, no wire change)

See `docs/MAC-ISSUES.md` rows 5–7. Where a fix would change what goes on the
wire (e.g. the order of `hashMismatch` and `fileEnd`) the desktop keeps the
Mac's wire behaviour and only fixes its own state handling; the details are
recorded here when Phase 5 lands.
