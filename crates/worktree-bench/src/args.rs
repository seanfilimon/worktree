//! Command-line arguments.

use std::path::PathBuf;
use std::str::FromStr;

use clap::Parser;

/// Load generator for the W0rkTree server: concurrent pushes, clones and
/// code searches, reported as latency percentiles and throughput.
///
/// Without --server it starts its own Go server (WT_SERVER_BIN, else
/// target/{debug,release}/worktree-server, else `go build`) on a free port
/// with a temporary data directory, and stops it on exit.
#[derive(Debug, Clone, Parser)]
#[command(name = "worktree-bench", version)]
pub struct Args {
    /// Target an existing server instead of starting one.
    #[arg(long, value_name = "URL")]
    pub server: Option<String>,

    /// Log in as this user (with --password); default: register a fresh
    /// user (the server must allow registration).
    #[arg(long, value_name = "NAME", requires = "password")]
    pub user: Option<String>,

    /// Password for --user.
    #[arg(long, env = "WT_BENCH_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,

    /// Go server binary to start (default: WT_SERVER_BIN, the cargo target
    /// directory, or `go build`).
    #[arg(long, value_name = "PATH", conflicts_with = "server")]
    pub server_bin: Option<PathBuf>,

    /// Extra flag for the started server (repeatable), e.g.
    /// --server-arg=--compress-objects=false.
    #[arg(
        long = "server-arg",
        value_name = "FLAG",
        allow_hyphen_values = true,
        conflicts_with = "server"
    )]
    pub server_args: Vec<String>,

    /// Comma-separated workloads to run, in this order: push, clone, search,
    /// edits (pack-delta §16.1: small edits to a large tree).
    #[arg(long, value_name = "LIST", default_value = "push,clone,search", value_parser = parse_workloads)]
    pub workload: Workloads,

    /// Concurrent workers per workload.
    #[arg(long, short = 'c', default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..=1024))]
    pub concurrency: u32,

    /// Operations per worker (push rounds, clones or searches).
    #[arg(long, short = 'n', default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
    pub iterations: u32,

    /// Files per tree.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..))]
    pub files: u32,

    /// Size of each file (bytes; k/m/g suffixes allowed, e.g. 4k).
    #[arg(long, default_value = "4k", value_parser = parse_size)]
    pub file_size: u64,

    /// Files rewritten by each push round after the first (0 = all).
    #[arg(long, default_value_t = 0)]
    pub changed: u32,

    /// Literal searched by the search workload (seeded into the tree).
    #[arg(long, default_value = "benchneedle")]
    pub query: String,

    /// Create this many (non-matching) access policies in the bench
    /// tenant first, so that every authorization evaluates them.
    #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u32).range(0..=2000))]
    pub policies: u32,

    /// Print the report as JSON instead of a table.
    #[arg(long)]
    pub json: bool,

    /// While the workloads run, capture a server CPU profile into FILE
    /// (needs a server with --pprof; started servers get it automatically
    /// with this flag, and the started server's admin is used).
    #[arg(long, value_name = "FILE")]
    pub cpu_profile: Option<PathBuf>,

    /// Start the --cpu-profile capture with this workload (default: the
    /// first one run).
    #[arg(long, value_name = "WORKLOAD")]
    pub profile_workload: Option<Workload>,

    /// Duration of the --cpu-profile capture (seconds).
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(1..=50))]
    pub profile_seconds: u32,

    /// Keep the temporary directory (worktrees, server data and logs).
    #[arg(long)]
    pub keep: bool,
}

/// One kind of workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload {
    Push,
    Clone,
    Search,
    /// pack-delta (§16.1): one large tree per worker, small edits per round.
    Edits,
}

impl FromStr for Workload {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "push" => Ok(Workload::Push),
            "clone" => Ok(Workload::Clone),
            "search" => Ok(Workload::Search),
            "edits" => Ok(Workload::Edits), // pack-delta (§16.1)
            other => Err(format!(
                "unknown workload '{other}' (want push, clone, search or edits)"
            )),
        }
    }
}

/// The selected workloads (deduplicated, in the given order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workloads(pub Vec<Workload>);

/// Parse "push,clone,search" (any subset, any order, "all" = push, clone
/// and search; `edits` must be named).
pub fn parse_workloads(s: &str) -> Result<Workloads, String> {
    let mut out = Vec::new();
    for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if part.eq_ignore_ascii_case("all") {
            for w in [Workload::Push, Workload::Clone, Workload::Search] {
                if !out.contains(&w) {
                    out.push(w);
                }
            }
            continue;
        }
        let w: Workload = part.parse()?;
        if !out.contains(&w) {
            out.push(w);
        }
    }
    if out.is_empty() {
        return Err("no workload given".into());
    }
    Ok(Workloads(out))
}

/// Parse a byte size: "4096", "4k", "1.5M", "2g" (binary multiples).
pub fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim();
    let lower = t.to_ascii_lowercase();
    let lower = lower.strip_suffix('b').unwrap_or(&lower);
    let (num, mult) = match lower.chars().last() {
        Some('k') => (&lower[..lower.len() - 1], 1u64 << 10),
        Some('m') => (&lower[..lower.len() - 1], 1 << 20),
        Some('g') => (&lower[..lower.len() - 1], 1 << 30),
        _ => (lower, 1),
    };
    let v: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("invalid size '{t}' (e.g. 4096, 4k, 1m)"))?;
    if !v.is_finite() || v < 1.0 {
        return Err(format!("size '{t}' must be at least 1 byte"));
    }
    let bytes = v * mult as f64;
    if bytes > (1u64 << 40) as f64 {
        return Err(format!("size '{t}' is too large"));
    }
    Ok(bytes.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, clap::Error> {
        Args::try_parse_from(std::iter::once("worktree-bench").chain(args.iter().copied()))
    }

    #[test]
    fn defaults() {
        let a = parse(&[]).unwrap();
        assert_eq!(a.server, None);
        assert_eq!(
            a.workload,
            Workloads(vec![Workload::Push, Workload::Clone, Workload::Search])
        );
        assert_eq!((a.concurrency, a.iterations, a.files), (4, 5, 100));
        assert_eq!(a.file_size, 4096);
        assert_eq!(a.changed, 0);
        assert_eq!(a.query, "benchneedle");
        assert!(!a.json && !a.keep);
    }

    #[test]
    fn full_command_line() {
        let a = parse(&[
            "--server",
            "http://127.0.0.1:7420",
            "--user",
            "alice",
            "--password",
            "pw",
            "--workload",
            "search,push",
            "--concurrency",
            "8",
            "-n",
            "3",
            "--files",
            "1000",
            "--file-size",
            "64k",
            "--changed",
            "10",
            "--json",
        ])
        .unwrap();
        assert_eq!(a.server.as_deref(), Some("http://127.0.0.1:7420"));
        assert_eq!(a.user.as_deref(), Some("alice"));
        assert_eq!(a.password.as_deref(), Some("pw"));
        assert_eq!(
            a.workload,
            Workloads(vec![Workload::Search, Workload::Push])
        );
        assert_eq!(
            (a.concurrency, a.iterations, a.files, a.changed),
            (8, 3, 1000, 10)
        );
        assert_eq!(a.file_size, 64 * 1024);
        assert!(a.json);
    }

    #[test]
    fn server_args_accept_flags() {
        let a = parse(&["--server-arg=--pprof", "--server-arg", "--rate-limit=10"]).unwrap();
        assert_eq!(a.server_args, vec!["--pprof", "--rate-limit=10"]);
    }

    #[test]
    fn profile_options() {
        let a = parse(&[
            "--cpu-profile",
            "cpu.pprof",
            "--profile-workload",
            "search",
            "--profile-seconds",
            "5",
        ])
        .unwrap();
        assert_eq!(a.cpu_profile, Some(PathBuf::from("cpu.pprof")));
        assert_eq!(a.profile_workload, Some(Workload::Search));
        assert_eq!(a.profile_seconds, 5);
        assert!(parse(&["--profile-workload", "nope"]).is_err());
        assert!(parse(&["--profile-seconds", "51"]).is_err());
        assert!(parse(&["--policies", "2001"]).is_err());
    }

    #[test]
    fn invalid_combinations_are_rejected() {
        assert!(
            parse(&["--user", "alice"]).is_err(),
            "--user needs --password"
        );
        assert!(parse(&["--concurrency", "0"]).is_err());
        assert!(parse(&["--iterations", "0"]).is_err());
        assert!(parse(&["--workload", "push,frobnicate"]).is_err());
        assert!(parse(&["--workload", ""]).is_err());
        assert!(parse(&["--server", "http://x", "--server-bin", "s.exe"]).is_err());
        assert!(parse(&["--server", "http://x", "--server-arg=--pprof"]).is_err());
        assert!(parse(&["--file-size", "0"]).is_err());
    }

    #[test]
    fn workload_lists() {
        assert_eq!(
            parse_workloads(" Clone , clone,push ").unwrap(),
            Workloads(vec![Workload::Clone, Workload::Push])
        );
        assert_eq!(parse_workloads("all").unwrap().0.len(), 3);
        assert!(parse_workloads(",,").is_err());
        assert!(parse_workloads("pull")
            .unwrap_err()
            .contains("unknown workload"));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("4096"), Ok(4096));
        assert_eq!(parse_size("4k"), Ok(4096));
        assert_eq!(parse_size("4KB"), Ok(4096));
        assert_eq!(parse_size("1.5m"), Ok(3 * 512 * 1024));
        assert_eq!(parse_size("2G"), Ok(2 << 30));
        assert_eq!(parse_size(" 10 "), Ok(10));
        assert!(parse_size("").is_err());
        assert!(parse_size("k").is_err());
        assert!(parse_size("-1").is_err());
        assert!(parse_size("0.2").is_err());
        assert!(parse_size("abc").is_err());
        assert!(parse_size("5000g").is_err());
    }
}
