use axum::extract::rejection::JsonRejection;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::error::{ApiError, ErrorCode};

/// `axum::Json` with problem+json rejections. The detail says what was wrong ("expected
/// JSON", "missing field `role`") but never repeats request content.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            // Over the body limit is 413; any other body read failure (client disconnect,
            // broken chunking) is a bad request, not "too large".
            Err(JsonRejection::BytesRejection(err))
                if err.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE =>
            {
                Err(ApiError::new(ErrorCode::PayloadTooLarge))
            }
            Err(JsonRejection::BytesRejection(_)) => {
                Err(ApiError::new(ErrorCode::ValidationFailed)
                    .with_detail("the request body could not be read"))
            }
            Err(JsonRejection::MissingJsonContentType(_)) => {
                Err(ApiError::new(ErrorCode::ValidationFailed)
                    .with_detail("expected Content-Type: application/json"))
            }
            Err(JsonRejection::JsonDataError(err)) => {
                Err(ApiError::new(ErrorCode::ValidationFailed)
                    .with_detail(safe_serde_detail(&err.body_text())))
            }
            Err(_) => {
                Err(ApiError::new(ErrorCode::ValidationFailed).with_detail("malformed JSON body"))
            }
        }
    }
}

/// `axum::extract::Query` whose rejection is `validation_failed`, never echoing input.
pub struct ApiQuery<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for ApiQuery<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let result: Result<axum::extract::Query<T>, QueryRejection> =
            axum::extract::Query::from_request_parts(parts, state).await;
        result.map(|axum::extract::Query(v)| Self(v)).map_err(|_| {
            ApiError::new(ErrorCode::ValidationFailed).with_detail("invalid query string")
        })
    }
}

/// `axum::extract::Path` whose rejection is `validation_failed`, never echoing input.
pub struct ApiPath<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for ApiPath<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let result: Result<axum::extract::Path<T>, PathRejection> =
            axum::extract::Path::from_request_parts(parts, state).await;
        result.map(|axum::extract::Path(v)| Self(v)).map_err(|_| {
            ApiError::new(ErrorCode::ValidationFailed).with_detail("invalid path parameter")
        })
    }
}

/// serde messages look like "missing field `role` at line 1 column 2" or "unknown variant
/// `x`, expected one of ...". Keep only the part before " at line" and drop any quoted value
/// that is not a field name, so user input never reaches the response.
fn safe_serde_detail(text: &str) -> String {
    let head = text.split(" at line").next().unwrap_or(text);
    // "unknown field `<client key>`" repeats a client-chosen key (deny_unknown_fields
    // bodies trigger it), so it gets a fixed message like the value errors.
    if head.contains("unknown field") {
        return "the body has a field this endpoint does not accept".to_string();
    }
    if head.contains("unknown variant")
        || head.contains("invalid type")
        || head.contains("invalid value")
    {
        return "a field has an invalid value".to_string();
    }
    head.rsplit(": ").next().unwrap_or(head).to_string()
}

#[cfg(test)]
mod tests {
    use super::safe_serde_detail;

    #[test]
    fn serde_details_never_echo_client_text() {
        for text in [
            "Failed to deserialize the JSON body into the target type: unknown field `<script>`, expected `status` or `role` at line 1 column 12",
            "Failed to deserialize the JSON body into the target type: status: unknown variant `hacked`, expected one of `active`, `disabled` at line 1 column 19",
        ] {
            let detail = safe_serde_detail(text);
            assert!(
                !detail.contains("<script>") && !detail.contains("hacked"),
                "{detail}"
            );
        }
        assert_eq!(
            safe_serde_detail(
                "Failed to deserialize the JSON body into the target type: missing field `display_name` at line 1 column 2"
            ),
            "missing field `display_name`"
        );
    }
}
