/// Errors that can occur in plugin code.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PdkError {
    #[error("host function error: {0}")]
    HostFn(String),

    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Custom(String),
}
