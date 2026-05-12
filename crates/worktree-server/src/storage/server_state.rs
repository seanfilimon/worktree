use crate::error::ServerError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerState {
    pub trees: HashMap<String, ServerTreeState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerTreeState {
    pub id: String,
    pub branches: HashMap<String, ServerBranchState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerBranchState {
    pub name: String,
    pub tip: Option<String>,
}

pub struct ServerStateStore {
    state_file: PathBuf,
}

impl ServerStateStore {
    pub fn new(root: PathBuf) -> Self {
        Self {
            state_file: root.join("server_state.json"),
        }
    }

    pub fn load(&self) -> Result<ServerState, ServerError> {
        if !self.state_file.exists() {
            return Ok(ServerState::default());
        }
        let content = fs::read_to_string(&self.state_file)
            .map_err(|e| ServerError::Storage(format!("Failed to read server state: {}", e)))?;
        serde_json::from_str(&content)
            .map_err(|e| ServerError::Storage(format!("Failed to parse server state: {}", e)))
    }

    pub fn save(&self, state: &ServerState) -> Result<(), ServerError> {
        if let Some(parent) = self.state_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| ServerError::Storage(format!("Failed to create state dir: {}", e)))?;
        }
        let content = serde_json::to_string_pretty(state).map_err(|e| {
            ServerError::Storage(format!("Failed to serialize server state: {}", e))
        })?;
        fs::write(&self.state_file, content)
            .map_err(|e| ServerError::Storage(format!("Failed to write server state: {}", e)))
    }
}
