use axum::Json;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::response::Response;
use serde_json::Value;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum ProxyError {
    #[error("invalid Responses request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Responses feature: {0}")]
    Unsupported(String),
    #[error("upstream Chat Completions request failed: {0}")]
    Upstream(String),
    #[error("upstream Chat Completions returned HTTP {status}: {message}")]
    UpstreamHttp { status: StatusCode, message: String },
    #[error("upstream Chat Completions request failed: {message}")]
    ModelNotFound {
        message: String,
        status: Option<StatusCode>,
    },
    #[error("upstream Chat Completions stream is invalid: {0}")]
    InvalidUpstream(String),
}

impl ProxyError {
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidRequest(message.into())
    }

    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::Unsupported(message.into())
    }

    pub(crate) fn upstream(message: impl Into<String>) -> Self {
        Self::Upstream(message.into())
    }

    pub(crate) fn invalid_upstream(message: impl Into<String>) -> Self {
        Self::InvalidUpstream(message.into())
    }

    pub(crate) fn upstream_http(status: StatusCode, message: impl Into<String>) -> Self {
        Self::UpstreamHttp {
            status,
            message: message.into(),
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            Self::InvalidRequest(_) | Self::Unsupported(_) => StatusCode::BAD_REQUEST,
            Self::UpstreamHttp { status, .. } => *status,
            Self::ModelNotFound { status, .. } => status.unwrap_or(StatusCode::BAD_GATEWAY),
            Self::Upstream(_) | Self::InvalidUpstream(_) => StatusCode::BAD_GATEWAY,
        }
    }

    pub(crate) fn code(&self) -> Option<&'static str> {
        match self {
            Self::ModelNotFound { .. } => Some("model_not_found"),
            Self::InvalidRequest(_)
            | Self::Unsupported(_)
            | Self::Upstream(_)
            | Self::UpstreamHttp { .. }
            | Self::InvalidUpstream(_) => None,
        }
    }

    pub(crate) fn error_body(&self) -> Value {
        let error_type = match self {
            Self::InvalidRequest(_) => "invalid_request_error",
            Self::Unsupported(_) => "unsupported_feature_error",
            Self::Upstream(_)
            | Self::UpstreamHttp { .. }
            | Self::ModelNotFound { .. }
            | Self::InvalidUpstream(_) => "upstream_error",
        };
        json!({
            "error": {
                "message": self.to_string(),
                "type": error_type,
                "code": self.code(),
                "param": Value::Null,
            }
        })
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        (self.status(), Json(self.error_body())).into_response()
    }
}
