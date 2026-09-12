//! Rate limiting module for KORE API v2.0
//!
//! Provides per-user and per-IP rate limiting using token bucket algorithm

use governor::{Quota, RateLimiter};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;

/// Rate limiter for API endpoints
pub struct RateLimitManager {
    // Per-user limiters
    user_limiters: Mutex<HashMap<String, RateLimiter>>,
    // Per-IP limiters
    ip_limiters: Mutex<HashMap<String, RateLimiter>>,
    // Default quota: 1000 requests per second
    default_quota: Quota,
}

impl RateLimitManager {
    /// Create new rate limit manager
    pub fn new() -> Self {
        Self {
            user_limiters: Mutex::new(HashMap::new()),
            ip_limiters: Mutex::new(HashMap::new()),
            default_quota: Quota::per_second(
                std::num::NonZeroU32::new(1000).unwrap(),
            ),
        }
    }

    /// Create with custom quota (requests per second)
    pub fn with_quota(requests_per_second: u32) -> Self {
        Self {
            user_limiters: Mutex::new(HashMap::new()),
            ip_limiters: Mutex::new(HashMap::new()),
            default_quota: Quota::per_second(
                std::num::NonZeroU32::new(requests_per_second).unwrap_or(std::num::NonZeroU32::new(1000).unwrap()),
            ),
        }
    }

    /// Check if user can make request
    pub fn check_user_limit(&self, username: &str) -> bool {
        let mut limiters = self.user_limiters.lock().unwrap();
        let limiter = limiters
            .entry(username.to_string())
            .or_insert_with(|| RateLimiter::direct(self.default_quota));

        limiter.check().is_ok()
    }

    /// Check if IP can make request
    pub fn check_ip_limit(&self, ip: &str) -> bool {
        let mut limiters = self.ip_limiters.lock().unwrap();
        let limiter = limiters
            .entry(ip.to_string())
            .or_insert_with(|| RateLimiter::direct(self.default_quota));

        limiter.check().is_ok()
    }
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_rate_limit() {
        let limiter = RateLimitManager::with_quota(10);

        // First 10 requests should pass
        for _ in 0..10 {
            assert!(limiter.check_user_limit("user1"));
        }

        // 11th request should fail
        assert!(!limiter.check_user_limit("user1"));
    }

    #[test]
    fn test_ip_rate_limit() {
        let limiter = RateLimitManager::with_quota(5);

        // First 5 requests should pass
        for _ in 0..5 {
            assert!(limiter.check_ip_limit("192.168.1.1"));
        }

        // 6th request should fail
        assert!(!limiter.check_ip_limit("192.168.1.1"));
    }

    #[test]
    fn test_different_users_separate_limits() {
        let limiter = RateLimitManager::with_quota(3);

        // User1 makes 3 requests
        for _ in 0..3 {
            assert!(limiter.check_user_limit("user1"));
        }
        assert!(!limiter.check_user_limit("user1"));

        // User2 should get their own limit
        assert!(limiter.check_user_limit("user2"));
    }
}
