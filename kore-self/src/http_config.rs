//! HTTP API bind address and optional token auth.

/// Default bind: localhost only (set `KORE_API_BIND=0.0.0.0` to expose LAN).
pub fn api_bind_host() -> String {
    std::env::var("KORE_API_BIND").unwrap_or_else(|_| "127.0.0.1".into())
}

/// Optional bearer token. When set, POST /sql and POST /load require auth.
pub fn api_token() -> Option<String> {
    std::env::var("KORE_API_TOKEN")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn authorize_request(raw_http: &str, method: &str, path: &str) -> Result<(), &'static str> {
    authorize_request_with_token(raw_http, method, path, api_token().as_deref())
}

pub fn authorize_request_with_token(
    raw_http: &str,
    method: &str,
    path: &str,
    token: Option<&str>,
) -> Result<(), &'static str> {
    let token = match token {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(()),
    };
    let needs_auth = method == "POST" && (path == "/sql" || path == "/load");
    if !needs_auth {
        return Ok(());
    }
    let lower = raw_http.to_lowercase();
    let bearer = format!("authorization: bearer {token}").to_lowercase();
    let header = format!("x-kore-token: {token}").to_lowercase();
    if lower.contains(&bearer) || lower.contains(&header) {
        Ok(())
    } else {
        Err("Unauthorized — set Authorization: Bearer <KORE_API_TOKEN> or X-KORE-Token")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bind_is_localhost() {
        std::env::remove_var("KORE_API_BIND");
        assert_eq!(api_bind_host(), "127.0.0.1");
    }

    #[test]
    fn auth_required_when_token_set() {
        let req = "POST /sql HTTP/1.1\r\nAuthorization: Bearer secret\r\n\r\n{}";
        assert!(authorize_request_with_token(req, "POST", "/sql", Some("secret")).is_ok());
        let bad = "POST /sql HTTP/1.1\r\n\r\n{}";
        assert!(authorize_request_with_token(bad, "POST", "/sql", Some("secret")).is_err());
        assert!(authorize_request_with_token(bad, "GET", "/status", Some("secret")).is_ok());
    }
}
