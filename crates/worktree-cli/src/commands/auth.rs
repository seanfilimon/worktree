use super::AuthAction;
use crate::output::format;
use std::path::Path;
use std::process::Command;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: AuthAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        AuthAction::Login { token_id, secret } => {
            let server_url = std::env::var("WT_SERVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

            let url = format!("{}/login", server_url);

            let mut payload = serde_json::Map::new();
            if let Some(tid) = token_id {
                payload.insert("token_id".to_string(), serde_json::Value::String(tid));
            }
            payload.insert("secret".to_string(), serde_json::Value::String(secret));
            let body = serde_json::Value::Object(payload).to_string();

            format::print_info(&format!("Logging into {}...", server_url));

            let output = Command::new("curl")
                .arg("-s")
                .arg("-X")
                .arg("POST")
                .arg(&url)
                .arg("-H")
                .arg("Content-Type: application/json")
                .arg("-d")
                .arg(&body)
                .output();

            match output {
                Ok(out) if out.status.success() => {
                    let response_str = String::from_utf8_lossy(&out.stdout);
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_str) {
                        if let Some(token) = json.get("token").and_then(|t| t.as_str()) {
                            format::print_success("Login successful!");

                            #[cfg(unix)]
                            format::print_kv("Command", &format!("export WT_SERVER_AUTH_TOKEN={}", token));
                            #[cfg(windows)]
                            format::print_kv("Command", &format!("set WT_SERVER_AUTH_TOKEN={}", token));

                            if let Ok(engine) = WorktreeEngine::open(Path::new(".")) {
                                let auth_file = engine.wt_dir().join("cache").join("auth_token");
                                let _ = std::fs::create_dir_all(auth_file.parent().unwrap());
                                if let Ok(_) = std::fs::write(&auth_file, token) {
                                    format::print_info("Token also saved to local worktree cache.");
                                }
                            }
                        } else {
                            format::print_error("Failed to parse token from response.");
                            if let Some(err) = json.get("error") {
                                format::print_error(&format!("Server error: {}", err));
                            }
                        }
                    } else {
                        format::print_error("Invalid JSON response from server.");
                    }
                }
                Ok(out) => {
                    let response_str = String::from_utf8_lossy(&out.stdout);
                    format::print_error(&format!("Login failed with status: {}", out.status));
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response_str) {
                         if let Some(err) = json.get("error") {
                             format::print_error(&format!("Server error: {}", err));
                         }
                    } else {
                         format::print_error(&String::from_utf8_lossy(&out.stderr));
                    }
                }
                Err(e) => {
                    format::print_error(&format!("Failed to execute login request: {}", e));
                    format::print_info("Please ensure curl is installed or check your connection.");
                }
            }
        }
    }
    Ok(())
}
