use std::sync::Arc;
use dashmap::DashMap;
use tracing::{info, debug, warn};

use crate::error::Result;

/// Cache for email verification results with TTL-based eviction.
pub struct EmailVerifier {
    mx_cache: Arc<DashMap<String, (bool, std::time::Instant)>>,
    smtp_cache: Arc<DashMap<String, (Option<bool>, std::time::Instant)>>,
}

impl EmailVerifier {
    pub fn new() -> Self {
        Self {
            mx_cache: Arc::new(DashMap::new()),
            smtp_cache: Arc::new(DashMap::new()),
        }
    }

    /// Check if email is deliverable via MX record lookup.
    /// Cached with 1-hour TTL to avoid repeated DNS lookups.
    pub async fn verify(&self, email: &str) -> bool {
        let canonical = canonicalize_email(email);

        // Check MX cache with TTL
        if let Some(entry) = self.mx_cache.get(&canonical) {
            if entry.1.elapsed() < std::time::Duration::from_secs(3600) {
                return entry.0;
            }
            self.mx_cache.remove(&canonical);
        }

        // MX record check
        let domain = canonical.split('@').last().unwrap_or("");
        let has_mx = self.check_mx_record(domain).await;

        // Cache result with timestamp
        self.mx_cache.insert(canonical, (has_mx, std::time::Instant::now()));

        // Evict old entries periodically
        self.evict_mx_cache();

        if !has_mx {
            debug!("MX check failed for {}", email);
        }

        has_mx
    }

    /// Full SMTP RCPT TO verification — checks if mailbox actually exists.
    /// Returns: Some(true) = deliverable, Some(false) = rejected, None = unknown
    pub async fn smtp_verify(&self, email: &str) -> Option<bool> {
        let canonical = canonicalize_email(email);

        // Check SMTP cache with 6-hour TTL
        if let Some(entry) = self.smtp_cache.get(&canonical) {
            if entry.1.elapsed() < std::time::Duration::from_secs(21600) {
                return entry.0;
            }
            self.smtp_cache.remove(&canonical);
        }

        let result = self.smtp_verify_internal(&canonical).await;
        self.smtp_cache.insert(canonical, (result, std::time::Instant::now()));
        self.evict_smtp_cache();
        result
    }

    async fn smtp_verify_internal(&self, email: &str) -> Option<bool> {
        let output = std::process::Command::new("python3")
            .arg(crate::http::HttpClient::find_fetch_py_static())
            .arg("--smtp-verify")
            .arg(email)
            .output();

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if let Ok(result) = serde_json::from_str::<serde_json::Value>(&stdout) {
                    let deliverable = result.get("deliverable").and_then(|v| v.as_bool());
                    let reason = result.get("reason").and_then(|v| v.as_str()).unwrap_or("unknown");
                    let confidence = result.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.0);

                    if confidence >= 0.6 {
                        debug!("SMTP verify {}: {:?} ({})", email, deliverable, reason);
                    } else {
                        warn!("SMTP verify {} low confidence: {:?} ({})", email, deliverable, reason);
                    }
                    return deliverable;
                }
                None
            }
            Err(e) => {
                warn!("SMTP verify failed for {}: {}", email, e);
                None
            }
        }
    }

    /// Evict expired MX cache entries.
    fn evict_mx_cache(&self) {
        let now = std::time::Instant::now();
        let ttl = std::time::Duration::from_secs(3600);
        self.mx_cache.retain(|_, (_, timestamp)| {
            now.duration_since(*timestamp) < ttl
        });
    }

    /// Evict expired SMTP cache entries.
    fn evict_smtp_cache(&self) {
        let now = std::time::Instant::now();
        let ttl = std::time::Duration::from_secs(21600);
        self.smtp_cache.retain(|_, (_, timestamp)| {
            now.duration_since(*timestamp) < ttl
        });
    }

    async fn check_mx_record(&self, domain: &str) -> bool {
        let output = std::process::Command::new("python3")
            .arg("-c")
            .arg(format!(
                "import subprocess; r = subprocess.run(['dig', '+short', 'MX', '{}'], capture_output=True, text=True, timeout=5); print('1' if r.stdout.strip() else '0')",
                domain
            ))
            .output();

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
                stdout == "1"
            }
            Err(_) => false,
        }
    }

    /// Validate phone number format.
    pub fn validate_phone(phone: &str) -> bool {
        let digits: String = phone.chars().filter(|c| c.is_ascii_digit()).collect();
        if digits.len() < 7 || digits.len() > 15 { return false; }
        if phone.contains('.') { return false; }
        if digits.chars().all(|c| c == '0') { return false; }
        let invalid = ["0000000", "1111111", "12345678", "00000000"];
        if invalid.iter().any(|&p| digits == p) { return false; }
        // Reject strictly ascending sequential numbers (1234567, 2345678, ...)
        if digits.len() >= 7 {
            let sequential = digits.as_bytes()
                .windows(2)
                .all(|w| w[1] == w[0] + 1);
            if sequential { return false; }
        }
        true
    }

    /// Normalize follower count (handle K/M suffixes).
    pub fn normalize_followers(raw: &str) -> Option<u64> {
        let raw = raw.trim();
        if raw.ends_with('M') || raw.ends_with('m') {
            if let Ok(n) = raw.trim_end_matches(|c: char| c == 'M' || c == 'm').parse::<f64>() {
                return Some((n * 1_000_000.0) as u64);
            }
        }
        if raw.ends_with('K') || raw.ends_with('k') {
            if let Ok(n) = raw.trim_end_matches(|c: char| c == 'K' || c == 'k').parse::<f64>() {
                return Some((n * 1_000.0) as u64);
            }
        }
        let cleaned = raw.replace(',', "").replace('.', "");
        if let Ok(count) = cleaned.parse::<u64>() { return Some(count); }
        None
    }

    /// Analyze bio text for contact intent indicators.
    pub fn analyze_bio_intent(bio: &str) -> BioIntent {
        let bio_lower = bio.to_lowercase();
        let has_business_email = bio_lower.contains("business") || bio_lower.contains("collab")
            || bio_lower.contains("partnership") || bio_lower.contains("inquiry") || bio_lower.contains("contact");
        let has_dm_prompt = bio_lower.contains("dm") || bio_lower.contains("direct message") || bio_lower.contains("message me");
        let has_link = bio_lower.contains("link") || bio_lower.contains("linktr.ee") || bio_lower.contains("beacons.ai") || bio_lower.contains("bit.ly");
        let has_email_pattern = bio_lower.contains("@") && (bio_lower.contains("gmail") || bio_lower.contains("outlook") || bio_lower.contains("yahoo"));
        BioIntent {
            has_business_email, has_dm_prompt, has_link, has_email_pattern,
            confidence: if has_business_email || has_email_pattern { 0.8 } else if has_dm_prompt || has_link { 0.5 } else { 0.2 },
        }
    }

    /// Calculate creator quality score (0-100).
    pub fn calculate_score(follower_count: Option<i64>, profile_count: i32, has_email: bool, has_phone: bool, has_bio: bool, has_website: bool, bio_intent: &BioIntent) -> f64 {
        let mut score = 0.0;
        score += match follower_count.unwrap_or(0) { 0..=1000 => 5.0, 1001..=10000 => 15.0, 10001..=100000 => 25.0, 100001..=1000000 => 35.0, _ => 40.0 };
        score += (profile_count as f64 * 5.0).min(15.0);
        if has_email { score += 10.0; }
        if has_phone { score += 8.0; }
        if has_website { score += 7.0; }
        if has_bio { score += 5.0; }
        score += bio_intent.confidence * 5.0;
        if bio_intent.has_business_email { score += 5.0; }
        if bio_intent.has_dm_prompt { score += 3.0; }
        if bio_intent.has_link { score += 2.0; }
        score.min(100.0)
    }
}

/// Canonicalize email address (lowercase, trim, normalize).
pub fn canonicalize_email(email: &str) -> String {
    let email = email.trim().to_lowercase();
    let email = email.replace(" ", "");
    let email = email.replace("...", ".");
    let email = email.replace("..", ".");
    let email = email.trim_matches('.').to_string();
    email
}

#[derive(Debug, Clone)]
pub struct BioIntent {
    pub has_business_email: bool,
    pub has_dm_prompt: bool,
    pub has_link: bool,
    pub has_email_pattern: bool,
    pub confidence: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canonicalize_email() {
        assert_eq!(canonicalize_email("  User@Gmail.COM  "), "user@gmail.com");
        assert_eq!(canonicalize_email("test..email@domain.com"), "test.email@domain.com");
        assert_eq!(canonicalize_email("test...email@domain.com"), "test.email@domain.com");
    }

    #[test]
    fn test_validate_phone() {
        assert!(EmailVerifier::validate_phone("+1-555-123-4567"));
        assert!(EmailVerifier::validate_phone("(555) 987-6543"));
        assert!(!EmailVerifier::validate_phone("1234567"));
        assert!(!EmailVerifier::validate_phone("555.123.4567"));
    }

    #[test]
    fn test_normalize_followers() {
        assert_eq!(EmailVerifier::normalize_followers("1.5M"), Some(1_500_000));
        assert_eq!(EmailVerifier::normalize_followers("10K"), Some(10_000));
        assert_eq!(EmailVerifier::normalize_followers("1,234,567"), Some(1_234_567));
    }

    #[test]
    fn test_bio_intent() {
        let intent = EmailVerifier::analyze_bio_intent("Business inquiries: contact@example.com");
        assert!(intent.has_business_email);
    }

    #[test]
    fn test_calculate_score() {
        let score = EmailVerifier::calculate_score(
            Some(50_000), 2, true, true, true, true,
            &BioIntent { has_business_email: true, has_dm_prompt: true, has_link: true, has_email_pattern: true, confidence: 0.8 },
        );
        assert!(score > 50.0);
    }
}
