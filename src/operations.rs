use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header::RETRY_AFTER, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::config::RateLimitConfig;

#[derive(Default)]
pub struct OperationalMetrics {
    requests_total: AtomicU64,
    client_errors_total: AtomicU64,
    server_errors_total: AtomicU64,
    rate_limited_total: AtomicU64,
    duration_microseconds_total: AtomicU64,
}

impl OperationalMetrics {
    fn record_request(&self, status: StatusCode, elapsed: Duration) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        if status.is_client_error() {
            self.client_errors_total.fetch_add(1, Ordering::Relaxed);
        }
        if status.is_server_error() {
            self.server_errors_total.fetch_add(1, Ordering::Relaxed);
        }
        let elapsed_micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
        self.duration_microseconds_total
            .fetch_add(elapsed_micros, Ordering::Relaxed);
    }

    fn record_rate_limited(&self) {
        self.rate_limited_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn render(&self, websocket_connections: usize) -> String {
        let requests = self.requests_total.load(Ordering::Relaxed);
        let duration_seconds =
            self.duration_microseconds_total.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        format!(
            "# HELP arena_http_requests_total HTTP requests completed by ArenaCore.\n\
# TYPE arena_http_requests_total counter\n\
arena_http_requests_total {requests}\n\
# HELP arena_http_client_errors_total HTTP 4xx responses.\n\
# TYPE arena_http_client_errors_total counter\n\
arena_http_client_errors_total {}\n\
# HELP arena_http_server_errors_total HTTP 5xx responses.\n\
# TYPE arena_http_server_errors_total counter\n\
arena_http_server_errors_total {}\n\
# HELP arena_http_rate_limited_total Requests rejected by the API rate limiter.\n\
# TYPE arena_http_rate_limited_total counter\n\
arena_http_rate_limited_total {}\n\
# HELP arena_http_request_duration_seconds HTTP request duration summary.\n\
# TYPE arena_http_request_duration_seconds summary\n\
arena_http_request_duration_seconds_sum {duration_seconds}\n\
arena_http_request_duration_seconds_count {requests}\n\
# HELP arena_websocket_connections Current authenticated WebSocket connections.\n\
# TYPE arena_websocket_connections gauge\n\
arena_websocket_connections {websocket_connections}\n",
            self.client_errors_total.load(Ordering::Relaxed),
            self.server_errors_total.load(Ordering::Relaxed),
            self.rate_limited_total.load(Ordering::Relaxed),
        )
    }
}

pub struct RateLimiter {
    config: RateLimitConfig,
    state: Mutex<HashMap<ClientKey, WindowCounter>>,
    metrics: Arc<OperationalMetrics>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig, metrics: Arc<OperationalMetrics>) -> Self {
        Self {
            config,
            state: Mutex::new(HashMap::new()),
            metrics,
        }
    }

    fn check(&self, client: ClientKey, now: Instant) -> Result<(), Duration> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state
            .retain(|_, counter| now.duration_since(counter.started_at) < self.config.window() * 2);
        let counter = state.entry(client).or_insert(WindowCounter {
            started_at: now,
            requests: 0,
        });
        let elapsed = now.duration_since(counter.started_at);
        if elapsed >= self.config.window() {
            counter.started_at = now;
            counter.requests = 0;
        }
        if counter.requests >= self.config.requests() {
            return Err(self.config.window().saturating_sub(elapsed));
        }
        counter.requests += 1;
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum ClientKey {
    Ip(IpAddr),
    Unavailable,
}

struct WindowCounter {
    started_at: Instant,
    requests: u32,
}

pub async fn observe_requests(
    State(metrics): State<Arc<OperationalMetrics>>,
    request: Request,
    next: Next,
) -> Response {
    let started_at = Instant::now();
    let response = next.run(request).await;
    metrics.record_request(response.status(), started_at.elapsed());
    response
}

pub async fn enforce_rate_limit(
    State(limiter): State<Arc<RateLimiter>>,
    request: Request,
    next: Next,
) -> Response {
    let client = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| ClientKey::Ip(address.ip()))
        .unwrap_or(ClientKey::Unavailable);
    match limiter.check(client, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            limiter.metrics.record_rate_limited();
            let retry_after_seconds = retry_after.as_secs().max(1);
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": {
                        "code": "rate_limited",
                        "message": "request rate limit exceeded"
                    }
                })),
            )
                .into_response();
            if let Ok(value) = HeaderValue::from_str(&retry_after_seconds.to_string()) {
                response.headers_mut().insert(RETRY_AFTER, value);
            }
            response
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_resets_after_window() {
        let metrics = Arc::new(OperationalMetrics::default());
        let limiter = RateLimiter::new(
            RateLimitConfig::new(1, Duration::from_secs(10)).unwrap(),
            metrics,
        );
        let now = Instant::now();

        assert!(limiter.check(ClientKey::Unavailable, now).is_ok());
        assert!(limiter.check(ClientKey::Unavailable, now).is_err());
        assert!(limiter
            .check(ClientKey::Unavailable, now + Duration::from_secs(10))
            .is_ok());
    }
}
