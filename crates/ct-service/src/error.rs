use serde::Serialize;

/// An error the UI can show: a kind for styling and a plain-language message.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct ServiceError {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The user typed something invalid.
    InvalidInput,
    NotFound,
    /// A data source is down or rate limiting; retrying later may help.
    Unavailable,
    /// The user stopped the work.
    Cancelled,
    Internal,
}

impl ServiceError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        ServiceError {
            kind,
            message: message.into(),
        }
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidInput, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

impl From<ct_providers::ProviderError> for ServiceError {
    fn from(e: ct_providers::ProviderError) -> Self {
        use ct_providers::ProviderError as P;
        let kind = match &e {
            P::InvalidInput(_) => ErrorKind::InvalidInput,
            P::NotFound(_) => ErrorKind::NotFound,
            P::RateLimited(_) | P::Unavailable { .. } | P::BadResponse { .. } => {
                ErrorKind::Unavailable
            }
            P::Rejected { .. } | P::Unsupported(_) => ErrorKind::Internal,
            P::Cancelled => ErrorKind::Cancelled,
        };
        let message = match &e {
            P::NotFound(_) => "Not found on the blockchain (check for a typo).".to_string(),
            _ => e.to_string(),
        };
        ServiceError { kind, message }
    }
}

impl From<ct_core::AddressError> for ServiceError {
    fn from(e: ct_core::AddressError) -> Self {
        Self::invalid(e.to_string())
    }
}

impl From<ct_labels::LabelError> for ServiceError {
    fn from(e: ct_labels::LabelError) -> Self {
        Self::internal(e.to_string())
    }
}

impl From<ct_store::StoreError> for ServiceError {
    fn from(e: ct_store::StoreError) -> Self {
        Self::internal(format!("local cache: {e}"))
    }
}

impl From<std::io::Error> for ServiceError {
    fn from(e: std::io::Error) -> Self {
        Self::internal(e.to_string())
    }
}

pub type Result<T, E = ServiceError> = std::result::Result<T, E>;
