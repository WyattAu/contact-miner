use regex::Regex;
use tracing::debug;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct HtmlParser {
    email_regex: Regex,
    phone_regex: Regex,
    url_regex: Regex,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct ContactInfo {
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub social_links: Vec<String>,
    pub website: Option<String>,
    pub bio: Option<String>,
    pub display_name: Option<String>,
    pub follower_count: Option<i64>,
}

impl HtmlParser {
    pub fn new() -> Self {
        Self {
            email_regex: Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            phone_regex: Regex::new(r"(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{3,4}").unwrap(),
            url_regex: Regex::new(r#"href="(https?[^"]+)""#).unwrap(),
        }
    }

    pub fn parse_search_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for cap in self.url_regex.find_iter(html) {
            let url = cap.as_str().trim_start_matches("href=\"").trim_end_matches('"').to_string();
            if url.contains("ecosia.org") || url.contains("bing.com") || url.contains("google.com")
                || url.ends_with(".css") || url.ends_with(".js") || url.ends_with(".png") || url.ends_with(".jpg") {
                continue;
            }

            let profile_url = self.normalize_to_profile_url(&url);
            if profile_url.is_empty() || seen.contains(&profile_url) { continue; }
            seen.insert(profile_url.clone());

            let title = extract_nearby_title(html, &url);
            let snippet = extract_nearby_snippet(html, &url);
            results.push(SearchResult { title, url: profile_url, snippet });
        }

        debug!("Parsed {} unique profile results", results.len());
        Ok(results)
    }

    fn normalize_to_profile_url(&self, url: &str) -> String {
        if url.contains("instagram.com/") {
            if let Some(username) = extract_ig_username(url) {
                return format!("https://www.instagram.com/{}/", username);
            }
            return String::new();
        }
        if url.contains("tiktok.com/@") {
            return url.split('?').next().unwrap_or(&url).to_string();
        }
        if url.contains("youtube.com/@") || url.contains("youtube.com/channel/") {
            let base = url.split('?').next().unwrap_or(&url);
            return format!("{}/about", base.trim_end_matches('/'));
        }
        if url.contains("twitter.com/") || url.contains("x.com/") {
            return url.split('?').next().unwrap_or(&url).to_string();
        }
        if url.contains("linktr.ee/") || url.contains("beacons.ai/") {
            return url.split('?').next().unwrap_or(&url).to_string();
        }
        String::new()
    }

    pub fn extract_contact_info(&self, html: &str) -> ContactInfo {
        ContactInfo {
            emails: self.extract_emails(html),
            phones: self.extract_phones(html),
            social_links: self.extract_social_links(html),
            website: self.extract_website(html),
            bio: extract_meta_content(html, "og:description")
                .or_else(|| extract_meta_content(html, "description")),
            display_name: extract_meta_content(html, "og:title"),
            follower_count: extract_follower_count(html).map(|f| f as i64),
        }
    }

    pub fn extract_emails(&self, text: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();

        // Standard form (also catches mailto: links, since we scan raw text)
        for m in self.email_regex.find_iter(text) {
            let e = m.as_str().to_string();
            if is_valid_email(&e) && !found.contains(&e) {
                found.push(e);
            }
        }

        // Obfuscated forms: "name [at] domain [dot] com", "name (at) domain dot com"
        if found.len() < 3 {
            if let Ok(re) = Regex::new(
                r"(?i)([a-zA-Z0-9._%+-]+)\s*[\[\(]?\s*at\s*[\]\)]?\s*([a-zA-Z0-9.-]+)\s*[\[\(]?\s*dot\s*[\]\)]?\s*([a-zA-Z]{2,})",
            ) {
                for cap in re.captures_iter(text) {
                    let candidate = format!(
                        "{}@{}.{}",
                        cap.get(1).map(|m| m.as_str()).unwrap_or(""),
                        cap.get(2).map(|m| m.as_str()).unwrap_or(""),
                        cap.get(3).map(|m| m.as_str()).unwrap_or("")
                    );
                    if is_valid_email(&candidate) && !found.contains(&candidate) {
                        found.push(candidate);
                    }
                }
            }
        }

        found.into_iter().map(|e| canonicalize_email(&e)).collect()
    }

    pub fn extract_phones(&self, text: &str) -> Vec<String> {
        self.phone_regex.find_iter(text)
            .map(|m| m.as_str().trim().to_string())
            .filter(|p| {
                let digits: String = p.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() < 7 || digits.len() > 15 { return false; }
                if p.contains('.') { return false; }
                // Unix timestamps in ms (13 digits, 17xxxxx era) were the main
                // source of garbage "phones" — reject them explicitly.
                if digits.len() == 13 && digits.starts_with("17") { return false; }
                if digits.chars().all(|c| c == digits.chars().next().unwrap_or('0')) { return false; }
                true
            }).collect()
    }

    pub fn extract_social_links(&self, html: &str) -> Vec<String> {
        let patterns = [
            r"https?://(?:www\.)?instagram\.com/[a-zA-Z0-9_.]+/?",
            r"https?://(?:www\.)?tiktok\.com/@[a-zA-Z0-9_.]+",
            r"https?://(?:www\.)?twitter\.com/[a-zA-Z0-9_]+",
            r"https?://(?:www\.)?x\.com/[a-zA-Z0-9_]+",
            r"https?://(?:www\.)?youtube\.com/@[a-zA-Z0-9_-]+",
            r"https?://(?:www\.)?youtube\.com/channel/[a-zA-Z0-9_-]+",
            r"https?://(?:www\.)?linkedin\.com/in/[a-zA-Z0-9_-]+",
            r"https?://linktr\.ee/[a-zA-Z0-9_.]+",
            r"https?://beacons\.ai/[a-zA-Z0-9_.]+",
        ];
        let mut links = Vec::new();
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                for cap in re.find_iter(html) {
                    let link = cap.as_str().to_string();
                    if !links.contains(&link) { links.push(link); }
                }
            }
        }
        links
    }

    fn extract_website(&self, html: &str) -> Option<String> {
        let patterns = [r#""website"\s*:\s*"(https?[^"]+)""#, r#"href="(https?://[^"]*(?:\.com|\.org|\.net|\.io)[^"]*)""#];
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(html) {
                    if let Some(url) = caps.get(1) {
                        let url = url.as_str().to_string();
                        if !url.contains("ecosia") && !url.contains("bing") && !url.contains("google") {
                            return Some(url);
                        }
                    }
                }
            }
        }
        None
    }
}

fn extract_nearby_title(html: &str, url: &str) -> String {
    if let Some(pos) = html.find(url) {
        let before = &html[..pos];
        if let Some(a_start) = before.rfind("<a ") {
            let a_tag = &html[a_start..pos];
            if let Some(text_start) = a_tag.find('>') {
                let text = &a_tag[text_start + 1..];
                if let Some(text_end) = text.find('<') {
                    return strip_html_tags(&text[..text_end]).trim().to_string();
                }
            }
        }
    }
    String::new()
}

fn extract_nearby_snippet(html: &str, url: &str) -> String {
    if let Some(pos) = html.find(url) {
        let after = &html[pos..];
        if let Some(p_start) = after.find("<p>") {
            let p = &after[p_start..];
            if let Some(p_end) = p.find("</p>") {
                return strip_html_tags(&p[3..p_end]).trim().to_string();
            }
        }
    }
    String::new()
}

fn extract_meta_content(html: &str, name: &str) -> Option<String> {
    let patterns = [format!("property=\"{}\"", name), format!("name=\"{}\"", name)];
    for pattern in &patterns {
        if let Some(pos) = html.find(pattern) {
            let before = &html[..pos];
            if let Some(meta_start) = before.rfind("<meta") {
                let after_pattern = &html[pos + pattern.len()..];
                let tag_end = after_pattern.find('>').map(|p| p + pos + pattern.len());
                if let Some(end) = tag_end {
                    let full_tag = &html[meta_start..=end];
                    if let Some(content_pos) = full_tag.find("content=\"") {
                        let content = &full_tag[content_pos + 9..];
                        if let Some(content_end) = content.find('"') {
                            return Some(content[..content_end].to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn extract_follower_count(html: &str) -> Option<u64> {
    let patterns = [
        r#""subscriberCountText":\{"simpleText":"([^"]+)""#,
        r#""subscriberCount"\s*:\s*"?(\d+)"?#,
        r#""followerCount"\s*:\s*(\d+)"#,
        r#"(\d[\d,.]+)\s*followers?"#,
        r#"(\d+\.?\d*[MmKk])\s*followers?"#,
    ];
    for pattern in &patterns {
        if let Ok(re) = Regex::new(pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(m) = caps.get(1) {
                    let raw = m.as_str().to_string();
                    if raw.ends_with('M') || raw.ends_with('m') {
                        if let Ok(n) = raw.trim_end_matches(|c: char| c == 'M' || c == 'm').parse::<f64>() {
                            return Some((n * 1_000_000.0) as u64);
                        }
                    } else if raw.ends_with('K') || raw.ends_with('k') {
                        if let Ok(n) = raw.trim_end_matches(|c: char| c == 'K' || c == 'k').parse::<f64>() {
                            return Some((n * 1_000.0) as u64);
                        }
                    } else {
                        let count_str = raw.replace(',', "").replace('.', "");
                        if let Ok(count) = count_str.parse::<u64>() { return Some(count); }
                    }
                }
            }
        }
    }
    None
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true, '>' => in_tag = false,
            _ if !in_tag => result.push(ch), _ => {}
        }
    }
    result.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
        .replace("&quot;", "\"").replace("&#39;", "'")
        .split_whitespace().collect::<Vec<&str>>().join(" ")
}

fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 { return false; }
    let local = parts[0]; let domain = parts[1];
    if local.is_empty() || local.starts_with('.') || local.ends_with('.') { return false; }
    if !domain.contains('.') || domain.ends_with('.') { return false; }
    if local.starts_with("u002F") || local.starts_with("u003e") || local.starts_with("u003c") { return false; }
    // Must not start with dash (common false positive)
    if local.starts_with('-') { return false; }
    // Must not end with special characters
    if local.ends_with('-') || local.ends_with('_') || local.ends_with('+') { return false; }
    // Filter out common non-creator prefixes
    let no_reply_prefixes = ["noreply", "no-reply", "donotreply", "mailer-daemon", "postmaster", "webmaster", "abuse", "bounce", "auto"];
    if no_reply_prefixes.iter().any(|p| local.starts_with(p)) { return false; }
    let blacklist = [
        "example.com", "google.com", "duckduckgo.com", "github.com", "github.io",
        "localhost", "test.com", "domain.com", "sentry.io", "wixpress.com",
        "feedspot.com", "nytimes.com", "bbc.co.uk", "bbc.com", "sbgtv.com",
        "wjla.com", "mef.hr", "o28395.ingest.us.sentry.io",
        "swisscows.com", "ecosia.org", "startpage.com", "qwant.com",
        "bing.com", "yandex.com", "mojeek.com",
        "1x.png", "2x.png", "furniture.lab",
        // JS bundle / source-map junk: webpack chunk names like
        // "getButtonText@desktop.ts" match the email regex
        ".ts", ".tsx", ".js", ".mjs", ".map", ".json", ".css", ".scss",
        ".woff", ".svg", ".webp",
        "cnn.com", "foxnews.com", "reuters.com", "apnews.com",
        "washingtonpost.com", "theguardian.com",
        "datadoghq.com", "newrelic.com", "amplitude.com",
        "segment.com", "mixpanel.com", "heap.io", "hotjar.com",
        "imgur.com", "flickr.com", "500px.com", "unsplash.com",
        "googletagmanager.com", "google-analytics.com", "facebook.net",
        "linktr.ee", "beacons.ai", "plannthat.com",
        "yourstore.com", "revolutionbeauty.com", "tropicskincare.com",
        "frenchfarmacie.com", "maryville.edu", "microsoft.com",
        "instagram.com", "tiktok.com", "twitter.com", "x.com", "youtube.com",
        "wix.com", "squarespace.com", "shopify.com", "wordpress.com",
        "namecheap.com", "godaddy.com", "cloudflare.com",
    ];
    // Filter out university/corporate emails
    let tld = domain.split('.').last().unwrap_or("");
    if tld == "edu" || tld == "gov" || tld == "mil" { return false; }
    // Filter domains containing university/edu patterns (catches onmicrosoft.com edu subdomains)
    if domain.contains("edu.") || domain.contains(".edu") || domain.contains("university")
        || domain.contains("college") || domain.contains("onmicrosoft")
        || domain.ends_with(".ac.uk") || domain.ends_with(".gov.uk")
        || domain.contains("wixpress") || domain.contains("sentry.") { return false; }
    for bl in &blacklist {
        // Entries starting with '.' match as suffixes (file-extension junk
        // like "getButtonText@desktop.ts"); everything else matches exactly.
        if bl.starts_with('.') {
            if domain.to_lowercase().ends_with(bl) { return false; }
        } else if domain.eq_ignore_ascii_case(bl) {
            return false;
        }
    }
    let tld = domain.split('.').last().unwrap_or("");
    if tld.len() < 2 { return false; }
    if email.contains("noreply") || email.contains("no-reply") || email.contains("donotreply") { return false; }
    true
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

fn extract_ig_username(url: &str) -> Option<String> {
    if let Some(after) = url.split("instagram.com/").nth(1) {
        let username = after.split('/').next()?;
        let skip = ["p", "reel", "tv", "stories", "explore", "accounts", "direct", "popular", "login"];
        if !username.is_empty() && !skip.contains(&username) && !username.starts_with('p') {
            return Some(username.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_valid_emails() {
        assert!(is_valid_email("user@gmail.com"));
        assert!(is_valid_email("info@company.co.uk"));
    }
    #[test]
    fn test_false_positives() {
        assert!(!is_valid_email("u002F@something.local"));
        assert!(!is_valid_email("test@example.com"));
        assert!(!is_valid_email("user@sentry.io"));
        assert!(!is_valid_email("user@swisscows.com"));
    }
    #[test]
    fn test_extract_ig_username() {
        assert_eq!(extract_ig_username("https://www.instagram.com/therock/"), Some("therock".to_string()));
        assert_eq!(extract_ig_username("https://www.instagram.com/reel/ABC/"), None);
    }
}
