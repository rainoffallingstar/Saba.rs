//! OGS REST login flow.
//!
//! Mirrors Seki-Sabaki's two-step login: fetch `/api/v1/ui/config` for the
//! CSRF token and session cookie, then POST `/api/v0/login` with the token and
//! cookie. The returned `user_jwt` becomes the WebSocket `authenticate` token.
//! The password is used once and never stored.

pub const OGS_SERVER_URL: &str = "https://online-go.com";
pub const OGS_USER_AGENT: &str = "Ryusei/0.1";

#[derive(Clone, Debug)]
pub struct OgsHttpResponse {
    pub status: u16,
    pub body: String,
    pub set_cookie: Vec<String>,
}

/// Injection seam so the login flow is testable without network access.
pub trait OgsRestFetch: Send {
    fn get(&mut self, url: &str) -> Result<OgsHttpResponse, String>;
    fn post_json(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<OgsHttpResponse, String>;
}

/// Production blocking HTTP client backed by `ureq` (rustls).
#[derive(Debug)]
pub struct UreqOgsRestFetch {
    agent: ureq::Agent,
}

impl UreqOgsRestFetch {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::new_with_defaults(),
        }
    }
}

impl Default for UreqOgsRestFetch {
    fn default() -> Self {
        Self::new()
    }
}

fn collect_set_cookie(headers: &ureq::http::HeaderMap) -> Vec<String> {
    headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .collect()
}

fn read_response_body(response: ureq::http::Response<ureq::Body>) -> Result<String, String> {
    let mut body = response.into_body();
    body.read_to_string().map_err(|error| error.to_string())
}

impl OgsRestFetch for UreqOgsRestFetch {
    fn get(&mut self, url: &str) -> Result<OgsHttpResponse, String> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|error| format!("OGS config request failed: {error}"))?;
        let status = response.status().as_u16();
        let set_cookie = collect_set_cookie(response.headers());
        let body = read_response_body(response)?;
        Ok(OgsHttpResponse {
            status,
            body,
            set_cookie,
        })
    }

    fn post_json(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<OgsHttpResponse, String> {
        let mut request = self.agent.post(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = request
            .send(body)
            .map_err(|error| format!("OGS login request failed: {error}"))?;
        let status = response.status().as_u16();
        let set_cookie = collect_set_cookie(response.headers());
        let body = read_response_body(response)?;
        Ok(OgsHttpResponse {
            status,
            body,
            set_cookie,
        })
    }
}

/// Extracts the `csrf_token` field from the OGS UI config JSON.
pub fn extract_csrf_token(body: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("invalid OGS config JSON: {error}"))?;
    value
        .get("csrf_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "OGS did not return a CSRF token".to_owned())
}

/// Builds a single `Cookie` header value from raw `Set-Cookie` lines, keeping
/// the latest value for each cookie name.
pub fn normalize_cookie_header(set_cookies: &[String]) -> Option<String> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for line in set_cookies {
        let pair = line.split(';').next().unwrap_or("").trim();
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if let Some(existing) = pairs.iter_mut().find(|(existing, _)| existing == name) {
            *existing = (name.to_owned(), value.to_owned());
        } else {
            pairs.push((name.to_owned(), value.to_owned()));
        }
    }
    if pairs.is_empty() {
        return None;
    }
    Some(
        pairs
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

/// Result of a successful OGS login.
#[derive(Clone, Debug)]
pub struct OgsLoginResult {
    pub jwt_token: String,
    pub cookie_header: Option<String>,
    pub user: serde_json::Value,
}

/// Parses the `POST /api/v0/login` response body into a JWT and user payload.
pub fn parse_ogs_login_response(body: &str) -> Result<OgsLoginResult, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|error| format!("invalid OGS login JSON: {error}"))?;
    let jwt_token = value
        .get("user_jwt")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| "OGS did not return a session token".to_owned())?;
    let user = value
        .get("user")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    Ok(OgsLoginResult {
        jwt_token,
        cookie_header: None,
        user,
    })
}

/// Runs the two-step login against an injected fetch implementation.
pub fn login_via_rest(
    fetch: &mut dyn OgsRestFetch,
    username: &str,
    password: &str,
) -> Result<OgsLoginResult, String> {
    let username = username.trim();
    if username.is_empty() {
        return Err("Username is required.".to_owned());
    }
    if password.is_empty() {
        return Err("Password is required.".to_owned());
    }

    let config = fetch.get(&format!("{OGS_SERVER_URL}/api/v1/ui/config"))?;
    if !(200..300).contains(&config.status) {
        return Err("OGS config request failed.".to_owned());
    }
    let csrf_token = extract_csrf_token(&config.body)?;
    let cookie_header = normalize_cookie_header(&config.set_cookie);

    let cookie = cookie_header.clone().unwrap_or_default();
    let mut headers = vec![
        ("User-Agent", OGS_USER_AGENT),
        ("Content-Type", "application/json"),
        ("X-CSRFToken", csrf_token.as_str()),
    ];
    if !cookie.is_empty() {
        headers.push(("Cookie", cookie.as_str()));
    }

    let body = serde_json::json!({ "username": username, "password": password }).to_string();
    let login = fetch.post_json(&format!("{OGS_SERVER_URL}/api/v0/login"), &headers, &body)?;
    if matches!(login.status, 400 | 401 | 403) {
        return Err("Invalid OGS username or password.".to_owned());
    }
    if !(200..300).contains(&login.status) {
        return Err("OGS login request failed.".to_owned());
    }
    let mut result = parse_ogs_login_response(&login.body)?;
    let merged_cookies = config
        .set_cookie
        .iter()
        .chain(login.set_cookie.iter())
        .cloned()
        .collect::<Vec<_>>();
    result.cookie_header = normalize_cookie_header(&merged_cookies);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeRestFetch {
        config_status: u16,
        config_body: String,
        login_status: u16,
        login_body: String,
        login_username: Option<String>,
        login_password: Option<String>,
    }

    impl OgsRestFetch for FakeRestFetch {
        fn get(&mut self, _url: &str) -> Result<OgsHttpResponse, String> {
            Ok(OgsHttpResponse {
                status: self.config_status,
                body: self.config_body.clone(),
                set_cookie: vec!["csrftoken=abc123; Path=/".to_owned()],
            })
        }

        fn post_json(
            &mut self,
            _url: &str,
            _headers: &[(&str, &str)],
            body: &str,
        ) -> Result<OgsHttpResponse, String> {
            let parsed: serde_json::Value = serde_json::from_str(body).unwrap();
            self.login_username = parsed
                .get("username")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            self.login_password = parsed
                .get("password")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            Ok(OgsHttpResponse {
                status: self.login_status,
                body: self.login_body.clone(),
                set_cookie: vec!["sessionid=xyz789; Path=/".to_owned()],
            })
        }
    }

    #[test]
    fn extracts_csrf_and_normalizes_cookies() {
        assert_eq!(
            extract_csrf_token(r#"{"csrf_token":"tok"}"#).unwrap(),
            "tok"
        );
        assert!(extract_csrf_token(r#"{}"#).is_err());
        assert_eq!(
            normalize_cookie_header(&[
                "csrftoken=a; Path=/".to_owned(),
                "sessionid=b; Path=/".to_owned(),
            ])
            .unwrap(),
            "csrftoken=a; sessionid=b"
        );
        assert_eq!(normalize_cookie_header(&[]), None);
    }

    #[test]
    fn login_succeeds_and_returns_jwt_and_cookie() {
        let mut fetch = FakeRestFetch {
            config_status: 200,
            config_body: r#"{"csrf_token":"abc123"}"#.to_owned(),
            login_status: 200,
            login_body: r#"{"user_jwt":"jwt-token","user":{"id":7,"username":"player"}}"#
                .to_owned(),
            ..Default::default()
        };
        let result = login_via_rest(&mut fetch, "player", "secret").expect("login succeeds");
        assert_eq!(result.jwt_token, "jwt-token");
        assert!(result.cookie_header.unwrap().contains("csrftoken=abc123"));
        assert_eq!(fetch.login_username.as_deref(), Some("player"));
        assert_eq!(fetch.login_password.as_deref(), Some("secret"));
    }

    #[test]
    fn login_reports_invalid_credentials() {
        let mut fetch = FakeRestFetch {
            config_status: 200,
            config_body: r#"{"csrf_token":"abc123"}"#.to_owned(),
            login_status: 403,
            login_body: "{}".to_owned(),
            ..Default::default()
        };
        let error = login_via_rest(&mut fetch, "player", "wrong").expect_err("login fails");
        assert!(error.contains("Invalid OGS username or password"));
    }

    #[test]
    fn login_requires_username_and_password() {
        let mut fetch = FakeRestFetch::default();
        assert!(login_via_rest(&mut fetch, "", "x").is_err());
        assert!(login_via_rest(&mut fetch, "player", "").is_err());
    }
}
