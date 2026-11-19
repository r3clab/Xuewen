//! axum's extractors, rejecting through `ApiError` so a malformed request
//! gets the API's `{"error": …}` body instead of axum's plain text.

use axum::extract::{FromRequest, FromRequestParts};

use super::error::ApiError;

#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub(crate) struct ApiJson<T>(pub(crate) T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub(crate) struct ApiPath<T>(pub(crate) T);

#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub(crate) struct ApiQuery<T>(pub(crate) T);
