use crate::error::Result;
use regex::Regex;
use tracing::debug;

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

impl Default for HtmlParser {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlParser {
    // Justified: fixed, compile-time-known regex literals.
    #[allow(clippy::unwrap_used)]
    pub fn new() -> Self {
        Self {
            email_regex: Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
            phone_regex: Regex::new(
                r"(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{3,4}",
            )
            .unwrap(),
            url_regex: Regex::new(r#"href="(https?[^"]+)""#).unwrap(),
        }
    }

    pub fn parse_search_results(&self, html: &str) -> Result<Vec<SearchResult>> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for cap in self.url_regex.find_iter(html) {
            let url = cap
                .as_str()
                .trim_start_matches("href=\"")
                .trim_end_matches('"')
                .to_string();
            if url.contains("ecosia.org")
                || url.contains("bing.com")
                || url.contains("google.com")
                || url.ends_with(".css")
                || url.ends_with(".js")
                || url.ends_with(".png")
                || url.ends_with(".jpg")
            {
                continue;
            }

            let profile_url = self.normalize_to_profile_url(&url);
            if profile_url.is_empty() || seen.contains(&profile_url) {
                continue;
            }
            seen.insert(profile_url.clone());

            let title = extract_nearby_title(html, &url);
            let snippet = extract_nearby_snippet(html, &url);
            results.push(SearchResult {
                title,
                url: profile_url,
                snippet,
            });
        }

        debug!("Parsed {} unique profile results", results.len());
        Ok(results)
    }

    /// Raw organic results for NON-platform verticals (iteration 54):
    /// `parse_search_results` normalizes to creator-platform profile URLs
    /// and discards everything else — correct for the creator lane, wrong
    /// for property/vertical discovery where the result IS a company site.
    /// Bing wraps organic links in `/ck/a?...&u=a1<base64url>` redirects —
    /// those are decoded to their targets. Returns external http(s) URLs,
    /// engine-internal + asset URLs stripped, deduped, queries stripped.
    pub fn parse_raw_results(&self, html: &str) -> Vec<SearchResult> {
        let mut results = Vec::new();
        let mut seen = std::collections::HashSet::new();

        // Bing redirect targets first (the real organic URLs).
        if let Ok(re) = Regex::new(r#"[?&]u=a1([A-Za-z0-9_-]{20,})"#) {
            for cap in re.captures_iter(html) {
                if let Some(b64) = cap.get(1).map(|m| m.as_str()) {
                    if let Some(url) = decode_base64url(b64) {
                        if let Some(r) = self.raw_result_for(&url, html) {
                            if seen.insert(r.url.clone()) {
                                results.push(r);
                            }
                        }
                    }
                }
            }
        }

        // Bing serves organic URLs to non-JS clients as <cite> display
        // forms ("https://site.co.uk <span>› path › more</span>") —
        // reconstruct https://site.co.uk/path from them.
        if let Ok(re) = Regex::new(r"<cite>(.*?)</cite>") {
            for cap in re.captures_iter(html) {
                let raw = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let text = strip_html_tags(raw);
                let text = text.split('›').next().unwrap_or("").trim();
                let mut parts = vec![text.to_string()];
                if let Some(rest) = cap.get(1).map(|m| m.as_str()) {
                    for seg in rest.split('›').skip(1) {
                        let seg = strip_html_tags(seg).trim().to_string();
                        if !seg.is_empty() && seg != "..." {
                            parts.push(seg);
                        }
                    }
                }
                if parts[0].is_empty() {
                    continue;
                }
                let url = if parts[0].starts_with("http") {
                    parts.join("/")
                } else {
                    format!("https://{}", parts.join("/"))
                };
                if let Some(r) = self.raw_result_for(&url, html) {
                    if seen.insert(r.url.clone()) {
                        results.push(r);
                    }
                }
            }
        }

        // Direct hrefs (brave/other engines expose real targets).
        for cap in self.url_regex.find_iter(html) {
            let url = cap
                .as_str()
                .trim_start_matches("href=\"")
                .trim_end_matches('"')
                .to_string();
            if !url.starts_with("http") || url.contains("/ck/a") {
                continue;
            }
            if let Some(r) = self.raw_result_for(&url, html) {
                if seen.insert(r.url.clone()) {
                    results.push(r);
                }
            }
        }
        debug!("Parsed {} unique raw results", results.len());
        results
    }

    fn raw_result_for(&self, url: &str, html: &str) -> Option<SearchResult> {
        let lower = url.to_lowercase();
        let internal = [
            "bing.com", "microsoft", "msn.com", "brave.com", "google.com",
            "yandex", "ecosia.org", "startpage", "swisscows", "qwant",
            "mojeek", "w3.org", "schema.org",
        ];
        if internal.iter().any(|i| lower.contains(i)) {
            return None;
        }
        if url.ends_with(".css")
            || url.ends_with(".js")
            || url.ends_with(".png")
            || url.ends_with(".jpg")
            || url.ends_with(".svg")
            || url.ends_with(".ico")
        {
            return None;
        }
        let clean = url.split('?').next().unwrap_or(url).to_string();
        if clean.len() < 12 || !clean.starts_with("http") {
            return None;
        }
        let title = extract_nearby_title(html, url);
        let snippet = extract_nearby_snippet(html, url);
        Some(SearchResult {
            title,
            url: clean,
            snippet,
        })
    }

    fn normalize_to_profile_url(&self, url: &str) -> String {
        if url.contains("instagram.com/") {
            if let Some(username) = extract_ig_username(url) {
                return format!("https://www.instagram.com/{}/", username);
            }
            return String::new();
        }
        if url.contains("tiktok.com/@") {
            return url.split('?').next().unwrap_or(url).to_string();
        }
        if url.contains("youtube.com/@") || url.contains("youtube.com/channel/") {
            let base = url.split('?').next().unwrap_or(url);
            return format!("{}/about", base.trim_end_matches('/'));
        }
        if url.contains("twitter.com/") || url.contains("x.com/") {
            return url.split('?').next().unwrap_or(url).to_string();
        }
        if url.contains("linktr.ee/") || url.contains("beacons.ai/") {
            return url.split('?').next().unwrap_or(url).to_string();
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

        // Iteration 54 backport (outreachkit iter 40): Cloudflare Scrape
        // Shield payloads decode in place before the standard scan.
        let revealed = outreachkit::gates::reveal_cfemail(text);
        let text = revealed.as_str();

        // Standard form. Canonicalize BEFORE dedup: gmail alias forms must
        // collapse here, not after the found-check (iter 46).
        for m in self.email_regex.find_iter(text) {
            let e = canonicalize_email(m.as_str());
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
                    let candidate = canonicalize_email(&format!(
                        "{}@{}.{}",
                        cap.get(1).map(|m| m.as_str()).unwrap_or(""),
                        cap.get(2).map(|m| m.as_str()).unwrap_or(""),
                        cap.get(3).map(|m| m.as_str()).unwrap_or("")
                    ));
                    if is_valid_email(&candidate) && !found.contains(&candidate) {
                        found.push(candidate);
                    }
                }
            }
        }

        found
    }

    pub fn extract_phones(&self, text: &str) -> Vec<String> {
        self.phone_regex
            .find_iter(text)
            .map(|m| m.as_str().trim().to_string())
            .filter(|p| {
                let digits: String = p.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() < 7 || digits.len() > 15 {
                    return false;
                }
                if p.contains('.') {
                    return false;
                }
                // Unix timestamps in ms (13 digits, 17xxxxx era) were the main
                // source of garbage "phones" — reject them explicitly.
                if digits.len() == 13 && digits.starts_with("17") {
                    return false;
                }
                if digits
                    .chars()
                    .all(|c| c == digits.chars().next().unwrap_or('0'))
                {
                    return false;
                }
                true
            })
            .collect()
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
                    if !links.contains(&link) {
                        links.push(link);
                    }
                }
            }
        }
        links
    }

    fn extract_website(&self, html: &str) -> Option<String> {
        let patterns = [
            r#""website"\s*:\s*"(https?[^"]+)""#,
            r#"href="(https?://[^"]*(?:\.com|\.org|\.net|\.io)[^"]*)""#,
        ];
        for pattern in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(caps) = re.captures(html) {
                    if let Some(url) = caps.get(1) {
                        let url = url.as_str().to_string();
                        if !url.contains("ecosia")
                            && !url.contains("bing")
                            && !url.contains("google")
                        {
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
    let patterns = [
        format!("property=\"{}\"", name),
        format!("name=\"{}\"", name),
    ];
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
                        if let Ok(n) = raw.trim_end_matches(['M', 'm']).parse::<f64>() {
                            return Some((n * 1_000_000.0) as u64);
                        }
                    } else if raw.ends_with('K') || raw.ends_with('k') {
                        if let Ok(n) = raw.trim_end_matches(['K', 'k']).parse::<f64>() {
                            return Some((n * 1_000.0) as u64);
                        }
                    } else {
                        let count_str = raw.replace([',', '.'], "");
                        if let Ok(count) = count_str.parse::<u64>() {
                            return Some(count);
                        }
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
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join(" ")
}

fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let local = parts[0];
    let domain = parts[1];
    if local.is_empty() || local.starts_with('.') || local.ends_with('.') {
        return false;
    }
    if !domain.contains('.') || domain.ends_with('.') {
        return false;
    }
    if local.starts_with("u002F") || local.starts_with("u003e") || local.starts_with("u003c") {
        return false;
    }
    // Must not start with dash (common false positive)
    if local.starts_with('-') {
        return false;
    }
    // Must not end with special characters
    if local.ends_with('-') || local.ends_with('_') || local.ends_with('+') {
        return false;
    }
    // Filter out common non-creator prefixes
    let no_reply_prefixes = [
        "noreply",
        "no-reply",
        "donotreply",
        "mailer-daemon",
        "postmaster",
        "webmaster",
        "abuse",
        "bounce",
        "auto",
    ];
    if no_reply_prefixes.iter().any(|p| local.starts_with(p)) {
        return false;
    }
    let blacklist = [
        "example.com",
        "google.com",
        "duckduckgo.com",
        "github.com",
        "github.io",
        "localhost",
        "test.com",
        "domain.com",
        "sentry.io",
        "wixpress.com",
        "feedspot.com",
        "nytimes.com",
        "bbc.co.uk",
        "bbc.com",
        "sbgtv.com",
        "wjla.com",
        "mef.hr",
        "o28395.ingest.us.sentry.io",
        "swisscows.com",
        "ecosia.org",
        "startpage.com",
        "qwant.com",
        "bing.com",
        "yandex.com",
        "mojeek.com",
        "1x.png",
        "2x.png",
        "furniture.lab",
        // JS bundle / source-map junk: webpack chunk names like
        // "getButtonText@desktop.ts" match the email regex
        ".ts",
        ".tsx",
        ".js",
        ".mjs",
        ".map",
        ".json",
        ".css",
        ".scss",
        ".woff",
        ".svg",
        ".webp",
        "cnn.com",
        "foxnews.com",
        "reuters.com",
        "apnews.com",
        "washingtonpost.com",
        "theguardian.com",
        "datadoghq.com",
        "newrelic.com",
        "amplitude.com",
        "segment.com",
        "mixpanel.com",
        "heap.io",
        "hotjar.com",
        "imgur.com",
        "flickr.com",
        "500px.com",
        "unsplash.com",
        "googletagmanager.com",
        "google-analytics.com",
        "facebook.net",
        "linktr.ee",
        "beacons.ai",
        "plannthat.com",
        "yourstore.com",
        "revolutionbeauty.com",
        "tropicskincare.com",
        "frenchfarmacie.com",
        "maryville.edu",
        "microsoft.com",
        "instagram.com",
        "tiktok.com",
        "twitter.com",
        "x.com",
        "youtube.com",
        "wix.com",
        "squarespace.com",
        "shopify.com",
        "wordpress.com",
        "namecheap.com",
        "godaddy.com",
        "cloudflare.com",
    ];
    // Filter out university/corporate emails
    let tld = domain.split('.').next_back().unwrap_or("");
    if tld == "edu" || tld == "gov" || tld == "mil" {
        return false;
    }
    // Filter domains containing university/edu patterns (catches onmicrosoft.com edu subdomains)
    if domain.contains("edu.")
        || domain.contains(".edu")
        || domain.contains("university")
        || domain.contains("college")
        || domain.contains("onmicrosoft")
        || domain.ends_with(".ac.uk")
        || domain.ends_with(".gov.uk")
        || domain.contains("wixpress")
        || domain.contains("sentry.")
    {
        return false;
    }
    for bl in &blacklist {
        // Entries starting with '.' match as suffixes (file-extension junk
        // like "getButtonText@desktop.ts"); everything else matches exactly.
        if bl.starts_with('.') {
            if domain.to_lowercase().ends_with(bl) {
                return false;
            }
        } else if domain.eq_ignore_ascii_case(bl) {
            return false;
        }
    }
    let tld = domain.split('.').next_back().unwrap_or("");
    if tld.len() < 2 {
        return false;
    }
    if email.contains("noreply") || email.contains("no-reply") || email.contains("donotreply") {
        return false;
    }
    true
}

/// Canonicalize email address — SINGLE implementation lives in outreachkit
/// (iteration 54 backport: gmail-family aliasing, Spec 07 I6).
pub fn canonicalize_email(email: &str) -> String {
    outreachkit::gates::canonicalize_email(email)
}

fn extract_ig_username(url: &str) -> Option<String> {
    if let Some(after) = url.split("instagram.com/").nth(1) {
        let username = after.split('/').next()?;
        let skip = [
            "p", "reel", "tv", "stories", "explore", "accounts", "direct", "popular", "login",
        ];
        if !username.is_empty() && !skip.contains(&username) && !username.starts_with('p') {
            return Some(username.to_string());
        }
    }
    None
}


/// Base64url decode (standard alphabet, no padding) — for Bing's
/// `/ck/a?...&u=a1<base64url>` organic-link redirects. Hand-rolled to keep
/// the dependency tree lean.
fn decode_base64url(input: &str) -> Option<String> {
    fn val(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a' + 26) as u32),
            b'0'..=b'9' => Some((b - b'0' + 52) as u32),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 + 3);
    let mut buf: u32 = 0;
    let mut bits = 0u32;
    for &b in bytes {
        let v = val(b)?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xFF) as u8);
        }
    }
    String::from_utf8(out).ok()
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
        assert_eq!(
            extract_ig_username("https://www.instagram.com/therock/"),
            Some("therock".to_string())
        );
        assert_eq!(
            extract_ig_username("https://www.instagram.com/reel/ABC/"),
            None
        );
    }
}

#[cfg(test)]
mod iter54_backport_tests {
    use super::*;

    #[test]
    fn cfemail_decode_fixed_vector() {
        // outreachkit iter 40 vector: key 0x37, "hello@jeffnippard.com".
        let href = r#"<a href="/cdn-cgi/l/email-protection#375f525b5b58775d525151595e47475645531954585a">x</a>"#;
        let parser = HtmlParser::new();
        assert!(parser.extract_emails(href).contains(&"hello@jeffnippard.com".to_string()));
    }

    #[test]
    fn gmail_aliases_collapse() {
        let parser = HtmlParser::new();
        let got = parser.extract_emails("mail john@gmail.com or j.o.hn+shop@gmail.com");
        assert_eq!(got, vec!["john@gmail.com".to_string()]);
        assert_eq!(canonicalize_email("a.b+x@googlemail.com"), "ab@gmail.com");
        assert_eq!(canonicalize_email("a.b+x@yahoo.com"), "a.b+x@yahoo.com");
    }
}

#[cfg(test)]
mod raw_results_tests {
    use super::*;

    #[test]
    fn bing_redirect_targets_are_unwrapped() {
        // "https://www.unitestudents.com/glasgow" base64url-encoded
        let html = r#"<li class="b_algo"><h2><a href="https://www.bing.com/ck/a?!&u=a1aHR0cHM6Ly93d3cudW5pdGVzdHVkZW50cy5jb20vZ2xhc2dvdw"></a></h2></li>"#;
        let parser = HtmlParser::new();
        let raw = parser.parse_raw_results(html);
        assert!(
            raw.iter().any(|r| r.url.contains("unitestudents.com/glasgow")),
            "redirect target must surface: {raw:?}"
        );
    }

    #[test]
    fn bing_cites_reconstruct_organic_urls() {
        let html = r#"<li class="b_algo"><cite>https://pbsanews.co.uk <span class="b_ctdif">› category › pbsa</span></cite></li>
        <li class="b_algo"><cite>https://www.knightfrank.co.uk <span class="b_ctdif">› research › article</span></cite></li>"#;
        let parser = HtmlParser::new();
        let raw = parser.parse_raw_results(html);
        let urls: Vec<&str> = raw.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.contains(&"https://pbsanews.co.uk/category/pbsa"), "{urls:?}");
        assert!(urls.contains(&"https://www.knightfrank.co.uk/research/article"), "{urls:?}");
    }

    #[test]
    fn raw_results_keep_company_sites() {
        let html = r#"<li class="b_algo"><h2><a href="https://www.unitestudents.com/glasgow">Unite Students Glasgow</a></h2><p>Student accommodation in Glasgow.</p></li>
        <li class="b_algo"><h2><a href="https://www.studentroost.co.uk/contact?utm=x">Contact</a></h2><p>Get in touch.</p></li>
        <li><a href="https://www.bing.com/search?q=x">next</a></li>"#;
        let parser = HtmlParser::new();
        let raw = parser.parse_raw_results(html);
        let urls: Vec<&str> = raw.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.contains(&"https://www.unitestudents.com/glasgow"), "{urls:?}");
        assert!(urls.contains(&"https://www.studentroost.co.uk/contact"), "{urls:?}");
        assert!(!urls.iter().any(|u| u.contains("bing.com")));
    }
}
