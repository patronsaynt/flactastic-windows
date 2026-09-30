//! An authenticated, framed channel to one peer (`SyncConnection.swift`).

use std::pin::Pin;
use std::time::Duration;

use openssl::ssl::{Ssl, SslContext};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_openssl::SslStream;

use crate::frame::{self, Decoder, Frame, FrameType};
use crate::tls::{self, Negotiated};
use crate::wire::WireMessage;

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
pub const DEFAULT_RECEIVE_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, thiserror::Error)]
pub enum ConnectionError {
    #[error("The connection was cancelled.")]
    Cancelled,
    /// With PSK this means the peer doesn't hold the key — revocation working.
    #[error("Couldn't establish a secure connection: {0}")]
    HandshakeFailed(String),
    #[error("{0}")]
    Network(#[from] std::io::Error),
    #[error("The other device closed the connection.")]
    ClosedByPeer,
    #[error("The other device stopped responding.")]
    TimedOut,
    #[error("The other device misbehaved: {0}")]
    ProtocolViolation(String),
}

impl ConnectionError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Network(_) | Self::TimedOut | Self::ClosedByPeer)
    }
}

struct Reader {
    half: ReadHalf<SslStream<TcpStream>>,
    decoder: Decoder,
    buf: Box<[u8]>,
}

pub struct SyncConnection {
    reader: Mutex<Reader>,
    writer: Mutex<WriteHalf<SslStream<TcpStream>>>,
    exporter: [u8; 32],
    pub negotiated: Negotiated,
    pub peer_addr: std::net::SocketAddr,
}

fn handshake_error(e: impl std::fmt::Display) -> ConnectionError {
    ConnectionError::HandshakeFailed(e.to_string())
}

impl SyncConnection {
    /// Dials `addr` and completes the TLS handshake (bounded by a timeout).
    pub async fn connect(addr: std::net::SocketAddr, ctx: &SslContext) -> Result<SyncConnection, ConnectionError> {
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| ConnectionError::TimedOut)??;
        Self::handshake(tcp, ctx, true).await
    }

    /// Completes the server side of an accepted TCP connection.
    pub async fn accept(tcp: TcpStream, ctx: &SslContext) -> Result<SyncConnection, ConnectionError> {
        Self::handshake(tcp, ctx, false).await
    }

    async fn handshake(tcp: TcpStream, ctx: &SslContext, client: bool) -> Result<SyncConnection, ConnectionError> {
        tcp.set_nodelay(true)?;
        let peer_addr = tcp.peer_addr()?;
        let ssl = Ssl::new(ctx).map_err(handshake_error)?;
        let mut stream = SslStream::new(ssl, tcp).map_err(handshake_error)?;
        let hs = async {
            if client {
                Pin::new(&mut stream).connect().await
            } else {
                Pin::new(&mut stream).accept().await
            }
        };
        tokio::time::timeout(HANDSHAKE_TIMEOUT, hs).await.map_err(|_| ConnectionError::TimedOut)?.map_err(handshake_error)?;
        let exporter = tls::exporter(stream.ssl()).map_err(handshake_error)?;
        let negotiated = tls::negotiated(stream.ssl());
        let (r, w) = tokio::io::split(stream);
        Ok(SyncConnection {
            reader: Mutex::new(Reader { half: r, decoder: Decoder::new(), buf: vec![0u8; 64 * 1024].into_boxed_slice() }),
            writer: Mutex::new(w),
            exporter,
            negotiated,
            peer_addr,
        })
    }

    /// TLS exporter secret for channel binding.
    pub fn exporter_secret(&self) -> &[u8; 32] {
        &self.exporter
    }

    pub async fn send(&self, msg: &WireMessage) -> Result<(), ConnectionError> {
        self.send_frame(FrameType::Control, &msg.encoded()).await
    }

    pub async fn send_chunk(&self, chunk: &[u8]) -> Result<(), ConnectionError> {
        self.send_frame(FrameType::FileChunk, chunk).await
    }

    async fn send_frame(&self, ty: FrameType, payload: &[u8]) -> Result<(), ConnectionError> {
        let bytes = frame::encode(ty, payload).map_err(|e| ConnectionError::ProtocolViolation(e.to_string()))?;
        let mut w = self.writer.lock().await;
        w.write_all(&bytes).await?;
        w.flush().await?;
        Ok(())
    }

    pub async fn receive_frame(&self, timeout: Duration) -> Result<Frame, ConnectionError> {
        let mut r = self.reader.lock().await;
        let work = async {
            loop {
                match r.decoder.next() {
                    Ok(Some(f)) => return Ok(f),
                    Ok(None) => {}
                    // The stream position is unknown from here on.
                    Err(e) => return Err(ConnectionError::ProtocolViolation(e.to_string())),
                }
                let Reader { half, buf, decoder } = &mut *r;
                let n = half.read(buf).await?;
                if n == 0 {
                    return Err(ConnectionError::ClosedByPeer);
                }
                decoder.append(&buf[..n]);
            }
        };
        tokio::time::timeout(timeout, work).await.map_err(|_| ConnectionError::TimedOut)?
    }

    /// A control message; a raw chunk here is a protocol violation.
    pub async fn receive_message(&self, timeout: Duration) -> Result<WireMessage, ConnectionError> {
        let f = self.receive_frame(timeout).await?;
        if f.ty != FrameType::Control {
            return Err(ConnectionError::ProtocolViolation("Expected a control message, got a file chunk.".into()));
        }
        WireMessage::decoded(&f.payload).map_err(|_| ConnectionError::ProtocolViolation("Unreadable control message.".into()))
    }

    /// Sends TLS close_notify and shuts the socket.
    pub async fn close(&self) {
        let mut w = self.writer.lock().await;
        let _ = w.shutdown().await;
    }
}
