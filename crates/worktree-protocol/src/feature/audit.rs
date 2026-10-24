//! Audit logging for tracking all operations and access decisions.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::core::id::{AccountId, TenantId, TreeId, BranchId};

/// Type of audit event
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    AccessDecision,
    PolicyChange,
    TenantAccess,
    LicenseChange,
    LicenseCheck,
    BranchProtection,
    SnapshotSigned,
    SecretDetected,
    AuthEvent,
    SyncEvent,
    ConfigChange,
    MergeRequestEvent,
    TagEvent,
    ReleaseEvent,
}

/// Outcome of an audited action
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AuditOutcome {
    Success,
    Denied,
    Error,
}

/// A single audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: String,
    pub event_type: AuditEventType,
    pub outcome: AuditOutcome,
    pub actor: Option<AccountId>,
    pub tenant_id: Option<TenantId>,
    pub tree_id: Option<TreeId>,
    pub branch_id: Option<BranchId>,
    pub action: String,
    pub resource: String,
    pub details: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl AuditEntry {
    pub fn new(
        event_type: AuditEventType,
        outcome: AuditOutcome,
        action: &str,
        resource: &str,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type,
            outcome,
            actor: None,
            tenant_id: None,
            tree_id: None,
            branch_id: None,
            action: action.to_string(),
            resource: resource.to_string(),
            details: String::new(),
            ip_address: None,
            user_agent: None,
            timestamp: Utc::now(),
        }
    }

    pub fn with_actor(mut self, actor: AccountId) -> Self {
        self.actor = Some(actor);
        self
    }

    pub fn with_tenant(mut self, tenant_id: TenantId) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }

    pub fn with_tree(mut self, tree_id: TreeId) -> Self {
        self.tree_id = Some(tree_id);
        self
    }

    pub fn with_branch(mut self, branch_id: BranchId) -> Self {
        self.branch_id = Some(branch_id);
        self
    }

    pub fn with_details(mut self, details: &str) -> Self {
        self.details = details.to_string();
        self
    }

    pub fn with_ip(mut self, ip: &str) -> Self {
        self.ip_address = Some(ip.to_string());
        self
    }
}

/// Append-only audit log
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn new() -> Self { Self::default() }

    pub fn record(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    pub fn entries(&self) -> &[AuditEntry] { &self.entries }

    pub fn filter_by_type(&self, event_type: &AuditEventType) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.event_type == *event_type).collect()
    }

    pub fn filter_by_actor(&self, actor: &AccountId) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.actor.as_ref() == Some(actor)).collect()
    }

    pub fn filter_by_tenant(&self, tenant_id: &TenantId) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.tenant_id.as_ref() == Some(tenant_id)).collect()
    }

    pub fn denied_events(&self) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.outcome == AuditOutcome::Denied).collect()
    }

    pub fn since(&self, since: DateTime<Utc>) -> Vec<&AuditEntry> {
        self.entries.iter().filter(|e| e.timestamp >= since).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log() {
        let mut log = AuditLog::new();
        let actor = AccountId::new();

        log.record(
            AuditEntry::new(AuditEventType::AccessDecision, AuditOutcome::Success, "read", "tree/backend")
                .with_actor(actor)
        );
        log.record(
            AuditEntry::new(AuditEventType::AccessDecision, AuditOutcome::Denied, "write", "tree/backend")
                .with_actor(actor)
        );

        assert_eq!(log.len(), 2);
        assert_eq!(log.filter_by_actor(&actor).len(), 2);
        assert_eq!(log.denied_events().len(), 1);
    }
}
