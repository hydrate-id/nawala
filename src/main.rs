use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tower::ServiceBuilder;
use vercel_runtime::axum::VercelLayer;
use vercel_runtime::{run, Error};

mod blocklist;

// no token (or no API_TOKEN set): 3 req/s per IP
const RATE_LIMIT: usize = 3;
const RATE_WINDOW: Duration = Duration::from_secs(1);

type Buckets = Mutex<HashMap<String, VecDeque<Instant>>>;
static BUCKETS: OnceLock<Buckets> = OnceLock::new();

#[derive(Deserialize)]
struct Params {
    domain: Option<String>,
    token: Option<String>,
}

fn valid_domain(domain: &str) -> bool {
    if domain.is_empty()
        || domain.len() > 253
        || !domain.is_ascii()
        || domain.contains("://")
        || domain.contains('/')
    {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for (i, label) in labels.iter().enumerate() {
        if label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-') {
            return false;
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return false;
        }
        if i == labels.len() - 1
            && (label.len() < 2 || !label.chars().all(|c| c.is_ascii_alphabetic()))
        {
            return false;
        }
    }
    true
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenvy::dotenv().ok();
    let router = Router::new().route("/health", get(health)).fallback(check);
    let app = ServiceBuilder::new()
        .layer(VercelLayer::new())
        .service(router);
    run(app).await
}

async fn health() -> Response {
    let mut r = json_resp(StatusCode::OK, json!({ "status": "ok" }));
    if let Ok(v) = hyper::header::HeaderValue::from_str("no-cache") {
        r.headers_mut().insert(hyper::header::CACHE_CONTROL, v);
    }
    r
}

async fn check(Query(p): Query<Params>, headers: HeaderMap) -> Response {
    // matching API_TOKEN (single, optional) skips the rate limit
    let privileged = is_unlimited_token(p.token.as_deref().unwrap_or(""));
    if !privileged {
        let ip = client_ip(&headers);
        if !take_slot(&ip) {
            return json_resp(
                StatusCode::TOO_MANY_REQUESTS,
                json!({ "error": "rate limit exceeded" }),
            );
        }
    }

    let domain = match p.domain {
        Some(d) if valid_domain(&d.to_lowercase()) => d.to_lowercase(),
        _ => return json_resp(StatusCode::BAD_REQUEST, json!({ "error": "invalid domain format" })),
    };

    let blocked = match blocklist::is_blocked(&domain) {
        Ok(b) => b,
        Err(_) => {
            return json_resp(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({ "error": "blocklist not loaded" }),
            )
        }
    };

    json_resp_cached(json!({
        "domain": domain,
        "status": if blocked { "blocked" } else { "allowed" },
    }))
}

// caller IP from Vercel proxy headers (not spoofable behind CF/RV)
fn client_ip(headers: &HeaderMap) -> String {
    for key in ["x-vercel-forwarded-for", "x-forwarded-for", "x-real-ip"] {
        if let Some(v) = headers.get(key) {
            if let Ok(s) = v.to_str() {
                let first = s.split(',').next().unwrap_or("").trim();
                if !first.is_empty() {
                    return first.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

fn is_unlimited_token(given: &str) -> bool {
    let cfg = std::env::var("API_TOKEN").unwrap_or_default();
    !cfg.is_empty() && given == cfg
}

fn take_slot(key: &str) -> bool {
    let mut buckets = BUCKETS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    allow_in(&mut buckets, key, RATE_LIMIT, RATE_WINDOW)
}

fn allow_in(
    buckets: &mut HashMap<String, VecDeque<Instant>>,
    key: &str,
    max: usize,
    window: Duration,
) -> bool {
    let now = Instant::now();
    let queue = buckets.entry(key.to_string()).or_default();
    while queue.front().is_some_and(|t| now.duration_since(*t) >= window) {
        queue.pop_front();
    }
    if queue.len() >= max {
        return false;
    }
    queue.push_back(now);
    true
}

fn json_resp(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}

// HTTP cache for clients (backend cache TTL ~1h)
fn json_resp_cached(value: Value) -> Response {
    let mut r = json_resp(StatusCode::OK, value);
    if let Ok(v) = hyper::header::HeaderValue::from_str("public, max-age=3600") {
        r.headers_mut().insert(hyper::header::CACHE_CONTROL, v);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_valid() {
        for ok in ["google.com", "sub.domain.co.id", "xn--bcher-kva.example", "a.co"] {
            assert!(valid_domain(ok), "{ok} should be valid");
        }
    }

    #[test]
    fn domain_invalid() {
        for bad in [
            "",
            "google",
            "google.",
            ".com",
            "-bad.com",
            "bad-.com",
            "http://google.com",
            "google.com/path",
            "with space.com",
            "a",
            "sub_domain.com",
            "google..com",
        ] {
            assert!(!valid_domain(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn rate_limit_blocks_over_limit() {
        let mut b: HashMap<String, VecDeque<Instant>> = HashMap::new();
        for _ in 0..5 {
            assert!(allow_in(&mut b, "t", 5, Duration::from_secs(1)));
        }
        assert!(!allow_in(&mut b, "t", 5, Duration::from_secs(1)));
    }

    #[test]
    fn rate_limit_independent_per_key() {
        let mut b: HashMap<String, VecDeque<Instant>> = HashMap::new();
        for _ in 0..5 {
            assert!(allow_in(&mut b, "t1", 5, Duration::from_secs(1)));
        }
        assert!(!allow_in(&mut b, "t1", 5, Duration::from_secs(1)));
        assert!(allow_in(&mut b, "t2", 5, Duration::from_secs(1)));
    }

    #[test]
    fn rate_limit_resets_after_window() {
        let mut b: HashMap<String, VecDeque<Instant>> = HashMap::new();
        for _ in 0..5 {
            allow_in(&mut b, "t", 5, Duration::from_millis(10));
        }
        assert!(!allow_in(&mut b, "t", 5, Duration::from_millis(10)));
        std::thread::sleep(Duration::from_millis(15));
        assert!(allow_in(&mut b, "t", 5, Duration::from_millis(10)));
    }

    #[test]
    fn client_ip_takes_first_xff() {
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "1.2.3.4, 5.6.7.8".parse().unwrap());
        assert_eq!(client_ip(&h), "1.2.3.4");
    }

    #[test]
    fn client_ip_fallback_x_real_ip() {
        let mut h = HeaderMap::new();
        h.insert("x-real-ip", "9.9.9.9".parse().unwrap());
        assert_eq!(client_ip(&h), "9.9.9.9");
        assert_eq!(client_ip(&HeaderMap::new()), "unknown");
    }
}
