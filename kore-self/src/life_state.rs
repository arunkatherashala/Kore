//! Persistent artificial-life vitality state for Aru.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifeStatus {
    Initializing,
    Alive,
    Paused,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LifeState {
    pub birth_timestamp: String,
    pub last_pulse_timestamp: String,
    pub pulse_count: u64,
    pub continuity_count: u64,
    pub maintenance_count: u64,
    pub status: LifeStatus,
    #[serde(default)]
    pub last_health_issue: Option<String>,
}

impl LifeState {
    pub fn new(now: &str) -> Self {
        Self {
            birth_timestamp: now.to_string(),
            last_pulse_timestamp: now.to_string(),
            pulse_count: 0,
            continuity_count: 0,
            maintenance_count: 0,
            status: LifeStatus::Initializing,
            last_health_issue: None,
        }
    }

    pub fn pulse(&mut self, now: &str, memory_count: usize, has_identity: bool) {
        let healthy_before_pulse = self.health_check().is_ok();
        self.pulse_count += 1;
        self.last_pulse_timestamp = now.to_string();
        if has_identity && memory_count > 0 && healthy_before_pulse {
            self.continuity_count += 1;
            self.status = LifeStatus::Alive;
        } else {
            self.status = LifeStatus::Degraded;
        }
        self.maintenance_count += 1;
    }

    pub fn health_check(&mut self) -> Result<(), String> {
        let issue = if self.birth_timestamp.trim().is_empty() {
            Some("birth timestamp is missing")
        } else if self.last_pulse_timestamp.trim().is_empty() {
            Some("last pulse timestamp is missing")
        } else if self.continuity_count > self.pulse_count {
            Some("continuity count exceeds pulse count")
        } else if self.maintenance_count < self.pulse_count {
            Some("maintenance count is behind pulse count")
        } else {
            None
        };
        self.last_health_issue = issue.map(str::to_string);
        match &self.last_health_issue {
            Some(issue) => {
                self.status = LifeStatus::Degraded;
                Err(issue.clone())
            }
            None => Ok(()),
        }
    }

    pub fn pause(&mut self, now: &str) {
        self.last_pulse_timestamp = now.to_string();
        self.status = LifeStatus::Paused;
        self.last_health_issue = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn life_pulse_establishes_continuity() {
        let mut life = LifeState::new("t0");
        life.pulse("t1", 3, true);
        assert_eq!(life.status, LifeStatus::Alive);
        assert_eq!(life.pulse_count, 1);
        assert_eq!(life.continuity_count, 1);
        assert_eq!(life.maintenance_count, 1);
    }

    #[test]
    fn life_without_identity_is_degraded() {
        let mut life = LifeState::new("t0");
        life.pulse("t1", 0, false);
        assert_eq!(life.status, LifeStatus::Degraded);
        assert_eq!(life.continuity_count, 0);
    }

    #[test]
    fn invalid_continuity_is_reported_as_degraded() {
        let mut life = LifeState::new("t0");
        life.continuity_count = 2;
        life.pulse_count = 1;
        assert!(life.health_check().is_err());
        assert_eq!(life.status, LifeStatus::Degraded);
        assert!(life.last_health_issue.is_some());
    }
}
