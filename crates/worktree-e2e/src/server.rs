//! A Go `worktree-server` process on a free localhost port.

use std::fs::File;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::api::Api;
use crate::bins;

/// Readiness line printed by the server on startup.
pub const READY_LINE: &str = "worktree-server listening on";
pub const START_TIMEOUT: Duration = Duration::from_secs(30);

/// Running server; killed on drop.
pub struct Server {
    child: Option<Child>,
    /// Base URL, e.g. `http://127.0.0.1:53211` (gRPC + REST share it).
    pub url: String,
    pub port: u16,
    pub admin_user: String,
    pub admin_password: String,
    pub data_dir: PathBuf,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind 127.0.0.1:0");
    l.local_addr().expect("local_addr").port()
}

fn read_lossy(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default()).into_owned()
}

impl Server {
    /// Build (once) and start the server with its data under `dir`. Waits
    /// for the readiness line on stdout or `GET /healthz` == 200.
    #[track_caller]
    pub fn start(dir: &Path) -> Server {
        let bin = bins::server_bin();
        let admin_password = format!("Adm-{}-9z", crate::random_hex(20));
        let mut last_err = String::new();
        for attempt in 1..=3 {
            match Self::try_start(&bin, dir, attempt, &admin_password) {
                Ok(s) => return s,
                Err((retryable, e)) => {
                    last_err = e;
                    if !retryable {
                        break;
                    }
                }
            }
        }
        panic!(
            "failed to start worktree-server ({})\n{last_err}",
            bin.display()
        );
    }

    fn try_start(
        bin: &Path,
        dir: &Path,
        attempt: u32,
        admin_password: &str,
    ) -> Result<Server, (bool, String)> {
        let port = free_port();
        let addr = format!("127.0.0.1:{port}");
        let data_dir = dir.join(format!("server-data-{attempt}"));
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| (false, format!("create {}: {e}", data_dir.display())))?;
        let stdout_path = dir.join(format!("server-{attempt}.stdout.log"));
        let stderr_path = dir.join(format!("server-{attempt}.stderr.log"));
        let out = File::create(&stdout_path).map_err(|e| (false, e.to_string()))?;
        let err = File::create(&stderr_path).map_err(|e| (false, e.to_string()))?;

        let mut cmd = Command::new(bin);
        cmd.arg("--addr")
            .arg(&addr)
            .arg("--data")
            .arg(&data_dir)
            .arg("--admin-user")
            .arg("admin")
            .arg("--admin-password")
            .arg(admin_password)
            .current_dir(dir)
            .env_remove("WT_ADDR")
            .env_remove("WT_DATA")
            .env_remove("WT_ADMIN_USER")
            .env_remove("WT_ADMIN_PASSWORD")
            .env_remove("WT_ADMIN_DIR")
            .env_remove("WT_ALLOW_REGISTRATION")
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err));
        let child = cmd
            .spawn()
            .map_err(|e| (false, format!("spawn {}: {e}", bin.display())))?;

        let mut server = Server {
            child: Some(child),
            url: format!("http://{addr}"),
            port,
            admin_user: "admin".into(),
            admin_password: admin_password.to_string(),
            data_dir,
            stdout_path,
            stderr_path,
        };

        let probe = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .no_proxy()
            .build()
            .expect("reqwest client");
        let started = Instant::now();
        loop {
            if let Some(child) = server.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    let logs = server.logs();
                    let lower = logs.to_lowercase();
                    let retryable = lower.contains("address already in use")
                        || lower.contains("only one usage of each socket address")
                        || lower.contains("bind:");
                    server.child = None;
                    return Err((
                        retryable,
                        format!("server exited during startup ({status})\n{logs}"),
                    ));
                }
            }
            if read_lossy(&server.stdout_path).contains(READY_LINE) {
                return Ok(server);
            }
            if let Ok(r) = probe.get(format!("{}/healthz", server.url)).send() {
                if r.status().as_u16() == 200 {
                    return Ok(server);
                }
            }
            if started.elapsed() > START_TIMEOUT {
                let logs = server.logs();
                return Err((
                    false,
                    format!("server not ready after {START_TIMEOUT:?} on {addr}\n{logs}"),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Server stdout + stderr (for failure reports).
    pub fn logs(&self) -> String {
        format!(
            "--- server stdout ({}) ---\n{}\n--- server stderr ({}) ---\n{}",
            self.stdout_path.display(),
            tail(&read_lossy(&self.stdout_path)),
            self.stderr_path.display(),
            tail(&read_lossy(&self.stderr_path))
        )
    }

    /// Anonymous REST client.
    pub fn api(&self) -> Api {
        Api::new(&self.url)
    }

    /// REST client logged in as the server admin.
    pub fn admin_api(&self) -> Api {
        Api::login(&self.url, &self.admin_user, &self.admin_password)
    }

    /// Kill the process (idempotent).
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(200);
    lines[start..].join("\n")
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}
