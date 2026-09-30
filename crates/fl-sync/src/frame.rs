//! `[type: u8][length: u32 BE][payload]` framing (`FrameCodec.swift`).

use crate::protocol::{FILE_CHUNK_BYTES, MAX_CONTROL_FRAME_BYTES};

pub const HEADER_BYTES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameType {
    /// JSON-encoded `WireMessage`.
    Control = 1,
    /// Raw bytes of the file transfer in progress.
    FileChunk = 2,
}

impl FrameType {
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            1 => Some(Self::Control),
            2 => Some(Self::FileChunk),
            _ => None,
        }
    }

    pub fn max_payload_bytes(self) -> usize {
        match self {
            Self::Control => MAX_CONTROL_FRAME_BYTES,
            Self::FileChunk => FILE_CHUNK_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub ty: FrameType,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("Unknown frame type {0}.")]
    UnknownFrameType(u8),
    #[error("{ty:?} frame declares {declared} bytes, limit is {limit}.")]
    PayloadTooLarge { ty: FrameType, declared: usize, limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Cannot encode {size}-byte {ty:?} frame; limit is {limit}.")]
pub struct EncodeError {
    pub ty: FrameType,
    pub size: usize,
    pub limit: usize,
}

pub fn encode(ty: FrameType, payload: &[u8]) -> Result<Vec<u8>, EncodeError> {
    let limit = ty.max_payload_bytes();
    if payload.len() > limit {
        return Err(EncodeError { ty, size: payload.len(), limit });
    }
    let mut out = Vec::with_capacity(HEADER_BYTES + payload.len());
    out.push(ty as u8);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Incremental decoder. Rejects a bad header before the payload arrives.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    start: usize,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_byte_count(&self) -> usize {
        self.buf.len() - self.start
    }

    pub fn append(&mut self, data: &[u8]) {
        if self.start > 0 && self.start >= self.buf.len() / 2 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(data);
    }

    pub fn next(&mut self) -> Result<Option<Frame>, DecodeError> {
        let avail = &self.buf[self.start..];
        if avail.len() < HEADER_BYTES {
            return Ok(None);
        }
        let ty = FrameType::from_u8(avail[0]).ok_or(DecodeError::UnknownFrameType(avail[0]))?;
        let declared = u32::from_be_bytes([avail[1], avail[2], avail[3], avail[4]]) as usize;
        let limit = ty.max_payload_bytes();
        if declared > limit {
            return Err(DecodeError::PayloadTooLarge { ty, declared, limit });
        }
        if avail.len() < HEADER_BYTES + declared {
            return Ok(None);
        }
        let payload = avail[HEADER_BYTES..HEADER_BYTES + declared].to_vec();
        self.start += HEADER_BYTES + declared;
        Ok(Some(Frame { ty, payload }))
    }

    pub fn drain(&mut self) -> Result<Vec<Frame>, DecodeError> {
        let mut v = Vec::new();
        while let Some(f) = self.next()? {
            v.push(f);
        }
        Ok(v)
    }
}
