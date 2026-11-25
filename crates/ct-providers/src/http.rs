//! Shared HTTP client: per-host rate limiting, retries with backoff, JSON decoding.
//!
//! Free APIs rarely publish limits, so the defaults are conservative. Being polite keeps
//! them free for everyone.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde::de::DeserializeOwned;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep, sleep_until};

use crate::error::{ProviderError, Result};

#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Minimum gap between requests to the same host.
    pub min_interval: Duration,
    /// Per-host overrides of `min_interval`.
    pub host_intervals: HashMap<String, Duration>,
    pub timeout: Duration,
    /// Retries after the first attempt for retryable failures (429, 5xx, network).
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub user_agent: String,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig {
            min_interval: Duration::from_millis(250),
            host_intervals: HashMap::new(),
            timeout: Duration::from_secs(30),
            max_retries: 4,
            initial_backoff: Duration::from_millis(750),
            user_agent: format!("chaintrail/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Cheap to clone; clones share connection pool and rate-limit state.
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    config: Arc<HttpConfig>,
    next_slot: Arc<Mutex<HashMap<String, Instant>>>,
}

impl HttpClient {
    pub fn new(config: HttpConfig) -> Result<Self> {
        let inner = reqwest::Client::builder()
            .user_agent(&config.user_agent)
            .timeout(config.timeout)
            .build()
            .map_err(|e| ProviderError::Unavailable {
                source_name: "http client".into(),
                detail: e.to_string(),
            })?;
        Ok(HttpClient {
            inner,
            config: Arc::new(config),
            next_slot: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// GET a URL and decode JSON.
    pub async fn get_json<T: DeserializeOwned>(&self, source_name: &str, url: &str) -> Result<T> {
        let body = self.get_text(source_name, url).await?;
        serde_json::from_str(&body).map_err(|e| ProviderError::BadResponse {
            source_name: source_name.to_string(),
            detail: format!("{e} (url: {url})"),
        })
    }

    /// GET a URL and return the body, retrying transient failures.
    pub async fn get_text(&self, source_name: &str, url: &str) -> Result<String> {
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .ok_or_else(|| ProviderError::InvalidInput(format!("bad url: {url}")))?;

        let mut backoff = self.config.initial_backoff;
        let mut attempt = 0;
        loop {
            self.wait_for_slot(&host).await;
            let outcome = self.inner.get(url).send().await;
            let retry_after = match outcome {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return resp.text().await.map_err(|e| ProviderError::Unavailable {
                            source_name: source_name.to_string(),
                            detail: e.to_string(),
                        });
                    }
                    let retry_after = parse_retry_after(&resp);
                    let retryable =
                        status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
                    if !retryable || attempt >= self.config.max_retries {
                        let body = resp.text().await.unwrap_or_default();
                        return Err(status_error(source_name, url, status, body));
                    }
                    tracing::debug!(%status, url, attempt, "retrying");
                    retry_after
                }
                Err(e) => {
                    if attempt >= self.config.max_retries {
                        return Err(ProviderError::Unavailable {
                            source_name: source_name.to_string(),
                            detail: e.to_string(),
                        });
                    }
                    tracing::debug!(error = %e, url, attempt, "retrying after network error");
                    None
                }
            };
            sleep(retry_after.unwrap_or(backoff)).await;
            backoff = backoff.saturating_mul(2);
            attempt += 1;
        }
    }

    async fn wait_for_slot(&self, host: &str) {
        let interval = self
            .config
            .host_intervals
            .get(host)
            .copied()
            .unwrap_or(self.config.min_interval);
        let slot = {
            let mut map = self.next_slot.lock().await;
            let now = Instant::now();
            let slot = map.get(host).copied().filter(|t| *t > now).unwrap_or(now);
            map.insert(host.to_string(), slot + interval);
            slot
        };
        sleep_until(slot).await;
    }
}

fn parse_retry_after(resp: &reqwest::Response) -> Option<Duration> {
    let secs: u64 = resp
        .headers()
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    // Cap so one bad header cannot stall a trace for hours.
    Some(Duration::from_secs(secs.min(60)))
}

fn status_error(source_name: &str, url: &str, status: StatusCode, body: String) -> ProviderError {
    let body: String = body.chars().take(300).collect();
    match status {
        StatusCode::NOT_FOUND => ProviderError::NotFound(url.to_string()),
        StatusCode::TOO_MANY_REQUESTS => ProviderError::RateLimited(source_name.to_string()),
        s if s.is_server_error() => ProviderError::Unavailable {
            source_name: source_name.to_string(),
            detail: format!("HTTP {s}: {body}"),
        },
        s => ProviderError::Rejected {
            source_name: source_name.to_string(),
            status: s.as_u16(),
            body,
        },
    }
}
