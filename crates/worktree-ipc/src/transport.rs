//! IPC transports over platform local sockets.
//!
//! - **Server** (used by `worktree-bg`): async, tokio-based — the daemon
//!   serves many concurrent CLI connections.
//! - **Client** (used by `worktree-sdk`): blocking — the CLI makes one
//!   short-lived request/response exchange per operation.
//!
//! Address mapping: `endpoint_for` produces the spec's display form
//! (`\\.\pipe\wt-worker-<hash>` / `/tmp/wt-worker-<hash>.sock`); this module
//! translates it to `interprocess` names (namespaced on Windows, filesystem
//! path on Unix).

use crate::error::{IpcError, Result};
use interprocess::local_socket as ls;
use interprocess::local_socket::traits::tokio::Listener as _;
use interprocess::local_socket::{GenericFilePath, GenericNamespaced, ToFsName, ToNsName};

const WINDOWS_PIPE_PREFIX: &str = r"\\.\pipe\";

fn socket_name(endpoint: &str) -> Result<ls::Name<'_>> {
    if let Some(pipe) = endpoint.strip_prefix(WINDOWS_PIPE_PREFIX) {
        pipe.to_ns_name::<GenericNamespaced>().map_err(IpcError::Io)
    } else {
        endpoint
            .to_fs_name::<GenericFilePath>()
            .map_err(IpcError::Io)
    }
}

/// Async server listener (daemon side).
pub struct IpcListener {
    inner: ls::tokio::Listener,
}

impl IpcListener {
    /// Bind the endpoint. On Unix a stale socket file from a crashed daemon
    /// is removed first.
    pub fn bind(endpoint: &str) -> Result<Self> {
        if !endpoint.starts_with(WINDOWS_PIPE_PREFIX) && std::path::Path::new(endpoint).exists() {
            // Stale socket from an unclean shutdown; binding requires removal.
            let _ = std::fs::remove_file(endpoint);
        }
        let name = socket_name(endpoint)?;
        let inner = ls::ListenerOptions::new()
            .name(name)
            .create_tokio()
            .map_err(IpcError::Io)?;
        Ok(Self { inner })
    }

    /// Accept the next client connection.
    pub async fn accept(&self) -> Result<ls::tokio::Stream> {
        self.inner.accept().await.map_err(IpcError::Io)
    }
}

/// Async framing over a tokio stream (server side).
pub mod tokio_frame {
    use crate::error::{IpcError, Result};
    use crate::frame::MAX_FRAME_LEN;
    use serde::de::DeserializeOwned;
    use serde::Serialize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Read one length-prefixed JSON frame.
    pub async fn read<R, T>(reader: &mut R) -> Result<T>
    where
        R: AsyncReadExt + Unpin,
        T: DeserializeOwned,
    {
        let mut len_buf = [0u8; 4];
        match reader.read_exact(&mut len_buf).await {
            Ok(_) => {}
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
        reader.read_exact(&mut payload).await?;
        serde_json::from_slice(&payload).map_err(IpcError::Decode)
    }

    /// Write one length-prefixed JSON frame.
    pub async fn write<W, T>(writer: &mut W, msg: &T) -> Result<()>
    where
        W: AsyncWriteExt + Unpin,
        T: Serialize,
    {
        let payload = serde_json::to_vec(msg).map_err(IpcError::Encode)?;
        if payload.len() > MAX_FRAME_LEN {
            return Err(IpcError::FrameTooLarge(payload.len()));
        }
        writer
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;
        Ok(())
    }
}

/// Blocking client connection (CLI side): one request/response at a time.
pub struct IpcClient {
    stream: ls::Stream,
}

impl IpcClient {
    /// Connect to a daemon endpoint. Returns [`IpcError::DaemonUnavailable`]
    /// when nothing is listening.
    pub fn connect(endpoint: &str) -> Result<Self> {
        use interprocess::local_socket::traits::Stream as _;
        let name = socket_name(endpoint)?;
        match ls::Stream::connect(name) {
            Ok(stream) => Ok(Self { stream }),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::AddrNotAvailable
                ) =>
            {
                Err(IpcError::DaemonUnavailable)
            }
            Err(e) => Err(IpcError::Io(e)),
        }
    }

    /// Send a request and wait for the matching response.
    pub fn call(&mut self, request: &crate::Request) -> Result<crate::Response> {
        crate::frame::write_frame(&mut self.stream, request)?;
        let response: crate::Response = crate::frame::read_frame(&mut self.stream)?;
        if response.id != request.id {
            return Err(IpcError::Decode(serde::de::Error::custom(format!(
                "response id '{}' does not match request id '{}'",
                response.id, request.id
            ))));
        }
        Ok(response)
    }
}
