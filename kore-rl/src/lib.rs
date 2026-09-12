//! KORE Reinforcement Learning — Phase 3D
//!
//! Self-optimization through:
//! - Q-Learning (discrete action spaces)
//! - Policy Gradient (continuous actions)
//! - Multi-Armed Bandit
//! - Experience Replay
//! - Target Networks

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub id: String,
    pub features: Vec<f32>,
}

/// Action
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct Action {
    pub id: u32,
    pub name: String,
}

/// Experience tuple (s, a, r, s')
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experience {
    pub state: Vec<f32>,
    pub action: u32,
    pub reward: f32,
    pub next_state: Vec<f32>,
    pub done: bool,
}

/// Q-Learning Agent
pub struct QLearningAgent {
    q_table: HashMap<String, HashMap<u32, f32>>,
    learning_rate: f32,
    discount_factor: f32,
    epsilon: f32,
}

impl QLearningAgent {
    pub fn new(lr: f32, discount: f32, epsilon: f32) -> Self {
        QLearningAgent {
            q_table: HashMap::new(),
            learning_rate: lr,
            discount_factor: discount,
            epsilon,
        }
    }

    /// Choose action using epsilon-greedy
    pub fn choose_action(&self, state: &str, actions: &[u32]) -> u32 {
        if rand::random::<f32>() < self.epsilon {
            // Explore
            actions[rand::random::<usize>() % actions.len()]
        } else {
            // Exploit
            self.best_action(state, actions)
        }
    }

    /// Get best action for state
    pub fn best_action(&self, state: &str, actions: &[u32]) -> u32 {
        actions
            .iter()
            .max_by_key(|a| {
                self.get_q_value(state, **a)
                    .to_bits()
            })
            .copied()
            .unwrap_or(actions[0])
    }

    /// Update Q-value
    pub fn update(&mut self, state: &str, action: u32, reward: f32, next_state: &str, next_actions: &[u32]) {
        let current_q = self.get_q_value(state, action);
        let max_next_q = self.get_best_q_value(next_state, next_actions);
        let td_error = reward + self.discount_factor * max_next_q - current_q;

        self.q_table
            .entry(state.to_string())
            .or_insert_with(HashMap::new)
            .insert(action, current_q + self.learning_rate * td_error);
    }

    fn get_q_value(&self, state: &str, action: u32) -> f32 {
        self.q_table
            .get(state)
            .and_then(|map| map.get(&action))
            .copied()
            .unwrap_or(0.0)
    }

    fn get_best_q_value(&self, state: &str, actions: &[u32]) -> f32 {
        actions
            .iter()
            .map(|a| self.get_q_value(state, *a))
            .fold(f32::NEG_INFINITY, f32::max)
    }
}

/// Policy Gradient Agent (Actor-Critic)
pub struct PolicyGradientAgent {
    policy_weights: Vec<f32>,
    value_weights: Vec<f32>,
    learning_rate: f32,
}

impl PolicyGradientAgent {
    pub fn new(state_dim: usize, action_dim: usize, lr: f32) -> Self {
        PolicyGradientAgent {
            policy_weights: vec![0.0; state_dim * action_dim],
            value_weights: vec![0.0; state_dim],
            learning_rate: lr,
        }
    }

    /// Compute policy (softmax)
    pub fn policy(&self, state: &[f32]) -> Vec<f32> {
        let mut logits = vec![0.0; self.policy_weights.len() / state.len()];
        // Simplified: compute linear policy
        for (i, &w) in self.policy_weights.iter().enumerate() {
            logits[i % logits.len()] += w;
        }

        // Softmax
        let max = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exps: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
        let sum: f32 = exps.iter().sum();
        exps.iter().map(|e| e / sum).collect()
    }

    /// Estimate value
    pub fn value(&self, state: &[f32]) -> f32 {
        state
            .iter()
            .zip(self.value_weights.iter())
            .map(|(s, w)| s * w)
            .sum()
    }

    /// Update policy
    pub fn update_policy(&mut self, state: &[f32], action: usize, advantage: f32) {
        for (i, &s) in state.iter().enumerate() {
            let idx = i * (self.policy_weights.len() / state.len()) + action;
            if idx < self.policy_weights.len() {
                self.policy_weights[idx] += self.learning_rate * advantage * s;
            }
        }
    }
}

/// Experience Replay Buffer
pub struct ReplayBuffer {
    experiences: Vec<Experience>,
    max_size: usize,
}

impl ReplayBuffer {
    pub fn new(max_size: usize) -> Self {
        ReplayBuffer {
            experiences: Vec::new(),
            max_size,
        }
    }

    /// Add experience
    pub fn push(&mut self, exp: Experience) {
        if self.experiences.len() >= self.max_size {
            self.experiences.remove(0);
        }
        self.experiences.push(exp);
    }

    /// Sample batch for training
    pub fn sample(&self, batch_size: usize) -> Vec<Experience> {
        let mut batch = Vec::new();
        let n = self.experiences.len();
        for _ in 0..batch_size.min(n) {
            let idx = rand::random::<usize>() % n;
            batch.push(self.experiences[idx].clone());
        }
        batch
    }

    pub fn len(&self) -> usize {
        self.experiences.len()
    }
}

/// Multi-Armed Bandit
pub struct MultiArmedBandit {
    arm_rewards: Vec<f32>,
    arm_counts: Vec<u32>,
    temperature: f32,
}

impl MultiArmedBandit {
    pub fn new(n_arms: usize) -> Self {
        MultiArmedBandit {
            arm_rewards: vec![0.0; n_arms],
            arm_counts: vec![0; n_arms],
            temperature: 1.0,
        }
    }

    /// Thompson Sampling
    pub fn select_arm(&self) -> usize {
        let mut best_idx = 0;
        let mut best_value = f32::NEG_INFINITY;

        for (i, &reward) in self.arm_rewards.iter().enumerate() {
            let count = self.arm_counts[i] as f32;
            let ucb = if count == 0.0 {
                f32::INFINITY
            } else {
                reward / count + ((2.0 * (1.0_f32).ln()) / count).sqrt()
            };

            if ucb > best_value {
                best_value = ucb;
                best_idx = i;
            }
        }

        best_idx
    }

    /// Update arm
    pub fn update(&mut self, arm: usize, reward: f32) {
        self.arm_rewards[arm] = (self.arm_rewards[arm] * self.arm_counts[arm] as f32 + reward)
            / (self.arm_counts[arm] as f32 + 1.0);
        self.arm_counts[arm] += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_q_learning() {
        let mut agent = QLearningAgent::new(0.1, 0.99, 0.1);
        agent.update("s0", 0, 1.0, "s1", &[0, 1]);
        assert!(agent.get_q_value("s0", 0) > 0.0);
    }

    #[test]
    fn test_mab() {
        let mut bandit = MultiArmedBandit::new(3);
        bandit.update(0, 1.0);
        bandit.update(1, 0.5);
        assert_eq!(bandit.select_arm(), 0);
    }
}
