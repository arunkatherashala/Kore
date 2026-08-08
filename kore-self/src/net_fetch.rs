//! Safe HTTP fetch — reqwest + host allowlist (no curl/PowerShell shell).

use std::time::Duration;

const USER_AGENT: &str = "KORE-self/2026";

/// Hosts KORE-self may fetch (HTTPS only).
const ALLOWED_HOSTS: &[&str] = &[
    "wikipedia.org",
    "wikimedia.org",
    "hacker-news.firebaseio.com",
    "api.github.com",
    "raw.githubusercontent.com",
];

pub fn is_allowed_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host_lower = host.to_lowercase();
    ALLOWED_HOSTS.iter().any(|allowed| {
        host_lower == *allowed || host_lower.ends_with(&format!(".{allowed}"))
    })
}

pub fn wikipedia_summary_url(lang_code: &str, topic: &str) -> Option<String> {
    let lang: String = lang_code
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(8)
        .collect();
    if lang.is_empty() {
        return None;
    }
    let topic: String = topic
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if topic.is_empty() {
        return None;
    }
    Some(format!(
        "https://{lang}.wikipedia.org/api/rest_v1/page/summary/{topic}"
    ))
}

pub fn fetch_text(url: &str, timeout_secs: u64) -> Result<String, String> {
    if !is_allowed_url(url) {
        return Err(format!("URL not allowlisted: {url}"));
    }
    let timeout = Duration::from_secs(timeout_secs.max(2).min(15));
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;
    let body = client
        .get(url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())?;
    Ok(body)
}

pub fn fetch_wikipedia_summary(
    lang_code: &str,
    topic: &str,
    timeout_secs: u64,
) -> Option<(String, String)> {
    let url = wikipedia_summary_url(lang_code, topic)?;
    let body = fetch_text(&url, timeout_secs).ok()?;
    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let title = json["title"].as_str()?.to_string();
    let extract = json["extract"].as_str()?.to_string();
    if extract.is_empty() {
        return None;
    }
    Some((title, extract))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_wikipedia() {
        assert!(is_allowed_url(
            "https://en.wikipedia.org/api/rest_v1/page/summary/Math"
        ));
        assert!(is_allowed_url("https://te.wikipedia.org/api/rest_v1/page/summary/X"));
        assert!(!is_allowed_url("http://en.wikipedia.org/x"));
        assert!(!is_allowed_url("https://evil.com/x"));
    }

    #[test]
    fn wikipedia_url_sanitizes() {
        let u = wikipedia_summary_url("en", "Mathematics").unwrap();
        assert!(u.contains("en.wikipedia.org"));
        assert!(is_allowed_url(&u));
        assert!(wikipedia_summary_url("", "x").is_none());
        assert!(wikipedia_summary_url("en", "").is_none());
    }

    #[test]
    fn allowlist_hn_and_github() {
        assert!(is_allowed_url("https://hacker-news.firebaseio.com/v0/topstories.json"));
        assert!(is_allowed_url("https://api.github.com/search/repositories?q=rust"));
    }
}
