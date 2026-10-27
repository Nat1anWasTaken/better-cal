//! RFC 9457 `application/problem+json` errors with field-level details.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ccal_core::problem::FieldError;
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct ProblemDetails {
    /// URI identifying the error type (`about:blank` style codes under /errors/).
    #[serde(rename = "type")]
    pub type_: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    /// Stable machine-readable code.
    pub code: String,
    /// Field-level problems (JSON Pointers into the request body).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
    /// Extra structured data for some errors (e.g. the current draft on conflict).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub detail: String,
    pub errors: Vec<FieldError>,
    pub data: Option<serde_json::Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, detail: impl Into<String>) -> ApiError {
        ApiError { status, code, detail: detail.into(), errors: vec![], data: None }
    }
    pub fn unauthorized() -> ApiError {
        ApiError::new(StatusCode::UNAUTHORIZED, "unauthenticated", "請先登入，或提供有效的 API token")
    }
    pub fn forbidden(detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::FORBIDDEN, "forbidden", detail)
    }
    pub fn not_found(what: &str) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("找不到{what}"))
    }
    pub fn conflict(code: &'static str, detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, code, detail)
    }
    pub fn bad_request(code: &'static str, detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, code, detail)
    }
    pub fn validation(errors: Vec<FieldError>) -> ApiError {
        ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY, code: "validation_failed", detail: "請求內容有欄位不符合規則".into(), errors, data: None
        }
    }
    pub fn too_many(detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::TOO_MANY_REQUESTS, "rate_limited", detail)
    }
    pub fn unavailable(detail: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "judge_unavailable", detail)
    }
    pub fn internal(e: impl std::fmt::Display) -> ApiError {
        tracing::error!(error = %e, "internal error");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "伺服器內部錯誤，請稍後再試")
    }
    pub fn with_data(mut self, data: serde_json::Value) -> ApiError {
        self.data = Some(data);
        self
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e
            && db.code().as_deref() == Some("23505")
        {
            return ApiError::conflict("already_exists", "資料已存在（唯一值重複）");
        }
        ApiError::internal(e)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ProblemDetails {
            type_: format!("/errors/{}", self.code),
            title: self.status.canonical_reason().unwrap_or("Error").to_string(),
            status: self.status.as_u16(),
            detail: self.detail,
            code: self.code.to_string(),
            errors: self.errors,
            data: self.data,
        };
        let mut res = (self.status, Json(body)).into_response();
        res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/problem+json"));
        res
    }
}

/// JSON body extractor that reports parse errors as problem+json.
pub struct JsonBody<T>(pub T);

impl<S, T> axum::extract::FromRequest<S> for JsonBody<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(JsonBody(v)),
            Err(rej) => {
                let msg = rej.body_text();
                // serde reports "<msg> at line X column Y"; keep it for API users.
                Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid_json", format!("JSON 格式或欄位不正確：{msg}")))
            }
        }
    }
}

/// An optional JSON body: an empty body yields `None`.
impl<S, T> axum::extract::OptionalFromRequest<S> for JsonBody<T>
where
    T: serde::de::DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: axum::extract::Request, state: &S) -> Result<Option<Self>, Self::Rejection> {
        let bytes = <axum::body::Bytes as axum::extract::FromRequest<S>>::from_request(req, state)
            .await
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_body", e.body_text()))?;
        if bytes.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(None);
        }
        serde_json::from_slice(&bytes)
            .map(|v| Some(JsonBody(v)))
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, "invalid_json", format!("JSON 格式或欄位不正確：{e}")))
    }
}
