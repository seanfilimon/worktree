//! `worktree-bench`: drive concurrent pushes, clones and code searches
//! against a W0rkTree server and report latency percentiles and
//! throughput (docs/ENGINEERING_PLAN.md §15.10, docs/performance.md).

mod args;
mod server;
mod stats;
mod workloads;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use serde::Serialize;
use worktree_sdk::remote::{self, LoginInfo, RestClient};

use args::{Args, Workload};
use stats::Summary;
use workloads::Plan;

fn main() -> ExitCode {
    let args = Args::parse();
    let tmp = match tempfile::Builder::new().prefix("worktree-bench-").tempdir() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot create a temporary directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    // The SDK keeps credentials (and user config) under $WT_HOME: isolate
    // them before any thread starts.
    let home = tmp.path().join("wt-home");
    if let Err(e) = std::fs::create_dir_all(&home) {
        eprintln!("error: create {}: {e}", home.display());
        return ExitCode::FAILURE;
    }
    std::env::set_var("WT_HOME", &home);
    std::env::remove_var("WT_TOKEN");

    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: tokio runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let code = rt.block_on(async {
        tokio::select! {
            r = run(&args, tmp.path()) => match r {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::FAILURE
                }
            },
            _ = tokio::signal::ctrl_c() => {
                eprintln!("interrupted");
                ExitCode::from(130)
            }
        }
    });
    // Dropping the runtime drops `run`'s future, which kills a started
    // server (Server::drop) before the temporary directory is removed.
    drop(rt);
    if args.keep {
        let kept = tmp.keep();
        eprintln!("kept {}", kept.display());
    }
    code
}

/// The JSON report.
#[derive(Serialize)]
struct Report<'a> {
    server: &'a str,
    started_server: bool,
    workloads: Vec<&'static str>,
    concurrency: u32,
    iterations: u32,
    files: u32,
    file_size: u64,
    changed: u32,
    policies: u32,
    results: &'a [Summary],
}

fn workload_name(w: Workload) -> &'static str {
    match w {
        Workload::Push => "push",
        Workload::Clone => "clone",
        Workload::Search => "search",
    }
}

async fn run(args: &Args, tmp: &Path) -> Result<ExitCode, String> {
    // 1. Server.
    let mut started = None;
    let url = match &args.server {
        Some(url) => remote::normalize_server_url(url).map_err(|e| e.to_string())?,
        None => {
            let bin = server::locate(args.server_bin.as_deref())?;
            let mut extra = args.server_args.clone();
            if args.cpu_profile.is_some() && !extra.iter().any(|a| a.starts_with("--pprof")) {
                extra.push("--pprof".into());
            }
            let s = server::Server::start(&bin, &tmp.join("server"), &extra)?;
            if !args.json {
                eprintln!(
                    "started {} at {} (data {})",
                    bin.display(),
                    s.url,
                    s.data_dir.display()
                );
            }
            let url = s.url.clone();
            started = Some(s);
            url
        }
    };

    // 2. Account.
    let login = account(args, &url).await?;
    if !args.json {
        eprintln!("bench user {} (tenant {})", login.username, login.tenant);
    }
    if args.policies > 0 {
        create_policies(&url, &login, args.policies).await?;
    }

    let run_id = format!("bench-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let work = tmp.join("work");
    std::fs::create_dir_all(&work).map_err(|e| format!("create {}: {e}", work.display()))?;
    let plan = Plan {
        server: url.clone(),
        tenant: login.tenant.clone(),
        token: login.token.clone(),
        work,
        run_id: run_id.clone(),
        concurrency: args.concurrency,
        iterations: args.iterations,
        files: args.files,
        file_size: args.file_size,
        changed: args.changed,
        query: args.query.clone(),
    };

    // 3. Optional CPU profile, captured from the start of the selected
    // workload (default: the first one).
    let profile_token = match (&args.cpu_profile, &started) {
        (None, _) => None,
        (Some(_), Some(s)) => Some(admin_token(&url, &s.admin_user, &s.admin_password).await?),
        (Some(_), None) => Some(login.token.clone()),
    };
    let profile_phase = args.profile_workload.unwrap_or(args.workload.0[0]);
    let mut profile = None;

    // 4. Workloads.
    let seed = format!("{run_id}-seed");
    let mut seeded = false;
    let mut rows = Vec::new();
    for &w in &args.workload.0 {
        if !args.json {
            eprintln!("running {} ...", workload_name(w));
        }
        if matches!(w, Workload::Clone | Workload::Search) && !seeded {
            workloads::seed(&plan, &seed).await?;
            seeded = true;
        }
        if let (Some(path), Some(token)) = (&args.cpu_profile, &profile_token) {
            if w == profile_phase && profile.is_none() {
                profile = Some(tokio::spawn(capture_profile(
                    url.clone(),
                    token.clone(),
                    args.profile_seconds,
                    path.clone(),
                )));
            }
        }
        let ops = match w {
            Workload::Push => workloads::push(&plan).await,
            Workload::Clone => workloads::clone(&plan, &seed).await,
            Workload::Search => workloads::search(&plan).await,
        };
        rows.extend(ops.iter().map(|op| op.samples.summarize(&op.name, op.wall)));
    }

    if let Some(task) = profile {
        match task.await {
            Ok(Ok(p)) => eprintln!("cpu profile written to {}", p.display()),
            Ok(Err(e)) => eprintln!("warning: cpu profile: {e}"),
            Err(e) => eprintln!("warning: cpu profile task: {e}"),
        }
    }

    // 5. Report.
    if args.json {
        let report = Report {
            server: &url,
            started_server: started.is_some(),
            workloads: args.workload.0.iter().map(|&w| workload_name(w)).collect(),
            concurrency: args.concurrency,
            iterations: args.iterations,
            files: args.files,
            file_size: args.file_size,
            changed: args.changed,
            policies: args.policies,
            results: &rows,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        println!(
            "\nworktree-bench: {} worker(s) x {} iteration(s), {} files x {} bytes, changed {}, policies {}\n",
            args.concurrency,
            args.iterations,
            args.files,
            args.file_size,
            if args.changed == 0 { "all".to_string() } else { args.changed.to_string() },
            args.policies
        );
        print!("{}", stats::table(&rows));
    }
    let failed = rows.iter().any(|r| r.errors > 0);
    if failed {
        if let Some(s) = &started {
            eprintln!("--- server log (tail) ---\n{}", s.log_tail());
        }
    }
    drop(started);
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Log in with --user/--password, or register a fresh user.
async fn account(args: &Args, url: &str) -> Result<LoginInfo, String> {
    if let (Some(user), Some(pw)) = (&args.user, &args.password) {
        return remote::login(url, user, pw)
            .await
            .map_err(|e| format!("login as {user}: {e}"));
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let username = format!("bench{}", &id[..10]);
    let password = format!("Pw-{id}-x9");
    remote::register(
        url,
        &username,
        &format!("{username}@example.com"),
        &password,
    )
    .await
    .map_err(|e| format!("register {username} (does the server allow registration?): {e}"))
}

/// A session token for `user` without touching the saved credentials.
async fn admin_token(url: &str, user: &str, password: &str) -> Result<String, String> {
    let anon = RestClient::new(url, None).map_err(|e| e.to_string())?;
    let v: serde_json::Value = anon
        .post(
            "/api/v1/auth/login",
            &serde_json::json!({"username": user, "password": password}),
        )
        .await
        .map_err(|e| format!("admin login: {e}"))?;
    v.get("token")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| "admin login returned no token".into())
}

/// GET /debug/pprof/profile?seconds=N into `path`.
async fn capture_profile(
    url: String,
    token: String,
    seconds: u32,
    path: PathBuf,
) -> Result<PathBuf, String> {
    let client = RestClient::new(&url, Some(token)).map_err(|e| e.to_string())?;
    let data = client
        .get_bytes_query("/debug/pprof/profile", &[("seconds", seconds.to_string())])
        .await
        .map_err(|e| {
            format!("{e} (is the server running with --pprof, and is the user a server admin?)")
        })?;
    tokio::fs::write(&path, data)
        .await
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// Create `n` enabled policies in the bench tenant that never match the
/// bench's own requests (they name other users and branches), so every
/// authorization has to evaluate them.
async fn create_policies(url: &str, login: &LoginInfo, n: u32) -> Result<(), String> {
    let client = RestClient::new(url, Some(login.token.clone())).map_err(|e| e.to_string())?;
    let path = format!(
        "/api/v1/tenants/{}/policies",
        remote::rest::seg(&login.tenant)
    );
    let tenant = &login.tenant;
    for i in 0..n {
        let body = serde_json::json!({
            "name": format!("bench-policy-{i:04}"),
            "statements": [
                {
                    "effect": "allow",
                    "actions": ["tree:read", "mr:*"],
                    "resources": [format!("wt:tree/{tenant}/other-{i}")],
                    "principals": [format!("user:someone-{i}")],
                },
                {
                    "effect": "deny",
                    "actions": ["branch:push"],
                    "resources": [format!("wt:branch/{tenant}/other-{i}/release")],
                    "principals": ["*"],
                },
            ],
        });
        let _: serde_json::Value = client
            .post(&path, &body)
            .await
            .map_err(|e| format!("create policy {i}: {e}"))?;
    }
    // Policy creation is not the measured load; give the server a moment.
    tokio::time::sleep(Duration::from_millis(50)).await;
    Ok(())
}
