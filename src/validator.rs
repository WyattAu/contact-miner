use regex::Regex;
use std::sync::LazyLock;

/// Domains that never host real contacts: search engines, platforms whose
/// support emails leak onto every page, anti-bot vendors, asset filenames.
pub fn default_blacklist() -> Vec<String> {
    vec![
        "example.com".to_string(),
        "google.com".to_string(),
        "duckduckgo.com".to_string(),
        "github.com".to_string(),
        "github.io".to_string(),
        "localhost".to_string(),
        "test.com".to_string(),
        "domain.com".to_string(),
        "sentry.io".to_string(),
        "wixpress.com".to_string(),
        "techaro.lol".to_string(),
        "feedspot.com".to_string(),
        "nytimes.com".to_string(),
        "bbc.co.uk".to_string(),
        "bbc.com".to_string(),
        "linktr.ee".to_string(),
        "beacons.ai".to_string(),
        "instagram.com".to_string(),
        "tiktok.com".to_string(),
        "twitter.com".to_string(),
        "x.com".to_string(),
        "youtube.com".to_string(),
        "wix.com".to_string(),
        "squarespace.com".to_string(),
        "shopify.com".to_string(),
        "wordpress.com".to_string(),
        "namecheap.com".to_string(),
        "godaddy.com".to_string(),
        "cloudflare.com".to_string(),
    ]
}

// Justified: fixed, compile-time-known regex literals.
#[allow(clippy::expect_used)]
static EMAIL_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}"#).expect("Invalid email regex")
});

#[allow(clippy::expect_used)]
static EMAIL_REGEX_ANCHORED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$"#).expect("Invalid email regex")
});

#[derive(Debug, Clone)]
pub struct EmailValidator {
    blacklist: Vec<String>,
}

impl EmailValidator {
    pub fn new(blacklist: &[String]) -> Self {
        let mut full_blacklist = blacklist.to_vec();
        full_blacklist.extend(default_blacklist());
        full_blacklist.sort();
        full_blacklist.dedup();

        Self {
            blacklist: full_blacklist,
        }
    }

    pub fn validate(&self, email: &str) -> bool {
        // Check regex (anchored for full match validation)
        if !EMAIL_REGEX_ANCHORED.is_match(email) {
            return false;
        }

        // Extract domain
        if let Some((local, domain)) = email.split_once('@') {
            // Filter URL-encoded prefix artifacts
            if local.starts_with("u003") {
                return false;
            }
            // Anti-bot/telemetry vendor emails embed themselves in challenge
            // pages scraped from everywhere (wixpress = Wix forms, sentry/techaro
            // = Anubis challenges). Never valid creators.
            let dl = domain.to_lowercase();
            if dl.contains("wixpress") || dl.contains("sentry") || dl.contains("techaro") {
                return false;
            }
            // JS bundle / source-file junk ("getButtonText@desktop.ts")
            for ext in [".ts", ".tsx", ".js", ".mjs", ".map", ".json", ".css"] {
                if dl.ends_with(ext) {
                    return false;
                }
            }
            // Check blacklist
            if self
                .blacklist
                .iter()
                .any(|d| domain.eq_ignore_ascii_case(d))
            {
                return false;
            }

            // Check common false positives
            let false_positives = [".png", ".jpg", ".gif", ".js", ".css", ".svg"];
            if false_positives.iter().any(|ext| email.ends_with(ext)) {
                return false;
            }

            // Filter out university/corporate emails
            let tld = domain.split('.').next_back().unwrap_or("");
            if tld == "edu" || tld == "gov" || tld == "mil" {
                return false;
            }
            if domain.contains("edu.")
                || domain.contains(".edu")
                || domain.contains("university")
                || domain.contains("college")
                || domain.contains("onmicrosoft")
                || domain.ends_with(".ac.uk")
                || domain.ends_with(".gov.uk")
            {
                return false;
            }
        }

        true
    }

    pub fn extract_emails(&self, text: &str) -> Vec<String> {
        EMAIL_REGEX
            .find_iter(text)
            .map(|m| m.as_str().to_string())
            .filter(|email| self.validate(email))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_email() {
        let validator = EmailValidator::new(&[]);
        assert!(validator.validate("user@gmail.com"));
        assert!(validator.validate("test.name@domain.org"));
        assert!(validator.validate("user+tag@sub.domain.com"));
    }

    #[test]
    fn test_invalid_email() {
        let validator = EmailValidator::new(&[]);
        assert!(!validator.validate("invalid"));
        assert!(!validator.validate("@domain.com"));
        assert!(!validator.validate("user@"));
        assert!(!validator.validate("user@.com"));
    }

    #[test]
    fn test_blacklisted_domain() {
        let validator = EmailValidator::new(&[]);
        assert!(!validator.validate("user@example.com"));
        assert!(!validator.validate("user@google.com"));
        assert!(!validator.validate("user@duckduckgo.com"));
    }

    #[test]
    fn test_false_positives() {
        let validator = EmailValidator::new(&[]);
        assert!(!validator.validate("image.png"));
        assert!(!validator.validate("script.js"));
        assert!(!validator.validate("style.css"));
    }

    #[test]
    fn test_extract_emails() {
        let validator = EmailValidator::new(&[]);
        let text = "Contact us at info@gmail.com or support@example.com";
        let emails = validator.extract_emails(text);
        assert_eq!(emails.len(), 1); // Only info@gmail.com (example.com is blacklisted)
        assert_eq!(emails[0], "info@gmail.com");
    }

    #[test]
    fn test_custom_blacklist() {
        let validator = EmailValidator::new(&["custom.com".to_string()]);
        assert!(!validator.validate("user@custom.com"));
    }
}
