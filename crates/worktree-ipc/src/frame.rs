//! Length-prefixed JSON framing (BgProcess.md §14.2).
//!
//! ```text
//! [4 bytes: message length (u32 big-endian)] [JSON payload]
//! ```
//!
//! Synchronous encode/decode over `std::io` streams. The async transports
//! in WT-PHASE-3 reuse these by framing over buffered byte channels.

use crate::error::{IpcError, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Read, Write};

/// Maximum accepted frame size (16 MiB) — guards against corrupt length
/// prefixes allocating unbounded buffers.
pub const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// Serialize `msg` and write it as a single frame.
pub fn write_frame<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> Result<()> {
    let payload = serde_json::to_vec(msg).map_err(IpcError::Encode)?;
    if payload.len() > MAX_FRAME_LEN {
        return Err(IpcError::FrameTooLarge(payload.len()));
    }
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()?;
    Ok(())
}

/// Read a single frame and deserialize it as `T`.
///
/// Returns [`IpcError::ConnectionClosed`] on a clean EOF at a frame
/// boundary.
pub fn read_frame<R: Read, T: DeserializeOwned>(reader: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(IpcError::ConnectionClosed)
        }
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_LEN {
        return Err(IpcError::FrameTooLarge(len));
    }
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload).map_err(IpcError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{Command, Request, Response};
    use std::io::Cursor;

    #[test]
    fn frame_roundtrip() {
        let req = Request::new(Command::Status, serde_json::json!({ "tree_id": "backend" }));

        let mut buf = Vec::new();
        write_frame(&mut buf, &req).unwrap();

        // Length prefix is big-endian and matches the payload.
        let len = u32::from_be_bytes(buf[..4].try_into().unwrap()) as usize;
        assert_eq!(len, buf.len() - 4);

        let decoded: Request = read_frame(&mut Cursor::new(&buf)).unwrap();
        assert_eq!(decoded.id, req.id);
        assert_eq!(decoded.command, "status");
    }

    #[test]
    fn multiple_frames_in_sequence() {
        let mut buf = Vec::new();
        write_frame(&mut buf, &Response::ok("a", serde_json::json!(1))).unwrap();
        write_frame(&mut buf, &Response::ok("b", serde_json::json!(2))).unwrap();

        let mut cursor = Cursor::new(&buf);
        let first: Response = read_frame(&mut cursor).unwrap();
        let second: Response = read_frame(&mut cursor).unwrap();
        assert_eq!(first.id, "a");
        assert_eq!(second.id, "b");
    }

    #[test]
    fn eof_at_boundary_is_connection_closed() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let err = read_frame::<_, Response>(&mut cursor).unwrap_err();
        assert!(matches!(err, IpcError::ConnectionClosed));
    }

    #[test]
    fn oversized_length_prefix_is_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(u32::MAX).to_be_bytes());
        buf.extend_from_slice(b"garbage");
        let err = read_frame::<_, Response>(&mut Cursor::new(&buf)).unwrap_err();
        assert!(matches!(err, IpcError::FrameTooLarge(_)));
    }
}
