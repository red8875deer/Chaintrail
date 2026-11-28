#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// The caller passed something malformed (bad txid, wrong-chain address).
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// The source does not know the requested object.
    #[error("not found: {0}")]
    NotFound(String),
    /// Rejected by the source as a bad request (non-retryable).
    #[error("request rejected by {source_name}: HTTP {status}: {body}")]
    Rejected {
        source_name: String,
        status: u16,
        body: String,
    },
    /// Still rate limited after retries.
    #[error("rate limited by {0}; try again later or use your own node")]
    RateLimited(String),
    /// Network failure or server error after retries.
    #[error("{source_name} unavailable: {detail}")]
    Unavailable { source_name: String, detail: String },
    /// The response did not have the expected shape.
    #[error("unexpected response from {source_name}: {detail}")]
    BadResponse { source_name: String, detail: String },
    #[error("feature not supported by {0}")]
    Unsupported(String),
    /// The caller stopped the work (e.g. the user pressed Stop during a trace).
    #[error("stopped by the user")]
    Cancelled,
}

impl ProviderError {
    /// Whether another provider might succeed where this one failed.
    pub fn should_fall_back(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimited(_)
                | ProviderError::Unavailable { .. }
                | ProviderError::BadResponse { .. }
                | ProviderError::Unsupported(_)
                // Blocked (bot protection, region): another source may still answer.
                | ProviderError::Rejected {
                    status: 401 | 403 | 451,
                    ..
                }
        )
    }
}

pub type Result<T, E = ProviderError> = std::result::Result<T, E>;
