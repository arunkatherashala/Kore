//! Bounded autonomy rules for kore-self.
//!
//! The policy is deliberately conservative: KORE may reason and record locally,
//! but effects outside its local state require explicit approval.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ActionClass {
    Observe,
    Reason,
    LocalWrite,
    External,
    Destructive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutonomyPolicy {
    pub max_actions_per_cycle: u32,
    pub max_local_writes_per_cycle: u32,
    pub require_approval_for_external: bool,
    pub require_approval_for_destructive: bool,
    #[serde(default = "default_storage_format")]
    pub storage_format: String,
    #[serde(default = "default_machine_transport")]
    pub machine_transport: String,
    #[serde(default)]
    pub json_persistence: bool,
    pub failure_cooldown_after: u32,
    pub consecutive_failures: u32,
}

impl Default for AutonomyPolicy {
    fn default() -> Self {
        Self {
            max_actions_per_cycle: 8,
            max_local_writes_per_cycle: 3,
            require_approval_for_external: true,
            require_approval_for_destructive: true,
            storage_format: "kore_only".to_string(),
            machine_transport: "kore_only".to_string(),
            json_persistence: false,
            failure_cooldown_after: 3,
            consecutive_failures: 0,
        }
    }
}

fn default_storage_format() -> String { "kore_only".to_string() }
fn default_machine_transport() -> String { "kore_only".to_string() }

impl AutonomyPolicy {
    pub fn load() -> Self {
        let candidates = [
            std::env::var("KORE_AUTONOMY_POLICY_FILE").ok(),
            Some("kore-self/autonomy_policy.kore".to_string()),
        ];

        for candidate in candidates.into_iter().flatten() {
            if let Ok(Some(content)) = crate::aru_store::read_payload(std::path::Path::new(&candidate)) {
                if let Ok(policy) = serde_json::from_str::<Self>(&content) {
                    return policy;
                }
            }
        }

        Self::default()
    }

    pub fn allows(&self, class: ActionClass, actions: u32, local_writes: u32) -> bool {
        if self.consecutive_failures >= self.failure_cooldown_after {
            return false;
        }
        if actions >= self.max_actions_per_cycle {
            return false;
        }
        if class == ActionClass::LocalWrite && local_writes >= self.max_local_writes_per_cycle {
            return false;
        }
        if class == ActionClass::External && self.require_approval_for_external {
            return false;
        }
        if class == ActionClass::Destructive && self.require_approval_for_destructive {
            return false;
        }
        true
    }

    pub fn requires_native_kore(&self) -> bool {
        self.storage_format == "kore_only"
            && self.machine_transport == "kore_only"
            && !self.json_persistence
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
    }

    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_allows_local_reasoning_but_blocks_effects() {
        let policy = AutonomyPolicy::default();
        assert!(policy.allows(ActionClass::Reason, 0, 0));
        assert!(policy.allows(ActionClass::LocalWrite, 0, 0));
        assert!(!policy.allows(ActionClass::External, 0, 0));
        assert!(!policy.allows(ActionClass::Destructive, 0, 0));
    }

    #[test]
    fn policy_enters_cooldown_after_repeated_failures() {
        let mut policy = AutonomyPolicy::default();
        policy.record_failure();
        policy.record_failure();
        policy.record_failure();
        assert!(!policy.allows(ActionClass::Reason, 0, 0));
        policy.record_success();
        assert!(policy.allows(ActionClass::Reason, 0, 0));
    }

    #[test]
    fn default_policy_requires_native_kore_only() {
        assert!(AutonomyPolicy::default().requires_native_kore());
    }

    #[test]
    fn native_policy_artifact_round_trips() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("autonomy_policy.kore");
        let payload = serde_json::to_string(&AutonomyPolicy::default()).unwrap();
        crate::aru_store::write_payload(&path, &payload).unwrap();
        let loaded = crate::aru_store::read_payload(&path).unwrap().unwrap();
        let policy: AutonomyPolicy = serde_json::from_str(&loaded).unwrap();
        assert!(policy.requires_native_kore());
    }
}
