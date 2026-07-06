//! IPC client to a running `worktree-bg` daemon.
//!
//! One blocking connection per [`RemoteClient`]; requests are serialized
//! through a mutex (the CLI performs one operation at a time anyway).

use crate::error::{Result, SdkError};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::Path;
use std::sync::Mutex;
use worktree_ipc::{Command, IpcClient, Request, ResponseStatus};

/// Handle to a daemon serving one worktree.
pub struct RemoteClient {
    connection: Mutex<IpcClient>,
}

impl RemoteClient {
    /// Try to connect to the daemon for the worktree at `root`.
    ///
    /// Returns `None` when no daemon is listening (the caller falls back
    /// to embedded mode).
    pub fn try_connect(root: &Path) -> Option<Self> {
        let endpoint = worktree_ipc::endpoint::endpoint_for(root);
        IpcClient::connect(&endpoint).ok().map(|connection| Self {
            connection: Mutex::new(connection),
        })
    }

    /// Send a command and deserialize the response payload.
    pub fn call<T: DeserializeOwned>(&self, command: Command, args: Value) -> Result<T> {
        let request = Request::new(command, args);
        let response = self
            .connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .call(&request)
            .map_err(|e| match e {
                worktree_ipc::IpcError::DaemonUnavailable
                | worktree_ipc::IpcError::ConnectionClosed => SdkError::DaemonUnavailable,
                other => SdkError::Daemon(other.to_string()),
            })?;

        match response.status {
            ResponseStatus::Ok => serde_json::from_value(response.data)
                .map_err(|e| SdkError::Daemon(format!("malformed daemon response: {e}"))),
            ResponseStatus::Error => {
                let message = response
                    .data
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown daemon error");
                Err(SdkError::Daemon(message.to_string()))
            }
        }
    }
}
