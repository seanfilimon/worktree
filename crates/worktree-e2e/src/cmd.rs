//! Process runner with timeouts and readable failure reports.
//!
//! stdout/stderr are redirected to files (in the test env's log dir) rather
//! than pipes, so a grandchild that inherits the handles (e.g. the daemon
//! spawned by `wt daemon start`) can never make the runner hang waiting for
//! EOF. stdin is `/dev/null` unless input is supplied, so prompts see EOF.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Default per-command timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

static SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Cmd {
    program: PathBuf,
    /// Short name shown in reports (e.g. "wt", "git").
    display_name: String,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    envs: Vec<(OsString, Option<OsString>)>,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
    log_dir: PathBuf,
    /// Who runs it (e.g. "alice"), for reports.
    actor: Option<String>,
}

impl Cmd {
    pub fn new(program: impl Into<PathBuf>, log_dir: impl Into<PathBuf>) -> Self {
        let program = program.into();
        let display_name = program
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| program.display().to_string());
        Self {
            program,
            display_name,
            args: Vec::new(),
            cwd: None,
            envs: Vec::new(),
            stdin: None,
            timeout: DEFAULT_TIMEOUT,
            log_dir: log_dir.into(),
            actor: None,
        }
    }

    pub fn arg(mut self, a: impl AsRef<OsStr>) -> Self {
        self.args.push(a.as_ref().to_os_string());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.args
            .extend(args.into_iter().map(|a| a.as_ref().to_os_string()));
        self
    }

    /// Insert arguments before all others (used for global flags).
    pub fn prefix_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut pre: Vec<OsString> = args
            .into_iter()
            .map(|a| a.as_ref().to_os_string())
            .collect();
        pre.append(&mut self.args);
        self.args = pre;
        self
    }

    pub fn cwd(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    pub fn env(mut self, k: impl AsRef<OsStr>, v: impl AsRef<OsStr>) -> Self {
        self.envs
            .push((k.as_ref().to_os_string(), Some(v.as_ref().to_os_string())));
        self
    }

    pub fn env_remove(mut self, k: impl AsRef<OsStr>) -> Self {
        self.envs.push((k.as_ref().to_os_string(), None));
        self
    }

    pub fn stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(data.into());
        self
    }

    pub fn timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    pub fn actor(mut self, who: impl Into<String>) -> Self {
        self.actor = Some(who.into());
        self
    }

    /// `wt --json <args>`: run, require success, parse stdout as JSON.
    #[track_caller]
    pub fn json(self) -> Value {
        self.prefix_args(["--json"]).ok().json()
    }

    /// Human-readable command line.
    pub fn cmdline(&self) -> String {
        let mut s = self.display_name.clone();
        for a in &self.args {
            let a = a.to_string_lossy();
            if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') {
                s.push_str(&format!(" \"{}\"", a.replace('"', "\\\"")));
            } else {
                s.push(' ');
                s.push_str(&a);
            }
        }
        s
    }

    /// Run to completion. Errors on spawn failure or timeout (the process is
    /// killed); never panics, so it is safe to call from `Drop`.
    pub fn try_run(self) -> Result<CmdOutput, String> {
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let _ = std::fs::create_dir_all(&self.log_dir);
        let base = format!("{seq:05}-{}", sanitize(&self.display_name));
        let out_path = self.log_dir.join(format!("{base}.stdout"));
        let err_path = self.log_dir.join(format!("{base}.stderr"));
        let cmdline = self.cmdline();
        let header = || {
            format!(
                "{}`{cmdline}` (cwd: {})",
                self.actor
                    .as_ref()
                    .map(|a| format!("[{a}] "))
                    .unwrap_or_default(),
                self.cwd
                    .as_ref()
                    .map(|c| c.display().to_string())
                    .unwrap_or_else(|| ".".into())
            )
        };

        let out_file = File::create(&out_path)
            .map_err(|e| format!("{}: create {}: {e}", header(), out_path.display()))?;
        let err_file = File::create(&err_path)
            .map_err(|e| format!("{}: create {}: {e}", header(), err_path.display()))?;
        let _ = std::fs::write(self.log_dir.join(format!("{base}.cmd")), header());

        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(dir) = &self.cwd {
            command.current_dir(dir);
        }
        for (k, v) in &self.envs {
            match v {
                Some(v) => command.env(k, v),
                None => command.env_remove(k),
            };
        }
        command
            .stdout(Stdio::from(out_file))
            .stderr(Stdio::from(err_file))
            .stdin(if self.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });

        let started = Instant::now();
        let mut child = command.spawn().map_err(|e| {
            format!(
                "{}: failed to spawn {}: {e}",
                header(),
                self.program.display()
            )
        })?;
        if let Some(data) = &self.stdin {
            if let Some(mut stdin) = child.stdin.take() {
                // The child may exit without reading; ignore broken pipes.
                let _ = stdin.write_all(data);
                let _ = stdin.flush();
            }
        }

        let status: ExitStatus = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() > self.timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        let out = CmdOutput::from_files(
                            cmdline.clone(),
                            self.cwd.clone(),
                            self.actor.clone(),
                            None,
                            &out_path,
                            &err_path,
                            started.elapsed(),
                        );
                        return Err(format!("TIMEOUT after {:?}\n{out}", self.timeout));
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(format!("{}: wait failed: {e}", header())),
            }
        };
        Ok(CmdOutput::from_files(
            cmdline,
            self.cwd.clone(),
            self.actor.clone(),
            status.code(),
            &out_path,
            &err_path,
            started.elapsed(),
        ))
    }

    /// Run to completion (panics on spawn failure / timeout).
    #[track_caller]
    pub fn run(self) -> CmdOutput {
        match self.try_run() {
            Ok(out) => out,
            Err(e) => panic!("{e}"),
        }
    }

    /// Run and require exit code 0.
    #[track_caller]
    pub fn ok(self) -> CmdOutput {
        let out = self.run();
        out.assert_success();
        out
    }

    /// Run and require a non-zero exit code.
    #[track_caller]
    pub fn fails(self) -> CmdOutput {
        let out = self.run();
        out.assert_failure();
        out
    }

    /// Run and require exactly exit code `code`.
    #[track_caller]
    pub fn code(self, code: i32) -> CmdOutput {
        let out = self.run();
        out.assert_code(code);
        out
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Remove ANSI escape sequences (CSI `ESC [ ... final`, and bare `ESC x`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct CmdOutput {
    pub cmdline: String,
    pub cwd: Option<PathBuf>,
    pub actor: Option<String>,
    /// Exit code (None if killed by a signal / timeout).
    pub code: Option<i32>,
    /// stdout with ANSI escapes removed.
    pub stdout: String,
    /// stderr with ANSI escapes removed.
    pub stderr: String,
    pub duration: Duration,
}

impl CmdOutput {
    fn from_files(
        cmdline: String,
        cwd: Option<PathBuf>,
        actor: Option<String>,
        code: Option<i32>,
        out: &Path,
        err: &Path,
        duration: Duration,
    ) -> Self {
        let read = |p: &Path| {
            strip_ansi(&String::from_utf8_lossy(
                &std::fs::read(p).unwrap_or_default(),
            ))
        };
        Self {
            cmdline,
            cwd,
            actor,
            code,
            stdout: read(out),
            stderr: read(err),
            duration,
        }
    }

    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// stdout + stderr.
    pub fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    /// Case-insensitive search in stdout + stderr.
    pub fn mentions(&self, needle: &str) -> bool {
        self.combined()
            .to_lowercase()
            .contains(&needle.to_lowercase())
    }

    #[track_caller]
    pub fn assert_success(&self) {
        assert!(self.success(), "command failed (expected exit 0)\n{self}");
    }

    #[track_caller]
    pub fn assert_failure(&self) {
        assert!(
            self.code.is_some() && !self.success(),
            "command unexpectedly succeeded (expected non-zero exit)\n{self}"
        );
    }

    #[track_caller]
    pub fn assert_code(&self, code: i32) {
        assert_eq!(
            self.code,
            Some(code),
            "unexpected exit code (expected {code})\n{self}"
        );
    }

    /// Parse stdout as JSON (panics with the full report on failure).
    #[track_caller]
    pub fn json(&self) -> Value {
        let text = self.stdout.trim();
        match serde_json::from_str(text) {
            Ok(v) => v,
            Err(e) => {
                // Tolerate log lines before the document: try from the first '{' / '['.
                if let Some(pos) = text.find(['{', '[']) {
                    if let Ok(v) = serde_json::from_str(&text[pos..]) {
                        return v;
                    }
                }
                panic!("stdout is not valid JSON ({e})\n{self}")
            }
        }
    }

    /// Non-empty stdout lines, trimmed.
    pub fn lines(&self) -> Vec<String> {
        self.stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect()
    }
}

fn tail(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut start = s.len() - max;
    while !s.is_char_boundary(start) {
        start += 1;
    }
    &s[start..]
}

impl fmt::Display for CmdOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(a) = &self.actor {
            write!(f, "[{a}] ")?;
        }
        writeln!(f, "command: {}", self.cmdline)?;
        if let Some(c) = &self.cwd {
            writeln!(f, "cwd:     {}", c.display())?;
        }
        writeln!(
            f,
            "exit:    {} ({:.2}s)",
            self.code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "killed".into()),
            self.duration.as_secs_f64()
        )?;
        writeln!(
            f,
            "--- stdout ---\n{}",
            tail(&self.stdout, 16 * 1024).trim_end()
        )?;
        write!(
            f,
            "--- stderr ---\n{}",
            tail(&self.stderr, 16 * 1024).trim_end()
        )
    }
}
