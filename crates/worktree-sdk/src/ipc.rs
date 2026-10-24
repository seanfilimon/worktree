//! CLI <-> background daemon (`worktree-bg`) protocol.
//!
//! Transport: newline-delimited JSON over TCP on 127.0.0.1. One request line,
//! one response line per connection. The daemon writes
//! `$WT_HOME/bg.json` = `{"pid": u32, "port": u16, "version": "..."}` on
//! startup and removes it on clean shutdown.
//!
//! The client side is [`send`] / [`send_to`]; the server side is [`serve`],
//! which the daemon (and tests) drive with a request handler.

use std::future::Future;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

use crate::error::{Result, SdkError};

/// Timeout for connecting, writing a request and (for quick requests)
/// receiving the response.
pub const IPC_TIMEOUT: Duration = Duration::from_secs(5);

/// Response timeout for requests that may do real work in the daemon
/// (snapshotting and staging every tree of a worktree, or setting up a
/// recursive watch over a large directory).
pub const IPC_SLOW_TIMEOUT: Duration = Duration::from_secs(300);

/// Upper bound on a single protocol line; anything longer is rejected.
pub const MAX_LINE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    /// Start watching a worktree root.
    Watch {
        path: PathBuf,
    },
    Unwatch {
        path: PathBuf,
    },
    /// Pause/resume auto-snapshots (all worktrees if path is None).
    Pause {
        path: Option<PathBuf>,
    },
    Resume {
        path: Option<PathBuf>,
    },
    /// Take an auto-snapshot now (all trees of the worktree).
    SnapshotNow {
        path: PathBuf,
    },
    Shutdown,
}

impl IpcRequest {
    /// How long a client waits for the response to this request.
    pub fn response_timeout(&self) -> Duration {
        match self {
            IpcRequest::Watch { .. } | IpcRequest::SnapshotNow { .. } => IPC_SLOW_TIMEOUT,
            _ => IPC_TIMEOUT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum IpcResponse {
    Status(DaemonStatus),
    Ok { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub pid: u32,
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub watched: Vec<WatchedWorktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedWorktree {
    pub root: PathBuf,
    pub paused: bool,
    pub pending_changes: usize,
    pub snapshots_created: u64,
    pub last_snapshot: Option<String>,
    pub last_snapshot_at: Option<DateTime<Utc>>,
    pub last_stage_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
    pub version: String,
}

/// `$WT_HOME/bg.json`.
pub fn daemon_info_path() -> PathBuf {
    crate::config::wt_home().join("bg.json")
}

/// Read `bg.json` if present (does not check liveness).
pub fn read_daemon_info() -> Result<Option<DaemonInfo>> {
    read_daemon_info_at(&daemon_info_path())
}

/// Read a daemon info file at an explicit path (`None` if it does not exist).
pub fn read_daemon_info_at(path: &Path) -> Result<Option<DaemonInfo>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Send one request to the running daemon. `DaemonNotRunning` if bg.json is
/// missing or the connection is refused (stale file).
pub async fn send(req: &IpcRequest) -> Result<IpcResponse> {
    let info = read_daemon_info()?.ok_or(SdkError::DaemonNotRunning)?;
    send_to(info.port, req).await
}

/// Send one request to a daemon listening on `127.0.0.1:port`.
/// `DaemonNotRunning` if nothing is listening there.
pub async fn send_to(port: u16, req: &IpcRequest) -> Result<IpcResponse> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let stream = match tokio::time::timeout(IPC_TIMEOUT, TcpStream::connect(addr)).await {
        Err(_) => return Err(timeout_error("connecting to")),
        Ok(Err(e)) if e.kind() == ErrorKind::ConnectionRefused => {
            return Err(SdkError::DaemonNotRunning)
        }
        Ok(Err(e)) => return Err(e.into()),
        Ok(Ok(stream)) => stream,
    };
    let (read_half, mut write_half) = stream.into_split();

    let mut line = serde_json::to_vec(req)?;
    line.push(b'\n');
    tokio::time::timeout(IPC_TIMEOUT, async {
        write_half.write_all(&line).await?;
        write_half.shutdown().await
    })
    .await
    .map_err(|_| timeout_error("sending a request to"))??;

    let response = tokio::time::timeout(req.response_timeout(), read_line(read_half))
        .await
        .map_err(|_| timeout_error("waiting for a response from"))??;
    match response {
        Some(text) => Ok(serde_json::from_str(&text)?),
        None => Err(SdkError::Other(
            "background daemon closed the connection without responding".into(),
        )),
    }
}

/// Serve the IPC protocol on `listener` until `shutdown` completes.
///
/// Every connection is handled on its own task: one request line is read,
/// passed to `handler`, and its response is written back as one line.
/// Malformed requests get an [`IpcResponse::Error`] without reaching the
/// handler. After `shutdown` fires no new connections are accepted, and
/// in-flight ones get up to [`IPC_TIMEOUT`] to finish (so the response to a
/// `Shutdown` request is still delivered).
pub async fn serve<H, Fut, S>(listener: TcpListener, handler: H, shutdown: S)
where
    H: Fn(IpcRequest) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = IpcResponse> + Send + 'static,
    S: Future<Output = ()>,
{
    tokio::pin!(shutdown);
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => {
                    let handler = handler.clone();
                    connections.spawn(async move {
                        if let Err(e) = handle_connection(stream, handler).await {
                            tracing::debug!(%peer, error = %e, "ipc connection failed");
                        }
                    });
                }
                Err(e) => {
                    // Typically resource exhaustion; back off instead of spinning.
                    tracing::warn!(error = %e, "ipc accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
    let drain = async { while connections.join_next().await.is_some() {} };
    if tokio::time::timeout(IPC_TIMEOUT, drain).await.is_err() {
        tracing::warn!("abandoning ipc connections still in flight at shutdown");
    }
}

async fn handle_connection<H, Fut>(stream: TcpStream, handler: H) -> Result<()>
where
    H: Fn(IpcRequest) -> Fut,
    Fut: Future<Output = IpcResponse>,
{
    let (read_half, mut write_half) = stream.into_split();
    let line = tokio::time::timeout(IPC_TIMEOUT, read_line(read_half))
        .await
        .map_err(|_| timeout_error("reading a request for"))??;
    let Some(line) = line else {
        return Ok(());
    };
    let response = match serde_json::from_str::<IpcRequest>(&line) {
        Ok(req) => handler(req).await,
        Err(e) => IpcResponse::Error {
            message: format!("invalid request: {e}"),
        },
    };
    let mut out = serde_json::to_vec(&response)?;
    out.push(b'\n');
    write_response(&mut write_half, &out).await
}

async fn write_response<W: AsyncWrite + Unpin>(writer: &mut W, bytes: &[u8]) -> Result<()> {
    tokio::time::timeout(IPC_TIMEOUT, async {
        writer.write_all(bytes).await?;
        writer.shutdown().await
    })
    .await
    .map_err(|_| timeout_error("writing a response for"))??;
    Ok(())
}

/// Read one '\n'-terminated line (terminator stripped). `None` on immediate
/// EOF; `InvalidArgument` if the line exceeds [`MAX_LINE_BYTES`].
async fn read_line<R: AsyncRead + Unpin>(reader: R) -> Result<Option<String>> {
    let mut reader = BufReader::new(reader.take(MAX_LINE_BYTES + 1));
    let mut line = String::new();
    let n = reader.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    if n as u64 > MAX_LINE_BYTES {
        return Err(SdkError::InvalidArgument(format!(
            "ipc message exceeds {MAX_LINE_BYTES} bytes"
        )));
    }
    let trimmed = line.trim_end_matches(['\n', '\r']);
    Ok(Some(trimmed.to_owned()))
}

fn timeout_error(action: &str) -> SdkError {
    SdkError::Other(format!("timed out {action} the background daemon"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use tokio::sync::{oneshot, Notify};

    /// A unique scratch directory under the system temp dir, removed on drop.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new() -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("wt-ipc-test-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("create scratch dir");
            Self(dir)
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_status() -> DaemonStatus {
        DaemonStatus {
            pid: 42,
            version: "0.1.0".into(),
            started_at: Utc::now(),
            watched: vec![WatchedWorktree {
                root: PathBuf::from("/work/project"),
                paused: true,
                pending_changes: 3,
                snapshots_created: 7,
                last_snapshot: Some("abcd1234".into()),
                last_snapshot_at: Some(Utc::now()),
                last_stage_at: None,
                last_error: Some("boom".into()),
            }],
        }
    }

    /// Serve `handler` on an ephemeral port; returns the port and a trigger
    /// that stops the server, plus the server task.
    async fn spawn_server<H, Fut>(
        handler: H,
    ) -> (u16, oneshot::Sender<()>, tokio::task::JoinHandle<()>)
    where
        H: Fn(IpcRequest) -> Fut + Clone + Send + 'static,
        Fut: Future<Output = IpcResponse> + Send + 'static,
    {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind");
        let port = listener.local_addr().expect("local addr").port();
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(serve(listener, handler, async {
            let _ = stop_rx.await;
        }));
        (port, stop_tx, task)
    }

    async fn fake_handler(req: IpcRequest) -> IpcResponse {
        match req {
            IpcRequest::Status => IpcResponse::Status(DaemonStatus {
                pid: 1,
                version: "test".into(),
                started_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"),
                watched: Vec::new(),
            }),
            IpcRequest::Watch { path } => IpcResponse::Ok {
                message: format!("watching {}", path.display()),
            },
            other => IpcResponse::Error {
                message: format!("unsupported: {other:?}"),
            },
        }
    }

    #[test]
    fn request_wire_format_is_tagged_snake_case() {
        let json = serde_json::to_value(IpcRequest::SnapshotNow { path: "/a".into() }).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"cmd": "snapshot_now", "path": "/a"})
        );
        let json = serde_json::to_value(IpcRequest::Pause { path: None }).unwrap();
        assert_eq!(json, serde_json::json!({"cmd": "pause", "path": null}));
        let parsed: IpcRequest = serde_json::from_str(r#"{"cmd":"shutdown"}"#).unwrap();
        assert_eq!(parsed, IpcRequest::Shutdown);
    }

    #[test]
    fn response_round_trips_through_json() {
        for resp in [
            IpcResponse::Status(sample_status()),
            IpcResponse::Ok {
                message: "fine".into(),
            },
            IpcResponse::Error {
                message: "bad".into(),
            },
        ] {
            let text = serde_json::to_string(&resp).unwrap();
            assert!(!text.contains('\n'), "responses must be single-line");
            let back: IpcResponse = serde_json::from_str(&text).unwrap();
            assert_eq!(back, resp);
        }
        let ok = serde_json::to_value(IpcResponse::Ok {
            message: "m".into(),
        })
        .unwrap();
        assert_eq!(ok, serde_json::json!({"result": "ok", "message": "m"}));
    }

    #[test]
    fn slow_requests_get_longer_timeouts() {
        assert_eq!(IpcRequest::Status.response_timeout(), IPC_TIMEOUT);
        assert_eq!(IpcRequest::Shutdown.response_timeout(), IPC_TIMEOUT);
        let snap = IpcRequest::SnapshotNow { path: "/a".into() };
        assert_eq!(snap.response_timeout(), IPC_SLOW_TIMEOUT);
        let watch = IpcRequest::Watch { path: "/a".into() };
        assert_eq!(watch.response_timeout(), IPC_SLOW_TIMEOUT);
    }

    #[test]
    fn daemon_info_missing_file_is_none() {
        let dir = ScratchDir::new();
        assert_eq!(read_daemon_info_at(&dir.0.join("bg.json")).unwrap(), None);
    }

    #[test]
    fn daemon_info_reads_written_file() {
        let dir = ScratchDir::new();
        let path = dir.0.join("bg.json");
        std::fs::write(&path, r#"{"pid": 1234, "port": 50123, "version": "0.1.0"}"#).unwrap();
        let info = read_daemon_info_at(&path).unwrap();
        assert_eq!(
            info,
            Some(DaemonInfo {
                pid: 1234,
                port: 50123,
                version: "0.1.0".into()
            })
        );
    }

    #[test]
    fn daemon_info_corrupt_file_is_serialization_error() {
        let dir = ScratchDir::new();
        let path = dir.0.join("bg.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(matches!(
            read_daemon_info_at(&path),
            Err(SdkError::Serialization(_))
        ));
    }

    #[tokio::test]
    async fn round_trip_status_ok_and_error() {
        let (port, stop, task) = spawn_server(fake_handler).await;

        match send_to(port, &IpcRequest::Status).await.unwrap() {
            IpcResponse::Status(status) => {
                assert_eq!(status.pid, 1);
                assert_eq!(status.version, "test");
                assert!(status.watched.is_empty());
            }
            other => panic!("unexpected response {other:?}"),
        }

        let resp = send_to(
            port,
            &IpcRequest::Watch {
                path: "proj".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            resp,
            IpcResponse::Ok {
                message: "watching proj".into()
            }
        );

        let resp = send_to(port, &IpcRequest::Shutdown).await.unwrap();
        assert_eq!(
            resp,
            IpcResponse::Error {
                message: "unsupported: Shutdown".into()
            }
        );

        stop.send(()).unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_clients_are_served() {
        let (port, stop, task) = spawn_server(fake_handler).await;
        let mut clients = JoinSet::new();
        for _ in 0..16 {
            clients.spawn(async move { send_to(port, &IpcRequest::Status).await });
        }
        while let Some(result) = clients.join_next().await {
            assert!(matches!(result.unwrap().unwrap(), IpcResponse::Status(_)));
        }
        stop.send(()).unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn malformed_request_gets_error_response() {
        let (port, stop, task) = spawn_server(fake_handler).await;
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        stream.write_all(b"{\"cmd\":\"explode\"}\n").await.unwrap();
        stream.shutdown().await.unwrap();
        let line = read_line(stream).await.unwrap().expect("a response line");
        match serde_json::from_str::<IpcResponse>(&line).unwrap() {
            IpcResponse::Error { message } => assert!(message.starts_with("invalid request")),
            other => panic!("unexpected response {other:?}"),
        }
        stop.send(()).unwrap();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn connection_refused_is_daemon_not_running() {
        // Bind then drop to obtain a port that (almost certainly) nobody serves.
        let port = {
            let l = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
            l.local_addr().unwrap().port()
        };
        let err = send_to(port, &IpcRequest::Status).await.unwrap_err();
        assert!(matches!(err, SdkError::DaemonNotRunning), "got {err:?}");
    }

    #[tokio::test]
    async fn shutdown_response_is_delivered_before_server_stops() {
        let stop = Arc::new(Notify::new());
        let handler = {
            let stop = stop.clone();
            move |req: IpcRequest| {
                let stop = stop.clone();
                async move {
                    if req == IpcRequest::Shutdown {
                        stop.notify_one();
                        // Give the accept loop a chance to observe shutdown first.
                        tokio::task::yield_now().await;
                    }
                    IpcResponse::Ok {
                        message: "bye".into(),
                    }
                }
            }
        };
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = {
            let stop = stop.clone();
            tokio::spawn(serve(
                listener,
                handler,
                async move { stop.notified().await },
            ))
        };

        let resp = send_to(port, &IpcRequest::Shutdown).await.unwrap();
        assert_eq!(
            resp,
            IpcResponse::Ok {
                message: "bye".into()
            }
        );
        tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("server stops after shutdown")
            .unwrap();
        assert!(matches!(
            send_to(port, &IpcRequest::Status).await,
            Err(SdkError::DaemonNotRunning)
        ));
    }

    #[tokio::test]
    async fn oversized_line_is_rejected() {
        let big = vec![b'x'; (MAX_LINE_BYTES + 10) as usize];
        let err = read_line(&big[..]).await.unwrap_err();
        assert!(matches!(err, SdkError::InvalidArgument(_)));
        assert_eq!(read_line(&b""[..]).await.unwrap(), None);
        assert_eq!(
            read_line(&b"abc\r\n"[..]).await.unwrap(),
            Some("abc".into())
        );
    }
}
