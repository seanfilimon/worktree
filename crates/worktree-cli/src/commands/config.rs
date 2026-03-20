use super::ConfigAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: ConfigAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ConfigAction::Show => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let config_content = worktree_sdk::engine::config::read_config(&engine)?;
            format::print_header("Worktree Configuration");
            println!();
            println!("{}", config_content);
            Ok(())
        }
        ConfigAction::Get { key } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let config_content = worktree_sdk::engine::config::read_config(&engine)?;

            // Parse the TOML and look up the key (supports dotted keys like "sync.auto")
            let table: toml::Table = config_content.parse().map_err(|e: toml::de::Error| {
                Box::<dyn std::error::Error>::from(format!("Failed to parse config: {}", e))
            })?;

            let value = resolve_toml_key(&table, &key);
            match value {
                Some(v) => {
                    format::print_kv(&key, &format_toml_value(&v));
                }
                None => {
                    format::print_warning(&format!("Key '{}' not found in configuration", key));
                }
            }
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let config_content = worktree_sdk::engine::config::read_config(&engine)?;

            let mut table: toml::Table = config_content.parse().map_err(|e: toml::de::Error| {
                Box::<dyn std::error::Error>::from(format!("Failed to parse config: {}", e))
            })?;

            set_toml_key(&mut table, &key, &value)?;

            let new_content = toml::to_string_pretty(&table).map_err(|e| {
                Box::<dyn std::error::Error>::from(format!("Failed to serialize config: {}", e))
            })?;

            let config_path = engine.wt_dir().join("config.toml");
            std::fs::write(&config_path, new_content)?;

            format::print_success(&format!("Set '{}' = '{}'", key, value));
            Ok(())
        }
    }
}

fn resolve_toml_key<'a>(table: &'a toml::Table, key: &str) -> Option<toml::Value> {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current: &toml::Value = &toml::Value::Table(table.clone());

    for part in &parts {
        match current {
            toml::Value::Table(t) => {
                current = t.get(*part)?;
            }
            _ => return None,
        }
    }

    Some(current.clone())
}

fn set_toml_key(table: &mut toml::Table, key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = key.split('.').collect();
    if parts.is_empty() {
        return Err("empty key".into());
    }

    let mut current = table;
    for part in &parts[..parts.len() - 1] {
        let entry = current
            .entry(part.to_string())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        match entry {
            toml::Value::Table(t) => current = t,
            _ => return Err(format!("key component '{}' is not a table", part).into()),
        }
    }

    let last_key = parts[parts.len() - 1];

    // Try to preserve the type of the existing value
    let parsed_value = if let Some(existing) = current.get(last_key) {
        match existing {
            toml::Value::Boolean(_) => {
                match value.to_lowercase().as_str() {
                    "true" | "1" | "yes" => toml::Value::Boolean(true),
                    "false" | "0" | "no" => toml::Value::Boolean(false),
                    _ => toml::Value::String(value.to_string()),
                }
            }
            toml::Value::Integer(_) => {
                if let Ok(i) = value.parse::<i64>() {
                    toml::Value::Integer(i)
                } else {
                    toml::Value::String(value.to_string())
                }
            }
            toml::Value::Float(_) => {
                if let Ok(f) = value.parse::<f64>() {
                    toml::Value::Float(f)
                } else {
                    toml::Value::String(value.to_string())
                }
            }
            _ => toml::Value::String(value.to_string()),
        }
    } else {
        // Guess the type from the value string
        if value == "true" || value == "false" {
            toml::Value::Boolean(value == "true")
        } else if let Ok(i) = value.parse::<i64>() {
            toml::Value::Integer(i)
        } else if let Ok(f) = value.parse::<f64>() {
            toml::Value::Float(f)
        } else {
            toml::Value::String(value.to_string())
        }
    };

    current.insert(last_key.to_string(), parsed_value);
    Ok(())
}

fn format_toml_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Integer(i) => i.to_string(),
        toml::Value::Float(f) => f.to_string(),
        toml::Value::Boolean(b) => b.to_string(),
        toml::Value::Datetime(d) => d.to_string(),
        toml::Value::Array(a) => {
            let items: Vec<String> = a.iter().map(format_toml_value).collect();
            format!("[{}]", items.join(", "))
        }
        toml::Value::Table(t) => {
            let items: Vec<String> = t
                .iter()
                .map(|(k, v)| format!("{} = {}", k, format_toml_value(v)))
                .collect();
            format!("{{ {} }}", items.join(", "))
        }
    }
}
