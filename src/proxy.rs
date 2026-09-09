use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use rand::Rng as _;

#[derive(Debug, Clone)]
pub struct ProxyPool {
    proxies: Vec<String>,
    health_scores: Arc<DashMap<String, f64>>,
    failure_counts: Arc<DashMap<String, AtomicUsize>>,
    success_counts: Arc<DashMap<String, AtomicUsize>>,
    quarantined: Arc<DashMap<String, bool>>,
}

impl ProxyPool {
    pub fn new(proxies: &[String]) -> Self {
        let health_scores = Arc::new(DashMap::new());
        let failure_counts = Arc::new(DashMap::new());
        let success_counts = Arc::new(DashMap::new());
        let quarantined = Arc::new(DashMap::new());

        for proxy in proxies {
            health_scores.insert(proxy.clone(), 1.0);
            failure_counts.insert(proxy.clone(), AtomicUsize::new(0));
            success_counts.insert(proxy.clone(), AtomicUsize::new(0));
            quarantined.insert(proxy.clone(), false);
        }

        Self {
            proxies: proxies.to_vec(),
            health_scores,
            failure_counts,
            success_counts,
            quarantined,
        }
    }

    pub fn select_proxy(&self) -> Option<String> {
        let mut rng = rand::thread_rng();

        // Filter out quarantined proxies
        let available: Vec<&String> = self
            .proxies
            .iter()
            .filter(|p| !self.quarantined.get(*p).map(|q| *q).unwrap_or(false))
            .collect();

        if available.is_empty() {
            // If all quarantined, reset quarantine
            for proxy in &self.proxies {
                self.quarantined.insert(proxy.clone(), false);
            }
            return self.proxies.first().cloned();
        }

        // Weight by health score
        let total_health: f64 = available
            .iter()
            .filter_map(|p| self.health_scores.get(*p))
            .map(|h| *h)
            .sum();

        if total_health <= 0.0 {
            return available.first().cloned().cloned();
        }

        let r: f64 = rng.gen_range(0.0..total_health);
        let mut cumulative = 0.0;

        for proxy in &available {
            if let Some(health) = self.health_scores.get(*proxy) {
                cumulative += *health;
                if r <= cumulative {
                    return Some((*proxy).clone());
                }
            }
        }

        available.first().cloned().cloned()
    }

    pub fn record_success(&self, proxy: &str) {
        if let Some(count) = self.success_counts.get(proxy) {
            count.fetch_add(1, Ordering::SeqCst);
        }
        if let Some(count) = self.failure_counts.get(proxy) {
            count.store(0, Ordering::SeqCst);
        }
        self.update_health(proxy);
    }

    pub fn record_failure(&self, proxy: &str) {
        if let Some(count) = self.failure_counts.get(proxy) {
            let failures = count.fetch_add(1, Ordering::SeqCst) + 1;
            if failures >= 3 {
                self.quarantined.insert(proxy.to_string(), true);
            }
        }
        self.update_health(proxy);
    }

    fn update_health(&self, proxy: &str) {
        let successes = self
            .success_counts
            .get(proxy)
            .map(|c| c.load(Ordering::SeqCst) as f64)
            .unwrap_or(0.0);
        let failures = self
            .failure_counts
            .get(proxy)
            .map(|c| c.load(Ordering::SeqCst) as f64)
            .unwrap_or(0.0);

        let total = successes + failures;
        let health = if total > 0.0 { successes / total } else { 1.0 };

        self.health_scores.insert(proxy.to_string(), health);
    }

    pub fn get_stats(&self) -> PoolStats {
        let total = self.proxies.len();
        let healthy = self.quarantined.iter().filter(|q| !*q.value()).count();
        let quarantined = total - healthy;

        PoolStats {
            total,
            healthy,
            quarantined,
        }
    }

    /// Get all proxy URLs for rotation.
    pub fn get_proxy_urls(&self) -> Vec<String> {
        self.proxies.clone()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolStats {
    pub total: usize,
    pub healthy: usize,
    pub quarantined: usize,
}
