//! W0rkTree CLI↔daemon IPC.
//!
//! Implements the protocol from `specs/bgprocess/BgProcess.md` §14:
//!
//! - Transport: Unix domain socket (Linux/macOS) or named pipe (Windows),
//!   one endpoint per worktree, addressed by a stable worktree-path hash.
//! - Framing: `[u32 big-endian length][JSON payload]`.
//! - Messages: [`Request`] `{id, command, args}` / [`Response`]
//!   `{id, status, data}` with a typed [`message::Command`] enum mirroring
//!   the spec's dispatch table.
//!
pub mod endpoint;
pub mod error;
pub mod frame;
pub mod message;
pub mod transport;

pub use error::{IpcError, Result};
pub use message::{Command, Request, Response, ResponseStatus};
pub use transport::{IpcClient, IpcListener};
